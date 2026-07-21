# Third-party notices

Schemaic itself is licensed under the MIT License (see [`LICENSE`](LICENSE)).

Distributed builds of Schemaic incorporate third-party material. This file
records the notices those licenses require to accompany a distribution. Nothing
below imposes copyleft on Schemaic's own source — every dependency is either
permissively licensed or dual-licensed with a permissive option, which Schemaic
elects.

## Bundled assets (embedded in the binary)

### IBM Plex fonts

`IBMPlexSans` and `IBMPlexMono` are embedded via `include_bytes!`
(`crates/schemaic-ui/fonts/`).

- Copyright © 2017 IBM Corp., with Reserved Font Name "Plex".
- Licensed under the SIL Open Font License, Version 1.1 (`OFL-1.1`).
- Full license text: [`crates/schemaic-ui/fonts/LICENSE.txt`](crates/schemaic-ui/fonts/LICENSE.txt).

The OFL permits bundling and redistribution (including commercially). The fonts
remain under the OFL — they are not relicensed under MIT. The name "Plex" is a
Reserved Font Name: a modified font must not be distributed under that name.

### Lucide icons

Several UI glyphs are Lucide icon paths embedded as SVG in
`crates/schemaic-ui/src/icons.rs`.

- Lucide Icons and Contributors — ISC License.
- Portions derived from the Feather project — MIT License, © 2013-present Cole Bemis.
- Full license text: [`licenses/Lucide-LICENSE.txt`](licenses/Lucide-LICENSE.txt).

## Rust dependencies (statically linked)

A release binary statically links a tree of Rust crates. Their licenses are all
permissive or permissively-electable:

- MIT, Apache-2.0 (incl. `WITH LLVM-exception`), BSD-2-Clause, BSD-3-Clause,
  ISC, Zlib, 0BSD, Unlicense, BSL-1.0, Unicode-3.0, CC0-1.0.
- Dual/multi-licensed crates are used under a permissive option — notably
  `self_cell` (`Apache-2.0 OR GPL-2.0-only`) is used under **Apache-2.0**, and
  `r-efi` (`… OR LGPL-2.1-or-later`) under MIT/Apache-2.0. No GPL/LGPL terms apply.
- Weak-copyleft `MPL-2.0` crates — `im`, `im-rc`, `bitmaps`, `sized-chunks`,
  `resvg`, `usvg` — are used **unmodified** from crates.io. MPL-2.0 is
  file-level: it does not affect Schemaic's own source, and the crates' source
  is already publicly available upstream.

The complete per-crate license manifest can be regenerated from the lockfile:

```sh
cargo install cargo-about
cargo about generate about.hbs > THIRD-PARTY-LICENSES.html
```

The license policy is enforced in CI by `cargo-deny` (see [`deny.toml`](deny.toml)),
which fails the build if a dependency's license falls outside the allow-list —
so a future `cargo update` cannot silently pull in a GPL/AGPL crate.
