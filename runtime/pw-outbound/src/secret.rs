//! **A secret header's value**: the host's, written to the connection and
//! nowhere else.

use std::fmt;

/// **A value the host holds and no program does** (an API key, a bearer
/// token, a client secret). Its `Debug` says only that it is one; it has no
/// `Display`; and no code outside this crate can read it back, so no host
/// code can hand it to a program, put it in a `Val`, or format it into an
/// error or a log line. The client writes it into the request's head, and
/// that is the one place it goes.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

    /// The value, for the request's head alone.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}
