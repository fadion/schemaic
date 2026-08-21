//! Rasterise an exported ER diagram's SVG to PNG.
//!
//! There is deliberately **no second renderer**: the PNG is the very SVG document
//! [`schemaic_core::erd_export::to_svg`] produced, drawn by `resvg`. A separate
//! raster path would be a second place for the diagram's appearance to live, and
//! the two would drift on the first change to a card.
//!
//! It lives in this crate because of the fonts. The card widths on the canvas were
//! measured against the bundled IBM Plex Sans ([`crate::fonts`]), so the renderer
//! is handed those exact bytes rather than left to find a face on the machine —
//! which is what makes the PNG's text land inside the boxes it was measured for,
//! on a build server as much as on a developer's desktop.
//!
//! Nothing here touches the filesystem or a signal: the caller passes a string and
//! gets bytes back, off the UI thread.

use resvg::{tiny_skia, usvg};

/// Hard cap on either PNG dimension. 8192 is the ceiling a lot of image pipelines
/// and GPU texture paths share, and past it a "picture of the schema" has stopped
/// being useful anyway.
pub const MAX_PNG_DIM: f32 = 8192.0;

/// Hard cap on total pixels. The pixmap is RGBA in memory, so this is a 160 MB
/// allocation at the limit — a whole-database diagram at 2× would otherwise ask
/// for several times that and fail, or swap, rather than export.
pub const MAX_PNG_PIXELS: f32 = 40_000_000.0;

/// The scale actually used for a `w × h` diagram, given the one asked for: `want`,
/// reduced until the result fits both [`MAX_PNG_DIM`] and [`MAX_PNG_PIXELS`].
///
/// A whole-database diagram can be tens of thousands of pixels wide on its own, so
/// this returns **less than 1.0** when it has to — the alternative is a failed
/// allocation at the moment the user asked for the export.
pub fn clamp_scale(w: f32, h: f32, want: f32) -> f32 {
    // Written through `is_finite` rather than as `!(w > 0.0)`: a NaN has to take
    // this branch too, and a bare `w <= 0.0` lets it through.
    let unusable = |v: f32| !v.is_finite() || v <= 0.0;
    if unusable(w) || unusable(h) || unusable(want) {
        return 1.0;
    }
    let by_dim = (MAX_PNG_DIM / w).min(MAX_PNG_DIM / h);
    let by_area = (MAX_PNG_PIXELS / (w * h)).sqrt();
    want.min(by_dim).min(by_area)
}

/// Render `svg` to PNG bytes at `scale`× its own size (clamped by [`clamp_scale`]).
///
/// The font database holds only the bundled faces — no system-font scan, so this
/// is deterministic and costs no startup sweep. `sans-serif` is pointed at IBM
/// Plex Sans as well as the family being named outright, because the document's
/// fallback chain ends at the generic and a machine without Plex installed would
/// otherwise resolve it to whatever `usvg` defaults to.
pub fn png_from_svg(svg: &str, scale: f32) -> Result<Vec<u8>, String> {
    let mut opt = usvg::Options {
        font_family: "IBM Plex Sans".to_string(),
        ..Default::default()
    };
    {
        let db = opt.fontdb_mut();
        for face in crate::fonts::SANS_FACES {
            db.load_font_data(face.to_vec());
        }
        db.set_sans_serif_family("IBM Plex Sans");
    }
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("Invalid SVG: {e}"))?;
    let size = tree.size();
    let scale = clamp_scale(size.width(), size.height(), scale);
    let w = (size.width() * scale).round().max(1.0) as u32;
    let h = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| format!("Diagram is too large to rasterise ({w}×{h})"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| format!("PNG encoding failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"50\" \
                       viewBox=\"-10 -10 100 50\"><rect x=\"-10\" y=\"-10\" width=\"100\" \
                       height=\"50\" fill=\"#101010\"/><text x=\"0\" y=\"20\" font-size=\"12\" \
                       fill=\"#ffffff\">users</text></svg>";

    #[test]
    fn clamp_scale_keeps_a_small_diagram_at_the_asked_for_scale() {
        assert_eq!(clamp_scale(800.0, 600.0, 2.0), 2.0);
        assert_eq!(clamp_scale(800.0, 600.0, 1.0), 1.0);
    }

    #[test]
    fn clamp_scale_reduces_below_one_when_the_diagram_alone_busts_the_cap() {
        // A whole-database diagram, wider than the dimension cap at 1×.
        let s = clamp_scale(20_000.0, 4_000.0, 2.0);
        assert!(s < 1.0, "{s}");
        assert!(20_000.0 * s <= MAX_PNG_DIM + 0.5, "{s}");
        assert!(20_000.0 * s * 4_000.0 * s <= MAX_PNG_PIXELS + 1.0, "{s}");
    }

    #[test]
    fn clamp_scale_honours_the_area_cap_before_the_dimension_cap() {
        // Both dimensions under 8192, but 8000×8000 at 1× is 64 M pixels.
        let s = clamp_scale(8_000.0, 8_000.0, 1.0);
        assert!(s < 1.0, "{s}");
        assert!((8_000.0 * s).powi(2) <= MAX_PNG_PIXELS + 1.0, "{s}");
    }

    #[test]
    fn clamp_scale_of_a_degenerate_size_is_one() {
        assert_eq!(clamp_scale(0.0, 10.0, 2.0), 1.0);
        assert_eq!(clamp_scale(10.0, 0.0, 2.0), 1.0);
        assert_eq!(clamp_scale(10.0, 10.0, 0.0), 1.0);
    }

    #[test]
    fn png_from_svg_writes_a_png_at_the_scaled_size() {
        let png = png_from_svg(SVG, 2.0).expect("renders");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic");
        // IHDR width/height are big-endian u32 at bytes 16..24.
        let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert_eq!((w, h), (200, 100));
    }

    #[test]
    fn png_from_svg_reports_a_broken_document_rather_than_panicking() {
        let err = png_from_svg("not an svg", 1.0).unwrap_err();
        assert!(err.starts_with("Invalid SVG:"), "{err}");
    }

    /// The whole picture path, end to end: a scene → `to_svg` → PNG.
    ///
    /// The document `to_svg` writes is not hand-authored SVG — it nests the card
    /// glyphs as `<svg>` elements with their own viewBox, and a renderer that
    /// didn't take them would drop every icon *silently*, leaving a PNG that still
    /// encoded fine. Rendering a real one here is what proves the two halves agree.
    #[test]
    fn a_rendered_scene_becomes_a_png_of_the_scene_s_size() {
        use schemaic_core::erd::{Pt, Rect};
        use schemaic_core::erd_export::{
            SvgColors, SvgEdge, SvgKey, SvgMetrics, SvgNode, SvgRow, SvgScene, to_svg,
        };

        let scene = SvgScene {
            bounds: Rect {
                x: -28.0,
                y: -28.0,
                w: 300.0,
                h: 160.0,
            },
            nodes: vec![SvgNode {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 78.0,
                title: "users".to_string(),
                stub: false,
                rows: vec![SvgRow {
                    name: "id".to_string(),
                    type_name: "int".to_string(),
                    key: SvgKey::Pk,
                    icon: Some(crate::icons::TABLE.to_string()),
                }],
                more: Some("+3 more".to_string()),
                icon: Some(crate::icons::TABLE.to_string()),
                icon_fill: "#7080a0".to_string(),
                title_fill: "#f0f0f0".to_string(),
                header_fill: "#303030".to_string(),
                border: "#404040".to_string(),
            }],
            edges: vec![SvgEdge {
                poly: vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 100.0, y: 60.0 }],
                markers: vec![(Pt { x: 1.0, y: 2.0 }, Pt { x: 8.0, y: 9.0 })],
            }],
            colors: SvgColors {
                canvas: "#101010".to_string(),
                card: "#202020".to_string(),
                text: "#e0e0e0".to_string(),
                type_text: "#909090".to_string(),
                key_pk: "#e0b040".to_string(),
                key_fk: "#40a0e0".to_string(),
                edge: "#606060".to_string(),
                muted: "#808080".to_string(),
            },
            metrics: SvgMetrics {
                header_h: 30.0,
                row_h: 24.0,
                expander_h: 22.0,
                pad_x: 10.0,
                radius: 6.0,
                title_size: 13.0,
                name_size: 11.5,
                type_size: 10.5,
                edge_width: 1.4,
                icon_size: 13.0,
                title_gap: 7.0,
                row_gap: 8.0,
            },
        };
        let png = png_from_svg(&to_svg(&scene), 2.0).expect("renders");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic");
        let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert_eq!((w, h), (600, 320));
        // Not a blank canvas: something other than the background colour was
        // drawn. A silent parse failure would still encode a valid, empty PNG.
        assert!(png.len() > 1000, "suspiciously small: {} bytes", png.len());
    }
}
