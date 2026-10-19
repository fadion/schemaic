// Embeds the application icon into the Windows executable so Explorer and the
// taskbar show it for the .exe itself (independent of the runtime window icon).
// No-op on non-Windows targets.
//
// It also reruns when a catalogue changes: `tr!` checks this crate's message
// ids against `schemaic-ui`'s `.ftl` files at compile time, and nothing else
// tells cargo that a new id there means recompiling here.
fn main() {
    println!("cargo:rerun-if-changed=../schemaic-ui/i18n");
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=../../assets/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon.ico");
        // Non-fatal: a missing resource compiler shouldn't break the whole build;
        // the icon is polish, not correctness.
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed .exe icon: {e}");
        }
    }
}
