-- Paired devices. Tokens are stored only as SHA-256 hashes; a revoked
-- device keeps its row (History still names it) but can't sync.
CREATE TABLE devices (
    device_id      TEXT PRIMARY KEY NOT NULL,
    name           TEXT NOT NULL,
    token_hash     TEXT NOT NULL UNIQUE,
    paired_at      TEXT NOT NULL,
    last_seen_at   TEXT,
    revoked_at     TEXT,
    -- How far ahead of the server's clock the device's last push was, when
    -- more than the warning threshold (milliseconds).
    clock_ahead_ms INTEGER
);

-- The one open pairing code, if any, as a hash. Single use; it expires,
-- and too many wrong guesses burn it.
CREATE TABLE pairing (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    code_hash  TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    failures   INTEGER NOT NULL DEFAULT 0
);
