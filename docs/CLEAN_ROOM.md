# Clean-room policy

Corral offers features similar to existing local PHP environments, including
commercial ones. To keep the project legally clean and truly
open source, everyone follows these rules.

## Allowed

- Studying **observable behavior**: which files an app writes, which processes
  it runs, which ports it listens on, its public docs and CLI help text.
- Writing up that behavior *in our own words* in `docs/research/`.
- Using **upstream open-source projects directly from their original source**
  (for example Laravel Valet, Expose, Mailpit and Xdebug), under their own licenses.

## Not allowed

- Copying code, configuration files, scripts, UI layouts, icons, images, text
  or names from a proprietary app. This includes files inside its `.app` bundle.
- Shipping or downloading another vendor's binaries, such as their PHP
  extensions, mail server binaries or `.phar` files.
- Decompiling code and translating it into ours.
- Building anything that bypasses or emulates another product's licensing.
- Using another product's trademarks in our name, branding or marketing.

## Process

1. **Research** (`docs/research/`): describe *what* happens, never *how the code does it*.
2. **Spec** (`docs/FEATURES.md`, issues): turn the research into requirements.
3. **Implementation**: written from the spec and from upstream documentation only.

When an upstream project is MIT-licensed, such as `laravel/valet`, take it from
its official repository with its license notice. Never take it from a copy
bundled inside another product.

## Third-party runtime licenses

Corral downloads runtimes on demand instead of bundling them, where licensing
requires it. Every downloadable component is listed in
[FEATURES.md](FEATURES.md#third-party-components) with its license.
