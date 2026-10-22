# Blotter Sync — Commit Ledger

The implementer commits each entry itself, as Luke (no AI co-author trailer), after its test steps pass, and **never pushes**: Luke syncs from VS Code. Same protocol as Blotter's ledger (Blotter roadmap §7). Work in the Blotter repo (`../Blotter`) is tracked in Blotter's own ledger; an entry here that depends on it names that entry.

Each entry lists the exact files to stage and the message to use. Files are delivered as full files.

## How to use an entry
1. Copy or replace the listed files into the repo at the given repo-relative paths.
2. Run the **Test steps**. Don't commit if anything fails; report it back instead.
3. Stage exactly the listed files (`git add <paths>`; `git rm <paths>` for deletions).
4. Commit with the given message verbatim.
5. Update the entry's status to `committed`, and note the short hash if you like.

## Entry template
```
### #NN — <short title>            [status: planned | ready | committed | superseded by #NN]
Phase: N
Needs: <Blotter ledger entries this depends on, if any>
Message:
    <type>(<scope>): <summary>

    <optional body>
Files:
    A  path/to/new-file
    M  path/to/changed-file
    D  path/to/deleted-file
Test steps:
    - <command>  → <expected>
    - <manual check>
Notes:
    - <known gaps / follow-ups>
```

Commit types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`.
Bug and visual fixes from Luke's feedback get their own entries (`fix(...)`), added in order. Earlier entries are never rewritten.

---

## Planned sequence
The file lists are filled in when an entry moves to `ready`. Planned commits may be split or merged; record any change here.

| # | Phase | Needs (Blotter) | Planned message | Status |
|---|---|---|---|---|
| 01 | — | — | `docs: Blotter Sync plan` | committed `d2b0f9b`, pushed |
| 01a | — | — | `docs: confirm the sync schema gate and the blotter-sync name` | committed `857bdae` |
| 02 | 0 | — | `chore: scaffold the blotter-sync package, lints and CI on Windows and Linux` | committed `359e5d1` |
| 03 | 1 | #73 | `feat: data dir, server state store and hello` | committed (hash in the next entry) |
| 04 | 1 | #73 | `feat: pairing, device tokens, revocation and the admin CLI` | planned |
| 05 | 1 | #73–#78 | `feat: push, pull and bootstrap` | planned |
| 06 | 1 | — | `feat: change stream over server-sent events` | planned |
| 07 | 1 | #79 | `feat: headless rollover, subscriptions, holidays and backups` | planned |
| 08 | 1 | — | `feat: snapshots, restore and import with epochs` | planned |
| 09 | 1 | — | `feat: read-only status page` | planned |
| 10 | 1 | — | `test: two replicas syncing over HTTP` | planned |
| 11 | 1 | — | `build: Dockerfile, non-root and read-only` | planned |
| 12 | 2 | — | `ci: multi-arch image and Linux and Windows binaries on release` | planned |
| 13 | 2 | — | `docs(deploy): compose profiles, native installs and the compatibility table` | planned |
| 14 | 2 | #85 | `chore(release): v1.0.0` | planned |
| 15 | 3 | — | `feat: Home Assistant add-on with an Ingress admin page` | planned (1.1.0, refined after 1.0.0) |

---

## Entries
<!-- Ready and committed entries go below, newest last. -->

### #01 — Blotter Sync plan            [status: committed — hash in the next entry]
Phase: —
Message:
    docs: Blotter Sync plan

    Blotter Sync, a headless Blotter that runs on a home server (Docker,
    or natively on Linux and Windows) and syncs every Blotter desktop
    through it, in its own repo. The brief is .claude/server-roadmap.md:
    the two-repo setup and compatibility table, the server, its API and
    security, deployment tiers, phases 0-3 (1.0.0, then the Home Assistant
    add-on as 1.1.0) and the decisions log. The app-side work (sync model,
    core groundwork, the app's sync engine and UI) is Blotter 1.2.1, in
    Blotter's roadmap §14 and ledger. Planned commits #02-#15.
Files:
    A  .claude/server-roadmap.md, .claude/COMMITS.md
Test steps:
    - none (docs only)
Notes:
    - Proposed, for Luke: D-S15 (gate on a sync schema version, not the full migration
      count) and D-S16 (the names Blotter Sync / blotter-sync).

### #01a — Confirm the sync schema gate and the blotter-sync name            [status: committed — hash in the next entry]
Phase: —
Message:
    docs: confirm the sync schema gate and the blotter-sync name

    Luke confirmed D-S15 (gating by a sync schema version that changes
    only with synced tables and columns) and D-S16 (Blotter Sync,
    blotter-sync, ghcr.io/lasr-0/blotter-sync). The GitHub repo is
    LASR-0/Blotter-Sync. #01 is d2b0f9b.
Files:
    M  .claude/server-roadmap.md, .claude/COMMITS.md
Test steps:
    - none (docs only)

### #02 — Scaffold, lints and CI            [status: committed — hash in the next entry]
Phase: 0
Message:
    chore: scaffold the blotter-sync package, lints and CI on Windows and Linux

    One package building the blotter-sync binary: axum on tokio, clap for
    the CLI (serve, the default), tracing to stdout (BLOTTER_SYNC_LOG).
    GET /v1/hello answers with the server's version and the Blotter
    release its core comes from; it grows pairing, epoch and the sync
    protocol and schema as they land. Default port 7458 (no common
    clash). Graceful shutdown on Ctrl+C, and on SIGTERM on Unix.

    blotter-core is a git dependency pinned to Blotter v1.2.0 until 1.2.1
    has the sync module; a git-ignored .cargo/config.toml can patch it to
    ../Blotter for local work (README). Lints, clippy.toml, rustfmt.toml,
    .gitattributes and the MIT licence match Blotter's. CI runs fmt,
    clippy -D warnings and tests on windows-latest and ubuntu-latest.
    #01a is 857bdae.
Files:
    A  Cargo.toml, Cargo.lock, src/lib.rs, src/main.rs
    A  .github/workflows/ci.yml, .gitattributes, .gitignore, clippy.toml, rustfmt.toml
    A  LICENSE, README.md
    M  .claude/server-roadmap.md   (default port), .claude/COMMITS.md
Test steps:
    - cargo fmt --all --check → clean
    - cargo clippy --all-targets --locked -- -D warnings → clean
    - cargo test --locked → 2 passed (hello, unknown path 404)
    - cargo run -- serve --listen 127.0.0.1:7458, then curl /v1/hello →
      {"name":"blotter-sync","version":"0.1.0","blotter_core":"1.2.0"}  (checked on Windows)
    - With the .cargo/config.toml patch, cargo tree -i blotter-core → the ../Blotter path;
      without it → the v1.2.0 tag
    - Pending: CI green on both runners after Luke pushes
Notes:
    - rust-version is 1.95, not Blotter's 1.88: rusqlite_migration 2.6.0 (via blotter-core)
      needs 1.95. Blotter's own rust-version is understated; worth fixing there in 16a.
    - Phase 0's Linux check is CI for now; the WSL run comes with Phase 2 (D-S14).

### #03 — Data directory, server state and hello            [status: committed — hash in the next entry]
Phase: 1
Needs: Blotter #73-#79 (pinned at aa31b69)
Message:
    feat: data dir, server state store and hello

    The server now opens a data directory (--data-dir or
    BLOTTER_SYNC_DATA_DIR; by default %ProgramData%\Blotter Sync on
    Windows, ~/.local/share/blotter-sync on Linux, mode 0700 there) with
    two databases: the canonical planner, blotter.db, opened through
    blotter-core like the app's and set to the server role so it also
    captures what the server owns; and server.db, the server's own state
    with its own migrations, starting with its id and epoch.

    GET /v1/hello answers with the server id, versions (Blotter Sync, the
    core's Blotter version, sync protocol and schema) and epoch. The API
    module defines the typed refusals every endpoint will use (version
    mismatches carry the server's versions); the envelopes move into
    blotter-core::sync once the app has a client (Blotter #80).
    Handlers run database work on the blocking pool.

    blotter-core moves from the v1.2.0 tag to Blotter aa31b69, which has
    the sync module. #02 is 359e5d1.
Files:
    A  src/api.rs, src/config.rs, src/server.rs, src/store.rs, migrations/0001_meta.sql
    M  src/lib.rs, src/main.rs, Cargo.toml, Cargo.lock, README.md
    M  .claude/COMMITS.md
Test steps:
    - cargo fmt --check, cargo clippy --all-targets -D warnings → clean
    - cargo test → 8 passed (data dir, state identity and epochs, both databases with the
      server role, hello)
    - Live on Windows: blotter-sync --data-dir <scratch> serve → hello answers protocol 1,
      schema 1, an epoch; blotter.db (migrated to 14) and server.db appear
Notes:
    - Windows: the data directory keeps %ProgramData%'s default permissions for now; an
      owner-only ACL (roadmap §5.4) comes with the native install docs (Phase 2).
    - The planner's days follow its synced timezone setting; UTC until a device sets one.
