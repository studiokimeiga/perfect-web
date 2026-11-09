//! The longer-holds ADR: **a subscription whose page asks is held until
//! something happens, up to [`HOLD`]**, a stream's and a long poll's alike.
//! Held, its page is live: each pass of the hold keeps its document within
//! [`LIVE`], so a commit's telling derives it. A page gone ends its hold at the
//! next pass, so it holds no thread for the rest of one. A regeneration that
//! failed is tried again within it, and its queue does not fill.
//!
//! Each test sets the hold it runs (`long_hold`, `poll_hold`, `stream_hold`).

use super::*;
use std::time::{Duration, Instant};

/// A document of session `session` served by `s`, and a connection whose
/// browser end is `page` and whose host end is `held`.
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
    s.long_hold = Duration::from_millis(1500);
    let (document, _page, mut held) = subscribed(&s, "idle");
    let begun = Instant::now();
    stream_frames(
        &s,
        &mut held,
        "idle",
        false,
        &format!("doc={document}&since={document}&hold=long"),
    );
    let held_for = begun.elapsed();
    assert!(held_for >= s.long_hold, "held its whole hold: {held_for:?}");
}

/// **A poll that does not ask is held as briefly as any page's was**: its
/// browser may have five more tabs of this origin.
#[test]
fn a_poll_that_does_not_ask_is_not_held_long() {
    let mut s = rendering_server();
    s.poll_hold = Duration::from_millis(100);
    s.long_hold = Duration::from_secs(20);
    let (document, _page, mut held) = subscribed(&s, "brief");
    let begun = Instant::now();
    stream_frames(
        &s,
        &mut held,
        "brief",
        false,
        &format!("doc={document}&since={document}"),
    );
    assert!(begun.elapsed() < Duration::from_secs(5));
}

/// **A stream that does not ask is held as briefly too.**
#[test]
fn a_stream_that_does_not_ask_is_not_held_long() {
    let mut s = rendering_server();
    s.stream_hold = Duration::from_millis(100);
    s.long_hold = Duration::from_secs(20);
    let (document, _page, mut held) = subscribed(&s, "briefly");
    let begun = Instant::now();
    stream_frames(
        &s,
        &mut held,
        "briefly",
        false,
        &format!("mode=stream&doc={document}&since={document}"),
    );
    assert!(begun.elapsed() < Duration::from_secs(5));
}

/// **A host holds so many long, and any more as briefly as any page**: each
/// is a thread, and the asking is a request's word.
#[test]
fn a_poll_past_the_limit_is_held_as_any_page_is() {
    let mut s = rendering_server();
    s.poll_hold = Duration::from_millis(100);
    s.long_hold = Duration::from_secs(20);
    s.long_hold_limit = 1;
    let first = s.hold_long().expect("the first place");
    assert!(s.hold_long().is_none(), "no second place");
    let (document, _page, mut held) = subscribed(&s, "past");
    let begun = Instant::now();
    stream_frames(
        &s,
        &mut held,
        "past",
        false,
        &format!("doc={document}&since={document}&hold=long"),
    );
    assert!(begun.elapsed() < Duration::from_secs(5));
    drop(first);
    assert!(s.hold_long().is_some(), "given back, its place is free");
}

/// **A hold gives its place back when it ends.**
#[test]
fn a_long_hold_gives_its_place_back() {
    let mut s = rendering_server();
    s.long_hold = Duration::from_millis(50);
    s.long_hold_limit = 1;
    let (document, _page, mut held) = subscribed(&s, "back");
    stream_frames(
        &s,
        &mut held,
        "back",
        false,
        &format!("doc={document}&since={document}&hold=long"),
    );
    assert!(s.hold_long().is_some(), "its place is free");
}

/// **A stream that asks is held long.**
#[test]
fn a_stream_that_asks_is_held_long() {
    let mut s = rendering_server();
    s.stream_hold = Duration::from_millis(100);
    s.long_hold = Duration::from_millis(1500);
    let (document, _page, mut held) = subscribed(&s, "long");
    let begun = Instant::now();
    stream_frames(
        &s,
        &mut held,
        "long",
        false,
        &format!("mode=stream&doc={document}&since={document}&hold=long"),
    );
    let held_for = begun.elapsed();
    assert!(held_for >= s.long_hold, "held its whole hold: {held_for:?}");
}

/// **A regeneration that failed is tried again within a long hold** (ADR-0176):
/// the failure its page would have waited out until its next request is 25 s
/// away, and the page sees the change within the hold.
#[test]
fn a_regeneration_that_failed_is_tried_again_within_a_hold() {
    let mut s = speculating_server();
    s.long_hold = Duration::from_secs(20);
    s.retried_every = Duration::from_millis(50);
    let session = "failing";
    s.serve_document_with_entries(session).expect("served");
    let arm = |n: &str| {
        let armed = posted(&s, "/bench/materializer?fail=next", session, n, "");
        assert!(armed.starts_with("HTTP/1.1 200"), "{armed}");
    };
    arm("arm-1");
    s.command_json(
        ADD,
        session,
        &[shown("espresso"), serde_json::json!(1)],
        Some("press-1"),
    )
    .expect("well-formed")
    .expect("committed");
    // The poll's own drain fails again: only a retry inside the hold can
    // answer it.
    arm("arm-2");
    let document = {
        let queue = s.pending.lock().expect("pending");
        latest(&queue, session).1
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let _page = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
    let mut held = Conn::of(listener.accept().expect("accept").0);
    let begun = Instant::now();
    stream_frames(
        &s,
        &mut held,
        session,
        false,
        &format!("doc={document}&since={document}&hold=long"),
    );
    assert!(
        begun.elapsed() < Duration::from_secs(10),
        "answered within the hold: {:?}",
        begun.elapsed()
    );
}

/// **And within a held stream**, whose page sees it the same way.
#[test]
fn a_regeneration_that_failed_is_tried_again_within_a_held_stream() {
    let mut s = speculating_server();
    s.long_hold = Duration::from_millis(1500);
    s.retried_every = Duration::from_millis(50);
    let session = "failing-stream";
    s.serve_document_with_entries(session).expect("served");
    let arm = |n: &str| {
        let armed = posted(&s, "/bench/materializer?fail=next", session, n, "");
        assert!(armed.starts_with("HTTP/1.1 200"), "{armed}");
    };
    arm("arm-1");
    s.command_json(
        ADD,
        session,
        &[shown("espresso"), serde_json::json!(1)],
        Some("press-1"),
    )
    .expect("well-formed")
    .expect("committed");
    arm("arm-2");
    let doc = {
        let queue = s.pending.lock().expect("pending");
        latest(&queue, session)
    };
    let document = doc.1;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let _page = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
    let mut held = Conn::of(listener.accept().expect("accept").0);
    stream_frames(
        &s,
        &mut held,
        session,
        false,
        &format!("mode=stream&doc={document}&since={document}&hold=long"),
    );
    let queue = s.pending.lock().expect("pending");
    let values = queue[&doc]
        .frames
        .iter()
        .filter(|(_, f)| matches!(f, StreamFrame::EntryValue { .. }))
        .count();
    assert_eq!(values, 1, "the value the failure held back");
}

/// **A stream held long ends before what it wrote fills its queue**: the page
/// acknowledges by asking again, and a page that never does would be told to
/// reload at [`MAX_WAITING`].
#[test]
fn a_stream_held_long_ends_before_its_queue_fills() {
    let mut s = rendering_server();
    s.long_hold = Duration::from_secs(8);
    let (document, _page, mut held) = subscribed(&s, "busy");
    let doc: Doc = ("busy".to_string(), document);
    let begun = Instant::now();
    std::thread::scope(|scope| {
        let s = &s;
        let held = &mut held;
        let holding = scope.spawn(move || {
            stream_frames(
                s,
                held,
                "busy",
                false,
                &format!("mode=stream&doc={document}&since={document}&hold=long"),
            );
        });
        std::thread::sleep(Duration::from_millis(100));
        {
            let mut queue = s.pending.lock().expect("pending");
            let waiting = queue.get_mut(&doc).expect("subscribed");
            for _ in 0..MAX_WAITING / 2 {
                waiting.push(StreamFrame::Recovery {
                    protocol: CURRENT,
                    recovery: pw_protocol::Recovery::Reload,
                });
            }
        }
        s.pending.wake();
        let _ = holding.join();
    });
    assert!(begun.elapsed() < Duration::from_secs(5), "{:?}", begun.elapsed());
    let queue = s.pending.lock().expect("pending");
    assert!(!queue[&doc].behind, "not told to reload");
}

/// **A held long poll keeps its page live**: past [`LIVE`] since its page
/// last asked, the document is live while its poll is held, so a commit's
/// telling derives it (ADR-0297).
#[test]
fn a_held_poll_keeps_its_page_live() {
    let mut s = rendering_server();
    s.long_hold = LIVE + Duration::from_secs(2);
    let (document, _page, mut held) = subscribed(&s, "live");
    let doc: Doc = ("live".to_string(), document);
    aged(&s, &doc);
    let query = format!("doc={document}&since={document}&hold=long");
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
    s.long_hold = LIVE + Duration::from_secs(2);
    let (document, _page, mut held) = subscribed(&s, "streamed");
    let doc: Doc = ("streamed".to_string(), document);
    aged(&s, &doc);
    let live = std::thread::scope(|scope| {
        let s = &s;
        let query = format!("mode=stream&doc={document}&since={document}&hold=long");
        scope.spawn(move || stream_frames(s, &mut held, "streamed", false, &query));
        std::thread::sleep(LIVE + Duration::from_secs(1));
        s.pending.lock().expect("pending")[&doc].live(Instant::now())
    });
    assert!(live, "live past LIVE while its stream is held");
}

/// How long a hold of [`HOLD`] lasted when its page went away
/// `after` it began, `hold` running it on the host's end of the connection.
fn held_after_gone(
    hold: impl FnOnce(Conn) + Send,
    page: TcpStream,
    held: Conn,
) -> Duration {
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
    let s = rendering_server();
    let (document, page, held) = subscribed(&s, "left");
    let query = format!("doc={document}&since={document}&hold=long");
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
    let s = rendering_server();
    let (document, page, held) = subscribed(&s, "gone");
    let s = &s;
    let query = format!("mode=stream&doc={document}&since={document}&hold=long");
    let lasted = held_after_gone(
        move |mut held| stream_frames(s, &mut held, "gone", false, &query),
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
