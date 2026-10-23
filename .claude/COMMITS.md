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
| 03 | 1 | #73 | `feat: data dir, server state store and hello` | committed `0b7d2ba` |
| 04 | 1 | #73 | `feat: pairing, device tokens, revocation and the admin CLI` | committed `a680b4a` |
| 05 | 1 | #73–#78 | `feat: push, pull and bootstrap` | committed `8869958` |
| 06 | 1 | — | `feat: change stream over server-sent events` | committed `0349037` |
| 07 | 1 | #79 | `feat: headless rollover, subscriptions, holidays and backups` | committed `25129d6` |
| 08 | 1 | — | `feat: snapshots, restore and import with epochs` | committed `cf84f15` |
| 09 | 1 | — | `feat: read-only status page` | committed `8d783a4` |
| 10 | 1 | — | `test: two replicas syncing over HTTP` | committed `42a2699` |
| 11 | 1 | — | `build: Dockerfile, non-root and read-only` | committed `139e752`; checked in CI |
| 12 | 2 | — | `ci: multi-arch image and Linux and Windows binaries on release` | committed (hash in the next entry) |
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

### #04 — Pairing, device tokens, revocation and the admin CLI            [status: committed — hash in the next entry]
Phase: 1
Message:
    feat: pairing, device tokens, revocation and the admin CLI

    Auth as D-S6 and roadmap §5.4 describe it.

    - `blotter-sync pair` opens a code (ABCD-EFGH, no look-alike letters)
      that works once, for 10 minutes, and is burned by the fifth wrong
      guess; codes are kept only as hashes. POST /v1/pair trades it, with
      the device's id (a ULID) and name, for a 256-bit token, shown once
      and stored only as its SHA-256. Pairing the same device again
      replaces its token and lifts a revocation.
    - Every request but hello must carry the sync protocol and schema
      headers; a mismatch is refused (409) with the server's versions
      and which side to update. Authenticated routes take a Device
      extractor: versions, then the bearer token (unknown → unauthorized,
      revoked → revoked).
    - Per-address limits: 10 pairing attempts or failed authentications a
      minute, then rate_limited (429). The server records client
      addresses for this.
    - DELETE /v1/devices/self disconnects the calling device.
    - `blotter-sync devices` lists devices (name, id, paired, last seen,
      status); `blotter-sync revoke <name or id>` disconnects one. Both
      work beside a running server.
    - hello says whether a pairing code is open. Tokens never reach the
      log.

    README: the admin commands; the data directory's Windows path had
    lost its backslash. #03 is 0b7d2ba.
Files:
    A  src/auth.rs, src/routes.rs, migrations/0002_devices.sql
    M  src/lib.rs, src/main.rs, src/api.rs, src/server.rs, src/store.rs
    M  README.md, .claude/COMMITS.md
Test steps:
    - cargo fmt --check, cargo clippy --all-targets -D warnings → clean
    - cargo test → 20 passed: codes and normalising, single use, expiry, burning after five
      wrong guesses, bad device details (code not spent), revoke and re-pair, ambiguous
      names; over HTTP: hello's pairing flag, pair → token → disconnect → revoked, typed
      refusals, protocol and schema mismatches both ways, missing headers, bad tokens, and
      the per-address limit for auth and pairing
    - Live on Windows: serve; `pair` from a second process; pair over HTTP; `devices` lists
      Laptop; `revoke laptop`; the old token gets "revoked"; the token isn't in the log
Notes:
    - Behind Caddy every client shares the proxy's address for the limit; fine for one
      household, noted for the deploy docs.

### #05 — Push, pull and bootstrap            [status: committed — hash in the next entry]
Phase: 1
Needs: Blotter #73-#78
Message:
    feat: push, pull and bootstrap

    The sync exchanges (roadmap §5.2), on blotter-core's RowState and
    merge.

    - POST /v1/push {epoch, batch_id, rows}: merges the rows into the
      canonical planner, logs every row the merge changed (including
      tombstones it worked out, and the server's own captured changes),
      and answers {seq, rejected}. A batch id seen before from that
      device gets its first answer back and changes nothing. A push from
      a clock more than 5 minutes ahead of the server's is noted on the
      device (shown by `blotter-sync devices`).
    - GET /v1/pull?epoch&since: the log after the device's cursor, 500
      rows a page with `more`, and the seq to ask from next.
    - GET /v1/bootstrap: every row the server holds, tombstones included,
      with the epoch and the seq to pull from afterwards.
    - The change log (server.db migration 3) keeps one entry per row: a
      new entry for a row replaces its older one, since each is the
      row's whole state, so a device that's behind still gets the latest
      and the log stays as large as the planner.
    - A stale epoch, or a cursor past the end of the log, gets
      stale_epoch (410): bootstrap again.
    - Bodies up to 32 MB (a first push carries a whole planner); gzip
      both ways.
    #04 is a680b4a.
Files:
    A  src/changes.rs, src/sync_tests.rs, migrations/0003_log.sql
    M  src/routes.rs, src/lib.rs, src/store.rs, Cargo.toml, Cargo.lock (tower-http)
    M  .claude/COMMITS.md
Test steps:
    - cargo fmt --check, cargo clippy --all-targets -D warnings → clean
    - cargo test → 28 passed; new, with blotter-core replicas through the router: two
      devices converge (different groups both survive) and match the server row for row;
      a resent push answers the same and logs once; a new epoch sends push and pull to
      bootstrap; a cursor ahead of the log does too; 520 items arrive in pages of 500;
      refused rows come back in the answer; a clock an hour ahead is noted; pull and
      bootstrap need a token
    - Live on Windows: pair, push half a row (refused with its reason, nothing else
      lost), bootstrap answers gzipped, pull from 0
Notes:
    - The envelopes (PushRequest and the rest) are this repo's until Blotter #80 moves them
      into blotter-core::sync.

### #06 — Change stream over server-sent events            [status: committed — hash in the next entry]
Phase: 1
Message:
    feat: change stream over server-sent events

    GET /v1/events (an authenticated device) streams `seq` events, each
    {"seq", "epoch"}: the current values as soon as it connects, then
    again whenever the server logs a change or the epoch moves. A notice
    only says "pull now" (or, with a new epoch, "bootstrap again"); rows
    always come from pull. Quiet streams send a comment every 20 s so
    proxies keep them open, and the stream is never compressed, so
    nothing buffers it.

    The server keeps the newest seq and epoch in a watch channel that the
    change log announces to after every append. #05 is 8869958.
Files:
    M  src/server.rs, src/changes.rs, src/routes.rs, src/sync_tests.rs
    M  Cargo.toml, Cargo.lock (tokio sync and time, tokio-stream)
    M  .claude/COMMITS.md
Test steps:
    - cargo fmt --check, cargo clippy --all-targets -D warnings → clean
    - cargo test → 30 passed; new: a device's stream opens with its bootstrap's seq and
      epoch (text/event-stream, no content-encoding even when gzip is accepted), and gives
      the new seq when another device pushes; the stream needs a token
    - Live on Windows: curl -N on /v1/events, then a push from another request → two
      events, seq 0 then 1, as the push lands
Notes:
    - A stream opened before its device was revoked keeps running until it reconnects;
      nothing flows through it but seqs, and pull refuses the revoked token.

### #07 — Headless duties            [status: committed — hash in the next entry]
Phase: 1
Needs: Blotter #79
Message:
    feat: headless rollover, subscriptions, holidays and backups

    The planner's automatic duties now run on the server (D-S2), every
    minute from five seconds after start, through blotter-core as the
    system actor:

    - the day's rollover, once a day after its time, in the planner's
      synced time zone;
    - pruning holidays that have ended, once a day;
    - public-holiday upkeep: next year's fetched from Nager.Date when due
      (one try a day, so being offline doesn't retry every minute) and
      imported;
    - subscription refreshes: every due calendar fetched and recorded,
      its events and fetch status captured as server-owned data;
    - the daily backup of the canonical planner, into snapshots/ beside
      it, keeping --keep-backups (BLOTTER_SYNC_KEEP_BACKUPS, default 20).

    A tick does the database work under the lock and says what to fetch;
    fetching (reqwest with rustls, 60 s timeout, 20 MB cap) happens
    outside it; then the results go in. Everything a duty changes goes
    into the change log and out on the change stream, like a device's
    push. A failing duty is logged and the others carry on. #06 is
    0349037.
Files:
    A  src/duties.rs
    M  src/server.rs, src/main.rs, src/lib.rs, Cargo.toml, Cargo.lock (reqwest)
    M  README.md, .claude/COMMITS.md
Test steps:
    - cargo fmt --check, cargo clippy --all-targets -D warnings → clean
    - cargo test → 36 passed; new: at a fixed 10:00, yesterday's task goes back to the
      Inbox and the item and last_rollover_date reach the log, and not again that day; an
      ended holiday is pruned and its tombstone logged; public holidays due once a day,
      imported from an answer, then not due; a calendar from a local HTTP server fetched,
      1 event and its fetch status logged, not due again for an hour; an unreachable
      calendar records "couldn't connect"; one daily backup beside the planner however
      many ticks
    - Live on Windows: serve --keep-backups 7 → "daily backup taken" five seconds in,
      blotter-<stamp>-daily.db in snapshots/
Notes:
    - rustls uses aws-lc-rs, which builds here on Windows; check its arm64 cross-build in
      Phase 2 (ring is the fallback).
    - The Nager.Date fetch itself is covered by Blotter's live test (#79); here the import
      runs from a canned answer.

### #08 — Snapshots, restore and import with epochs            [status: committed — hash in the next entry]
Phase: 1
Message:
    feat: snapshots, restore and import with epochs

    The server's planner can be snapshotted, restored and replaced from a
    JSON export, from a device or the CLI (roadmap §5.1, H5).

    - GET /v1/snapshots lists them (id, reason, manual, note, size, time;
      never the server's file paths); POST /v1/snapshots takes a manual
      one, with an optional note.
    - POST /v1/snapshots/{id}/restore restores one, after a "before
      restore" snapshot; POST /v1/import replaces the planner with a
      Blotter JSON export, after a "before import" one. Both start a new
      epoch: the change log and stored push answers are cleared, the
      restored stamps count as sent, and the change stream announces the
      epoch, so every device gets stale_epoch and bootstraps the new
      planner whole.
    - `blotter-sync snapshot [--note]`, `snapshots` and `restore <id>` do
      the same beside a running server; its next duty tick sees the new
      epoch and announces it.
    - Admin commands log only warnings, so their output reads cleanly.
    #07 is 25129d6.
Files:
    A  src/admin.rs
    M  src/routes.rs, src/main.rs, src/lib.rs, src/duties.rs, src/sync_tests.rs
    M  README.md, .claude/COMMITS.md
Test steps:
    - cargo fmt --check, cargo clippy --all-targets -D warnings → clean
    - cargo test → 39 passed; new: snapshot (no path in the answer, listed with its note),
      edit, restore → a new epoch, "before_restore" snapshot, the old epoch refused, a
      new device bootstraps the earlier title, the log empty; an export imported over the
      server's planner replaces it ("before_import"); an unknown snapshot is invalid,
      listing needs a token, an in-memory server keeps none
    - Live on Windows: serve, pair, `snapshot --note …`, `snapshots` lists it beside the
      daily one, `restore <id>` from a second process; within the next tick the open
      change stream shows the new epoch, and hello agrees
Notes:
    - In Blotter, Settings → Snapshots in Home server mode uses these endpoints (#84).

### #09 — Read-only status page            [status: committed — hash in the next entry]
Phase: 1
Message:
    feat: read-only status page

    GET / shows what the server is and who syncs with it (roadmap §5.1):
    Blotter Sync's version and the core's, the sync protocol and schema,
    the server id and epoch, how many changes are logged, the latest
    backup, whether a pairing code is open, and each device with when it
    paired, when it was last seen and its status (syncing, disconnected,
    or a clock running ahead of the server's). No actions and no tokens;
    device names are escaped. Plain server-rendered HTML in the Blotter
    palette, light and dark from the system, refreshing every minute; the
    details stack on narrow screens. #08 is cf84f15.
Files:
    A  src/status.rs
    M  src/routes.rs, src/lib.rs
    M  .claude/COMMITS.md
Test steps:
    - cargo fmt --check, cargo clippy --all-targets -D warnings → clean
    - cargo test → 41 passed; new: an empty server says how to pair; a device named
      "<b>Luke's</b> laptop" shows escaped, its token nowhere on the page; times read
      "2026-10-22 03:03 UTC"
    - Looked at in headless Edge (a paired device, a daily backup, a code open): light at
      900 px, dark at 600 and 900 px; nothing overflows
Notes:
    - Headless Edge won't go below ~500 px wide, so phone width wasn't seen; the details
      list stacks under 520 px and the device table scrolls inside its card.

### #10 — Two replicas syncing over HTTP            [status: committed — hash in the next entry]
Phase: 1
Message:
    test: two replicas syncing over HTTP

    An end-to-end test with the app's side of the protocol played by two
    blotter-core Planners and reqwest, against the server on a local
    port (roadmap §7): both pair and bootstrap; the laptop creates an
    event through the operation layer and pushes; the desktop, listening
    on the change stream, hears the new seq, pulls and has it. Then a
    reschedule on one and a retitle on the other, made offline, both
    survive, and both replicas match the server row for row. Undo on the
    laptop refuses to reverse its reschedule over the desktop's retitle,
    and does reverse its own edit to an item no one else touched, which
    then syncs. #09 is 8d783a4.
Files:
    A  tests/two_replicas.rs
    M  Cargo.toml, Cargo.lock (reqwest json, dev only)
    M  .claude/COMMITS.md
Test steps:
    - cargo fmt --check, cargo clippy --all-targets -D warnings → clean
    - cargo test → 41 unit + 1 integration passed
Notes:
    - For Luke: undo is item-level (Blotter #27, #78): any later change to the item, from
      any device, blocks undoing an earlier edit to it, even in another field group. A
      finer undo (only the fields it would revert) is possible as a Blotter change if
      that's wanted.

### #11 — Dockerfile, non-root and read-only            [status: committed — hash in the next entry; CI Docker job passed]
Phase: 1
Message:
    build: Dockerfile, non-root and read-only

    A two-stage image (roadmap §6): the server is built with cargo
    --locked in rust:1.98-bookworm, then copied into distroless
    cc-debian12 running as nonroot (65532), with /data as the volume
    and SQLite's temporary files kept there so the root filesystem can
    be mounted read-only. Development is native on Windows, so CI
    builds and runs the image instead of WSL: a new Docker job checks
    the image user, starts it with --read-only on a named volume,
    answers /v1/hello and the status page, runs pair, snapshot and
    snapshots through docker exec, and keeps its server id in a new
    container on the same volume. #10 is 42a2699.
Files:
    A  Dockerfile, .dockerignore
    M  .github/workflows/ci.yml (the docker job)
    M  .claude/COMMITS.md
Test steps:
    - The CI "Docker image" job passes on the next push (not run locally: no Docker on
      the dev machine)
Notes:
    - Changes D-S14's "Docker tested in WSL" to "Docker tested in CI" for development;
      the pre-release live check in WSL (roadmap §7) still stands.
    - The base tags (rust:1.98-bookworm, distroless cc-debian12:nonroot) are confirmed by
      that first CI build.

### #12 — Release workflow            [status: committed — hash in the next entry]
Phase: 2
Message:
    ci: multi-arch image and Linux and Windows binaries on release

    release.yml (roadmap §6.1), on a v* tag: checks the tag against
    Cargo.toml's version; builds the image for linux/amd64 and
    linux/arm64, each on its own native runner, pushes both by digest
    and joins them into ghcr.io/lasr-0/blotter-sync tagged with the
    version, major.minor, major and latest; builds blotter-sync for
    Linux amd64 and arm64 (on Ubuntu 22.04, so glibc 2.35 and later)
    and blotter-sync.exe for Windows x64 (C runtime linked in), each
    archived with the README and licence; and attaches them with
    SHA256SUMS to a draft release, published by hand after the live
    checks. A manual run builds everything without pushing or
    releasing, keeping the binaries as artifacts. #11 is 139e752.
Files:
    A  .github/workflows/release.yml
    M  .claude/COMMITS.md
Test steps:
    - Not runnable locally. Luke: Actions → Release → Run workflow on main → every job
      green; the three archives are downloadable and `blotter-sync --version` runs
      (the job runs it too)
    - The first v* tag pushes the image; then set the GHCR package public once
      (roadmap §6.1)
Notes:
    - Native arm64 runners (ubuntu-22.04-arm, ubuntu-24.04-arm) instead of QEMU or
      cross-compiling, the roadmap's Phase 2 lead: free for public repos, and no
      cross toolchain for aws-lc and the bundled SQLite.
    - Mirrors Blotter's release.yml: draft on a tag, artifacts only on a manual run.
    - Action majors checked against their repos' tags on 2026-10-23.
