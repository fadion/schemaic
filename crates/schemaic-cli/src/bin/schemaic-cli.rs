//! The console-subsystem CLI binary.
//!
//! **This exists for Windows.** `schemaic.exe` is built
//! `windows_subsystem = "windows"` so that launching the app does not flash a
//! console window — and a GUI-subsystem process has no console to print to, so
//! `schemaic query …` typed at a prompt would return instantly and silently.
//! This binary is the same CLI with a console attached, shipped alongside the
//! app as `schemaic.com`: `PATHEXT` resolves `.COM` before `.EXE`, so typing
//! `schemaic` runs this while shortcuts, the Start menu and Explorer keep
//! launching the GUI.
//!
//! On Linux and macOS there is no subsystem split, so the CLI is reached
//! through `schemaic`'s own argv branch and this binary is not packaged. It is
//! still *built* everywhere, because a target that only compiles on one
//! platform is a target that breaks on that platform first.
//!
//! It deliberately does no work of its own: everything is
//! [`schemaic_cli::run::main`], which is the same entry point the app's argv
//! branch calls. Two front ends, one program.

fn main() -> std::process::ExitCode {
    schemaic_cli::run::main(std::env::args_os())
}
