# Blotter Sync — Roadmap

> **Blotter Sync** is a headless Blotter that runs on your home server and keeps every desktop's planner in sync through it.

This document is the source of truth for the Blotter Sync server, which lives in its own repository (`LASR-0/Blotter-sync`, this repo). It is a **handover brief** for the implementing agent, in the same style as the Blotter app's roadmap: each phase gives intent, constraints, acceptance criteria (AC) and leads to verify. It does not prescribe file-level changes; the implementer decides structure within the constraints here.

**Two repositories, two documents:**
- **This roadmap** covers the server: its HTTP API, pairing and auth, storage, headless duties, deployment (Docker, native Linux and Windows) and the Home Assistant add-on.
- **The Blotter roadmap, §14** (`../Blotter/.claude/roadmap.md` on this machine, `.claude/roadmap.md` in `LASR-0/Blotter`) covers everything the app and `blotter-core` need for Blotter 1.2.1: the sync model, field groups, data classification, change capture, the merge function, the wire types and the app's sync engine and UI. The server builds on that work, so the implementer reads both.

**Workflow:** this roadmap → implementation, phase by phase, tracked commit by commit in this repo's `.claude/COMMITS.md` (its own ledger, from #01) → Luke as the feedback loop.

---

## 1. Goals and scope

### Goals (Blotter Sync 1.0.0, with Blotter 1.2.1)
- **One planner across devices.** Every Blotter desktop (Windows, Linux) connected to the same server sees the same items, reminders, tags, block-outs, holidays, subscriptions, custom themes and keybindings, within seconds when online.
- **Offline-first stays true.** A device never needs the server to work. "Away from home" only affects how fresh the data is.
- **A headless Blotter.** The server embeds `blotter-core` and owns the planner's automatic duties: end-of-day rollover, subscription refreshes, holiday pruning and public-holiday upkeep, and daily backups of the canonical planner.
- **Runs on Linux and Windows.** Docker (multi-arch, amd64 + arm64) is the primary deployment, with Compose profiles for the three tiers: LAN, + Caddy, + Tailscale. Plain binaries for Linux (amd64, arm64) and Windows (x64) run the same server without Docker.
- **Versioned on its own,** with a compatibility table in both repos so users know exactly which Blotter and Blotter Sync versions work together (§3).

### Next (Blotter Sync 1.1.0)
- **Home Assistant add-on**, with an Ingress admin page for pairing (§9, Phase 3). Refined once 1.0.0 is live.

### Explicitly out of scope
- Phones, tablets and a web UI. Sync is desktop-only (Windows and Linux).
- End-to-end encryption. The server must read the planner to run rollover and refresh subscriptions (D-S4).
- Built-in TLS in the server. Encryption comes from the LAN boundary, Tailscale or a reverse proxy (D-S5).
- AI on the server. API keys and CLI sign-ins stay on each device; chat, briefings and proposals stay device-local.
- Multiple users or multiple planners per server. One server holds one planner.
- A native Windows service (parked, §11). On Windows 1.0.0 runs from a terminal or at startup through Task Scheduler.
- Remote MCP, mDNS discovery, merging JSON imports (parked, §11).

---

## 2. Glossary
| Term | Meaning |
|---|---|
| **Replica** | A full copy of the planner: each device's `blotter.db`, and the server's. |
| **Device** | One paired Blotter desktop install. Identified by a ULID created on first run of Blotter 1.2.1, named by the user ("Desktop", "Laptop"). |
| **HLC** | Hybrid logical clock: wall-clock milliseconds plus a counter plus the device id, so every write has a unique, totally ordered stamp. |
| **Field group** | A set of columns that always merge together (Blotter roadmap §14). |
| **Seq** | The server's monotonic sequence number for every accepted change. A device's pull cursor is the last seq it has applied. |
| **Epoch** | An id for the server's planner history. It changes only when the planner is replaced wholesale (snapshot restore, JSON import), which tells every device to re-bootstrap. |
| **Bootstrap** | Fetching the server's full current state plus its seq, instead of replaying the whole log. |
| **Sync protocol** | The version of the wire types and endpoint behaviour, a constant in `blotter-core::sync`. |
| **Sync schema** | The version of the synced tables and columns, a constant in `blotter-core::sync` pinned by a test (D-S15). |

---

## 3. Two repos and compatibility

### 3.1 How the server gets `blotter-core` (D-S11)
- `blotter-core` stays in the Blotter repo. The server depends on it as a **git dependency pinned to a Blotter release tag** (`tag = "v1.2.1"`), so every Blotter Sync release names exactly the core it was built with.
- **Local development** points at the source on this machine instead (`../Blotter/crates/blotter-core`), through a Cargo `[patch]` or `.cargo/config.toml` override that is not committed, so the core and the server can change together before a Blotter tag exists. CI always builds against the pinned tag.
- The core must not gain tokio, axum or reqwest. Networking lives in the app's `src-tauri` and in this repo.

### 3.2 Compatibility rule (D-S12, D-S15)
- Every request carries the device's **sync protocol** and **sync schema** versions. The server refuses with a typed error when either differs (§5.3).
- Both versions are constants in `blotter-core::sync`. The sync schema changes only when a synced table or column changes, so Blotter releases that touch only device-local tables (chat, providers, snapshots) or the UI keep working with the same server.
- **A Blotter release that changes either version needs a matching Blotter Sync release** built against it. Both are recorded in the table below.

### 3.3 Compatibility table
Kept identical in both repos' READMEs (and here and in the Blotter roadmap §14). Each release adds a row or widens one.

| Blotter Sync | Blotter | Sync protocol | Sync schema |
|---|---|---|---|
| 1.0.x | 1.2.1 and later 1.2.x | 1 | 1 |

Blotter 1.2.0 and earlier have no sync. Blotter Sync 1.1.0 (the HA add-on) is expected to keep protocol 1 and schema 1.

---

## 4. What the server builds on
Blotter 1.2.1's core work (Blotter roadmap §14, 16a) gives the server everything it shares with the app:
- the same schema, migrations, operations, validation and slot engine, with no Tauri dependency;
- ULIDs for every id, so devices create rows offline without collisions;
- the **sync registry** (what syncs, what is device-local, what is server-owned), the **merge function** (last-writer-wins per field group by HLC, delete wins, cascades follow the parent, natural-unique collisions keep the earlier row) and the **wire types** for every endpoint, all in `blotter-core::sync`;
- network-free versions of subscription refresh and public-holiday upkeep, so the server does the HTTP and hands the bytes to the core, as the app does.

The server merges every push with the same function the devices use, so all replicas converge on the same state. The server doesn't re-implement any planner logic; if it needs something the core doesn't expose, that is a change in the Blotter repo first.

---

## 5. Server

### 5.1 Shape
- One Cargo package building one binary, **`blotter-sync`** (`blotter-sync.exe` on Windows), with the server and its admin CLI.
- **HTTP API** with axum on tokio; JSON bodies; gzip; body-size limits; structured logs to stdout.
- **Change stream** over Server-Sent Events: a small "something changed, pull now" notice with the new seq, not the data itself. SSE is plain HTTP, so it passes through Caddy and Tailscale untouched.
- **Storage** in a data directory:
  - `blotter.db`, the canonical planner, opened through `blotter-core` exactly as the app opens it.
  - Server-only state (devices, token hashes, pairing codes, the change log with seqs, the epoch) in its own file with its own migrations, so the core's migration list stays the app's.
  - Default location: `/data` in Docker; `$XDG_DATA_HOME/blotter-sync` (falling back to `~/.local/share/blotter-sync`) on Linux; `%ProgramData%\Blotter Sync` on Windows. Overridden with `--data-dir` or `BLOTTER_SYNC_DATA_DIR`.
- **Configuration:** listen address and port, data dir, backup count and log level, from flags or environment variables (Docker-friendly), no config file needed.
- **Headless duties** run on a timer, through the core's operations as actor `system` (D-S2): rollover in the user's synced `timezone` at `rollover_time`; subscription refresh (the server needs outbound internet); holiday pruning; public-holiday upkeep; daily backups of the canonical planner (the core's snapshot code, kept count configurable).
- **Admin CLI** in the same binary, run directly or with `docker exec`: `pair` (prints a pairing code), `devices` (list), `revoke <device>`, `snapshot`, `restore <id>`, `status`. It works against the data dir while the server runs.
- **A read-only status page** at `/`: version, sync protocol and schema, epoch, devices and their last sync, clock-skew warnings. No admin actions on the web in 1.0.0; the HA add-on adds an Ingress page for pairing (Phase 3).

### 5.2 API surface (leads, not a contract — the wire types in `blotter-core::sync` are the contract)
| Endpoint | Purpose |
|---|---|
| `GET /v1/hello` | No auth. Server id, Blotter Sync version, the core's Blotter version, sync protocol and schema, epoch, whether pairing is open. |
| `POST /v1/pair` | Pairing code + device id + device name → device token. |
| `GET /v1/bootstrap` | Full synced state + clocks + current seq + epoch. |
| `POST /v1/push` | A batch of captured changes. Idempotent by change id; returns the seqs assigned and any rejections. |
| `GET /v1/pull?since=<seq>` | Changes after a seq, paged. |
| `GET /v1/events` | SSE stream of seq notices. |
| `GET/POST /v1/snapshots…` | List, create and restore server snapshots; JSON import (replace) into the server. |
| `DELETE /v1/devices/self` | Disconnect: revoke this device's own token. |

### 5.3 Gating
- A request whose sync protocol or sync schema differs from the server's is refused with a typed error that carries both sides' versions, so the app can say which side to update ("Update Blotter Sync to 1.0.1", "Update Blotter on this device to 1.2.2"), using the compatibility table (§3.3).
- A request with an unknown or stale epoch gets a typed "re-bootstrap" error.
- Clock skew: the server warns (status page, `hello`) when a device's HLC wall time is more than 5 minutes ahead of the server's.

### 5.4 Security (D-S6)
- **Pairing:** an 8-character code from `blotter-sync pair`, single use, expires after 10 minutes, at most 5 failed attempts before the code is burned. `/v1/pair` refuses when no code is open.
- **Tokens:** 256-bit random, shown to the device once, stored on the server only as a hash, revocable. (On the device the token lives in the OS keychain; that's the app's side.)
- **No tokens or planner content in logs.** Per-IP rate limits on `pair` and on failed auth.
- **Container:** runs as a non-root user, read-only root filesystem, data only under `/data`.
- **Native installs:** the data dir is created readable only by the user running the server (an owner-only ACL on Windows, mode 0700 on Linux).
- The app's rule (plain `http://` only to loopback, private LAN ranges and Tailscale's range) is enforced on the device; the server listens on whatever it is told to.

---

## 6. Deployment

### 6.1 Image, binaries and releases
- Multi-arch image **`ghcr.io/lasr-0/blotter-sync`** (linux/amd64, linux/arm64), tagged with the version and `latest`, built and pushed by this repo's `release.yml` on a `v*` tag. arm64 matters now: the HA Green that Phase 3 targets is aarch64.
- Plain binaries attached to the GitHub release: `blotter-sync` for linux amd64 and arm64, `blotter-sync.exe` for windows x64.

### 6.2 Compose profiles (D-S5)
| Profile | Runs | Reachable via |
|---|---|---|
| default | `blotter-sync` | LAN (`http://<host>:<port>`) |
| `caddy` | + Caddy with automatic HTTPS | LAN + a public domain |
| `tailscale` | + a Tailscale sidecar | LAN + tailnet (`http://blotter.<tailnet>.ts.net`, optionally HTTPS via Tailscale Serve) |

Profiles combine (`--profile caddy --profile tailscale`). `deploy/` holds `compose.yaml`, a `Caddyfile` example and a `.env.example`; `deploy/README.md` walks through each tier as its own section, so a LAN-only user never reads about domains.

### 6.3 Without Docker
- **Linux:** the binary plus an example systemd unit in `deploy/` (a dedicated user, the data dir under `/var/lib/blotter-sync`).
- **Windows:** the `.exe` run from a terminal, or at startup through Task Scheduler (documented step by step). Reaching it from another device needs a Windows Firewall rule for the port, which the README shows. Tailscale on the host gives the on-the-go route; Caddy for Windows works the same as the `caddy` profile.

---

## 7. Quality bar
- The convergence, constraint and registry tests live with the merge function in `blotter-core` (Blotter roadmap §14). This repo tests the server.
- **Server tests:** the API through axum's test harness (auth, pairing limits, gating, idempotent push, paging, SSE notice, epochs), the headless duties against a fixed clock, and one end-to-end test with two `Planner` replicas syncing over real HTTP.
- **CI on `windows-latest` and `ubuntu-latest`** from the first commit, building against the pinned Blotter tag.
- **Live check before each release:** two Blotter instances with different `BLOTTER_DATA_DIR`s against the server, natively on Windows, and the Docker image in WSL through each Compose profile Luke can reach (LAN at least; Tailscale and Caddy where available).
- Clippy (`-D warnings`) and fmt clean; no `unwrap()` on user-data or network paths.

---

## 8. Commit protocol
The same as Blotter's (Blotter roadmap §7), in this repo's own ledger, `.claude/COMMITS.md`, numbered from #01: one entry per increment, full files, tests before commit. The implementer commits as Luke (no AI co-author trailer) and never pushes. Work in the Blotter repo is tracked in Blotter's ledger; an entry here that needs a Blotter change names that entry.

---

## 9. Phases
Every phase leaves `main` buildable and tested. Phases 1 and 2 make 1.0.0; Phase 3 makes 1.1.0.

### Phase 0 — Scaffold
**Intent:** a Cargo package depending on `blotter-core` (pinned tag in CI, local path in development), lints and formatting matching Blotter's, CI on Windows and Linux.

**AC:**
- `cargo run` starts a server that answers `GET /v1/hello` on Windows and Linux.
- CI builds and tests on both, against the pinned Blotter tag.
- The local-path override is documented in the README and not committed.

**Leads:** until Blotter 1.2.1 is tagged, CI pins a Blotter commit (`rev = …`) instead of a tag; switch to the tag at release.

### Phase 1 — The server
**Needs:** Blotter 16a (the sync groundwork in the core, Blotter ledger #73–#79) for everything past `hello` and pairing.

**Intent:** a working `blotter-sync` that pairs devices, accepts pushes, serves pulls and bootstrap, streams notices and runs the headless duties, natively and in Docker.

**AC:**
- `hello`, `pair`, `bootstrap`, `push`, `pull`, `events` behave as §5.2–5.4, with tests for every refusal (bad code, expired code, burned code, revoked token, protocol mismatch, schema mismatch, stale epoch).
- Push is idempotent: resending a batch changes nothing and returns the same seqs.
- Rollover runs once a day at the synced time in the synced zone, and its moves reach devices as ordinary changes. Subscriptions refresh; holidays prune; public holidays stay up to date; daily backups rotate.
- Server snapshots, restore and replace-import bump the epoch.
- The admin CLI works natively and through `docker exec`.
- A single-arch Docker image builds (in WSL) and runs as non-root with a read-only root filesystem.

**Leads:**
- Choose a default port with no common clash; document it.
- Use the core's network-free subscription and holiday code (Blotter #79) with this repo's own HTTP client (reqwest with rustls, so the Windows and Linux builds need no OpenSSL).
- Server state migrations: the same embedded-migration approach as the core, in a separate file.
- Concurrency: SQLite in WAL mode with one writer; push batches serialise through it.

### Phase 2 — Deployment and release
**Intent:** anyone can run the server from the published image at one of the three tiers, or from a plain binary on Linux or Windows; 1.0.0 ships alongside Blotter 1.2.1.

**AC:**
- `release.yml` builds and pushes the multi-arch image to GHCR and attaches the Linux and Windows binaries on a version tag.
- `deploy/compose.yaml` with the `caddy` and `tailscale` profiles, a `Caddyfile` example, `.env.example`, a systemd unit, and `deploy/README.md` covering each tier, native Linux and Windows, pairing, backups, upgrading and revoking a device.
- The README carries the compatibility table (§3.3), identical to Blotter's.
- Live checks (§7) pass and are recorded in the release entry.

**Leads:**
- arm64: cross-compiling (cargo-zigbuild or cross, with rusqlite's bundled SQLite) is far faster than building under QEMU; compare in CI.
- GHCR: the workflow needs `packages: write`; the package's visibility is set public once by Luke.
- Caddy flushes `text/event-stream` responses without buffering; confirm SSE through the reverse proxy in the live check.
- Tailscale: verify the current sidecar setup (`TS_AUTHKEY`, state volume, `TS_SERVE_CONFIG` for HTTPS on `*.ts.net`) against Tailscale's Docker docs at implementation time.
- Release order: Blotter tags 1.2.1 first; this repo pins that tag, then tags 1.0.0.

### Phase 3 — Home Assistant add-on (1.1.0, sketch)
**Intent:** install Blotter Sync from Home Assistant on the HA Green and similar hosts.

To refine once 1.0.0 is live; expected shape:
- An add-on using the published GHCR image (amd64 + aarch64), data in the add-on's `/data`, the sync port mapped directly.
- An **Ingress admin page** (behind HA's own login) to make pairing codes, list and revoke devices, and see status, since add-on users can't easily `docker exec`. Ingress can't carry device sync traffic, because it requires an HA session.
- Docs for remote access via HA's official Tailscale add-on. Nabu Casa's remote UI can't proxy the sync port.

**Open for that phase:** whether the add-on repository is `repository.yaml` in this repo or a separate `blotter-ha` repo; whether the Ingress page also comes to the status page outside HA.

---

## 10. Decisions log

| # | Decision | Status |
|---|---|---|
| D-S1 | Sync is desktop-only (Windows, Linux). The app has two modes, **Local only** (default) and **Home server**, with an ordered address list tried in turn | Confirmed (Luke, 2026-10-21) |
| D-S2 | The server is a **headless Blotter**: it embeds `blotter-core` and owns rollover, subscription refresh, holiday pruning, public-holiday upkeep and daily backups. AI stays on devices | Confirmed (Luke, 2026-10-21) |
| D-S3 | Conflicts resolve **per field** (last-writer-wins by HLC), state-based sync with the server ordering changes | Confirmed (Luke, 2026-10-21) |
| D-S4 | No end-to-end encryption (incompatible with D-S2) | Confirmed (follows D-S2) |
| D-S5 | Transport: the server speaks plain HTTP; encryption comes from the LAN boundary, a Tailscale sidecar (on-the-go route) or Caddy (public domain). Deployment tiers are Compose profiles: default, `caddy`, `tailscale`. Built-in TLS deferred. Docker is the primary deployment | Confirmed (Luke, 2026-10-21) |
| D-S6 | Auth is ours and always on: single-use pairing codes from the CLI, per-device hashed tokens, revocation; the app refuses plain HTTP to non-private addresses | Confirmed (Luke, 2026-10-21) |
| D-S7 | Change capture with **SQLite triggers** into an outbox (catches writes outside the op layer and from the MCP process), suppressed while applying remote changes | Confirmed (Luke, 2026-10-22) |
| D-S8 | "Per field" means **per field group**: columns tied by a CHECK, FK pair or op invariant merge together | Confirmed (Luke, 2026-10-22) |
| D-S9 | Every device fires reminders (`fired_at` device-local); snoozing on any device applies everywhere | Confirmed (Luke, 2026-10-22) |
| D-S10 | Delete wins over concurrent edits; children of a deleted row are deleted; natural-unique collisions keep the earlier row | Confirmed (Luke, 2026-10-22) |
| D-S11 | The server lives in **its own repo**, `LASR-0/Blotter-sync`, with its own version. It depends on `blotter-core` as a git dependency pinned to a Blotter release tag, with a local-path override for development. Supersedes "a crate in the Blotter repo" | Confirmed (Luke, 2026-10-22) |
| D-S12 | **Blotter 1.2.1** brings sync to the app; **Blotter Sync 1.0.0** is the first server release. Compatibility is by sync protocol and sync schema, recorded in a compatibility table in both repos. Supersedes "both ship as v1.3.0" | Confirmed (Luke, 2026-10-22) |
| D-S13 | The Home Assistant add-on is the next release after the server goes live: **Blotter Sync 1.1.0** | Confirmed (Luke, 2026-10-22; was v1.4.0) |
| D-S14 | The server builds and runs **natively on Windows (x64) and Linux (amd64, arm64)** as well as in Docker. Development is native on Windows; Linux and Docker are tested in WSL before each release, and CI covers both from the start | Confirmed (Luke, 2026-10-22) |
| D-S15 | Gating uses a **sync schema** version that changes only with synced tables and columns (pinned by a registry test), not the core's full migration count, so app releases that touch only device-local data don't need a new server | **Proposed** |
| D-S16 | Product, binary and image are named **Blotter Sync**, `blotter-sync`, `ghcr.io/lasr-0/blotter-sync`, after the repo | **Proposed** |
| O1 | Custom themes (the `themes` table) and keybinding overrides **sync**; the **selected theme** stays per device. (The OS-level quick-capture shortcut isn't a keybinding override, so it was never going to sync.) | Confirmed (Luke, 2026-10-22) |

---

## 11. Parked
- A native Windows service (`blotter-sync service install`), instead of Task Scheduler.
- mDNS discovery of a server on the LAN (`blotter-sync.local`).
- Remote MCP through the server, so agents can reach the planner from anywhere.
- Built-in TLS (self-signed with pinning in the pairing code, or ACME).
- Merging JSON imports instead of replacing.
- Log compaction and tombstone purging (only needed once logs grow large; devices behind the horizon would re-bootstrap).
- Phones, tablets or a web UI served by the server.
