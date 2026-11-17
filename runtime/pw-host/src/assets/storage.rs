//! Typed physical storage boundary; a namespace marker is not app authority.
use super::Descriptor;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Namespace(String);
impl Namespace {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == 32 && text.bytes().all(|b| matches!(b,b'0'..=b'9'|b'a'..=b'f')))
            .then(|| Self(text.into()))
    }
    pub fn hex(&self) -> &str {
        &self.0
    }
}
/// The trusted adapter owns database liveness and namespace enrollment. Storage
/// never adopts a plain Ref or follows a mutable hash-to-current pointer.
pub trait Storage: Send + Sync {
    /// Fresh enrollment is exclusive; existing enrollment only verifies the
    /// retained marker. A missing existing marker must never be recreated.
    fn enroll(&self, namespace: &Namespace, fresh: bool) -> io::Result<()>;
    /// **`namespace`'s owner, recorded or held to** (L5a's review,
    /// 2026-11-16: a copy of a database pointed at its original's blob root
    /// would collect bytes the original still serves). A fresh enrollment
    /// records `owner`, the identity of the database that records the
    /// namespace. An existing one must name the same owner, and one that
    /// names none is refused, as a missing marker is. Storage that keeps
    /// nothing beyond its process (memory) has no other database to tell
    /// apart, and answers Ok.
    fn own(&self, _namespace: &Namespace, _owner: &str, _fresh: bool) -> io::Result<()> {
        Ok(())
    }
    /// Reserve a NEVER-used generation then keep exact canonical bytes. Failure
    /// may leave an orphan reservation; reuse must still refuse.
    fn keep_new(
        &self,
        namespace: &Namespace,
        descriptor: &Descriptor,
        bytes: &[u8],
    ) -> io::Result<()>;
    /// Validate the immutable descriptor and exact bytes. None means missing
    /// payload; the authoritative HTTP resolver decides liveness/404 vs503.
    fn get_generation(
        &self,
        namespace: &Namespace,
        descriptor: &Descriptor,
    ) -> io::Result<Option<Vec<u8>>>;
    /// Delete only this captured payload, retaining its identity reservation.
    fn collect_generation(&self, namespace: &Namespace, descriptor: &Descriptor) -> io::Result<()>;
}
