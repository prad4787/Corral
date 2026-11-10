# Corral

> Working name. A free, open-source local PHP development environment for macOS,
> built with Rust and Tauri 2.

Corral gives you one-click PHP sites on `*.test` domains: multiple PHP versions,
per-site isolation, local HTTPS, databases and other services, a mail catcher,
a dump viewer, Xdebug, and a log viewer. **Every feature is free.** There is no
"Pro" tier, no license checks, and no telemetry by default.

## Status

Early development (Phase 1). The CLI works today in **high-port mode**
(sites on `:18080`/`:18443`, no privileged helper yet).

```sh
cargo build -p corral-cli
alias corral=./target/debug/corral

corral setup --high-ports                 # downloads Caddy (checksum-verified)
corral php install 8.4 --allow-unverified # static-php.dev publishes no checksums yet
corral park ~/Sites                       # every folder becomes <folder>.test
corral trust                              # prints the one-time sudo commands for DNS + HTTPS
corral start                              # runs DNS, PHP-FPM and Caddy
```

Other commands: `link`, `isolate 8.3`, `secure`/`unsecure`, `use 8.4`, `tld`, `doctor`.
Desktop app: `pnpm -C app install && pnpm -C app tauri dev`.

To try the whole flow without touching your system, run `scripts/spike-e2e.sh`.

## Repository layout

| Path | What |
|---|---|
| `crates/corral-core` | Paths, config, site registry, framework drivers, PHP layout |
| `crates/corral-dns` | Wildcard resolver for the local TLD |
| `crates/corral-tls` | Name-constrained local CA + per-site certificates |
| `crates/corral-proxy` | Caddy JSON generation |
| `crates/corral-supervisor` | Child-process supervision |
| `crates/corral-engine` | Ties it all together; runtime downloads |
| `crates/corral-cli` | The `corral` binary |
| `app/` | Tauri 2 + Svelte desktop app |

## Docs

See:

- [docs/FEATURES.md](docs/FEATURES.md): the full feature list and how each feature will be built
- [docs/PLAN.md](docs/PLAN.md): phased roadmap with acceptance criteria
- [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md): rules for researching other apps without copying them
- [CONTRIBUTING.md](CONTRIBUTING.md): commit and branch conventions

## Target platform

macOS 13+ (Ventura or later), on Apple Silicon and Intel. Other platforms are out of scope for v1.

## License

MIT. See [LICENSE](LICENSE).
