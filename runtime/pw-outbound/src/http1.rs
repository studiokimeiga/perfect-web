//! **HTTP/1.1, as the client writes a request and reads an answer**
//! (RFC 9112), strictly: one request per connection, `Connection: close`;
//! an answer read by its stated length or its chunks and never to the
//! connection's close, so a cut is never taken for an end; every byte of it
//! bounded.

use std::io::{self, Read};

use crate::origin::Origin;
use crate::{Answer, Error, Limits, Phase, Request, io_error};

/// The header names the client writes itself, or that would change how the
/// connection is used: a request may set none of them.
const RESERVED: [&str; 10] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "proxy-connection",
    "te",
    "trailer",
    "upgrade",
    "expect",
];

/// Interim (1xx) answers taken before the final one.
const MOST_INTERIM: usize = 8;

/// Field lines in one head.
const MOST_FIELDS: usize = 128;

/// RFC 9110 §5.6.2's token.
fn is_token(name: &[u8]) -> bool {
    !name.is_empty()
        && name.iter().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// **A value the client writes**: visible ASCII, spaces and tabs, none at
/// either end; no control, so no CR or LF, which would end the field and
/// begin another.
fn is_value(value: &str) -> bool {
    !value.starts_with([' ', '\t'])
        && !value.ends_with([' ', '\t'])
        && value
            .bytes()
            .all(|b| b == b'\t' || (0x20..0x7f).contains(&b))
}

fn check_name(name: &str) -> Result<(), Error> {
    if !is_token(name.as_bytes()) {
        return Err(Error::Request(format!(
            "`{name}` is not a field name (a token)"
        )));
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {
        return Err(Error::Request(format!(
            "`{name}` is the client's own to write"
        )));
    }
    Ok(())
}

/// **The head of `request`**, every field checked: a name a token and none
/// of the client's own, a value that cannot end its field. A secret's value
/// is checked as any other, and never shown in the refusal.
pub(crate) fn request_head(
    request: &Request,
    origin: &Origin,
    target: &str,
) -> Result<Vec<u8>, Error> {
    let mut head = format!(
        "{} {target} HTTP/1.1\r\nhost: {}\r\n",
        request.method.as_str(),
        origin.authority()
    )
    .into_bytes();
    let mut field = |name: &str, value: &str| {
        head.extend_from_slice(name.to_ascii_lowercase().as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(value.as_bytes());
        head.extend_from_slice(b"\r\n");
    };
    for (name, value) in &request.headers {
        check_name(name)?;
        if !is_value(value) {
            return Err(Error::Request(format!(
                "the value of `{name}` holds a control, a line break, a character outside \
                 ASCII, or a space at an end"
            )));
        }
        field(name, value);
    }
    for (name, secret) in &request.secrets {
        check_name(name)?;
        if !is_value(secret.expose()) {
            return Err(Error::Request(format!(
                "the secret value of `{name}` holds a control, a line break, a character \
                 outside ASCII, or a space at an end (not shown)"
            )));
        }
        field(name, secret.expose());
    }
    if let Some(body) = &request.body {
        field("content-length", &body.len().to_string());
    }
    head.extend_from_slice(b"connection: close\r\n\r\n");
    Ok(head)
}

// --- reading -----------------------------------------------------------------

/// **What may yet be read of one part of an answer**: its head, or a chunked
/// body's framing.
struct Budget {
    left: usize,
    limit: usize,
    what: &'static str,
}

impl Budget {
    fn new(limit: usize, what: &'static str) -> Budget {
        Budget {
            left: limit,
            limit,
            what,
        }
    }

    fn take(&mut self, n: usize) -> Result<(), Error> {
        if n > self.left {
            return Err(Error::TooLarge {
                what: self.what,
                limit: self.limit,
            });
        }
        self.left -= n;
        Ok(())
    }
}

/// **An answer's bytes, buffered**, read only as far as asked.
struct Input<R> {
    inner: R,
    buf: Box<[u8]>,
    start: usize,
    end: usize,
}

impl<R: Read> Input<R> {
    fn new(inner: R) -> Input<R> {
        Input {
            inner,
            buf: vec![0; 8 * 1024].into_boxed_slice(),
            start: 0,
            end: 0,
        }
    }

    /// More bytes, where none are buffered: how many, 0 at the end.
    fn fill(&mut self) -> Result<usize, Error> {
        if self.start < self.end {
            return Ok(self.end - self.start);
        }
        self.start = 0;
        self.end = 0;
        loop {
            match self.inner.read(&mut self.buf) {
                Ok(n) => {
                    self.end = n;
                    return Ok(n);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(io_error(e, Phase::Exchange)),
            }
        }
    }

    fn buffered(&self) -> usize {
        self.end - self.start
    }

    /// **A line, ended by CRLF**, without it; each byte, the CRLF among
    /// them, taken from `budget`. A bare LF, or a CR not before the LF, is
    /// refused (RFC 9112 §2.2).
    fn line(&mut self, budget: &mut Budget) -> Result<Vec<u8>, Error> {
        let mut line = Vec::new();
        loop {
            if self.fill()? == 0 {
                return Err(Error::Framing(
                    "the connection closed in the middle of a line".into(),
                ));
            }
            let byte = self.buf[self.start];
            self.start += 1;
            budget.take(1)?;
            if byte == b'\n' {
                if line.pop() != Some(b'\r') {
                    return Err(Error::Framing("a line ended by a bare LF".into()));
                }
                if line.contains(&b'\r') {
                    return Err(Error::Framing("a bare CR inside a line".into()));
                }
                return Ok(line);
            }
            line.push(byte);
        }
    }

    /// **Exactly `n` more bytes** onto `out`, or the cut said.
    fn exact(&mut self, n: usize, out: &mut Vec<u8>) -> Result<(), Error> {
        let mut want = n;
        while want > 0 {
            if self.fill()? == 0 {
                return Err(Error::Framing(format!(
                    "the connection closed {} bytes into a body of {n}",
                    n - want
                )));
            }
            let take = want.min(self.buffered());
            out.extend_from_slice(&self.buf[self.start..self.start + take]);
            self.start += take;
            want -= take;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Version {
    Http10,
    Http11,
}

/// **A status line**: `HTTP/1.1` (or `HTTP/1.0`), a space, three digits from
/// 100 to 599, and a reason or none.
fn status_line(line: &[u8]) -> Result<(Version, u16), Error> {
    let bad = || {
        Error::Framing(format!(
            "`{}` is not a status line",
            String::from_utf8_lossy(&line[..line.len().min(40)])
        ))
    };
    let version = match line.get(..9) {
        Some(b"HTTP/1.1 ") => Version::Http11,
        Some(b"HTTP/1.0 ") => Version::Http10,
        _ => return Err(bad()),
    };
    let code = line.get(9..12).ok_or_else(bad)?;
    if !code.iter().all(u8::is_ascii_digit) || !matches!(line.get(12), None | Some(b' ')) {
        return Err(bad());
    }
    let status = code.iter().fold(0u16, |n, d| n * 10 + u16::from(d - b'0'));
    if !(100..=599).contains(&status) {
        return Err(bad());
    }
    Ok((version, status))
}

/// **A field line**: a token, `:`, and a value of no control but HTAB, its
/// spaces at either end dropped. A line beginning with a space or tab
/// continues the one before (obs-fold), which is refused, as is whitespace
/// before the `:` (RFC 9112 §5.1, §5.2).
fn field_line(line: &[u8]) -> Result<(String, String), Error> {
    if matches!(line.first(), Some(b' ' | b'\t')) {
        return Err(Error::Framing(
            "a field line folded onto the one before it (obs-fold)".into(),
        ));
    }
    let colon = line
        .iter()
        .position(|b| *b == b':')
        .ok_or_else(|| Error::Framing("a field line without a `:`".into()))?;
    let (name, value) = (&line[..colon], &line[colon + 1..]);
    if !is_token(name) {
        return Err(Error::Framing(
            "a field name that is not a token (or whitespace before its `:`)".into(),
        ));
    }
    let value = value
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t'))
        .map_or(&[][..], |s| &value[s..]);
    let value = value
        .iter()
        .rposition(|b| !matches!(b, b' ' | b'\t'))
        .map_or(&[][..], |e| &value[..=e]);
    if value
        .iter()
        .any(|b| (*b < 0x20 && *b != b'\t') || *b == 0x7f)
    {
        return Err(Error::Framing("a field value holding a control".into()));
    }
    Ok((
        String::from_utf8_lossy(name).to_ascii_lowercase(),
        String::from_utf8_lossy(value).into_owned(),
    ))
}

type Head = (Version, u16, Vec<(String, String)>);

/// **One head**: its status line and its fields, to the empty line.
fn head<R: Read>(input: &mut Input<R>, budget: &mut Budget) -> Result<Head, Error> {
    let (version, status) = status_line(&input.line(budget)?)?;
    let mut fields = Vec::new();
    loop {
        let line = input.line(budget)?;
        if line.is_empty() {
            return Ok((version, status, fields));
        }
        if fields.len() == MOST_FIELDS {
            return Err(Error::Framing(format!(
                "a head of more than {MOST_FIELDS} fields"
            )));
        }
        fields.push(field_line(&line)?);
    }
}

fn values<'a>(fields: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    fields
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// **An answer, read off `reader`**: interim answers passed over, a
/// redirect refused, the body read by its framing, and nothing after it.
pub(crate) fn read_answer<R: Read>(reader: R, limits: &Limits) -> Result<Answer, Error> {
    let mut input = Input::new(reader);
    let mut budget = Budget::new(limits.head, "head");
    let mut interim = 0;
    let (version, status, fields) = loop {
        let (version, status, fields) = head(&mut input, &mut budget)?;
        match status {
            101 => {
                return Err(Error::Framing(
                    "the server switched protocols, which the client never asks".into(),
                ));
            }
            100..=199 => {
                interim += 1;
                if interim > MOST_INTERIM {
                    return Err(Error::Framing(format!(
                        "more than {MOST_INTERIM} interim answers"
                    )));
                }
            }
            _ => break (version, status, fields),
        }
    };
    // Fetch's redirect statuses: never followed, so nothing the answer says
    // takes the client, or a secret it sent, to another origin.
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        return Err(Error::Redirected {
            status,
            location: values(&fields, "location").first().map(|l| l.to_string()),
        });
    }
    if let Some(coding) = values(&fields, "content-encoding")
        .into_iter()
        .find(|c| !c.eq_ignore_ascii_case("identity"))
    {
        return Err(Error::Framing(format!(
            "the body is encoded as `{coding}`, which the client never asks"
        )));
    }
    let body = if status == 204 || status == 304 {
        Vec::new()
    } else {
        body(&mut input, version, &fields, limits)?
    };
    if input.buffered() > 0 {
        return Err(Error::Framing(
            "bytes follow the answer past the length its framing states".into(),
        ));
    }
    Ok(Answer {
        status,
        headers: fields,
        body,
    })
}

/// **A body, as its head frames it** (RFC 9112 §6.3): chunked, or of its
/// stated length; both at once, a length stated twice, a coding other than
/// `chunked`, or no length at all is refused.
fn body<R: Read>(
    input: &mut Input<R>,
    version: Version,
    fields: &[(String, String)],
    limits: &Limits,
) -> Result<Vec<u8>, Error> {
    let codings = values(fields, "transfer-encoding");
    let lengths = values(fields, "content-length");
    if !codings.is_empty() {
        if !lengths.is_empty() {
            return Err(Error::Framing(
                "the answer states both a transfer coding and a length, the shape of a \
                 smuggled message"
                    .into(),
            ));
        }
        if version == Version::Http10 {
            return Err(Error::Framing(
                "an HTTP/1.0 answer states a transfer coding".into(),
            ));
        }
        if codings.len() != 1 || !codings[0].eq_ignore_ascii_case("chunked") {
            return Err(Error::Framing(format!(
                "the transfer coding `{}`: `chunked` alone is read",
                codings.join(", ")
            )));
        }
        return chunked(input, limits);
    }
    let length = match lengths.as_slice() {
        [] => {
            return Err(Error::Framing(
                "the answer states no length, so its end would be the connection's close, \
                 where a cut is not told from an end"
                    .into(),
            ));
        }
        [one] => *one,
        _ => {
            return Err(Error::Framing(
                "the answer states its length more than once".into(),
            ));
        }
    };
    if length.is_empty() || length.len() > 19 || !length.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Framing(format!("`{length}` is not a length")));
    }
    let length: u64 = length
        .parse()
        .map_err(|_| Error::Framing(format!("`{length}` is not a length")))?;
    if length > limits.body as u64 {
        return Err(Error::TooLarge {
            what: "body",
            limit: limits.body,
        });
    }
    let mut body = Vec::with_capacity(length as usize);
    input.exact(length as usize, &mut body)?;
    Ok(body)
}

/// **A chunked body** (RFC 9112 §7.1): each chunk's size in hex, its
/// extensions passed over, its data, and its CRLF; then the last chunk and
/// the trailer fields, read and dropped. The data is bounded by the body's
/// limit, and the framing around it by as many bytes again.
fn chunked<R: Read>(input: &mut Input<R>, limits: &Limits) -> Result<Vec<u8>, Error> {
    let mut framing = Budget::new(limits.body, "body's chunk framing");
    let mut body = Vec::new();
    loop {
        let line = input.line(&mut framing)?;
        let size = line.split(|b| *b == b';').next().unwrap_or(&[]);
        let size = size
            .iter()
            .rposition(|b| !matches!(b, b' ' | b'\t'))
            .map_or(&[][..], |e| &size[..=e]);
        if size.is_empty() || size.len() > 16 || !size.iter().all(u8::is_ascii_hexdigit) {
            return Err(Error::Framing(
                "a chunk's size is not a hexadecimal number".into(),
            ));
        }
        let size = size.iter().fold(0u64, |n, d| {
            n * 16 + u64::from((*d as char).to_digit(16).unwrap_or(0))
        });
        if size == 0 {
            loop {
                let trailer = input.line(&mut framing)?;
                if trailer.is_empty() {
                    return Ok(body);
                }
                field_line(&trailer)?;
            }
        }
        if body.len() as u64 + size > limits.body as u64 {
            return Err(Error::TooLarge {
                what: "body",
                limit: limits.body,
            });
        }
        input.exact(size as usize, &mut body)?;
        if !input.line(&mut framing)?.is_empty() {
            return Err(Error::Framing(
                "a chunk is longer than its size states".into(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Method, Secret};

    fn limits() -> Limits {
        Limits {
            head: 1024,
            body: 64,
            ..Limits::default()
        }
    }

    fn read(bytes: &[u8]) -> Result<Answer, Error> {
        read_answer(bytes, &limits())
    }

    fn framing(bytes: &[u8]) -> bool {
        matches!(read(bytes), Err(Error::Framing(_)))
    }

    /// **A request's head is written as RFC 9112 writes one**: the target,
    /// the host and its port, each field, the length of a body, and the
    /// connection closed after it.
    #[test]
    fn a_requests_head_is_written_whole() {
        let origin = Origin::parse("https://api.provider.test:8443").unwrap();
        let request = Request::post("https://x/", "application/json", b"{}".to_vec())
            .header("Idempotency-Key", "order-1:interaction-2")
            .secret("Authorization", Secret::new("Bearer sk_test_1"));
        assert_eq!(request.method(), Method::Post);
        let head = request_head(&request, &origin, "/v1/payment_intents");
        assert_eq!(
            head.map(|h| String::from_utf8_lossy(&h).into_owned()),
            Ok(
                "POST /v1/payment_intents HTTP/1.1\r\nhost: api.provider.test:8443\r\n\
                content-type: application/json\r\nidempotency-key: order-1:interaction-2\r\n\
                authorization: Bearer sk_test_1\r\ncontent-length: 2\r\nconnection: close\r\n\r\n"
                    .to_string()
            )
        );
        let origin = Origin::parse("https://api.provider.test").unwrap();
        let head = request_head(&Request::get("https://x/"), &origin, "/v1/x?a=1");
        assert_eq!(
            head.map(|h| String::from_utf8_lossy(&h).into_owned()),
            Ok(
                "GET /v1/x?a=1 HTTP/1.1\r\nhost: api.provider.test\r\nconnection: close\r\n\r\n"
                    .to_string()
            )
        );
    }

    /// **No field the client writes can end its line**: a value holding a
    /// CR, an LF or another control, or a name that is not a token, is
    /// refused, a secret's value as any other and never shown; and the
    /// client's own fields are its own.
    #[test]
    fn no_field_can_split_a_request() {
        let origin = Origin::parse("https://api.provider.test").unwrap();
        let get = || Request::get("https://x/");
        let refused = |request: Request| {
            matches!(request_head(&request, &origin, "/"), Err(Error::Request(_)))
        };
        for value in [
            "a\r\nx-injected: 1",
            "a\nb",
            "a\rb",
            "a\0b",
            "a\x7fb",
            "é",
            " a",
            "a ",
        ] {
            assert!(refused(get().header("x-value", value)), "{value:?}");
        }
        for name in ["", "x value", "x:value", "x\r\nvalue", "x\u{e9}"] {
            assert!(refused(get().header(name, "v")), "{name:?}");
        }
        for name in [
            "Host",
            "content-length",
            "Transfer-Encoding",
            "connection",
            "keep-alive",
            "proxy-connection",
            "te",
            "trailer",
            "upgrade",
            "Expect",
        ] {
            assert!(refused(get().header(name, "x")), "{name}");
        }
        let secret = request_head(
            &get().secret(
                "authorization",
                Secret::new("Bearer sk_live\r\nx-injected: 1"),
            ),
            &origin,
            "/",
        );
        assert!(matches!(secret, Err(Error::Request(_))), "{secret:?}");
        let said = format!("{secret:?} {}", secret.clone().err().unwrap());
        assert!(
            !said.contains("sk_live") && !said.contains("injected"),
            "{said}"
        );
        let tab = request_head(&get().header("x-value", "a\tb"), &origin, "/");
        assert!(tab.is_ok(), "{tab:?}");
    }

    /// **A body of a stated length is read to that length, and no further**:
    /// the answer is read once it is whole, though the connection stays
    /// open.
    #[test]
    fn a_body_is_read_by_its_length() {
        let answer =
            read(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\ncontent-length: 5\r\n\r\nhello");
        assert_eq!(answer.as_ref().map(|a| a.status()), Ok(200));
        assert_eq!(answer.as_ref().map(|a| a.text()), Ok("hello".to_string()));
        assert_eq!(
            answer.as_ref().map(|a| a.header("CONTENT-TYPE")),
            Ok(Some("text/plain"))
        );
        let open = read_answer(
            Open(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok".to_vec()),
            &limits(),
        );
        assert_eq!(open.map(|a| a.into_body()), Ok(b"ok".to_vec()));
        let old = read(b"HTTP/1.0 200 OK\r\ncontent-length: 0\r\n\r\n");
        assert!(old.is_ok(), "{old:?}");
        let bare = read(b"HTTP/1.1 200\r\ncontent-length: 0\r\n\r\n");
        assert!(bare.is_ok(), "{bare:?}");
    }

    /// A reader that is never at its end: what a connection held open is.
    struct Open(Vec<u8>);

    impl Read for Open {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let n = buf.len().min(self.0.len());
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0.drain(..n);
            Ok(n)
        }
    }

    /// **A length is the one length**: cut short, followed by more, stated
    /// twice or as a list, signed, or not stated at all, it is refused.
    #[test]
    fn a_length_is_held_to() {
        let short = read(b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\nhello");
        assert_eq!(
            short,
            Err(Error::Framing(
                "the connection closed 5 bytes into a body of 10".into()
            ))
        );
        assert!(framing(
            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nokEXTRA"
        ));
        assert!(framing(
            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\ncontent-length: 2\r\n\r\nok"
        ));
        assert!(framing(
            b"HTTP/1.1 200 OK\r\ncontent-length: 2, 2\r\n\r\nok"
        ));
        assert!(framing(b"HTTP/1.1 200 OK\r\ncontent-length: +2\r\n\r\nok"));
        assert!(framing(b"HTTP/1.1 200 OK\r\ncontent-length: -2\r\n\r\nok"));
        assert!(framing(b"HTTP/1.1 200 OK\r\ncontent-length:\r\n\r\nok"));
        assert!(framing(b"HTTP/1.1 200 OK\r\n\r\nread to the close"));
        assert!(framing(b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n"));
    }

    /// **A chunked body is read chunk by chunk**, extensions and trailers
    /// passed over; a chunk longer or shorter than its size, a size that is
    /// not hex, or a cut is refused.
    #[test]
    fn a_chunked_body_is_read_by_its_chunks() {
        let answer = read(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
              5;name=value\r\nhello\r\n1 \r\n,\r\n6\r\n world\r\n0\r\nx-trailer: t\r\n\r\n",
        );
        assert_eq!(answer.map(|a| a.text()), Ok("hello, world".to_string()));
        let empty = read(b"HTTP/1.1 200 OK\r\ntransfer-encoding: CHUNKED\r\n\r\n0\r\n\r\n");
        assert_eq!(empty.map(|a| a.into_body()), Ok(Vec::new()));
        for bad in [
            &b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n3\r\nhello\r\n0\r\n\r\n"[..],
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n9\r\nhello\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\nz\r\nhello\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n",
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n1 1\r\nh\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\nextra",
        ] {
            assert!(framing(bad), "{}", String::from_utf8_lossy(bad));
        }
    }

    /// **A coding other than `chunked`, a length beside a coding, and a
    /// coding in HTTP/1.0 are refused**: the shapes of a smuggled message.
    #[test]
    fn a_transfer_coding_is_chunked_alone() {
        for bad in [
            &b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\ncontent-length: 5\r\n\r\n0\r\n\r\n"[..],
            b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: gzip, chunked\r\n\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: identity\r\n\r\n",
            b"HTTP/1.0 200 OK\r\ntransfer-encoding: chunked\r\n\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ncontent-encoding: gzip\r\ncontent-length: 0\r\n\r\n",
        ] {
            assert!(framing(bad), "{}", String::from_utf8_lossy(bad));
        }
        let identity =
            read(b"HTTP/1.1 200 OK\r\ncontent-encoding: identity\r\ncontent-length: 0\r\n\r\n");
        assert!(identity.is_ok(), "{identity:?}");
    }

    /// **A head is lines ended by CRLF, of fields as RFC 9112 writes them**:
    /// a bare LF or CR, a folded line, whitespace before a `:`, a line with
    /// none, a control in a value, or a status line of another protocol is
    /// refused.
    #[test]
    fn a_head_is_held_to_its_grammar() {
        for bad in [
            &b"HTTP/1.1 200 OK\nContent-Length: 0\r\n\r\n"[..],
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\n\n",
            b"HTTP/1.1 200 OK\r\nx: a\rb\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nx: a\r\n folded\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ncontent-length : 0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nno colon\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nx: a\x01b\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/2 200\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/1.1 20 OK\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/1.1 600 Odd\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/1.1 099 Odd\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/1.1 2000\r\ncontent-length: 0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n",
            b"",
        ] {
            assert!(framing(bad), "{}", String::from_utf8_lossy(bad));
        }
        let mut many = b"HTTP/1.1 200 OK\r\n".to_vec();
        for i in 0..129 {
            many.extend_from_slice(format!("x{i}: y\r\n").as_bytes());
        }
        many.extend_from_slice(b"content-length: 0\r\n\r\n");
        let many = read_answer(&many[..], &Limits::default());
        assert!(matches!(many, Err(Error::Framing(_))), "{many:?}");
    }

    /// **Interim answers are passed over, a final one with no body has
    /// none, and a switch of protocol is refused**.
    #[test]
    fn interim_and_bodiless_answers() {
        let answer = read(
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nlink: </x>\r\n\r\n\
              HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok",
        );
        assert_eq!(
            answer.map(|a| (a.status(), a.text())),
            Ok((200, "ok".to_string()))
        );
        assert!(framing(
            b"HTTP/1.1 101 Switching Protocols\r\nupgrade: h2c\r\n\r\n"
        ));
        let interim = b"HTTP/1.1 100 Continue\r\n\r\n".repeat(9);
        assert!(framing(&interim));
        for status in [204, 304] {
            let answer = read(format!("HTTP/1.1 {status} X\r\n\r\n").as_bytes());
            assert_eq!(
                answer.map(|a| (a.status(), a.into_body())),
                Ok((status, Vec::new()))
            );
        }
    }

    /// **A redirect is refused, not followed**: each of Fetch's redirect
    /// statuses, with where it points; what is not one is an answer.
    #[test]
    fn a_redirect_is_refused() {
        for status in [301, 302, 303, 307, 308] {
            let answer = read(
                format!(
                    "HTTP/1.1 {status} Moved\r\nlocation: https://elsewhere.test/x\r\ncontent-length: 0\r\n\r\n"
                )
                .as_bytes(),
            );
            assert_eq!(
                answer,
                Err(Error::Redirected {
                    status,
                    location: Some("https://elsewhere.test/x".into())
                })
            );
        }
        for status in [300, 304, 305, 306] {
            let answer = read(
                format!("HTTP/1.1 {status} X\r\nlocation: /x\r\ncontent-length: 0\r\n\r\n")
                    .as_bytes(),
            );
            assert_eq!(answer.map(|a| a.status()), Ok(status));
        }
    }

    /// **An answer is bounded**: a stated length past the body's limit is
    /// refused before any of it is read; chunks past it, and a head past its
    /// own, are refused as they arrive; an answer at the limits is taken.
    #[test]
    fn an_answer_is_bounded() {
        let huge = read(b"HTTP/1.1 200 OK\r\ncontent-length: 99999999999999\r\n\r\n");
        assert_eq!(
            huge,
            Err(Error::TooLarge {
                what: "body",
                limit: 64
            })
        );
        let past = read(
            format!(
                "HTTP/1.1 200 OK\r\ncontent-length: 65\r\n\r\n{}",
                "a".repeat(65)
            )
            .as_bytes(),
        );
        assert_eq!(
            past,
            Err(Error::TooLarge {
                what: "body",
                limit: 64
            })
        );
        let at = read(
            format!(
                "HTTP/1.1 200 OK\r\ncontent-length: 64\r\n\r\n{}",
                "a".repeat(64)
            )
            .as_bytes(),
        );
        assert_eq!(at.map(|a| a.into_body().len()), Ok(64));
        let chunks = read(
            format!(
                "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n20\r\n{0}\r\n21\r\n{0}a\r\n0\r\n\r\n",
                "a".repeat(32)
            )
            .as_bytes(),
        );
        assert_eq!(
            chunks,
            Err(Error::TooLarge {
                what: "body",
                limit: 64
            })
        );
        let tiny = b"1\r\na\r\n".repeat(40);
        let mut framed = b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n".to_vec();
        framed.extend_from_slice(&tiny);
        framed.extend_from_slice(b"0\r\n\r\n");
        let framed = read(&framed);
        assert_eq!(
            framed,
            Err(Error::TooLarge {
                what: "body's chunk framing",
                limit: 64
            })
        );
        let head = read(
            format!(
                "HTTP/1.1 200 OK\r\nx: {}\r\ncontent-length: 0\r\n\r\n",
                "a".repeat(1024)
            )
            .as_bytes(),
        );
        assert_eq!(
            head,
            Err(Error::TooLarge {
                what: "head",
                limit: 1024
            })
        );
        let endless = read_answer(Endless, &limits());
        assert_eq!(
            endless,
            Err(Error::TooLarge {
                what: "head",
                limit: 1024
            })
        );
    }

    /// A peer that sends one header line forever.
    struct Endless;

    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            buf.fill(b'a');
            Ok(buf.len())
        }
    }
}
