# Cangling Keeper — Project Summary

## What it is

**Cangling Keeper** (维护中心) is a local desktop application built with
**Rust + Tauri v2**, targeting **Windows** and **Linux**. It manages a list of
remote hosts and lets you run commands over SSH with one click, plus set up
persistent SSH port-forward tunnels.

## Tech stack

| Layer      | Technology                                            |
| ---------- | ----------------------------------------------------- |
| Backend    | Rust (edition 2024)                                   |
| UI shell   | Tauri v2 (`withGlobalTauri` enabled, no npm/bundler)  |
| Frontend   | Plain HTML/CSS/JS + xterm.js (in `ui/vendor/`)        |
| SSH        | `russh` (pure-Rust, async, `ring` crypto backend)     |
| Storage    | SQLite via `rusqlite` (bundled)                        |
| Async      | `tokio`                                               |
| Tunneling  | `tun` crate (async feature)                           |

## Key features

- Define, edit, delete SSH hosts (name, hostname/IP, port, username, password).
- Run a command over SSH on a selected host; show stdout/stderr/exit status.
- Define SSH local port-forward tunnels (`ssh -N -L ...`) — paste an SSH command
  to auto-fill, or fill fields manually.
- Connect/disconnect tunnels with live connection state.
- Tunnel auth via password or an ed25519 key pair (generated in-app).
- Hosts and tunnels persist to SQLite at `./config/data.sql`; keys at
  `./config/keys/`.

> **Security note (current state):** passwords are stored in plain text in the
> local SQLite database, and SSH host keys are not yet verified
> (trust-on-first-use is planned). Treat as a local tool on a trusted machine.

## Project structure

```
cangling-keeper/
├── Cargo.toml              # crate + Tauri deps (root-level Tauri project)
├── build.rs                # Tauri build script
├── tauri.conf.json         # Tauri v2 config (frontendDist: "ui")
├── capabilities/default.json
├── icons/
├── src/
│   ├── main.rs             # entry point
│   ├── lib.rs              # Tauri app + #[tauri::command] handlers
│   ├── host.rs             # host model
│   ├── host_actions.rs     # host-related actions
│   ├── tunnel.rs           # tunnel model + SSH command parsing
│   ├── store.rs            # SQLite persistence (hosts + tunnels)
│   ├── ssh.rs              # SSH exec / port-forward / keygen (russh)
│   ├── auth.rs             # authentication helpers
│   ├── certificate.rs      # certificate handling
│   ├── proxy.rs            # proxy/tunnel helpers
│   └── scripts/            # bundled shell scripts (update/version probing)
├── ui/                     # frontend (no bundler)
│   ├── index.html
│   ├── styles.css
│   ├── main.js
│   └── vendor/             # xterm.js, xterm.css, xterm-addon-fit.js
├── config/                 # runtime data (data.sql, keys/)
├── gen/schemas/            # generated JSON schemas
└── doc/                    # documentation
```

## Build prerequisites

### Linux (Debian/Ubuntu)
```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### Windows
- MSVC C++ Build Tools ("Desktop development with C++" workload)
- Rust MSVC toolchain
- WebView2 runtime (preinstalled on Win 10/11)

### Both
```sh
cargo install tauri-cli --version "^2" --locked
```

## Run / build

```sh
cargo tauri dev      # dev with hot reload
cargo tauri build    # release bundles -> target/release/bundle/
```

Bundles:
- Windows: `.msi` / `.exe` (NSIS)
- Linux: `.deb`, `.rpm`, AppImage

## CI/CD

`.github/workflows/release.yml` builds on **ubuntu-22.04** and
**windows-latest** on every `v*` tag and creates a draft GitHub release with the
bundled artifacts. Push a tag (`git tag v0.1.0 && git push --tags`) to trigger
it.

## Notes

- Frontend calls Rust via `window.__TAURI__.core.invoke("name", { camelCaseArgs })`.
- Data directory `./config/` is created automatically on first launch.
- `[profile.release]` uses `lto`, `opt-level="s"`, `strip`, and `panic="abort"`
  for small binaries.
