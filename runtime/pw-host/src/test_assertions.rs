//! Assertions about domain results; setup I/O keeps ordinary expect/panic paths.
#![allow(dead_code)]

pub(crate) trait AssertOk<T> {
    fn assert_ok(self, message: &str) -> T;
}
pub(crate) trait AssertErr<E> {
    fn assert_err(self, message: &str) -> E;
}
impl<T, E> AssertOk<T> for Result<T, E> {
    fn assert_ok(self, message: &str) -> T {
        assert!(self.is_ok(), "{message}");
        match self {
            Ok(value) => value,
            Err(_) => unreachable!(),
        }
    }
}
impl<T, E> AssertErr<E> for Result<T, E> {
    fn assert_err(self, message: &str) -> E {
        assert!(self.is_err(), "{message}");
        match self {
            Err(error) => error,
            Ok(_) => unreachable!(),
        }
    }
}
impl<T> AssertOk<T> for Option<T> {
    fn assert_ok(self, message: &str) -> T {
        assert!(self.is_some(), "{message}");
        match self {
            Some(value) => value,
            None => unreachable!(),
        }
    }
}
