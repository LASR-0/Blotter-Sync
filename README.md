# Blotter Sync

A headless [Blotter](https://github.com/LASR-0/Blotter) that runs on your home server and keeps every Blotter desktop's planner in sync through it.

It runs the same core as the app, so besides syncing it takes over the planner's automatic duties for paired devices: end-of-day rollover, calendar subscription refreshes, holiday upkeep and daily backups. Devices keep working offline and catch up when they can reach the server again.

> **Status:** in development, towards 1.0.0 alongside Blotter 1.2.1. Nothing here is ready to run against a real planner yet.

## Compatibility

Blotter and Blotter Sync release separately. A server works with a Blotter version when their sync protocol and sync schema match; the app tells you which side to update when they don't.

| Blotter Sync | Blotter | Sync protocol | Sync schema |
|---|---|---|---|
| 1.0.x | 1.2.1 and later 1.2.x | 1 | 1 |

Blotter 1.2.0 and earlier have no sync.

## Running

Planned for 1.0.0: a Docker image (`ghcr.io/lasr-0/blotter-sync`, amd64 and arm64) with Compose profiles for LAN, Caddy and Tailscale, and plain binaries for Linux and Windows.

Until then, from source:

```sh
cargo run -- serve                          # listens on 0.0.0.0:7458
cargo run -- serve --listen 127.0.0.1:7458
curl http://127.0.0.1:7458/v1/hello
```

Admin commands (run them beside a running server, or with `docker exec`):

```sh
blotter-sync pair              # a single-use pairing code, valid 10 minutes
blotter-sync devices           # paired devices, last seen, revoked or not
blotter-sync revoke Laptop     # disconnect a device by name or id
```

| Setting | Flag | Environment |
|---|---|---|
| Listen address | `--listen` | `BLOTTER_SYNC_LISTEN` |
| Data directory | `--data-dir` | `BLOTTER_SYNC_DATA_DIR` |
| Daily backups kept | `--keep-backups` | `BLOTTER_SYNC_KEEP_BACKUPS` (default 20) |
| Log filter | | `BLOTTER_SYNC_LOG` (e.g. `debug`) |

The default port is **7458**. The data directory holds the canonical planner (`blotter.db`) and the server's own state (`server.db`); by default it is `%ProgramData%\Blotter Sync` on Windows and `~/.local/share/blotter-sync` on Linux (`$XDG_DATA_HOME` if set).

## Development

Needs Rust 1.95 or later. Development happens on Windows; CI builds and tests on Windows and Linux.

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

### `blotter-core`

The planner core comes from the Blotter repo, pinned to a release in `Cargo.toml`. To build against a Blotter checkout next to this one instead (to change the core and the server together), create `.cargo/config.toml` (git-ignored):

```toml
[patch."https://github.com/LASR-0/Blotter.git"]
blotter-core = { path = "../Blotter/crates/blotter-core" }
```

This rewrites `blotter-core`'s source in `Cargo.lock`; don't commit that change (`git checkout Cargo.lock`). When the server needs a newer core, the pin in `Cargo.toml` moves to that Blotter commit or tag, and CI checks it.

## Licence

[MIT](LICENSE)
