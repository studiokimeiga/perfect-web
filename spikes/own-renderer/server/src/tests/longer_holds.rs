//! ADR-0402: **a subscription is held until something happens,
//! up to [`HOLD`]**, a stream's and a long poll's alike. Held, its page is
//! live: each pass of the hold keeps its document within [`LIVE`], so a
//! commit's telling derives it. A page gone ends its hold at the next pass,
//! so it holds no thread for the rest of one.
//!
//! The hold of each test's host is set by the test (`poll_hold`,
//! `stream_hold`); every other test's host holds as hosts did before.

use super::*;
use std::time::{Duration, Instant};

/// A document of session `session` served by `s`, and a connection whose
/// browser end is `page` and whose host end is `held`, as the host holds a
/// connection (ADR-0401's [`Conn`]).
fn subscribed(s: &Server, session: &str) -> (u64, TcpStream, Conn) {
    let (_, document) = s.serve_document(session);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let page = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
    let (held, _) = listener.accept().expect("accept");
    (document, page, Conn::of(held))
}

/// `doc`'s page last asked longer ago than [`LIVE`]: a test host's served
/// document is live for an hour (`Subscriber::asked_at`), so the hold's own
/// look is what makes it live again.
fn aged(s: &Server, doc: &Doc) {
    s.pending
        .lock()
        .expect("pending")
        .get_mut(doc)
        .expect("subscribed")
        .seen = Instant::now() - LIVE - Duration::from_secs(1);
}

/// **An idle long poll is held its whole hold**, and is not taken for a page
/// gone: its browser holds the connection open and sends nothing.
#[test]
fn an_idle_poll_is_held_its_whole_hold() {
    let mut s = rendering_server();
    s.poll_hold = Duration::from_millis(1500);
    let (document, _page, mut held) = subscribed(&s, "idle");
    let begun = Instant::now();
    stream_frames(
        &s,
        &mut held,
        "idle",
        false,
        &format!("doc={document}&since={document}"),
    );
    let held_for = begun.elapsed();
    assert!(held_for >= s.poll_hold, "held its whole hold: {held_for:?}");
}

/// **A held long poll keeps its page live**: past [`LIVE`] since its page
/// last asked, the document is live while its poll is held, so a commit's
/// telling derives it (ADR-0297).
#[test]
fn a_held_poll_keeps_its_page_live() {
    let mut s = rendering_server();
    s.poll_hold = LIVE + Duration::from_secs(2);
    let (document, _page, mut held) = subscribed(&s, "live");
    let doc: Doc = ("live".to_string(), document);
    aged(&s, &doc);
    let query = format!("doc={document}&since={document}");
    let live = std::thread::scope(|scope| {
        let s = &s;
        scope.spawn(move || stream_frames(s, &mut held, "live", false, &query));
        std::thread::sleep(LIVE + Duration::from_secs(1));
        s.pending.lock().expect("pending")[&doc].live(Instant::now())
    });
    assert!(live, "live past LIVE while its poll is held");
}

/// **A held stream keeps its page live**, as a held long poll does.
#[test]
fn a_held_stream_keeps_its_page_live() {
    let mut s = rendering_server();
    s.stream_hold = LIVE + Duration::from_secs(2);
    let (document, _page, mut held) = subscribed(&s, "streamed");
    let doc: Doc = ("streamed".to_string(), document);
    aged(&s, &doc);
    let live = std::thread::scope(|scope| {
        let s = &s;
        scope.spawn(move || stream_open(s, &mut held, "streamed", false, document, document));
        std::thread::sleep(LIVE + Duration::from_secs(1));
        s.pending.lock().expect("pending")[&doc].live(Instant::now())
    });
    assert!(live, "live past LIVE while its stream is held");
}

/// How long a hold of [`HOLD`] lasted when its page went away
/// `after` it began, `hold` running it on the host's end of the connection.
fn held_after_gone(hold: impl FnOnce(Conn) + Send, page: TcpStream, held: Conn) -> Duration {
    std::thread::scope(|scope| {
        let begun = Instant::now();
        let holding = scope.spawn(move || hold(held));
        std::thread::sleep(Duration::from_millis(300));
        drop(page);
        let _ = holding.join();
        begun.elapsed()
    })
}

/// **A long poll whose page has gone ends its hold**, at its next pass,
/// rather than holding a thread for the rest of [`HOLD`].
#[test]
fn a_polls_page_gone_ends_its_hold() {
    let mut s = rendering_server();
    s.poll_hold = HOLD;
    let (document, page, held) = subscribed(&s, "left");
    let query = format!("doc={document}&since={document}");
    let s = &s;
    let lasted = held_after_gone(
        move |mut held| stream_frames(s, &mut held, "left", false, &query),
        page,
        held,
    );
    assert!(
        lasted < Duration::from_secs(5),
        "ended once its page went: {lasted:?}"
    );
}

/// **A stream whose page has gone ends its hold**, as a long poll does.
#[test]
fn a_streams_page_gone_ends_its_hold() {
    let mut s = rendering_server();
    s.stream_hold = HOLD;
    let (document, page, held) = subscribed(&s, "gone");
    let s = &s;
    let lasted = held_after_gone(
        move |mut held| stream_open(s, &mut held, "gone", false, document, document),
        page,
        held,
    );
    assert!(
        lasted < Duration::from_secs(5),
        "ended once its page went: {lasted:?}"
    );
}

/// **A host holds a subscription 25 s when nothing happens**, under the
/// 30 s a hosting router allows before an answer's first byte, and longer
/// than [`LIVE`], which each pass of a hold renews.
#[test]
fn a_host_holds_a_subscription_twenty_five_seconds() {
    assert_eq!(HOLD, Duration::from_secs(25));
    assert!(HOLD < Duration::from_secs(30) && HOLD > LIVE, "{HOLD:?}");
}
