# Roadmap

Each phase ends with a tagged release and a checklist that must pass on a
**clean macOS user account** (Apple Silicon, and Intel when available).
Feature numbers refer to [FEATURES.md](FEATURES.md).

Rules for every phase:

- One feature per branch, merged to `main` through a PR with green CI (see [CONTRIBUTING.md](../CONTRIBUTING.md)).
- Whenever the core changes the system (files, keychain, resolver), it ships with an **undo path**, and that undo is tested.
- No step requires a passwordless sudo rule.

## Status (2026-11-10)

| Task | State | Notes |
|---|---|---|
| 0.1 Workspace + app shell | ✅ Done | Cargo workspace with 8 crates, Tauri 2.11 + SvelteKit |
| 0.2 CI | ✅ Done | `.github/workflows/ci.yml` (not run yet: no remote) |
| 0.3 Spike: ports | ✅ Done | Root is needed for `127.0.0.1:80/443`, so we keep fd-passing ([ADR-0003](adr/0003-privileged-helper.md)) |
| 0.4 Spike: helper | ⏳ Open | Needs a signing identity to test SMAppService |
| 0.5 Spike: PHP | 🟡 Partial | static-php.dev builds exist but have **no checksums**. Installs need `--allow-unverified` until we host a manifest |
| 0.6 Spike: Caddy | 🟡 Ready to run | `scripts/spike-e2e.sh` runs the whole flow in a sandbox. It hasn't run in this environment because running downloaded binaries needs your approval |
| 1.1 Config/state | ✅ Done | TOML for config and sites. SQLite is deferred to mail/dumps (Phase 3) |
| 1.2 PHP manager | ✅ Done | Discovery, configs, shims, install from static-php.dev |
| 1.3 Supervisor | ✅ Done | Process groups, backoff, SIGTERM→SIGKILL |
| 1.4 DNS | ✅ Done | Own resolver. `/etc/resolver` is written by hand with `corral trust` until the helper exists |
| 1.5 TLS | ✅ Done | Name-constrained CA, 397-day leaves |
| 1.6 Proxy | ✅ Done | Caddy JSON with live `--watch` reload |
| 1.7 Site registry | ✅ Done | Park, link, isolate, secure, 9 drivers |
| 1.8 CLI | ✅ Done (direct) | Talks to the files directly. The HTTP API (5.1) is still open |
| 1.9 UI | 🟡 Partial | Sites, detail, PHP, start/stop. Onboarding, tray and launch-at-login are open |
| 1.10 Uninstall | ⏳ Open | |
| 3.1 MySQL | ✅ Done (pulled forward) | 8.4.11 pinned by SHA-256; `corral service`/`corral db`; validated with a real Laravel 11 app (48 migrations + seeders) |

**Real-machine validation (2026-11-10, macOS 26.6.2 arm64, another local PHP tool running alongside):**
`scripts/spike-e2e.sh` passes 13/13 checks. Two real Laravel apps (11 and 12) are served over trusted HTTPS on :18443, with another tool's DNS
resolving `.test`. Bugs found and fixed this way: orphaned children after SIGTERM, an FPM-socket startup race, deprecation log
flooding, pools not reloading after config changes, MySQL datadir parents missing, and MySQL root not reachable over TCP.

---

## Phase 0: Foundations and risk spikes (target: 1–2 weeks)

Goal: prove the three risky parts work before building the UI on top of them.

| Task | Output |
|---|---|
| 0.1 Cargo workspace + Tauri 2 app skeleton + Svelte UI shell | `pnpm tauri dev` opens a window |
| 0.2 CI: `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `pnpm check`, macOS runner | Green pipeline |
| 0.3 **Spike: ports.** Can an unprivileged process bind `127.0.0.1:80`? Test fd-passing from a root process | Note in ADR-0003 |
| 0.4 **Spike: helper.** SMAppService daemon + XPC round trip from the Tauri app (signed and ad-hoc builds) | Working sample + ADR-0003 marked Accepted |
| 0.5 **Spike: PHP.** Build PHP 8.4 CLI + FPM with static-php-cli in CI (arm64 + x86_64), with a common extension set | Downloadable artifacts + checksum manifest |
| 0.6 **Spike: Caddy.** Laravel + WordPress through `php_fastcgi` over a Unix socket; measure reload time | ADR-0002 marked Accepted |

**Exit:** all spikes are documented, and each ADR is marked Accepted or replaced.

## Phase 1: MVP, "sites just work" (target: 4–6 weeks), `v0.1.0`

Features: 1.1–1.3, 1.5–1.8, 1.10–1.15, 1.18, 1.20, 1.21, 2.1, 2.2, 2.9, 5.1.

Order:

1. `corral-core` config and state (SQLite migrations) and the paths module.
2. PHP version manager: manifest, download with SHA-256 checks, install, remove, set default, shims.
3. Supervisor: FPM pool per version, restart on crash, log capture.
4. DNS resolver + helper `WriteResolver`.
5. TLS CA + helper `TrustCA`, and per-site certs.
6. Proxy config generation + Caddy lifecycle.
7. Site registry: park, link, drivers, isolate.
8. Local API + `corral` CLI.
9. UI: onboarding, sites list and detail, tray menu.
10. Uninstall flow.

**Acceptance (on a clean account):**

- [ ] Fresh install → onboarding → `laravel new demo` in a parked dir → `https://demo.test` loads with a trusted cert in under 5 minutes, with no Terminal sudo.
- [ ] `corral isolate 8.3` in one site and 8.4 in another, and both serve the correct `phpversion()`.
- [ ] `php -v` in a new terminal shows the default version.
- [ ] Reboot → services come back up when "launch at login" is on.
- [ ] `corral uninstall` leaves no files in `/etc/resolver`, no CA in the keychain, no helper, and no PATH lines.
- [ ] Unit tests for drivers, config generation and the resolver. An integration test runs a real FPM + Caddy in CI.

## Phase 2: Daily-driver polish (target: 3–4 weeks), `v0.2.0`

Features: 1.4, 1.9, 1.16, 1.17, 2.3, 2.4, 2.5, 2.7, 5.2.

**Acceptance:**

- [ ] Toggle a PHP extension in the UI → FPM reloads → `php -m` reflects the change.
- [ ] Change the TLD to `.localhost` → all sites are reachable at the new TLD, and the old resolver file is removed.
- [ ] Node 20 and 22 install side by side, and you can switch between them.
- [ ] New project wizard creates a Laravel starter kit and links it.

## Phase 3: All "Pro" features, free (target: 6–8 weeks), `v0.3.0`

Features: 2.8, 3.1–3.11, 4.1–4.6, 4.8.

Order: service framework (a manifest per service, a shared supervisor and an
instance model) → Valkey + MySQL + PostgreSQL → the rest of the services →
mail catcher → dumps package + receiver → Xdebug debug pool → log viewer → sharing.

**Acceptance:**

- [ ] Two MySQL instances on different ports run at the same time and survive an app restart.
- [ ] `Mail::raw()` from a Laravel site shows up in the mail UI with attachments.
- [ ] `dump($user)` in a controller and in `php artisan tinker` both show up in the dumps window. The query watcher shows SQL with timings.
- [ ] Setting a breakpoint in VS Code/PhpStorm with the Xdebug cookie stops at it, and normal requests still go through the non-debug pool at full speed.
- [ ] Every service lists its license in the UI before download.

## Phase 4: Release quality (target: 3–4 weeks), `v1.0.0`

Features: 1.19, 2.6, 4.7, 4.9, 5.3.

- Developer ID signing + notarization in CI. Signed updater manifests on GitHub Releases.
- Homebrew cask.
- Docs site, plus a migration guide from Valet and similar tools (import parked paths and links).
- Opt-in crash reporting.
- Performance budget: idle CPU under 0.5%, idle RAM for the app under 150 MB (not counting PHP).

**Acceptance:** a notarized `.dmg` installs on macOS 13 through 26 without
Gatekeeper warnings, and auto-update from 0.9 → 1.0 works.

---

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| SMAppService fails on unsigned builds | Contributors can't test the helper | `corral dev-setup` osascript fallback (ADR-0003) |
| No Developer ID certificate | Can't distribute without Gatekeeper prompts | Budget for the Apple Developer Program before Phase 4 |
| Static PHP builds lack some extensions | Some apps break | Make the extension set configurable, and let users add PECL extensions per version (phase 2+) |
| Licenses of the service binaries (SSPL/AGPL/RSAL) | Can't legally redistribute them | Download only from upstream, never bundle, and show the license (§3) |
| Port 80/443 already used (Valet, Docker, Apache) | Install fails | Detect it during onboarding, show which process holds the port, and offer alternate ports |
| Conflicts with an existing Valet/Homebrew PHP on the PATH | Wrong `php` runs | Onboarding checks the PATH order, and `corral doctor` reports the problem |
