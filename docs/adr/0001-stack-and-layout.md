# ADR-0001: Stack and repository layout

- **Status:** Accepted
- **Date:** 2026-11-10

## Context

We need a macOS desktop app, a CLI and a privileged helper that share one core
library. The core does process supervision, file generation and networking.

## Decision

- **Rust** for all native code, in a single Cargo workspace.
- **Tauri 2** for the desktop shell, with **Svelte 5 + TypeScript + Vite** for the UI (managed with pnpm).
- **SQLite** (rusqlite) for state (sites, services, mails, dumps), and TOML for user config.
- **tokio** for async work, with a single supervisor owning all child processes.

```
corral/
├─ Cargo.toml                 workspace
├─ crates/
│  ├─ corral-core/            domain: sites, drivers, php, services, config, state
│  ├─ corral-supervisor/      child process lifecycle, logs, health checks
│  ├─ corral-dns/             *.test resolver (hickory-dns)
│  ├─ corral-tls/             local CA + leaf certs (rcgen)
│  ├─ corral-proxy/           web server config generation (Caddy admin API)
│  ├─ corral-api/             local HTTP API (axum), used by the UI and CLI
│  ├─ corral-mail/            SMTP sink + store
│  ├─ corral-dumps/           dump receiver (unix socket) + store
│  ├─ corral-helper-proto/    XPC message types shared with the helper
│  ├─ corral-helper/          privileged launch daemon (binary)
│  └─ corral-cli/             `corral` binary
├─ app/                       Tauri app
│  ├─ src/                    Svelte UI
│  └─ src-tauri/              Tauri Rust shell (thin; uses corral-core)
├─ php/corral-dumps/          Composer package for dumps and watchers (PHP)
└─ docs/
```

The UI never touches the system directly. It calls Tauri commands that go
through `corral-core`, which is the same code path the CLI uses (through `corral-api`).

### Amendment (2026-11-10)

- Added **`corral-engine`**. It renders configs, runs DNS, FPM and Caddy, and
  downloads runtimes. Both the CLI and the app use it.
- `corral-api`, `corral-mail`, `corral-dumps` and `corral-helper*` will be created when
  their phase starts. No empty placeholder crates.
- Config and the site registry use **TOML** files (`config.toml`, `sites.toml`),
  which are easy to read and diff. SQLite is kept for high-volume data (mail, dumps).

## Consequences

- One language for every native part, and one test suite.
- The Tauri shell stays thin, so the core can be tested without a GUI.
- macOS-specific code lives behind `cfg(target_os = "macos")` modules, which leaves room to port later.
