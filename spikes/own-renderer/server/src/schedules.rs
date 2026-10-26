//! **A command runs when the clock passes its time** (the scheduled-commands
//! track, ADR-0334): what the host keeps of a run a command scheduled, and
//! when it runs one again.
//!
//! A command's `schedule cmd(args) at when` calls its schedules' import,
//! which stages the run with the command's writes ([`Scheduling`]): written
//! in the layer's transaction, or the materializer's, or not at all. One run
//! is pending for each command and its arguments, by its key: scheduled
//! again, it moves, and `unschedule cmd(args)` cancels it. The host runs each
//! one due on behalf of the user whose command scheduled it, as a press is
//! run, keyed by its scheduling so that it commits once
//! (`Server::run_scheduled`).

use pw_materialize::QueueAuthority;
use pw_materialize::{Materializer, ScheduledRun};

/// Immutable selection bound to the live queue that actually selected it.
pub(crate) struct Candidate<'a> {
    run: ScheduledRun,
    authority: &'a QueueAuthority,
}
impl std::ops::Deref for Candidate<'_> {
    type Target = ScheduledRun;
    fn deref(&self) -> &ScheduledRun {
        &self.run
    }
}
impl Candidate<'_> {
    pub(crate) fn belongs_to(&self, authority: &QueueAuthority) -> bool {
        self.authority.matches(authority)
    }
    pub(crate) fn completing(&self, now_ms: i64) -> Completing<'_> {
        Completing {
            run: &self.run,
            authority: self.authority,
            now_ms,
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Completing<'a> {
    pub run: &'a ScheduledRun,
    authority: &'a QueueAuthority,
    pub now_ms: i64,
}
impl Completing<'_> {
    pub(crate) fn belongs_to(&self, authority: &QueueAuthority) -> bool {
        self.authority.matches(authority)
    }
}
pub(crate) fn select(
    runs: &dyn Runs,
    now_ms: i64,
    limit: usize,
) -> Result<Vec<Candidate<'_>>, String> {
    let authority = runs.authority();
    Ok(runs
        .due_runs(now_ms, limit)?
        .into_iter()
        .map(|run| Candidate { run, authority })
        .collect())
}

/// **One run a command scheduled, or cancelled**, as its schedules' import
/// was given it, staged with the command's writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scheduling {
    /// The command and its arguments: one run is pending for each.
    pub key: String,
    /// The command's component.
    pub command: String,
    /// Its arguments, as a press sends them: a JSON array.
    pub args: String,
    pub when: When,
}

/// When a scheduled run is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum When {
    /// At an instant: milliseconds since 1970, on the host's clock.
    At(i64),
    /// A duration in milliseconds after the scheduling command's commit.
    After(i64),
    /// Never: the run pending for the key is cancelled.
    Cancel,
}

/// **A scheduling as its commit writes it**: due at an instant, or the run
/// pending for its key cancelled (`due_ms` none), on behalf of `user`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scheduled {
    pub key: String,
    pub command: String,
    pub args: String,
    pub user: String,
    pub due_ms: Option<i64>,
}

impl Scheduling {
    /// As the commit at `now_ms`, on the host's clock, writes it for `user`:
    /// a duration counted from the commit, which the host reads then.
    pub(crate) fn at_commit(&self, now_ms: i64, user: &str) -> Scheduled {
        Scheduled {
            key: self.key.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            user: user.to_string(),
            due_ms: match self.when {
                When::At(at) => Some(at),
                When::After(ms) => Some(now_ms.saturating_add(ms)),
                When::Cancel => None,
            },
        }
    }
}

/// **A run's key**: its command and its arguments, as a press sends them.
pub(crate) fn key(command: &str, args: &str) -> String {
    format!("{command}\u{1f}{args}")
}

/// How many times a run that trapped or was refused runs again before it is
/// kept, failed for good (a Durable Object's alarm, at most six runs).
pub(crate) const RETRIES: i64 = 5;

/// **How long after its `failures`-th failure a run runs again**: 2 s,
/// doubling, a minute at most.
pub(crate) fn backoff_ms(failures: i64) -> i64 {
    2000_i64
        .saturating_mul(1 << (failures - 1).clamp(0, 16))
        .min(60_000)
}

/// **Where a program's scheduled runs are kept**, outside a command's
/// transaction: the runs due, and each answered or failed. A layer whose
/// writes outlive the host keeps its own, written with its commands' writes
/// (`Staged::commit`); the host's materializer keeps the rest.
pub(crate) trait Runs: Send + Sync {
    fn authority(&self) -> &QueueAuthority;
    fn current(&self, expected: &ScheduledRun, now_ms: i64) -> Result<bool, String>;
    fn retry_current(
        &self,
        expected: &ScheduledRun,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<bool, String>;
    /// The runs due at `now_ms`, at most `limit`, earliest first; those
    /// failed for good aside.
    fn due_runs(&self, now_ms: i64, limit: usize) -> Result<Vec<ScheduledRun>, String>;
    /// When the next pending run is due, where one is.
    fn next_run(&self) -> Result<Option<i64>, String>;
    /// A run answered: its scheduling removed. One scheduled again since is
    /// another scheduling, and stays.
    #[cfg(test)]
    fn ran(&self, id: i64) -> Result<(), String>;
    /// A run that failed, to run again at `due_ms`, its `attempts` so far;
    /// or, `failed` with why, kept and never run again.
    #[cfg(test)]
    fn retry(
        &self,
        id: i64,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<(), String>;
    /// Every run kept, pending or failed for good, earliest first: what a
    /// test looks at.
    #[cfg(test)]
    fn all_runs(&self) -> Result<Vec<ScheduledRun>, String>;
}

impl Runs for Materializer {
    fn authority(&self) -> &QueueAuthority {
        self.queue_authority()
    }
    fn current(&self, expected: &ScheduledRun, now_ms: i64) -> Result<bool, String> {
        self.current_run(expected, now_ms)
    }
    fn retry_current(
        &self,
        expected: &ScheduledRun,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<bool, String> {
        self.retry_current_run(expected, attempts, due_ms, failed)
    }
    fn due_runs(&self, now_ms: i64, limit: usize) -> Result<Vec<ScheduledRun>, String> {
        Ok(Materializer::due_runs(self, now_ms, limit))
    }

    fn next_run(&self) -> Result<Option<i64>, String> {
        Ok(Materializer::next_run(self))
    }

    #[cfg(test)]
    fn ran(&self, id: i64) -> Result<(), String> {
        Materializer::ran(self, id);
        Ok(())
    }

    #[cfg(test)]
    fn retry(
        &self,
        id: i64,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<(), String> {
        Materializer::retry(self, id, attempts, due_ms, failed);
        Ok(())
    }

    #[cfg(test)]
    fn all_runs(&self) -> Result<Vec<ScheduledRun>, String> {
        Ok(self.scheduled_runs())
    }
}

/// **Each scheduling, in the materializer's transaction**, with the rows
/// and events it commits.
pub(crate) fn write(tx: &rusqlite::Transaction<'_>, scheduled: &[Scheduled]) -> Result<(), String> {
    for s in scheduled {
        match s.due_ms {
            Some(due) => Materializer::schedule(tx, &s.key, &s.command, &s.args, &s.user, due)?,
            None => Materializer::unschedule(tx, &s.key)?,
        }
    }
    Ok(())
}

/// The PostgreSQL layers' schedules (their migrations' `schedules` table),
/// each statement on the client it is given: in a command's transaction for
/// [`pg::write`], on its own for the rest.
pub(crate) mod pg {
    use super::{Scheduled, ScheduledRun};
    use postgres::Client;

    /// **Each scheduling, in a command's transaction**: the run pending for
    /// its key replaced, or cancelled.
    pub(crate) fn write(c: &mut Client, scheduled: &[Scheduled]) -> Result<(), postgres::Error> {
        for s in scheduled {
            c.execute("DELETE FROM schedules WHERE key = $1", &[&s.key])?;
            if let Some(due) = s.due_ms {
                c.execute(
                    "INSERT INTO schedules (key, command, args, user_id, due_ms, protocol) \
                     VALUES ($1, $2, $3, $4, $5, 1)",
                    &[&s.key, &s.command, &s.args, &s.user, &due],
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn due_runs(
        c: &mut Client,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<ScheduledRun>, postgres::Error> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        Ok(c.query(
            "SELECT id, command, args, user_id, due_ms, attempts, failed, protocol FROM schedules \
             WHERE failed IS NULL AND due_ms <= $1 ORDER BY due_ms, id LIMIT $2",
            &[&now_ms, &limit],
        )?
        .iter()
        .map(run)
        .collect())
    }

    /// Every run kept, pending or failed for good, earliest first.
    #[cfg(test)]
    pub(crate) fn all_runs(c: &mut Client) -> Result<Vec<ScheduledRun>, postgres::Error> {
        Ok(c.query(
            "SELECT id, command, args, user_id, due_ms, attempts, failed, protocol FROM schedules \
             ORDER BY due_ms, id",
            &[],
        )?
        .iter()
        .map(run)
        .collect())
    }

    pub(crate) enum Fence {
        Consumed,
        NotCurrent,
        Refused,
    }

    pub(crate) fn current(
        c: &mut Client,
        expected: &ScheduledRun,
        now_ms: i64,
    ) -> Result<bool, postgres::Error> {
        let found = c.query_opt("SELECT id, command, args, user_id, due_ms, attempts, failed, protocol FROM schedules WHERE id = $1", &[&expected.id])?.as_ref().map(run);
        Ok(found.as_ref() == Some(expected)
            && expected.failed.is_none()
            && expected.due_ms <= now_ms)
    }
    pub(crate) fn consume(
        c: &mut Client,
        expected: &ScheduledRun,
        now_ms: i64,
        mut counted: impl FnMut(),
    ) -> Result<Fence, postgres::Error> {
        counted();
        let found = c.query_opt("SELECT id, command, args, user_id, due_ms, attempts, failed, protocol FROM schedules WHERE id = $1 FOR UPDATE", &[&expected.id])?.as_ref().map(run);
        if found.as_ref() != Some(expected)
            || expected.protocol != 1
            || expected.failed.is_some()
            || expected.due_ms > now_ms
        {
            return Ok(Fence::NotCurrent);
        }
        counted();
        let changed = c.execute("DELETE FROM schedules WHERE id = $1 AND command = $2 AND args = $3 AND user_id = $4 AND due_ms = $5 AND attempts = $6 AND failed IS NULL AND protocol = 1", &[&expected.id, &expected.command, &expected.args, &expected.user, &expected.due_ms, &expected.attempts])?;
        Ok(if changed == 1 {
            Fence::Consumed
        } else {
            Fence::Refused
        })
    }
    pub(crate) fn retry_current(
        c: &mut Client,
        expected: &ScheduledRun,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<bool, postgres::Error> {
        c.execute("UPDATE schedules SET attempts = $7, due_ms = $8, failed = $9 WHERE id = $1 AND command = $2 AND args = $3 AND user_id = $4 AND due_ms = $5 AND attempts = $6 AND failed IS NOT DISTINCT FROM $10 AND protocol = $11", &[&expected.id, &expected.command, &expected.args, &expected.user, &expected.due_ms, &expected.attempts, &attempts, &due_ms, &failed, &expected.failed, &expected.protocol]).map(|n| n == 1)
    }

    fn run(r: &postgres::Row) -> ScheduledRun {
        ScheduledRun {
            id: r.get(0),
            command: r.get(1),
            args: r.get(2),
            user: r.get(3),
            due_ms: r.get(4),
            attempts: r.get(5),
            failed: r.get(6),
            protocol: r.get(7),
        }
    }

    pub(crate) fn next_run(c: &mut Client) -> Result<Option<i64>, postgres::Error> {
        c.query_one(
            "SELECT MIN(due_ms) FROM schedules WHERE failed IS NULL",
            &[],
        )
        .map(|r| r.get(0))
    }

    #[cfg(test)]
    pub(crate) fn ran(c: &mut Client, id: i64) -> Result<(), postgres::Error> {
        c.execute("DELETE FROM schedules WHERE id = $1", &[&id])
            .map(|_| ())
    }

    #[cfg(test)]
    pub(crate) fn retry(
        c: &mut Client,
        id: i64,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<(), postgres::Error> {
        c.execute(
            "UPDATE schedules SET attempts = $2, due_ms = $3, failed = $4 WHERE id = $1",
            &[&id, &attempts, &due_ms, &failed],
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A run that failed runs again after 2 s, doubling to a minute.**
    #[test]
    fn a_failed_run_waits_two_seconds_doubling_to_a_minute() {
        let waits: Vec<i64> = (1..=7).map(backoff_ms).collect();
        assert_eq!(waits, [2000, 4000, 8000, 16000, 32000, 60000, 60000]);
    }

    /// **A duration is counted from the commit; an instant is itself; a
    /// cancel is due never.**
    #[test]
    fn a_duration_is_counted_from_the_commit() {
        let s = |when| Scheduling {
            key: key("t.expire", "[\"o-1\"]"),
            command: "t.expire".into(),
            args: "[\"o-1\"]".into(),
            when,
        };
        assert_eq!(s(When::After(30)).at_commit(1000, "u").due_ms, Some(1030));
        assert_eq!(s(When::At(5)).at_commit(1000, "u").due_ms, Some(5));
        assert_eq!(s(When::Cancel).at_commit(1000, "u").due_ms, None);
        assert_eq!(s(When::At(5)).at_commit(1000, "u").user, "u");
    }
}
