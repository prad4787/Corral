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

## Fallbacks and open questions (Phase 0 spike)

- Find out whether an unprivileged process can bind `127.0.0.1:80` on macOS 13–26.
  If it can, drop `BindPorts`.
- Unsigned or ad-hoc dev builds: SMAppService may refuse to register. For
  development only, provide `corral dev-setup`, which runs the same steps once
  through `osascript … with administrator privileges`.
- Distribution needs a Developer ID certificate and notarization (see PLAN Phase 4).
