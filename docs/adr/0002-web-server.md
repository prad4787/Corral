# ADR-0002: Web server, Caddy first

- **Status:** Proposed (to be confirmed by the Phase 0 spike)
- **Date:** 2026-11-10

## Context

We need a reverse proxy and FastCGI front end on `127.0.0.1:80/443`. It must
route each `<site>.<tld>` to the right PHP-FPM socket, serve static files,
terminate TLS with our local CA, and reload without dropping connections.

## Options

| | Caddy | nginx |
|---|---|---|
| License | Apache-2.0 | BSD-2 |
| Config | JSON admin API, reloaded live | Text files + `nginx -s reload` |
| TLS | Built-in internal CA/issuer | Manual certs per site |
| FastCGI/PHP | `php_fastcgi` directive | `fastcgi_pass` |
| Binary | One Go binary (~40 MB) | Small, but needs a static build for macOS |

## Decision

Use **Caddy** by default. Generate its JSON config from the site registry
and push it through the admin API, bound to a Unix socket. Keep the proxy
behind a `ProxyBackend` trait so an nginx backend can be added later if
performance or compatibility requires it.

## Validate in the Phase 0 spike

- Caddy `php_fastcgi` works with a Laravel front controller and with WordPress.
- A whole request (from DNS lookup to PHP response) takes under 5 ms on a warm path.
- Config reload with 200 sites takes under 200 ms.
