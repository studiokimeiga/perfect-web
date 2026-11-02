-- ADR0389: cookie-scoped legacy epochs preserve existing sessions; every new
-- open supplies an OS nonce. Quiesce old workers before adopting this schema.
ALTER TABLE sessions ADD COLUMN epoch bytea NOT NULL
    DEFAULT decode(repeat('00', 16), 'hex') CHECK (octet_length(epoch) = 16);
