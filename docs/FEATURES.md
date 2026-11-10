# Feature inventory

Every feature from the reference research,
with how Corral will build it from open-source parts. **All features are free.**

Legend:

- **Ref tier:** whether the feature is free or paid in the reference app.
- **Phase:** see [PLAN.md](PLAN.md).
- **Build:** what we write ourselves in Rust or TypeScript.
- **Reuse:** upstream open-source components, downloaded on demand unless stated otherwise.

## 1. Core environment

| # | Feature | Ref tier | Corral approach | Build / Reuse | Phase |
|---|---|---|---|---|---|
| 1.1 | Multiple PHP versions (7.4 to 8.5) | Free | Download prebuilt static CLI + FPM binaries per minor version into `~/Library/Application Support/Corral/php/<ver>/` | Reuse: static-php-cli (MIT) to build our own release artifacts in GitHub Actions. PHP License 3.01 allows redistribution | 1 |
| 1.2 | Global default PHP | Free | `~/.corral/bin/php` shim that points at the default version | Build | 1 |
| 1.3 | Per-site PHP version ("isolate") | Free | One FPM pool per version (`fpm-<ver>.sock`), and each site's server config points at the right socket | Build | 1 |
| 1.4 | Per-version `php.ini` editing and extension toggles | Free | Generated base ini plus a user override file, editable in the UI, with an FPM reload after changes | Build | 2 |
| 1.5 | Parked directories | Free | Each folder in a parked path becomes `<folder>.test` | Build: Rust site registry | 1 |
| 1.6 | Linked sites (custom name and path) | Free | Registry entries in SQLite (not symlinks) | Build | 1 |
| 1.7 | Framework detection (Laravel, WordPress, Statamic, Craft, Symfony, static, generic `public/`) | Free | Rust "driver" trait that detects the project type and returns the docroot and front controller | Build: written from Valet's MIT-licensed driver *logic*, taken from upstream laravel/valet | 1 |
| 1.8 | `.test` DNS | Free | Small DNS server in Rust on `127.0.0.1:5353` that answers `*.test → 127.0.0.1`. The helper writes `/etc/resolver/test` | Reuse: hickory-dns (MIT/Apache-2). Replaces dnsmasq (GPL) | 1 |
| 1.9 | Configurable TLD | Free | Setting that regenerates the resolver file and site configs | Build | 2 |
| 1.10 | Web server on `:80`/`:443` | Free | Bundled **Caddy**, configured through its admin API (JSON). nginx stays as a fallback option ([ADR-0002](adr/0002-web-server.md)) | Reuse: Caddy (Apache-2) | 1 |
| 1.11 | Local HTTPS ("secure" a site) | Free | Our own local CA (generated with rcgen) trusted in the System keychain through the helper. Per-site certs, or Caddy's internal issuer | Build + Reuse: rcgen (MIT/Apache) | 1 |
| 1.12 | Privileged helper | Free | `SMAppService` launch daemon (macOS 13+) that talks to the app over XPC. It only allows a small set of commands: write/remove the resolver file, trust/untrust the CA, start the web server bound to 80/443. **No sudoers NOPASSWD entries** | Build: Rust + objc2 bindings ([ADR-0003](adr/0003-privileged-helper.md)) | 1 |
| 1.13 | CLI (`corral link`, `park`, `secure`, `isolate`, `use`, `sites`, `share`, `tld`, `open`, `logs`) | Free | Rust binary that uses the same core crate as the app | Build | 1 |
| 1.14 | Shell PATH integration | Free | Add `~/.corral/bin` to the PATH in `~/.zshrc`/`~/.bashrc`/fish config, after asking the user (and removing it cleanly on uninstall) | Build | 1 |
| 1.15 | Composer | Free | Download `composer.phar` with its signature checked, plus a shim | Reuse: Composer (MIT) | 1 |
| 1.16 | Laravel installer | Free | `composer global require laravel/installer`, or the phar, plus a shim | Reuse: laravel/installer (MIT) | 2 |
| 1.17 | Node.js version manager | Free | Download official Node tarballs from nodejs.org (checksums verified) and add a `node` shim. No nvm needed | Build + Reuse: Node (MIT) | 2 |
| 1.18 | Launch at login | Free | `SMAppService.mainApp` / Tauri autostart plugin | Reuse: tauri-plugin-autostart | 1 |
| 1.19 | Auto-update | Free | Tauri updater using GitHub Releases with signed manifests | Reuse: tauri-plugin-updater | 4 |
| 1.20 | Onboarding wizard | Free | First run: install the helper, choose PHP versions, choose parked paths, set up the shell | Build | 1 |
| 1.21 | Uninstall | Free | `corral uninstall` and a UI button that removes the resolver file, CA trust, helper, shims and data | Build | 1 |

## 2. Sites UI

| # | Feature | Ref tier | Corral approach | Build / Reuse | Phase |
|---|---|---|---|---|---|
| 2.1 | Sites list + search + detail pane | Free | Tauri webview (Svelte 5) with a sidebar and detail split view | Build | 1 |
| 2.2 | Site info tab (path, URL, PHP, driver, secure) | Free | Driven by the core API | Build | 1 |
| 2.3 | `.env` editor | Free | CodeMirror 6 editor with dotenv highlighting | Reuse: CodeMirror (MIT) | 2 |
| 2.4 | Open in IDE / terminal / browser / Finder | Free | Configurable IDE command, Terminal/iTerm/Warp/Ghostty choice | Build | 2 |
| 2.5 | Run tinker / artisan | Free | Opens a terminal at the site with the right PHP on the PATH | Build | 2 |
| 2.6 | Site previews (thumbnails) | Free | Off-screen webview snapshot, cached | Build | 4 |
| 2.7 | New project wizard (starter kits) | Free | Wraps `laravel new`, `composer create-project` and other templates | Build | 2 |
| 2.8 | Site sharing (public tunnel) | Free | Pluggable providers: Expose (MIT), cloudflared (Apache-2), ngrok (user-installed) | Reuse | 3 |
| 2.9 | Menu-bar (tray) quick actions | Free | Tauri tray: start/stop all, switch PHP, open site | Build | 1 |

## 3. Services (paid in the reference app, free here)

All services are **downloaded on demand from upstream** and run as supervised
child processes under the user account, with data in
`~/Library/Application Support/Corral/services/<name>/<instance>/`.
Each one supports multiple instances, a version picker, a custom port,
start/stop/restart, autostart, live logs, and a "copy env vars" button
(for example `DB_HOST`/`DB_PORT`).

| # | Service | License | Notes | Phase |
|---|---|---|---|---|
| 3.1 | MySQL | GPL-2.0 (FOSS exception) | Oracle tarballs, `mysqld --initialize-insecure` | 3 |
| 3.2 | MariaDB | GPL-2.0 | Upstream builds | 3 |
| 3.3 | PostgreSQL | PostgreSQL License | Postgres.app-style binaries or EDB builds | 3 |
| 3.4 | Valkey | BSD-3 | Default Redis-compatible store | 3 |
| 3.5 | Redis | RSALv2 / SSPLv1 / AGPLv3 | Download only, never bundled | 3 |
| 3.6 | MongoDB | SSPL | Download only, never bundled | 3 |
| 3.7 | Meilisearch | MIT (community edition) | Single binary | 3 |
| 3.8 | Typesense | GPL-3.0 | Single binary | 3 |
| 3.9 | MinIO | AGPL-3.0 | Single binary, download only | 3 |
| 3.10 | Laravel Reverb | MIT | Runs `php artisan reverb:start` for a chosen site | 3 |
| 3.11 | Database GUI | Adminer (Apache-2) | Served as a built-in `adminer.test` site | 3 |

## 4. Developer tools (paid in the reference app, free here)

| # | Feature | Corral approach | Build / Reuse | Phase |
|---|---|---|---|---|
| 4.1 | Mail catcher (SMTP 2525) | Our own Rust SMTP sink (`mailin-embedded` or `samotop`) that stores messages in SQLite. UI shows a list, HTML/text/source views, headers and attachments, with notifications. Laravel `.env` hints | Build. Option: run Mailpit (MIT) as a sidecar instead | 3 |
| 4.2 | Dump viewer (`dump()`, `dd()`) | **Our own** Composer package `corral/dumps`, loaded through `auto_prepend_file`. It registers a Symfony VarDumper handler that sends HTML/JSON to a local Unix socket. **No proprietary PHP extension** | Build (PHP + Rust receiver) | 3 |
| 4.3 | Laravel watchers: queries (min duration), logs, HTTP client, views, cache, events, jobs | Laravel service provider in `corral/dumps`, auto-registered when Laravel is detected. Each watcher can be toggled | Build | 3 |
| 4.4 | Dumps from the CLI (artisan, tests) | The same prepend file applies to CLI php.ini when "watch CLI" is on | Build | 3 |
| 4.5 | Pin, clear, filter and search dumps; color per type | UI | Build | 3 |
| 4.6 | Xdebug (one click) | Xdebug built per PHP version (Xdebug License, BSD-style). A **second FPM pool per version** with Xdebug loaded. The web server sends a request to the debug pool when an `XDEBUG_SESSION` cookie or query parameter is present | Reuse + Build | 3 |
| 4.7 | Automatic breakpoint detection | Check the IDE's debug port (9003). If the IDE is listening, route to the debug pool | Build | 4 |
| 4.8 | Log viewer | Rust tailer for `storage/logs/*.log`, with a Monolog line parser, level filters, search and stack-trace folding | Build | 3 |
| 4.9 | Request/internal API log | Log of every core operation, shown in the UI | Build | 4 |

## 5. Automation and integrations

| # | Feature | Corral approach | Phase |
|---|---|---|---|
| 5.1 | Local HTTP API | `127.0.0.1:<port>` JSON API, token-protected (token stored in `0600` file). The CLI and UI both use it | 1 |
| 5.2 | URL scheme (`corral://open?site=…`) | tauri-plugin-deep-link | 2 |
| 5.3 | MCP server | `corral mcp` (stdio) exposes sites, PHP, services, dumps, logs and mails to AI agents | 4 |
| 5.4 | AppleScript / Shortcuts | Shortcuts App Intents are deferred. The CLI and HTTP API cover scripting | later |
| 5.5 | Forge / Vapor remote sites | Optional plugin using the public Forge API with the user's token | later |

## 6. Deliberately not replicated

- License checks, "Pro" gating, promo windows. Everything is free.
- Default-on analytics or crash reporting. Crash reports are **opt-in** only.
- AppleScript `.sdef` parity (replaced by the CLI, HTTP API and MCP server).

## Third-party components

| Component | License | Distribution |
|---|---|---|
| Tauri 2, tauri plugins | MIT / Apache-2.0 | Bundled |
| Svelte, CodeMirror 6 | MIT | Bundled |
| hickory-dns, rcgen, tokio, rusqlite | MIT / Apache-2.0 | Bundled (statically linked) |
| Caddy | Apache-2.0 | Bundled or downloaded |
| PHP (static-php-cli builds) | PHP-3.01 / MIT | Downloaded |
| Xdebug | Xdebug-1.03 | Downloaded |
| Composer, laravel/installer, Expose | MIT | Downloaded |
| Node.js | MIT | Downloaded |
| Services in §3 | See table | Downloaded only |
| Adminer | Apache-2.0 | Downloaded |
