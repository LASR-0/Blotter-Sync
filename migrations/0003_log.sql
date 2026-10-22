-- Every change the server accepted, in order: what devices pull. Each
-- entry is a row's whole state after the change, so an entry replaces
-- any earlier one for the same row: a device behind still gets the
-- latest state, and the log stays as large as the planner.
CREATE TABLE log (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    tbl         TEXT NOT NULL,
    row_key     TEXT NOT NULL,
    row_json    TEXT NOT NULL,
    -- The device whose push made it; null for the server's own changes.
    from_device TEXT,
    at          TEXT NOT NULL,
    UNIQUE (tbl, row_key)
);

-- Answers to pushes, by the device's batch id, so a resent push changes
-- nothing and gets the same answer.
CREATE TABLE pushes (
    device_id     TEXT NOT NULL,
    batch_id      TEXT NOT NULL,
    response_json TEXT NOT NULL,
    at            TEXT NOT NULL,
    PRIMARY KEY (device_id, batch_id)
);
