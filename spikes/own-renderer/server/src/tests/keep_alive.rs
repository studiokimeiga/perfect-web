//! The keep-alive ADR: **a host keeps a connection for its next request**
//! where HTTP/1.1 keeps it, and closes it where the request asks, where the
//! request's body was not read whole, where the answer's end is the
//! connection's (a stream), and past the host's cap. It frames a request's
//! body strictly, so that no request is read out of another's body, and
//! tells each commit to the readers it reached after its answer, not after
//! its connection closes.
//!
//! Each test talks to `handle` over a real connection, as a browser does, and
//! reads each answer by its `content-length`.

use super::*;
use std::time::{Duration, Instant};

/// Long enough that a connection found closed was closed by its answer, not
/// by the host's idle wait; longer than the client's read timeout.
const LONG: Duration = Duration::from_secs(30);

/// **A client connected to `s`**: `talk` writes to the connection and reads
/// its answers, while the host answers on its own thread, as it answers
/// each connection.
fn connected<T>(
    s: &Server,
    talk: impl FnOnce(&mut TcpStream, &mut BufReader<TcpStream>) -> T,
) -> T {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(s, stream);
        });
        let mut client = TcpStream::connect(at).expect("connect");
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a read timeout");
        let mut answers = BufReader::new(client.try_clone().expect("clone"));
        let said = talk(&mut client, &mut answers);
        let _ = client.shutdown(std::net::Shutdown::Both);
        said
    })
}

/// **One answer, read by its `content-length`**: its head and its body.
fn answer(answers: &mut BufReader<TcpStream>) -> (String, String) {
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if answers.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        head.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    let _ = answers.read_exact(&mut body);
    (head, String::from_utf8_lossy(&body).into_owned())
}

/// Whether the host has closed the connection: the next read finds its end,
/// within the client's read timeout.
fn closed(answers: &mut BufReader<TcpStream>) -> bool {
    matches!(answers.fill_buf(), Ok(bytes) if bytes.is_empty())
}

/// A `GET` of `path` by session `a`, with `more` headers.
fn get(path: &str, more: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: t\r\nCookie: pw-session=a\r\n{more}\r\n")
}

/// A post of the feed by session `a`, its body `body`, with `more` headers.
fn post(body: &str, more: &str) -> String {
    format!(
        "POST /command/feed.app.post HTTP/1.1\r\nHost: t\r\nCookie: pw-session=a\r\n\
         pw-interaction: kept-1\r\ncontent-type: application/json\r\n{more}\r\n{body}"
    )
}

/// A host of the feed whose kept connections wait [`LONG`] for their next
/// request.
fn kept_feed() -> Served {
    let mut s = served_feed();
    s.server.keep_alive = LONG;
    s
}

/// **A connection is kept for its next request**: two requests, one after
/// the other, each answered on one connection. The first answer says the
/// connection is kept, and for how long.
#[test]
fn a_connection_is_kept_for_its_next_request() {
    let s = kept_feed();
    let (first, second) = connected(&s, |client, answers| {
        client
            .write_all(get("/pw-handlers", "").as_bytes())
            .expect("first");
        let first = answer(answers);
        client
            .write_all(get("/pw-handlers", "").as_bytes())
            .expect("second");
        (first, answer(answers))
    });
    assert!(
        first.0.starts_with("HTTP/1.1 200 OK") && first.0.contains(KEEPS),
        "{}",
        first.0
    );
    assert!(second.0.starts_with("HTTP/1.1 200 OK"), "{}", second.0);
}

/// **A command's body read whole keeps its connection**: a post, then a
/// `GET` on the same connection, each answered.
#[test]
fn a_commands_body_read_whole_keeps_its_connection() {
    let s = kept_feed();
    let body = "[\"Kept.\"]";
    let request = post(body, &format!("content-length: {}\r\n", body.len()));
    let (posted, then) = connected(&s, |client, answers| {
        client.write_all(request.as_bytes()).expect("post");
        let posted = answer(answers);
        client
            .write_all(get("/pw-handlers", "").as_bytes())
            .expect("get");
        (posted, answer(answers))
    });
    assert!(
        posted.0.starts_with("HTTP/1.1 202") && posted.0.contains(KEEPS),
        "{}",
        posted.0
    );
    assert!(then.0.starts_with("HTTP/1.1 200 OK"), "{}", then.0);
}

/// **Requests sent together are answered in turn**: two requests in one
/// write, a file found and one not, each answered in its order.
#[test]
fn requests_sent_together_are_answered_in_turn() {
    let s = kept_feed();
    let both = format!(
        "{}{}",
        get("/pw-handlers", ""),
        get("/no-such-file.mjs", "")
    );
    let (first, second) = connected(&s, |client, answers| {
        client.write_all(both.as_bytes()).expect("both");
        (answer(answers), answer(answers))
    });
    assert!(first.0.starts_with("HTTP/1.1 200 OK"), "{}", first.0);
    assert!(second.0.starts_with("HTTP/1.1 404"), "{}", second.0);
}

/// **A request that asks to close is answered, then its connection closed**
/// (`connection: close`), well before the host's idle wait.
#[test]
fn a_request_that_asks_to_close_is_answered_and_closed() {
    let s = kept_feed();
    let (said, ended) = connected(&s, |client, answers| {
        client
            .write_all(get("/pw-handlers", "Connection: close\r\n").as_bytes())
            .expect("request");
        let said = answer(answers);
        (said, closed(answers))
    });
    assert!(said.0.contains("connection: close\r\n"), "{}", said.0);
    assert!(ended, "closed by its answer");
}

/// **An HTTP/1.0 request is answered, then its connection closed**: HTTP/1.0
/// keeps no connection unless asked, and this host keeps none for it.
#[test]
fn an_http_1_0_request_is_answered_and_closed() {
    let s = kept_feed();
    let (said, ended) = connected(&s, |client, answers| {
        client
            .write_all(b"GET /pw-handlers HTTP/1.0\r\nHost: t\r\nCookie: pw-session=a\r\n\r\n")
            .expect("request");
        let said = answer(answers);
        (said, closed(answers))
    });
    assert!(said.0.contains("connection: close\r\n"), "{}", said.0);
    assert!(ended, "closed by its answer");
}

/// **A kept connection is closed once it has waited its idle time** for a
/// next request that never comes.
#[test]
fn a_kept_connection_is_closed_after_its_idle_wait() {
    let mut s = served_feed();
    s.server.keep_alive = Duration::from_millis(300);
    let (said, waited, ended) = connected(&s, |client, answers| {
        client
            .write_all(get("/pw-handlers", "").as_bytes())
            .expect("request");
        let said = answer(answers);
        let begun = Instant::now();
        let ended = closed(answers);
        (said, begun.elapsed(), ended)
    });
    assert!(said.0.contains(KEEPS), "{}", said.0);
    assert!(
        ended && waited < Duration::from_secs(5),
        "closed after its idle wait: {ended}, waited {waited:?}"
    );
}

/// **A request framed by `transfer-encoding` is refused, and its connection
/// closed**: this host reads no chunked body, and a request hidden in one is
/// never answered (request smuggling).
#[test]
fn transfer_encoding_is_refused_and_its_connection_closed() {
    let s = kept_feed();
    let hidden = get("/pw-handlers", "");
    let request = post(
        &format!("0\r\n\r\n{hidden}"),
        "Transfer-Encoding: chunked\r\n",
    );
    let (said, ended) = connected(&s, |client, answers| {
        client.write_all(request.as_bytes()).expect("request");
        let said = answer(answers);
        (said, closed(answers))
    });
    assert!(
        said.0.starts_with("HTTP/1.1 501") && said.0.contains("connection: close\r\n"),
        "{}",
        said.0
    );
    assert!(ended, "the hidden request is never answered");
}

/// The answer to `request`, and whether its connection was then closed.
fn refused(request: &str) -> ((String, String), bool) {
    let s = kept_feed();
    connected(&s, |client, answers| {
        client.write_all(request.as_bytes()).expect("request");
        let said = answer(answers);
        (said, closed(answers))
    })
}

/// **Two `content-length`s are refused, and the connection closed**: which
/// one ends the body, and so where the next request begins, would be a
/// guess.
#[test]
fn two_content_lengths_are_refused_and_closed() {
    let ((head, _), ended) = refused(&post("[]", "content-length: 2\r\ncontent-length: 3\r\n"));
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    assert!(ended, "closed");
}

/// **A `content-length` that is no number is refused, and the connection
/// closed**.
#[test]
fn a_content_length_that_is_no_number_is_refused_and_closed() {
    let ((head, _), ended) = refused(&post("[]", "content-length: +2\r\n"));
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    assert!(ended, "closed");
}

/// **A header folded onto the line before is refused, and the connection
/// closed** (RFC 9112 §5.2).
#[test]
fn a_folded_header_is_refused_and_closed() {
    let ((head, _), ended) = refused(&get("/pw-handlers", "X-Note: a\r\n folded\r\n"));
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    assert!(ended, "closed");
}

/// **Whitespace between a header's name and its colon is refused, and the
/// connection closed** (RFC 9112 §5.1): `content-length : 2` read as no
/// length would leave the body to be read as the next request.
#[test]
fn whitespace_before_a_headers_colon_is_refused_and_closed() {
    let ((head, _), ended) = refused(&post("[]", "content-length : 2\r\n"));
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    assert!(ended, "closed");
}

/// **An answer written before its request's body is read closes the
/// connection**: a body larger than a command's bound is refused unread
/// (413), and what was sent of it is never read as a next request.
#[test]
fn an_answer_before_its_body_is_read_closes_the_connection() {
    let ((head, _), ended) = refused(&post("", "content-length: 70000\r\n"));
    assert!(
        head.starts_with("HTTP/1.1 413") && head.contains("connection: close\r\n"),
        "{head}"
    );
    assert!(ended, "closed");
}

/// **Past the host's cap, an answer closes its connection**: at
/// [`KEPT_AT_MOST`] connections open the answer keeps its connection, and
/// with one more open it closes it.
#[test]
fn past_the_cap_an_answer_closes_its_connection() {
    let s = kept_feed();
    let at = |others: usize| {
        s.server
            .open
            .store(others, std::sync::atomic::Ordering::SeqCst);
        connected(&s, |client, answers| {
            client
                .write_all(get("/pw-handlers", "").as_bytes())
                .expect("request");
            answer(answers).0
        })
    };
    let kept = at(KEPT_AT_MOST - 1);
    let past = at(KEPT_AT_MOST);
    assert!(kept.contains(KEEPS), "at the cap: {kept}");
    assert!(
        past.contains("connection: close\r\n"),
        "past the cap: {past}"
    );
}

/// **A stream's connection is closed with it** (`mode=stream`, the NDJSON
/// adapter): its body ends with the connection, so a request that may keep
/// its connection is not kept. (The poll's answer has a length, and keeps
/// it.)
#[test]
fn a_streams_connection_is_closed_with_it() {
    let s = kept_feed();
    let (said, ended) = connected(&s, |client, answers| {
        client
            .write_all(get("/stream?doc=1&since=1&mode=stream", "").as_bytes())
            .expect("request");
        let mut head = String::new();
        loop {
            let mut line = String::new();
            if answers.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        let mut rest = String::new();
        let _ = answers.read_to_string(&mut rest);
        (head, closed(answers))
    });
    assert!(said.contains("connection: close\r\n"), "{said}");
    assert!(ended, "closed with its stream");
}

/// **A request answered with nothing closes its connection**: a
/// subscription cut off (charter §15.5's forced reconnect, ADR-0175) is
/// closed with no answer, and the page subscribes again. Kept, the page
/// would wait for an answer that never comes.
#[test]
fn a_request_answered_with_nothing_closes_its_connection() {
    let s = kept_feed();
    s.connection_faults
        .lock()
        .expect("connection faults")
        .entry("a".into())
        .or_default()
        .cut_until = Some(Instant::now() + LONG);
    let (said, ended) = connected(&s, |client, answers| {
        client
            .write_all(get("/stream?doc=1&since=1", "").as_bytes())
            .expect("request");
        let said = answer(answers).0;
        (said, closed(answers))
    });
    assert!(said.is_empty(), "no answer: {said}");
    assert!(ended, "closed with no answer");
}

/// **Arming a dropped command closes the kept connections waiting**
/// (ADR-0175's one-shot network error): the session's next command then
/// goes on a connection of its own, and its drop reaches the page. On a kept
/// connection a browser would send the command again by itself, and the
/// page would never see the drop.
#[test]
fn arming_a_drop_closes_the_kept_connections_waiting() {
    let s = kept_feed();
    let (armed, ended) = connected(&s, |client, answers| {
        client
            .write_all(get("/pw-handlers", "").as_bytes())
            .expect("request");
        let _ = answer(answers);
        let armed = connected(&s, |arm, armed| {
            arm.write_all(
                b"POST /bench/drop?next=command&at=before HTTP/1.1\r\nHost: t\r\n\
                  Cookie: pw-session=a\r\ncontent-length: 0\r\nConnection: close\r\n\r\n",
            )
            .expect("arm");
            answer(armed).0
        });
        (armed, closed(answers))
    });
    assert!(armed.starts_with("HTTP/1.1 200"), "{armed}");
    assert!(ended, "the kept connection closed when the drop was armed");
}

/// **A commit is told to the readers it reached after its answer, while its
/// connection is kept** (ADR-0219): not when the connection closes, which a
/// kept connection may not do for seconds.
#[test]
fn a_commit_is_told_after_its_answer_while_its_connection_is_kept() {
    let s = kept_feed();
    s.telling
        .lock()
        .expect("telling")
        .push((String::new(), Vec::new()));
    let told = connected(&s, |client, answers| {
        client
            .write_all(get("/pw-handlers", "").as_bytes())
            .expect("request");
        let _ = answer(answers);
        // Told by the connection's thread just after the answer is written.
        let until = Instant::now() + Duration::from_secs(5);
        while !s.telling.lock().expect("telling").is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        s.telling.lock().expect("telling").is_empty()
    });
    assert!(told, "the telling taken while the connection is kept");
}

/// **The header that says how long a connection is kept says the host's
/// idle wait**.
#[test]
fn the_keep_alive_header_says_the_idle_wait() {
    assert!(
        KEEPS.contains(&format!(
            "keep-alive: timeout={}\r\n",
            KEEP_ALIVE_IDLE.as_secs()
        )),
        "{KEEPS}"
    );
}
