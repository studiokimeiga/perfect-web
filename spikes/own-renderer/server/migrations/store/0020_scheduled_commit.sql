-- ADR-0388: existing generations need reconciliation; new writers explicitly use1.
-- Quiesce old workers before this upgrade. Mixed-version execution is unsupported.
ALTER TABLE schedules ADD COLUMN protocol bigint NOT NULL DEFAULT 0 CHECK (protocol IN (0, 1));
