# `cli/schemaic-cli.rs`

`cli/schemaic-cli.rs` — the console-subsystem binary, at `src/bin/`. Its body is one call to
`run::main`, deliberately: it is the same entry point the app's argv branch calls, two front
ends over one program. It is **built on every platform** even though only Windows packages it,
because a target that compiles on one platform only is a target that breaks on that platform
first.
