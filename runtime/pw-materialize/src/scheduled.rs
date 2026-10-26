//! Transactional scheduled-generation fencing (ADR-0388).
use crate::{Materializer, ScheduledRun};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;

/// Opaque local authority of one live queue adapter. Not a durable/global id.
/// No Clone: a distinct adapter gets a distinct live allocation.
#[derive(Debug)]
pub struct QueueAuthority(Arc<u8>);
impl QueueAuthority {
    pub fn new() -> Self {
        Self(Arc::new(0))
    }
    pub fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Default for QueueAuthority {
    fn default() -> Self {
        Self::new()
    }
}

fn read_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledRun> {
    Ok(ScheduledRun {
        id: r.get(0)?,
        command: r.get(1)?,
        args: r.get(2)?,
        user: r.get(3)?,
        due_ms: r.get(4)?,
        attempts: r.get(5)?,
        failed: r.get(6)?,
        protocol: r.get(7)?,
    })
}
impl Materializer {
    pub fn queue_authority(&self) -> &QueueAuthority {
        &self.queue_authority
    }

    /// Early check only; consume_run repeats it in the actual commit.
    pub fn current_run(&self, expected: &ScheduledRun, now_ms: i64) -> Result<bool, String> {
        let db = self.db();
        let found = db.query_row(
            "SELECT id, command, args, user_id, due_ms, attempts, failed, protocol FROM schedules WHERE id = ?1",
            params![expected.id], read_run,
        ).optional().map_err(|_| "scheduled generation could not be read".to_string())?;
        Ok(found.as_ref() == Some(expected)
            && expected.failed.is_none()
            && expected.due_ms <= now_ms)
    }

    /// Exact current generation consumed in the transaction of rows/receipt/events.
    /// A matching row whose DELETE is suppressed is refusal, not completion.
    pub fn consume_run(
        tx: &rusqlite::Transaction<'_>,
        expected: &ScheduledRun,
        now_ms: i64,
    ) -> Result<bool, String> {
        let found = tx.query_row(
            "SELECT id, command, args, user_id, due_ms, attempts, failed, protocol FROM schedules WHERE id = ?1",
            params![expected.id], read_run,
        ).optional().map_err(|_| "scheduled generation could not be read".to_string())?;
        if found.as_ref() != Some(expected)
            || expected.protocol != 1
            || expected.failed.is_some()
            || expected.due_ms > now_ms
        {
            return Ok(false);
        }
        let changed = tx.execute(
            "DELETE FROM schedules WHERE id = ?1 AND command = ?2 AND args = ?3 AND user_id = ?4 AND due_ms = ?5 AND attempts = ?6 AND failed IS NULL AND protocol = 1",
            params![expected.id, expected.command, expected.args, expected.user, expected.due_ms, expected.attempts],
        ).map_err(|_| "scheduled completion was refused".to_string())?;
        if changed != 1 {
            return Err("scheduled completion removed no current row".into());
        }
        Ok(true)
    }

    /// A late failure cannot overwrite another attempt, backoff or replacement.
    pub fn retry_current_run(
        &self,
        expected: &ScheduledRun,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<bool, String> {
        let db = self.db();
        db.execute(
            "UPDATE schedules SET attempts = ?7, due_ms = ?8, failed = ?9 WHERE id = ?1 AND command = ?2 AND args = ?3 AND user_id = ?4 AND due_ms = ?5 AND attempts = ?6 AND failed IS ?10 AND protocol = ?11",
            params![expected.id, expected.command, expected.args, expected.user, expected.due_ms, expected.attempts, attempts, due_ms, failed, expected.failed, expected.protocol],
        ).map(|n| n == 1).map_err(|_| "scheduled retry was refused".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsigned_receipt_times_never_wrap_to_negative_database_instants() {
        let m = Materializer::new(crate::Clock::default(), "receipt-bound");
        m.command(|tx| {
            Materializer::record_interaction(tx, "future", "[]", "answer", u64::MAX)?;
            Ok::<_, String>(Vec::new())
        })
        .expect("a bounded record");
        assert_eq!(
            m.recalled("future", i64::MAX as u64),
            Some(("[]".into(), "answer".into()))
        );
        m.command(|tx| {
            Materializer::record_interaction(tx, "old", "[]", "old-answer", 1)?;
            Materializer::prune_interactions(tx, u64::MAX);
            Ok::<_, String>(Vec::new())
        })
        .expect("pruned at the signed database bound");
        assert_eq!(m.recalled("old", 0), None);
    }
}
