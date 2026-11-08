-- ADR-0334, amended 2026-11-07 (ADR-0407's decision 4): a run is pending for
-- each command, its arguments and the user whose command scheduled it. A key
-- written before named the command and its arguments alone, so one reader's
-- scheduling of the same arguments replaced, moved or cancelled another
-- reader's pending run, which then ran on the other's behalf. Each kept key
-- gains its user, as `schedules::key` writes it now: the user, the command and
-- the arguments, each after a U+001F. A key that was unique stays unique.
UPDATE schedules SET key = user_id || chr(31) || key;
