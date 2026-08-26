# Cangling Keeper

A local desktop application built with **Rust + Tauri v2**, targeting
**Windows** and **Linux**. It manages a list of remote hosts and lets you run
commands over SSH with one click.

## Features

- Define, edit, and delete SSH hosts (name, hostname/IP, port, username, password).
- Select a host and run a command over SSH, showing stdout/stderr/exit status.
- Define SSH local port-forward tunnels (`ssh -N -L ...`) — paste an SSH command
  to auto-fill the form, or fill the fields manually.
- Connect/disconnect tunnels; the list shows live connection state.
- Tunnel auth via password or an ed25519 key pair (generate one in-app).
- Hosts and tunnels persist to a SQLite database at `./config/data.sql`.

> **Security note (current state):** passwords are stored in plain text in the
> local SQLite database, and SSH host keys are not yet verified
> (trust-on-first-use is a planned improvement). Treat this as a local tool on a
> trusted machine for now.

## Project structure

```
cangling-keeper/
├── Cargo.toml              # Rust crate + Tauri dependencies
├── build.rs                # Tauri build script
├── tauri.conf.json         # Tauri v2 configuration
├── capabilities/           # Tauri v2 capability (permission) files
│   └── default.json
├── icons/                  # App icons (window + bundle)
├── src/
│   ├── main.rs             # Entry point
│   ├── lib.rs              # Tauri app + #[tauri::command] handlers
│   ├── host.rs             # Host model
│   ├── tunnel.rs           # Tunnel model + SSH command parsing
│   ├── store.rs            # SQLite persistence (hosts + tunnels)
│   └── ssh.rs              # SSH exec, port forwarding, key generation (russh)
└── ui/                     # Frontend (plain HTML/CSS/JS, no bundler)
    ├── index.html
    ├── styles.css
    └── main.js
```

The frontend is intentionally dependency-free. `withGlobalTauri` is enabled in
`tauri.conf.json`, so JavaScript can call Rust commands via
`window.__TAURI__.core.invoke(...)` without npm or a bundler.

SSH is implemented with [`russh`](https://crates.io/crates/russh) (pure-Rust,
async) using its `ring` crypto backend, which avoids OpenSSL/libssh2 C
dependencies and needs no extra system libraries beyond a C compiler.

## Prerequisites

### Both platforms

- [Rust](https://rustup.rs) (stable toolchain)
- [Tauri CLI](https://v2.tauri.app/reference/cli/):

  ```sh
  cargo install tauri-cli --version "^2" --locked
  ```

### Linux (Debian/Ubuntu)

#### Build dependencies (compile-time only)

These are required only to *build* the app (headers + toolchain). End users do
not need them:

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

#### Runtime dependencies (needed to run the binary)

The compiled binary dynamically links against these shared libraries. Installing
the `-dev` packages above pulls them in automatically on a dev machine, but end
users need the runtime (non-`-dev`) versions:

| Build-time (`-dev`)             | Runtime (end users)         |
| ------------------------------- | --------------------------- |
| `libwebkit2gtk-4.1-dev`         | `libwebkit2gtk-4.1-0`       |
| `libxdo-dev`                    | `libxdo3`                   |
| `libssl-dev`                    | `libssl3`                   |
| `librsvg2-dev`                  | `librsvg2-2`                |
| `libayatana-appindicator3-dev`  | `libayatana-appindicator3-1` |

When distributing as a `.deb`, Tauri declares these runtime dependencies
automatically so `apt` installs them for the user.

(For other distros see https://v2.tauri.app/start/prerequisites/)

### Windows

- Microsoft Visual Studio **C++ Build Tools** with the "Desktop development
  with C++" workload (MSVC).
- Rust **MSVC** toolchain (`rustup default stable-msvc`).
- WebView2 runtime — preinstalled on Windows 10/11, no action needed.

## Run (development)

```sh
cargo tauri dev
```

This opens the window and serves `ui/` with hot reload.

## Build a release bundle

```sh
cargo tauri build
```

Bundles are written to `target/release/bundle/`:

- Windows: `.msi` / `.exe` (NSIS)
- Linux: `.deb`, `.rpm`, and/or AppImage

## Adding commands

1. Add a function in `src/lib.rs` (or a module) and mark it with `#[tauri::command]`.
2. Register it in `.invoke_handler(tauri::generate_handler![...])`.
3. Call it from `ui/main.js` with `window.__TAURI__.core.invoke("name", { args })`
   (argument names use camelCase, e.g. Rust `host_id` → JS `hostId`).

### Data location

Hosts and tunnels are stored in a SQLite database at `./config/data.sql`,
relative to the directory the app is launched from (in development this is the
project root). Generated SSH keys are written to `./config/keys/`. The
`config/` directory is created automatically on first launch.
