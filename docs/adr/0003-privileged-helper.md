# ADR-0003: Privileged helper

- **Status:** Proposed (to be confirmed by the Phase 0 spike)
- **Date:** 2026-11-10

## Context

Some operations need root:

1. Writing `/etc/resolver/<tld>`.
2. Trusting our CA in the System keychain.
3. Listening on `127.0.0.1:80` and `:443`.

The reference app uses an SMJobBless helper and adds a `sudoers.d` entry. We
want the smallest possible privileged surface and **no passwordless sudo rules**.

## Decision

- Ship `corral-helper` as an **SMAppService launch daemon** (macOS 13+),
  declared in `Corral.app/Contents/Library/LaunchDaemons/`. The user approves it
  once in System Settings → Login Items.
- The app talks to the helper over **XPC (a Mach service)**. Every incoming
  message is checked against the caller's code signature (team ID + bundle ID).
- Allowed commands, and nothing else:
  - `WriteResolver { tld, port }` / `RemoveResolver { tld }`
  - `TrustCA { der }` / `UntrustCA { sha256 }`
  - `BindPorts { ports: [80, 443] }`: binds the sockets and passes the file
    descriptors to the unprivileged proxy (launchd socket activation or
    `SCM_RIGHTS`), so the web server itself **never runs as root**.
  - `Uninstall`
- All paths are fixed inside the helper. No arbitrary path or shell argument
  is ever accepted.

## Spike 0.3 result: port binding (2026-11-10, macOS 26.6.2, arm64)

| Bind as normal user | Result |
|---|---|
| `127.0.0.1:80` / `:443` | `EACCES` (permission denied) |
| `0.0.0.0:443` | OK (macOS allows wildcard low ports without root) |
| `127.0.0.1:8080` | OK |

Binding to `0.0.0.0` would put every dev site on the LAN, so we don't use it.
**`BindPorts` stays.** The helper binds `127.0.0.1:{80,443}` and sends the
listening file descriptors over XPC (`xpc_fd_create`). The unprivileged
`corral` process accepts on them and forwards raw TCP to the web server's
internal ports (`internal_http_port` / `internal_https_port`, defaults
18080/18443). TLS is still terminated by the web server, never by root code.

Until the helper is installed, Corral works in **high-port mode**: sites are
served on the internal ports directly (`https://blog.test:18443`). That mode
is also used by tests, CI and contributors without a signing certificate.

## Fallbacks and open questions (Phase 0 spike)
- Unsigned or ad-hoc dev builds: SMAppService may refuse to register. For
  development only, provide `corral dev-setup`, which runs the same steps once
  through `osascript … with administrator privileges`.
- Distribution needs a Developer ID certificate and notarization (see PLAN Phase 4).
