//! One generic invocation's actual staged leases and explicit commit outcome.
//! Current caller/final fences and authoritative receipt inspection belong to
//! the actual adapter bridge. No receipt absence is rollback evidence here.
use super::*;
use crate::identity::ReaderIdentity;
use pw_host::assets::{Descriptor, Reference};

const PENDING_COUNT: usize = 1024;
const PENDING_PROOF_BYTES: usize = 16 * 1024 * 1024;
const ATTEMPT_FIELD: &str = "upload_attempt";

#[derive(Clone)]
struct Proof {
    key: String,
    sent: String,
    answer: String,
    at_ms: i64,
}
impl Proof {
    fn bytes(&self) -> usize {
        self.key
            .len()
            .saturating_add(self.sent.len())
            .saturating_add(self.answer.len())
    }
}
/// An inspection must come from the actual receipt under the adapter's
/// reacquired lifetime. This type grants no browser/program authority.
pub(crate) struct Receipt {
    pub key: String,
    pub sent: String,
    pub answer: String,
    pub at_ms: i64,
}
#[derive(Clone)]
pub(crate) struct Ticket {
    attempt: String,
    identity: Arc<()>,
    proof: Arc<Proof>,
}
impl Ticket {
    pub(crate) fn key(&self) -> &str {
        &self.proof.key
    }
}
impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingUpload").finish_non_exhaustive()
    }
}
pub(super) struct Pending {
    ticket: Ticket,
    claimed: Vec<Claimed>,
}
#[derive(Clone)]
pub(crate) struct Payload {
    pub declaration: String,
    pub descriptor: Descriptor,
    pub bytes: Arc<[u8]>,
}
enum Fate {
    Held,
    Published,
    Discarded,
}
struct Held {
    token: String,
    claimed: Claimed,
    fate: Fate,
}
enum Phase {
    Open,
    Pending(Ticket),
    Finished,
}
pub(crate) struct Invocation {
    leases: Leases,
    asked: crate::data::Requested,
    reader: ReaderIdentity,
    attempt: String,
    held: BTreeMap<String, Held>,
    payloads: BTreeMap<String, Payload>,
    phase: Phase,
}
fn opaque() -> String {
    crate::identity::hex(&crate::identity::random_bytes::<16>())
}
impl Invocation {
    /// The caller supplies the admitted immutable reader and retains its root
    /// and final authority fence. No principal cache/source is recaptured here.
    pub(crate) fn new(leases: Leases, session: &str, reader: ReaderIdentity) -> Self {
        Self {
            leases,
            asked: crate::data::Requested::by(session),
            reader,
            attempt: opaque(),
            held: BTreeMap::new(),
            payloads: BTreeMap::new(),
            phase: Phase::Open,
        }
    }
    fn open(&self) -> Result<(), String> {
        if matches!(self.phase, Phase::Open) {
            Ok(())
        } else {
            Err("this upload invocation has ended its resource operations".into())
        }
    }
    pub(crate) fn acquired(&self) -> bool {
        !self.held.is_empty() || matches!(self.phase, Phase::Pending(_))
    }
    pub(crate) fn claim(
        &mut self,
        declaration: &str,
        session: &str,
    ) -> Result<Option<String>, String> {
        self.open()?;
        let cookie = self.asked.session("upload.claim", session)?;
        if self.held.contains_key(declaration) {
            return Ok(None);
        }
        let Some(claimed) = self.leases.claim_for(cookie, declaration, &self.reader) else {
            return Ok(None);
        };
        let token = opaque();
        self.held.insert(
            declaration.into(),
            Held {
                token: token.clone(),
                claimed,
                fate: Fate::Held,
            },
        );
        Ok(Some(token))
    }
    pub(crate) fn publish(
        &mut self,
        declaration: &str,
        session: &str,
        token: &str,
        propose: impl FnOnce(&str, Descriptor, Arc<[u8]>) -> Result<Reference, String>,
    ) -> Result<Option<Reference>, String> {
        self.open()?;
        self.asked.session("upload.publish", session)?;
        let Some(held) = self
            .held
            .get(declaration)
            .filter(|h| h.token == token && matches!(h.fate, Fate::Held))
        else {
            return Ok(None);
        };
        let claimed = &held.claimed;
        let bytes = {
            let state = self
                .leases
                .state
                .lock()
                .map_err(|_| "upload staging is unavailable")?;
            let lease = state
                .leases
                .get(&lease_key(&claimed.session, &claimed.declaration))
                .filter(|l| {
                    l.claimed
                        && l.id == claimed.id
                        && l.reader == claimed.reader
                        && l.key == claimed.key
                })
                .ok_or("the claimed upload lease changed")?;
            if lease.kind != claimed.kind
                || lease.width != claimed.width
                || lease.height != claimed.height
            {
                return Err("the claimed upload descriptor changed".into());
            }
            // No storage operation holds the shared lease-state mutex.
            claimed.key.clone()
        };
        let bytes = self
            .leases
            .staged
            .get(&bytes)
            .map_err(|_| "the staged upload could not be read")?
            .ok_or("the staged upload has no bytes")?;
        let measured =
            sniff::measure(&bytes).map_err(|_| "the staged upload is not its canonical image")?;
        if BlobKey::of(&bytes) != claimed.key
            || measured.kind != claimed.kind
            || measured.width != claimed.width
            || measured.height != claimed.height
        {
            return Err("the staged upload differs from its immutable descriptor".into());
        }
        let incoming = Descriptor {
            reference: Reference {
                key: claimed.key.hex().into(),
                generation: opaque(),
                kind: claimed.kind.word().into(),
                width: claimed.width,
                height: claimed.height,
            },
            bytes: bytes.len() as u64,
        };
        let bytes: Arc<[u8]> = Arc::from(bytes);
        let selected = propose(declaration, incoming.clone(), Arc::clone(&bytes))?;
        selected.validate()?;
        let mut same = selected.clone();
        same.generation = incoming.reference.generation.clone();
        if same != incoming.reference {
            return Err("the row adapter changed the canonical upload descriptor".into());
        }
        self.held
            .get_mut(declaration)
            .ok_or("the upload claim disappeared")?
            .fate = Fate::Published;
        self.payloads.insert(
            declaration.into(),
            Payload {
                declaration: declaration.into(),
                descriptor: Descriptor {
                    reference: selected.clone(),
                    bytes: incoming.bytes,
                },
                bytes,
            },
        );
        Ok(Some(selected))
    }
    pub(crate) fn discard(
        &mut self,
        declaration: &str,
        session: &str,
        token: &str,
    ) -> Result<Option<Shown>, String> {
        self.open()?;
        self.asked.session("upload.discard", session)?;
        let Some(held) = self
            .held
            .get_mut(declaration)
            .filter(|h| h.token == token && matches!(h.fate, Fate::Held))
        else {
            return Ok(None);
        };
        let declared = self
            .leases
            .declared
            .iter()
            .find(|d| d.name == declaration)
            .ok_or("unknown upload declaration")?;
        held.fate = Fate::Discarded;
        Ok(Some(Shown {
            src: format!("{}/{}", declared.route, held.claimed.id),
            width: held.claimed.width,
            height: held.claimed.height,
        }))
    }
    pub(crate) fn payloads(&self) -> impl Iterator<Item = &Payload> {
        self.payloads.values()
    }
    pub(crate) fn receipt_answer(&self, answer: &str) -> Result<String, String> {
        self.open()?;
        if self.held.is_empty() {
            return Ok(answer.into());
        }
        let mut answer: serde_json::Value = serde_json::from_str(answer)
            .map_err(|_| "the upload outcome is not a durable answer")?;
        let object = answer
            .as_object_mut()
            .ok_or("the upload outcome is not a durable answer")?;
        if object.get("committed").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err("upload claims need a committed durable outcome".into());
        }
        object.insert(
            ATTEMPT_FIELD.into(),
            serde_json::Value::String(self.attempt.clone()),
        );
        Ok(answer.to_string())
    }
    /// Freeze before entering any call that could send COMMIT. A later unwind
    /// keeps this pending entry; only explicit known outcome resolves it.
    pub(crate) fn begin_commit(
        &mut self,
        record: Option<&crate::data::Recorded<'_>>,
    ) -> Result<(), String> {
        self.open()?;
        if self.held.is_empty() {
            self.phase = Phase::Finished;
            return Ok(());
        }
        if self.held.values().any(|h| matches!(h.fate, Fate::Held)) {
            return Err("an acquired upload claim has no final disposition".into());
        }
        let record = record.ok_or("upload claims require an actual durable interaction record")?;
        let proof_bytes = record
            .key
            .len()
            .saturating_add(record.sent.len())
            .saturating_add(record.answer.len());
        if proof_bytes > PENDING_PROOF_BYTES {
            return Err("too many uploads are waiting for commit confirmation".into());
        }

        let value: serde_json::Value = serde_json::from_str(record.answer)
            .map_err(|_| "upload receipt has no attempt proof")?;
        if value.get(ATTEMPT_FIELD).and_then(serde_json::Value::as_str) != Some(&self.attempt)
            || value.get("committed").and_then(serde_json::Value::as_bool) != Some(true)
        {
            return Err("upload receipt does not identify this invocation attempt".into());
        }
        let proof = Proof {
            key: record.key.into(),
            sent: record.sent.into(),
            answer: record.answer.into(),
            at_ms: i64::try_from(record.at_ms).unwrap_or(i64::MAX),
        };
        let ticket = Ticket {
            attempt: self.attempt.clone(),
            identity: Arc::new(()),
            proof: Arc::new(proof),
        };
        let mut state = self
            .leases
            .state
            .lock()
            .map_err(|_| "upload staging is unavailable")?;
        if state.pending.len() >= PENDING_COUNT
            || ticket.proof.bytes() > PENDING_PROOF_BYTES.saturating_sub(state.pending_proof_bytes)
        {
            return Err("too many uploads are waiting for commit confirmation".into());
        }
        if state.pending.contains_key(&self.attempt) {
            return Err("upload attempt identity is already pending".into());
        }
        let claimed = std::mem::take(&mut self.held)
            .into_values()
            .map(|h| h.claimed)
            .collect();
        state.pending_proof_bytes += ticket.proof.bytes();
        state.pending.insert(
            self.attempt.clone(),
            Pending {
                ticket: ticket.clone(),
                claimed,
            },
        );
        self.phase = Phase::Pending(ticket);
        Ok(())
    }
    pub(crate) fn applied(&mut self) -> Result<(), String> {
        match &self.phase {
            Phase::Pending(ticket) => self.leases.finish_attempt(ticket, true)?,
            Phase::Finished if self.held.is_empty() => return Ok(()),
            _ => return Err("this upload invocation has no pending commit".into()),
        }
        self.phase = Phase::Finished;
        Ok(())
    }
    pub(crate) fn refused(&mut self) -> Result<(), String> {
        match &self.phase {
            Phase::Pending(ticket) => self.leases.finish_attempt(ticket, false)?,
            Phase::Open => {
                for held in std::mem::take(&mut self.held).into_values() {
                    self.leases.unclaim(held.claimed);
                }
            }
            Phase::Finished => return Ok(()),
        }
        self.phase = Phase::Finished;
        Ok(())
    }
}
impl Drop for Invocation {
    fn drop(&mut self) {
        if matches!(self.phase, Phase::Open) {
            for held in std::mem::take(&mut self.held).into_values() {
                self.leases.unclaim(held.claimed);
            }
        }
    }
}
impl Shared {
    pub(crate) fn pending_attempts(&self, limit: usize) -> Result<Vec<Ticket>, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "upload staging is unavailable")?;
        Ok(state
            .pending
            .values()
            .take(limit)
            .map(|p| p.ticket.clone())
            .collect())
    }
    fn finish_attempt(&self, ticket: &Ticket, applied: bool) -> Result<(), String> {
        let claimed = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "upload staging is unavailable")?;
            let pending = state
                .pending
                .get(&ticket.attempt)
                .ok_or("upload attempt is no longer pending")?;
            if !Arc::ptr_eq(&pending.ticket.identity, &ticket.identity) {
                return Err("upload attempt belongs to another invocation".into());
            }
            let pending = state
                .pending
                .remove(&ticket.attempt)
                .ok_or("upload attempt disappeared")?;
            state.pending_proof_bytes -= pending.ticket.proof.bytes();
            pending.claimed
        };
        for claim in claimed {
            if applied {
                self.take(claim);
            } else {
                self.unclaim(claim);
            }
        }
        Ok(())
    }
    /// Called only with an authoritative same-adapter receipt inspection under
    /// reacquired lifetime exclusion. Absence is ALWAYS indeterminate here.
    pub(crate) fn reconcile_attempt(
        &self,
        ticket: &Ticket,
        receipt: Option<&Receipt>,
    ) -> Result<bool, String> {
        {
            let state = self
                .state
                .lock()
                .map_err(|_| "upload staging is unavailable")?;
            let pending = state
                .pending
                .get(&ticket.attempt)
                .ok_or("upload attempt is no longer pending")?;
            if !Arc::ptr_eq(&pending.ticket.identity, &ticket.identity) {
                return Err("upload attempt belongs to another invocation".into());
            }
        }
        let Some(receipt) = receipt else {
            return Ok(false);
        };
        if receipt.key != ticket.proof.key
            || receipt.sent != ticket.proof.sent
            || receipt.answer != ticket.proof.answer
            || receipt.at_ms != ticket.proof.at_ms
        {
            return Ok(false);
        }
        self.finish_attempt(ticket, true)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
