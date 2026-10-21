// Reruns when a catalogue changes: `tr!` checks this crate's message ids
// against the `.ftl` files under `i18n/` at compile time, and the non-test
// build records no dependency on them otherwise — so an id renamed in a
// catalogue alone left a stale build that still passed the check.
fn main() {
    println!("cargo:rerun-if-changed=i18n");
}
