# Contributing

Read [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md) first. Contributions that copy
code or assets from proprietary apps will be rejected.

## Branches

- `main` must always build and pass CI.
- Name feature branches `<type>/<phase>.<task>-short-name`, for example `feat/1.2-php-installer` or `spike/0.4-smappservice`.
- Merge with **squash merge**. The PR title becomes the commit message.

## Commit messages ([Conventional Commits](https://www.conventionalcommits.org/))

```
<type>(<scope>): <summary in imperative, ≤72 chars>

<body: what and why, wrapped at 72>

Refs: FEATURES 1.2, PLAN 1.2
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `spike`.

**Scopes:** `core`, `php`, `sites`, `dns`, `tls`, `proxy`, `helper`, `supervisor`,
`services`, `mail`, `dumps`, `xdebug`, `logs`, `cli`, `api`, `ui`, `research`, `adr`.

Breaking changes use `feat!:` or add a `BREAKING CHANGE:` footer.

## Definition of done (every PR)

- [ ] `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- [ ] `pnpm -C app check && pnpm -C app test`
- [ ] Any change to the system has an undo path, and that undo is tested
- [ ] FEATURES.md and PLAN.md checkboxes are updated
- [ ] Any new third-party component is added to the license table in FEATURES.md

## Releases

Tags follow `vMAJOR.MINOR.PATCH`. Each phase in PLAN.md ends with a minor release.
