<div align="right">

**English** · [中文](getting-started.zh.md)

</div>

# Getting started

How to build, run and deploy Solidway. For what it does, see the [README](../README.md).

## Requirements

- Rust stable (the workspace uses edition 2024)
- Node.js 22+ and pnpm 10 (for the console)
- On Linux: `pkg-config` and `libwebkit2gtk-4.1-dev` are only needed for the desktop app; the server binary itself has no system dependencies beyond glibc.

## Build from source

The release build embeds the console, so build the console first:

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir web build

cargo build --release -p solidway
```

The binary is `target/release/solidway`.

## Run

```bash
./target/release/solidway ./data --mode standalone
```

First start:

1. creates `./data/`,
2. prints a one-time bootstrap token,
3. listens on `127.0.0.1:6565`.

```text
solidway: listening addr=127.0.0.1:6565
solidway: bootstrap token=<token> url=http://127.0.0.1:6565/bootstrap#<token>
```

Open the URL, set the admin password, then work in the console: create a route, add provider credentials, create an API key, and point your SDK at `http://<host>:6565/v1`.

Useful flags:

| Flag | Meaning |
|---|---|
| `--mode standalone` \| `desktop` | Desktop mode enables restart requests and rejects stdin-EOF exit |
| `--listen <addr>` | Override the bind address and port for this run (not persisted) |
| `--web-assets <dir>` | Serve the console from disk instead of the embedded copy |
| `--reset-password` | Put the admin account back into the unset-password state and exit |
| `--master-key <hex\|base64>` | Supply the master key for recovery only |

`solidway --help` prints the same list.

## Deploy on Linux

```bash
./scripts/package-linux.sh        # -> dist/linux/*.deb, *.rpm
```

The packages install:

- `/usr/bin/solidway`
- `/usr/lib/systemd/system/solidway.service` (system user `solidway`, data root `/var/lib/solidway`, `CAP_NET_BIND_SERVICE` for ports 80/443)
- `/var/lib/solidway` as data root

Packages are built for the distribution you build on: the binary links glibc, so build on the oldest distribution you intend to support.

## Desktop app

A Tauri shell that supervises the gateway locally, opens the console in a webview, and can start at login and live in the tray. Linux, Windows and macOS.

Build with:

```bash
./scripts/package-linux-app.sh     # deb + AppImage
./scripts/package-windows.sh       # NSIS/MSI + portable zip
./scripts/package-mac.sh           # .app + .dmg (signed)
./scripts/notarize-mac.sh          # notarize + staple .app/.dmg (release; needs API key)
```

Each package ships the gateway binary next to the application executable, so a single install is enough. macOS release builds are signed with Developer ID and notarized (no Gatekeeper prompt); Windows builds are unsigned — expect SmartScreen prompts on first launch.

## Data directory

`<data_root>` is the only stateful location:

| Platform | Path |
|---|---|
| Linux | `~/.local/share/xyz.solidway.desktop` |
| macOS | `~/Library/Application Support/xyz.solidway.desktop` |
| Windows | `%APPDATA%\xyz.solidway.desktop` |
| Linux package | `/var/lib/solidway` |

| File | Contents |
|---|---|
| `solidway.db` | users, routes, provider credentials (encrypted), settings, sessions |
| `usage_detail.db` | per-request usage detail (retention configurable) |
| `master.key` | encryption key for provider credentials — **back up separately** |
| `solidway.log` | gateway stdout/stderr (desktop app only) |

Without `master.key`, provider credentials in `solidway.db` cannot be decrypted. Back up the pair together, and never commit either.

## TLS

Two supported shapes:

1. **Built-in ACME** — configure public domains in the console (**System → HTTPS**); Solidway requests and renews certificates with `http-01` or `tls-alpn-01` and needs ports 80/443.
2. **Reverse proxy** — terminate TLS upstream and keep Solidway on loopback. Forward the original `Host` and client IP headers.

## Common operations

```bash
# Restart the service
sudo systemctl restart solidway

# Follow the gateway log
journalctl -u solidway -f

# Reset the admin password (unset state; set a new one in the console afterwards)
sudo -u solidway /usr/bin/solidway /var/lib/solidway --reset-password
```
