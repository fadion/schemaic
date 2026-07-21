//! Bundled UI fonts (IBM Plex, OFL 1.1), embedded in the binary and registered
//! with Floem's shared font system at startup.
//!
//! We override the *generic* families — `sans-serif` → IBM Plex Sans (all UI
//! chrome) and `monospace` → IBM Plex Mono (the SQL editor and code/chips) —
//! so no call site needs an explicit family; Floem resolves the generics to
//! these faces. Bundling (rather than relying on installed fonts) makes the app
//! render identically on every machine.

use floem::text::FONT_SYSTEM;

const SANS_REGULAR: &[u8] = include_bytes!("../fonts/IBMPlexSans-Regular.ttf");
const SANS_BOLD: &[u8] = include_bytes!("../fonts/IBMPlexSans-Bold.ttf");
const MONO_REGULAR: &[u8] = include_bytes!("../fonts/IBMPlexMono-Regular.ttf");
const MONO_BOLD: &[u8] = include_bytes!("../fonts/IBMPlexMono-Bold.ttf");

/// Register the bundled faces and point the generic `sans-serif`/`monospace`
/// families at them. Call once, before building the window.
pub fn load_fonts() {
    let mut font_system = FONT_SYSTEM.lock();
    let db = font_system.db_mut();
    for face in [SANS_REGULAR, SANS_BOLD, MONO_REGULAR, MONO_BOLD] {
        db.load_font_data(face.to_vec());
    }
    // Regular + Bold share one family name each (RIBBI), so `font_bold()`
    // (weight 700) selects the Bold face automatically.
    db.set_sans_serif_family("IBM Plex Sans");
    db.set_monospace_family("IBM Plex Mono");
}

#[cfg(test)]
mod tests {
    use super::*;
    use floem::text::fontdb::{Family, Query};

    /// After loading, the generic `sans-serif`/`monospace` families must
    /// resolve to the bundled IBM Plex faces (guards the embedded-bytes path
    /// and the generic-family override together).
    #[test]
    fn generics_resolve_to_ibm_plex() {
        load_fonts();
        let mut font_system = FONT_SYSTEM.lock();
        let db = font_system.db_mut();
        let resolved = |generic: Family| {
            let id = db
                .query(&Query {
                    families: &[generic],
                    ..Default::default()
                })
                .expect("generic family resolves to a face");
            db.face(id).expect("face exists").families[0].0.clone()
        };
        assert_eq!(resolved(Family::SansSerif), "IBM Plex Sans");
        assert_eq!(resolved(Family::Monospace), "IBM Plex Mono");
    }
}
