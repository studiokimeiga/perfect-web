//! **An origin, as the URL Standard's tuple**: a scheme, a host and a port,
//! and nothing after them. A grant names origins; a request's URL is read
//! into its origin and its target, and is sent only where the grant declares
//! that origin, compared whole.
//!
//! The reading is strict where a looser one would let two spellings name one
//! origin, or one spelling two: a host is lowercase ASCII letters, digits and
//! `-` in labels (an internationalized name written as its A-labels), or an
//! IP address written as the URL Standard writes one; a host the URL
//! Standard would read as an IPv4 address in another form (`0x7f.1`,
//! `127.1`, `2130706433`) is refused rather than read two ways; a port is a
//! number from 1 to 65535; credentials and fragments are refused.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::Error;

/// **The scheme an origin is reached by.** `Http` only for a development
/// origin (a [`crate::Grant::development`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scheme {
    Https,
    Http,
}

impl Scheme {
    fn default_port(self) -> u16 {
        match self {
            Scheme::Https => 443,
            Scheme::Http => 80,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Https => "https",
            Scheme::Http => "http",
        }
    }
}

/// **An origin's host**: a name, lowercase, or an IP address.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Host {
    Name(String),
    Ip(IpAddr),
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Host::Name(name) => f.write_str(name),
            Host::Ip(IpAddr::V4(a)) => write!(f, "{a}"),
            Host::Ip(IpAddr::V6(a)) => write!(f, "[{a}]"),
        }
    }
}

/// **An origin**: compared as its scheme, its host and its port, so
/// `https://API.example.com:443` and `https://api.example.com` are one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin {
    scheme: Scheme,
    host: Host,
    port: u16,
}

impl Origin {
    /// **`text` read as an origin**: `scheme://host[:port]`, a `/` after it
    /// at most.
    pub fn parse(text: &str) -> Result<Origin, Error> {
        let (origin, rest) = split(text)
            .map_err(|why| Error::Config(format!("`{}` is not an origin: {why}", shown(text))))?;
        if !(rest.is_empty() || rest == "/") {
            return Err(Error::Config(format!(
                "`{}` is not an origin: a scheme, a host and a port, and nothing after them",
                shown(text)
            )));
        }
        Ok(origin)
    }

    /// **A URL read as its origin and the target a request names**: its
    /// path and query (RFC 9112's origin-form), `/` where it names none. The
    /// URL is never quoted in the error: its query may hold what a log must
    /// not.
    pub(crate) fn split_url(url: &str) -> Result<(Origin, String), Error> {
        let (origin, rest) = split(url).map_err(|why| Error::Request(format!("the URL {why}")))?;
        let target = match rest.as_bytes().first() {
            None => "/".to_string(),
            Some(b'?') => format!("/{rest}"),
            Some(_) => rest.to_string(),
        };
        check_target(&target)?;
        Ok((origin, target))
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// **The authority a request's `Host` names**: the host, and the port
    /// where it is not the scheme's.
    pub(crate) fn authority(&self) -> String {
        if self.port == self.scheme.default_port() {
            self.host.to_string()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.scheme.as_str(), self.authority())
    }
}

/// **The origin of `url`**, as written canonically, or `None` where it
/// names none: what may be shown of a URL whose path or query may hold a
/// secret.
pub(crate) fn origin_of(url: &str) -> Option<String> {
    split(url).ok().map(|(origin, _)| origin.to_string())
}

/// An origin's text, shortened for a message: a grant's origin is the
/// deployment's configuration, never a secret, but may be long.
fn shown(text: &str) -> String {
    text.chars().take(80).collect()
}

/// **The origin a URL names, and what follows its authority.**
fn split(text: &str) -> Result<(Origin, &str), &'static str> {
    let (scheme, rest) = text.split_once("://").ok_or("names no scheme")?;
    let scheme = if scheme.eq_ignore_ascii_case("https") {
        Scheme::Https
    } else if scheme.eq_ignore_ascii_case("http") {
        Scheme::Http
    } else {
        return Err("names a scheme other than https or http");
    };
    let end = rest.find(['/', '?', '#', '\\']).unwrap_or(rest.len());
    let (authority, after) = rest.split_at(end);
    if after.contains('#') {
        return Err("holds a fragment, which is never sent");
    }
    if after.starts_with('\\') {
        return Err("holds a `\\` where its path begins");
    }
    if authority.contains('@') {
        return Err("holds credentials, which are never sent in a URL");
    }
    let (host, port) = host_and_port(authority)?;
    let port = port.unwrap_or(scheme.default_port());
    Ok((Origin { scheme, host, port }, after))
}

fn host_and_port(authority: &str) -> Result<(Host, Option<u16>), &'static str> {
    if authority.is_empty() {
        return Err("names no host");
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let (v6, after) = rest
            .split_once(']')
            .ok_or("holds an IPv6 address without its `]`")?;
        let address: Ipv6Addr = v6
            .parse()
            .map_err(|_| "holds an IPv6 address that does not read as one")?;
        let port = match after {
            "" => None,
            _ => Some(port_number(
                after
                    .strip_prefix(':')
                    .ok_or("holds something after its IPv6 address")?,
            )?),
        };
        return Ok((Host::Ip(IpAddr::V6(address)), port));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port_number(port)?)),
        None => (authority, None),
    };
    if host.contains(':') {
        return Err("holds an IPv6 address outside brackets");
    }
    Ok((host_named(host)?, port))
}

fn port_number(text: &str) -> Result<u16, &'static str> {
    const WHY: &str = "holds a port that is not a number from 1 to 65535";
    if text.is_empty() || text.len() > 5 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(WHY);
    }
    match text.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(WHY),
    }
}

fn host_named(text: &str) -> Result<Host, &'static str> {
    if !text.is_ascii() {
        return Err(
            "holds a name outside ASCII: an internationalized name is written as its A-labels (`xn--`)",
        );
    }
    let host = text.to_ascii_lowercase();
    // The URL Standard reads a host whose last label is a number as an IPv4
    // address, in any of several forms: only the four decimal numbers are
    // taken, so that no spelling is read two ways.
    let last = host.rsplit('.').next().unwrap_or("");
    let numeric = !last.is_empty()
        && (last.bytes().all(|b| b.is_ascii_digit())
            || last
                .strip_prefix("0x")
                .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit())));
    if numeric {
        return match host.parse::<Ipv4Addr>() {
            Ok(address) if address.to_string() == host => Ok(Host::Ip(IpAddr::V4(address))),
            _ => Err("holds an IPv4 address written other than as four decimal numbers"),
        };
    }
    if host.len() > 253 {
        return Err("holds a name longer than 253 characters");
    }
    for label in host.split('.') {
        if label.is_empty() {
            return Err("holds an empty label (a leading, trailing or doubled `.`)");
        }
        if label.len() > 63 {
            return Err("holds a label longer than 63 characters");
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err("holds a label beginning or ending with `-`");
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("holds a name of other than letters, digits and `-`");
        }
    }
    Ok(Host::Name(host))
}

/// **A target as RFC 9112's origin-form writes one**: `/`, then a path and
/// an optional query of URI characters (RFC 3986's unreserved, sub-delims,
/// `:`, `@`, `/`, `?` and `%` with two hex digits). A space, a control, a
/// line break, `#` or a character outside ASCII is refused, so no target
/// splits a request or names another.
fn check_target(target: &str) -> Result<(), Error> {
    let bytes = target.as_bytes();
    if bytes.first() != Some(&b'/') {
        return Err(Error::Request("the target does not begin with `/`".into()));
    }
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3);
                if !hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                    return Err(Error::Request(
                        "the path or query holds a `%` without two hex digits".into(),
                    ));
                }
                i += 3;
                continue;
            }
            b'a'..=b'z'
            | b'A'..=b'Z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'@'
            | b'/'
            | b'?' => {}
            _ => {
                return Err(Error::Request(
                    "the path or query holds a character a URI does not: a space, a control, \
                     a line break or one outside ASCII"
                        .into(),
                ));
            }
        }
        i += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(text: &str) -> Result<Origin, Error> {
        Origin::parse(text)
    }

    /// **One origin, however it is spelled**: the scheme and the name in any
    /// case, the default port written or not, a trailing `/`.
    #[test]
    fn an_origin_is_its_scheme_host_and_port() {
        let canonical = origin("https://api.provider.test");
        assert!(canonical.is_ok(), "{canonical:?}");
        for same in [
            "HTTPS://API.Provider.Test",
            "https://api.provider.test:443",
            "https://api.provider.test/",
        ] {
            assert_eq!(origin(same), canonical, "{same}");
        }
        let canonical = canonical.unwrap();
        assert_eq!(canonical.to_string(), "https://api.provider.test");
        assert_eq!(canonical.port(), 443);
        for other in [
            "https://api.provider.test:8443",
            "http://api.provider.test",
            "https://provider.test",
            "https://api.provider.test.example",
        ] {
            assert_ne!(origin(other), Ok(canonical.clone()), "{other}");
        }
        assert_eq!(
            origin("http://127.0.0.1:3301").map(|o| o.to_string()),
            Ok("http://127.0.0.1:3301".to_string())
        );
        assert_eq!(
            origin("https://[::1]:8443").map(|o| (o.to_string(), o.authority())),
            Ok(("https://[::1]:8443".to_string(), "[::1]:8443".to_string()))
        );
        assert_eq!(
            origin("http://localhost").map(|o| o.authority()),
            Ok("localhost".to_string())
        );
    }

    /// **What is not an origin is refused**, each for its reason, never
    /// read as some other origin.
    #[test]
    fn what_is_not_an_origin_is_refused() {
        for text in [
            "api.provider.test",
            "ftp://api.provider.test",
            "https://",
            "https://user:pass@api.provider.test",
            "https://api.provider.test/v1",
            "https://api.provider.test?x=1",
            "https://api.provider.test#top",
            "https://api.provider.test:",
            "https://api.provider.test:0",
            "https://api.provider.test:65536",
            "https://api.provider.test:443x",
            "https://api.provider.test.",
            "https://.provider.test",
            "https://api..provider.test",
            "https://-api.provider.test",
            "https://api-.provider.test",
            "https://api_1.provider.test",
            "https://bücher.test",
            "https://0x7f.1",
            "https://127.1",
            "https://2130706433",
            "https://01.2.3.4",
            "https://1.2.3.4.5",
            "https://::1",
            "https://[::1",
            "https://[fe80::1%25eth0]",
            "https://[::1]x",
            "https://api.provider.test\\@evil.test",
        ] {
            let read = origin(text);
            assert!(matches!(read, Err(Error::Config(_))), "{text}: {read:?}");
        }
        // Credentials and a fragment are refused as themselves.
        for (text, why) in [
            (
                "https://user:pass@api.provider.test",
                "holds credentials, which are never sent in a URL",
            ),
            (
                "https://user@api.provider.test",
                "holds credentials, which are never sent in a URL",
            ),
            (
                "https://api.provider.test/#top",
                "holds a fragment, which is never sent",
            ),
        ] {
            assert_eq!(
                origin(text),
                Err(Error::Config(format!("`{text}` is not an origin: {why}"))),
                "{text}"
            );
        }
        let long = format!("https://{}.test", "a".repeat(64));
        assert!(origin(&long).is_err(), "a label of 64");
        let ok = format!("https://{}.test", "a".repeat(63));
        assert!(origin(&ok).is_ok(), "a label of 63");
    }

    /// **A URL is its origin and its target**; a target is a path and a
    /// query of URI characters, and nothing that would split a request.
    #[test]
    fn a_url_is_its_origin_and_a_target_of_uri_characters() {
        let split = |url: &str| Origin::split_url(url).map(|(o, t)| (o.to_string(), t));
        assert_eq!(
            split("https://api.provider.test"),
            Ok(("https://api.provider.test".into(), "/".into()))
        );
        assert_eq!(
            split("https://api.provider.test?a=1"),
            Ok(("https://api.provider.test".into(), "/?a=1".into()))
        );
        assert_eq!(
            split("https://api.provider.test:8443/v1/payment_intents/pi_1/cancel?expand[]=x"),
            Err(Error::Request(
                "the path or query holds a character a URI does not: a space, a control, a line \
                 break or one outside ASCII"
                    .into()
            )),
            "`[` is not a URI character"
        );
        assert_eq!(
            split("https://api.provider.test:8443/v1/pi_1%2Fx?expand%5B%5D=x&b=c~d"),
            Ok((
                "https://api.provider.test:8443".into(),
                "/v1/pi_1%2Fx?expand%5B%5D=x&b=c~d".into()
            ))
        );
        for url in [
            "https://api.provider.test/a b",
            "https://api.provider.test/a\r\nx-injected: 1",
            "https://api.provider.test/a\tb",
            "https://api.provider.test/ä",
            "https://api.provider.test/%zz",
            "https://api.provider.test/%2",
            "https://api.provider.test/a#b",
            "https://api.provider.test/\"",
            "https://api.provider.test\\x",
            "https://user@api.provider.test/",
        ] {
            let read = split(url);
            assert!(matches!(read, Err(Error::Request(_))), "{url:?}: {read:?}");
        }
    }

    /// **A URL's error never quotes the URL**: its query may hold a key.
    #[test]
    fn a_urls_error_never_quotes_it() {
        for url in [
            "ftp://api.provider.test/?key=sk_test_hidden",
            "https://user:sk_test_hidden@api.provider.test/",
            "https://api.provider.test/?key=sk_test_hidden#x",
            "https://api.provider.test/?key=sk_test_hidden x",
        ] {
            let read = Origin::split_url(url);
            assert!(read.is_err(), "{url}");
            let said = format!("{:?} {}", read, read.clone().err().unwrap());
            assert!(!said.contains("sk_test_hidden"), "{said}");
        }
    }
}
