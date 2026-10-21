# `ui/erd_raster.rs`

`erd_raster.rs` — SVG → PNG for the ER-diagram export and the *Chart* modal's PNG save
(through `ErdDoc::Raster`, above), and the only reason `resvg` is a direct
dependency (it was already in the tree via floem's `svg` view, same version, so it costs no
extra compile). **Which version is floem's choice, not ours**: 0.43 is what `floem_renderer`
pulls, and moving the workspace to 0.48 would not upgrade that copy — it would stand a second
resvg, usvg and fontdb beside it and spend exactly the saving the shared compile is here for.
So this dependency follows floem's rather than the registry's latest. It lives in this crate
**because of the fonts**: the cards' widths were
measured against the bundled IBM Plex Sans, so `png_from_svg` loads those exact bytes
(`fonts::SANS_FACES`) into its own `fontdb` **first**, as the named family (`PLEX_SANS`) and
`sans-serif` both — so whatever Plex can draw comes from the bytes the cards were measured
against, and the text lands inside its boxes on a build server as much as on a desktop. The
machine's fonts are loaded **behind** them (`export_options`, fontdb's `load_system_fonts`) as
the fallback for glyphs Plex has none of: with the bundled faces alone, a CJK, Arabic or Hebrew
table name rendered as a row of empty boxes in the PNG while the canvas, which falls back
through cosmic-text, showed it. The trade is that those glyphs now depend on the machine — taken
over bundling a CJK face at 10–20 MB. The scan runs once per process, into a
`OnceLock<Arc<fontdb::Database>>`, on the first export's worker — measured on Windows at ~57 ms
for that export and ~4 ms for each after. **That layer is the one the tests leave out**: reaching
the system fonts is filesystem access, so they render through `font_options` (bundled only, via
the test helper `png`). resvg's `system-fonts` feature is named in `Cargo.toml` for it, though
`floem_renderer` already enabled it, so no build changed. `clamp_scale` is the guard that
matters: a whole-database diagram can be
tens of thousands of pixels wide on its own, and the pixmap is RGBA in memory, so the requested
2× is reduced until the result fits both `MAX_PNG_DIM` and `MAX_PNG_PIXELS` — returning **less
than 1.0** when it has to, because the alternative is a failed allocation at the moment the
user asked for the export. Should the pixmap still fail to allocate, `render_png` says *"Image
is too large to rasterise (W×H)"* — *Image*, because a chart's PNG comes through it as well as
a diagram's. It touches no signal and no file, so the app runs it on a worker
thread (`export_erd`) and writes the bytes it returns.
**Its words are the catalogue's** (`raster-*`) — its three refusals, which
reach the user as the export's failure notice: `raster-invalid-svg` and `raster-png-failed`
with the library's own `$error`, and `raster-too-large` with `$width` and `$height` passed as
text. Off the UI thread is no obstacle: `tr!` reads `i18n::LOADER`, a `LazyLock` static.
`png_from_svg_reports_a_broken_document_rather_than_panicking` still reaches *Invalid SVG:*
for real; `the_unreachable_refusals_read_as_they_did_in_english` pins the two no test provokes.
