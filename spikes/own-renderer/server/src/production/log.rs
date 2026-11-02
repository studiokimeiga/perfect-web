//! **What the host says of itself** (track `deploy`): in a production
//! deployment, one JSON object a line on standard error, which journald, a
//! container's runtime or a platform's log router reads field by field; in
//! development, the lines the server always wrote, unchanged.
//!
//! Every line has `ts` (RFC 3339, UTC, milliseconds), `level`, `event` (a
//! fixed word a query can select on) and `msg`; a request's line adds its
//! method, its route without its query (which may hold what a reader
//! searched for), its status and how long it took. No line holds a secret,
//! a cookie or a body: the callers give text the server already printed.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Whether this process writes JSON lines: set once, by `main`, for a
/// production deployment.
static JSON: OnceLock<bool> = OnceLock::new();

/// **This process's lines are JSON from now on.**
pub(crate) fn use_json() {
    let _ = JSON.set(true);
}

fn json() -> bool {
    *JSON.get().unwrap_or(&false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn word(self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// **An error the host lives through, said** as `eprintln!` said it in
/// development, and as one JSON line in production: the server's own
/// error lines, each in place of the `eprintln!` it was.
macro_rules! error_line {
    ($($arg:tt)*) => {
        $crate::production::log::error("error", &format!($($arg)*))
    };
}
pub(crate) use error_line;

/// **A panic, said as a line**: a request's thread that panics is one
/// request lost, and what it says goes where every other line goes, as
/// JSON, not as Rust's default text.
pub(crate) fn panics_as_lines() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("a request's").to_string();
        emit(
            Level::Error,
            "panic",
            &format!("pw dev server: {name} thread panicked: {info}"),
            &[],
        );
    }));
}

/// **A trace event** (`PW_TRACE`): on standard error in development, as it
/// was, and a `debug` line in production.
pub(crate) fn trace(message: &str) {
    if json() {
        emit(Level::Debug, "trace", message, &[]);
    } else {
        eprintln!("{message}");
    }
}

/// **A request, said when it is answered** (production only): created
/// as its line is read, and said as it is dropped, whichever way the
/// answer returned.
pub(crate) struct Logged {
    method: String,
    route: String,
    started: std::time::Instant,
}

impl Logged {
    pub(crate) fn new(method: &str, path: &str) -> Logged {
        // Its route without its query, which may hold what a reader
        // searched for; and bounded, as a line in a log should be.
        let route = path.split('?').next().unwrap_or(path);
        let route: String = route.chars().take(512).collect();
        let method: String = method.chars().take(16).collect();
        Logged {
            method,
            route,
            started: std::time::Instant::now(),
        }
    }
}

impl Drop for Logged {
    fn drop(&mut self) {
        request(
            &self.method,
            &self.route,
            take_status(),
            self.started.elapsed().as_millis(),
        );
    }
}

/// Something the host did, said.
pub(crate) fn info(event: &str, message: &str) {
    emit(Level::Info, event, message, &[]);
}

/// Something that went wrong and that the host lives through.
pub(crate) fn warn(event: &str, message: &str) {
    emit(Level::Warn, event, message, &[]);
}

/// Something that went wrong.
pub(crate) fn error(event: &str, message: &str) {
    emit(Level::Error, event, message, &[]);
}

/// **A line, said**: as JSON on standard error in production; as the text
/// alone in development, what is said of a start on standard output and
/// anything else on standard error, where the development server always
/// wrote them.
pub(crate) fn emit(level: Level, event: &str, message: &str, fields: &[(&str, serde_json::Value)]) {
    if json() {
        eprintln!("{}", line(now_ms(), level, event, message, fields));
    } else if level == Level::Info {
        println!("{message}");
    } else {
        eprintln!("{message}");
    }
}

/// **A request answered**, in production: its method, its route, its
/// status (none where nothing was written) and how long it took. A page's
/// subscription asks again within a second all the while it is open, and a
/// platform asks for health every few seconds: those are said only where
/// they failed.
pub(crate) fn request(method: &str, route: &str, status: Option<u16>, ms: u128) {
    if !json() {
        return;
    }
    if !said(route, status) {
        return;
    }
    let level = match status {
        Some(s) if s >= 500 => Level::Error,
        _ => Level::Info,
    };
    let fields = [
        ("method", serde_json::Value::from(method)),
        ("route", serde_json::Value::from(route)),
        (
            "status",
            status.map_or(serde_json::Value::Null, serde_json::Value::from),
        ),
        (
            "ms",
            serde_json::Value::from(u64::try_from(ms).unwrap_or(u64::MAX)),
        ),
    ];
    eprintln!("{}", line(now_ms(), level, "request", "", &fields));
}

/// Whether a request's line is said: every one but a subscription's and a
/// health check's, unless it failed.
pub(crate) fn said(route: &str, status: Option<u16>) -> bool {
    let routine = matches!(route, "/stream" | "/_pw/health" | "/_pw/ready");
    !routine || status.is_none_or(|s| s >= 500)
}

thread_local! {
    /// The status of the answer this thread's request was given: a
    /// connection is one request and one thread (`main.rs`'s `handle`).
    static STATUS: std::cell::Cell<Option<u16>> = const { std::cell::Cell::new(None) };
}

/// **The status the request on this thread was answered with**, as a
/// response head is written.
pub(crate) fn answered(status: u16) {
    STATUS.with(|s| s.set(Some(status)));
}

/// The status this thread's request was answered with, and forgotten.
pub(crate) fn take_status() -> Option<u16> {
    STATUS.with(|s| s.take())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// **One line, as JSON**: its time, level, event and message, then each
/// field. serde_json writes the escapes, so no text can end the line or the
/// object early.
pub(crate) fn line(
    at_ms: i64,
    level: Level,
    event: &str,
    message: &str,
    fields: &[(&str, serde_json::Value)],
) -> String {
    let mut object = serde_json::Map::new();
    object.insert("ts".into(), rfc3339(at_ms).into());
    object.insert("level".into(), level.word().into());
    object.insert("event".into(), event.into());
    if !message.is_empty() {
        object.insert("msg".into(), message.into());
    }
    for (name, value) in fields {
        object.insert((*name).to_string(), value.clone());
    }
    serde_json::Value::Object(object).to_string()
}

/// **An instant as RFC 3339 says it, in UTC**, to the millisecond:
/// `1970-01-01T00:00:00.000Z`. The civil date from the day count is Howard
/// Hinnant's `civil_from_days` (proleptic Gregorian, any day).
pub(crate) fn rfc3339(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let in_day = ms.rem_euclid(86_400_000);
    let (hour, minute, second, milli) = (
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1000 % 60,
        in_day % 1000,
    );
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milli:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **An instant is written as RFC 3339 says, in UTC**: the epoch, a
    /// leap day, a year's last millisecond, and one before the epoch.
    #[test]
    fn an_instant_is_written_as_rfc_3339_says() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(rfc3339(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
        assert_eq!(rfc3339(1_735_689_599_999), "2024-12-31T23:59:59.999Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(rfc3339(-1), "1969-12-31T23:59:59.999Z");
    }

    /// **A line is one JSON object**, whatever its text holds: a line
    /// break, a quote or a control is escaped, and the line is one line.
    #[test]
    fn a_line_is_one_json_object() {
        let said = line(
            0,
            Level::Error,
            "start",
            "a \"quoted\"\nsecond line\u{7}",
            &[("route", "/x".into()), ("status", 503.into())],
        );
        assert!(!said.contains('\n'), "{said}");
        let read: serde_json::Value = serde_json::from_str(&said).expect("JSON");
        assert_eq!(read["ts"], "1970-01-01T00:00:00.000Z");
        assert_eq!(read["level"], "error");
        assert_eq!(read["event"], "start");
        assert_eq!(read["msg"], "a \"quoted\"\nsecond line\u{7}");
        assert_eq!(read["route"], "/x");
        assert_eq!(read["status"], 503);
    }

    /// **A subscription's and a health check's requests are said only
    /// where they failed**; every other request is said.
    #[test]
    fn routine_requests_are_said_only_where_they_failed() {
        for route in ["/stream", "/_pw/health", "/_pw/ready"] {
            assert!(!said(route, Some(200)), "{route}");
            assert!(!said(route, Some(404)), "{route}");
            assert!(said(route, Some(503)), "{route}");
            assert!(said(route, None), "{route}");
        }
        assert!(said("/stores/47", Some(200)));
        assert!(said("/bench/clock", Some(404)));
    }

    /// **A thread's status is its request's**, taken once.
    #[test]
    fn a_threads_status_is_its_requests_taken_once() {
        answered(304);
        assert_eq!(take_status(), Some(304));
        assert_eq!(take_status(), None);
        std::thread::spawn(|| assert_eq!(take_status(), None))
            .join()
            .expect("joined");
    }
}
