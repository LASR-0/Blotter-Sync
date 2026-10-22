-- Who this server is. One row, made the first time the data directory is
-- used. The epoch changes whenever the planner is replaced wholesale.
CREATE TABLE meta (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    server_id  TEXT NOT NULL,
    epoch      TEXT NOT NULL,
    created_at TEXT NOT NULL
);
