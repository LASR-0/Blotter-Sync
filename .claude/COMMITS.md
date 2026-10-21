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
| 01 | — | — | `docs: Blotter Sync plan` | committed (hash in the next entry) |
| 02 | 0 | — | `chore: scaffold the blotter-sync package, lints and CI on Windows and Linux` | planned |
| 03 | 1 | #73 | `feat: data dir, server state store and hello` | planned |
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
