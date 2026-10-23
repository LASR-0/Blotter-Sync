# Running Blotter Sync

Blotter Sync runs on one machine that stays on (a home server, a NAS, a Raspberry Pi, an always-on PC) and every Blotter desktop pairs with it. Check the [compatibility table](../README.md#compatibility) first: your Blotter version and your Blotter Sync version have to match on it.

Pick how your devices will reach it:

| Tier | Reach it from | Address in Blotter | Section |
|---|---|---|---|
| LAN | Home only | `http://<host>:7458` | [Docker on your LAN](#docker-on-your-lan) |
| + Tailscale | Anywhere your devices run Tailscale | `https://blotter.<tailnet>.ts.net` | [Add Tailscale](#add-tailscale) |
| + Caddy | Anywhere, through your own domain | `https://sync.example.com` | [Add a public domain with Caddy](#add-a-public-domain-with-caddy) |

Every tier keeps the LAN address too. In Blotter, list the addresses in order (LAN first) under **Settings → Sync**, and the app uses the first one that answers.

No Docker? See [Linux without Docker](#linux-without-docker) or [Windows](#windows). The rest of this guide ([pairing](#pairing-a-device), [backups](#backups-and-restoring), [upgrading](#upgrading), [disconnecting a device](#disconnecting-a-device)) applies to every install.

## Docker on your LAN

Needs Docker with Compose. The image is `ghcr.io/lasr-0/blotter-sync`, for amd64 and arm64.

1. Copy `compose.yaml` and `.env.example` from this folder into a folder on the server (add `Caddyfile` and `tailscale/serve.json` if you'll use those tiers).
2. Rename `.env.example` to `.env`. The defaults are fine for the LAN.
3. Start it:

   ```sh
   docker compose up -d
   ```

4. Open `http://<host>:7458` in a browser on the LAN. The status page says the server is running and how to pair.

The planner, the server's state and the daily backups live in the `data` volume. The container runs as a non-root user with a read-only filesystem.

Run admin commands with `docker compose exec`:

```sh
docker compose exec blotter-sync blotter-sync pair
```

## Add Tailscale

The Tailscale sidecar joins your tailnet as its own machine (named `blotter` by default) and serves Blotter Sync over HTTPS at `https://blotter.<tailnet>.ts.net`. Devices away from home reach it whenever Tailscale is connected on them.

1. In the [Tailscale admin console](https://login.tailscale.com/admin/dns), turn on **MagicDNS** and **HTTPS Certificates**.
2. Create an auth key under **Settings → Keys**. Make it reusable or not, but **not ephemeral**, so the machine stays on your tailnet across restarts.
3. In `.env`, set `TS_AUTHKEY` to the key. Change `TS_HOSTNAME` if you want another machine name.
4. Start with the profile:

   ```sh
   docker compose --profile tailscale up -d
   ```

5. On a device running Tailscale, open `https://blotter.<tailnet>.ts.net`. The first visit can take a few seconds while the certificate is issued.

Once the machine shows up in the admin console, the key isn't needed again (its state is kept in the `tailscale-state` volume), so you can delete it from `.env`. Add the `.ts.net` address to each device's address list in Blotter, after the LAN one.

## Add a public domain with Caddy

Caddy gets a certificate from Let's Encrypt for your domain and proxies to Blotter Sync. Only do this if you need to sync from devices without Tailscale: the pairing and sync endpoints are then reachable from the internet. They need a device token or a pairing code, both rate-limited, but Tailscale exposes nothing.

1. Point a DNS name (for example `sync.example.com`) at your public IP, and forward ports **80** and **443** on your router to the server.
2. In `.env`, set `BLOTTER_DOMAIN=sync.example.com`.
3. Start with the profile:

   ```sh
   docker compose --profile caddy up -d
   ```

4. Open `https://sync.example.com/v1/hello` from outside your network. It should answer with the server's details.

From the internet the status page answers 404, because it lists your devices. It still shows on the LAN. Delete the `@public_status` lines in `Caddyfile` if you want it public.

To reach Blotter Sync **only** through Caddy or Tailscale, set `BLOTTER_SYNC_PORT=127.0.0.1:7458` in `.env`.

Profiles combine: `docker compose --profile caddy --profile tailscale up -d`.

## Linux without Docker

Download `blotter-sync-<version>-linux-amd64.tar.gz` (or `-arm64`) from the [releases](https://github.com/LASR-0/Blotter-Sync/releases). It needs glibc 2.35 or later (Ubuntu 22.04, Debian 12, Fedora 36 and later).

```sh
tar -xzf blotter-sync-*-linux-*.tar.gz
cd blotter-sync-*/
sudo useradd --system --no-create-home --shell /usr/sbin/nologin blotter-sync
sudo install -m 0755 blotter-sync /usr/local/bin/
sudo curl -fsSLo /etc/systemd/system/blotter-sync.service \
  https://raw.githubusercontent.com/LASR-0/Blotter-Sync/main/deploy/blotter-sync.service
sudo systemctl daemon-reload
sudo systemctl enable --now blotter-sync
```

The service runs as the `blotter-sync` user, keeps its data in `/var/lib/blotter-sync` (mode 0700) and listens on port 7458. Logs: `journalctl -u blotter-sync`.

Run admin commands as that user, against the same data dir:

```sh
sudo -u blotter-sync blotter-sync --data-dir /var/lib/blotter-sync pair
```

If a firewall is on, open the port to the LAN (`sudo ufw allow from 192.168.0.0/16 to any port 7458`, for example). For the other tiers, install Tailscale on the host and run `sudo tailscale serve --bg 7458`, or put Caddy in front with the same `Caddyfile`, changing `blotter-sync:7458` to `localhost:7458`.

## Windows

Download `blotter-sync-<version>-windows-x64.zip` from the [releases](https://github.com/LASR-0/Blotter-Sync/releases) and unzip it somewhere permanent, such as `C:\Program Files\Blotter Sync`.

**Try it from a terminal:**

```powershell
& "C:\Program Files\Blotter Sync\blotter-sync.exe"
```

It keeps its data in `C:\ProgramData\Blotter Sync` and listens on port 7458. Stop it with Ctrl+C.

**Keep the data private.** Other accounts on the PC can read that folder by default. Limit it to your account:

```powershell
icacls "C:\ProgramData\Blotter Sync" /inheritance:r /grant:r "${env:USERNAME}:(OI)(CI)F" "SYSTEM:(OI)(CI)F"
```

**Start it with Windows** using Task Scheduler:

1. Open **Task Scheduler** → **Create Task** (not "Basic Task").
2. **General:** name it `Blotter Sync`; choose **Run whether user is logged on or not**. Leave the account as your own, so the admin commands can read the same data folder.
3. **Triggers:** **New** → **At startup**.
4. **Actions:** **New** → **Start a program** → `C:\Program Files\Blotter Sync\blotter-sync.exe`, arguments `serve`.
5. **Settings:** untick **Stop the task if it runs longer than 3 days**.
6. Click OK, enter your password, then right-click the task → **Run**. Check `http://localhost:7458`.

**Let other devices reach it.** Allow the port through Windows Firewall, for private networks only, from an administrator PowerShell:

```powershell
New-NetFirewallRule -DisplayName "Blotter Sync" -Direction Inbound -Protocol TCP -LocalPort 7458 -Profile Private -Action Allow
```

**Away from home:** install Tailscale on the PC and run `tailscale serve --bg 7458` for `https://<pc-name>.<tailnet>.ts.net`, or run Caddy for Windows with the same `Caddyfile` (change `blotter-sync:7458` to `localhost:7458`).

Admin commands run from any terminal on that PC: `& "C:\Program Files\Blotter Sync\blotter-sync.exe" pair`.

## Pairing a device

1. On the server, open a pairing code:

   ```sh
   blotter-sync pair          # or: docker compose exec blotter-sync blotter-sync pair
   ```

   The code works once, for 10 minutes. Five wrong tries burn it.
2. In Blotter, go to **Settings → Sync** → **Home server** → **Pair**, enter the server's address and the code, and name the device ("Desktop", "Laptop").
3. Add the other addresses for this server (Tailscale, your domain) to the address list.

For the first device, Blotter offers to send its planner to the (empty) server. For a new install, choose **Connect to a home server** when Blotter first starts, so it fetches the planner instead of creating a fresh one.

Plain `http://` addresses are only allowed on the LAN and Tailscale. Anything else needs `https://`.

## Backups and restoring

The server takes a backup of the planner every day and keeps the last 20 (`BLOTTER_SYNC_KEEP_BACKUPS` changes that). Take one yourself before anything risky:

```sh
blotter-sync snapshot --note "before cleaning up tags"
blotter-sync snapshots                # daily, manual and automatic ones, newest first
blotter-sync restore <id>
```

Restoring keeps a "before restore" snapshot first, then every device fetches the restored planner. Restoring and importing work from Blotter too, in **Snapshots**, once a device is paired.

To back up the server itself, copy its data folder while it's stopped: the `data` volume (`docker run --rm -v blotter-sync_data:/data -v "$PWD":/backup alpine tar -czf /backup/blotter-sync.tar.gz -C /data .`), `/var/lib/blotter-sync`, or `C:\ProgramData\Blotter Sync`.

## Upgrading

Check the [compatibility table](../README.md#compatibility): a new sync protocol or schema means upgrading Blotter on every device and Blotter Sync together. Devices that don't match the server stop syncing, and the app says which side to update. They keep working offline.

- **Docker:** `docker compose pull && docker compose up -d`. The `1` tag follows every 1.x release; set `BLOTTER_SYNC_TAG` in `.env` to pin one.
- **Linux:** replace `/usr/local/bin/blotter-sync`, then `sudo systemctl restart blotter-sync`.
- **Windows:** end the task in Task Scheduler, replace `blotter-sync.exe`, and run the task again.

The server updates its own data on first start. Take a snapshot first if you like.

## Disconnecting a device

```sh
blotter-sync devices          # names, last seen, status
blotter-sync revoke Laptop    # by name or id
```

The device stops syncing at once and keeps its own copy of the planner. It can pair again later with a new code. A device can also disconnect itself from **Settings → Sync** → **Disconnect**.
