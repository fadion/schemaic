//! The three small stores **the UI owns and the app only persists**: per-column
//! display formatters, identity colours (database and table), and favourited
//! databases.
//!
//! The third cut of the `app_view` split, and it is deliberately *not* shaped
//! like [`crate::history_store`] and [`crate::snippet_store`] — the difference
//! is the point, and mistaking it would produce a module that looks like its
//! siblings and guarantees nothing.
//!
//! Those two own their mutators, so each can hold the rule *every mutation is
//! followed by the right kind of save* and gate it. These three cannot: their
//! signals go raw into the `Ui` bundle and every mutation happens in
//! `schemaic-ui` — the grid's "Format as" menu, the schema tree's Colour and
//! Favorite entries. **The whole app-side surface is a load and a save**, so
//! what this module can own is exactly that much: the three file names, once
//! each, and one [`saver`] so a fourth store cannot arrive with a fourth shape.
//! The rule those three stores actually need is enforced where it can be seen,
//! by `schemaic_ui::persisted_store_gate`.
//!
//! **That sentence was true of two of the three.** `format.json`'s writer was
//! exempted there on a premise that was false of the code — that `grid.rs`
//! upserts "through `GridState::fmt_rules` rather than a `format::` mutator",
//! where `grid.rs` calls `format::upsert` in exactly the shape the other
//! needles match — so deleting that store's save left both gates green while
//! this paragraph claimed all three were covered. The needle is in now. An
//! exemption is a claim about the code, and the cost of not checking one is a
//! sentence here that reads like a guarantee.
//!
//! **`db_colors` and `table_colors` are two signals over one file**, which is
//! the only invariant left on this side: they share `db_colors.json`, so
//! writing either half has to write both or the other is lost. [`saver`]'s
//! closure builds the whole `DbColorsFile` every time, and the struct literal is
//! what makes that unforgettable — a save that wrote one half would not compile.
//!
//! Each save takes its [`persist::Saving`] rather than assuming `Replacing`,
//! because each has a caller that is a **deletion**: connection deletion prunes
//! all three and must erase, or the deleted connection's databases, tables and
//! columns stay readable in `<store>.json.bak` under a confirm saying they
//! cannot be recovered. That dispatch is `persist::write_json_store`'s, which is
//! why — unlike the two stores above — this module holds no `match` of its own.

use std::rc::Rc;

use floem::prelude::*;
use serde::Serialize;

use schemaic_core::db_color::{DbColorRule, DbColorsFile, TableColorRule};
use schemaic_core::favorite::{FavoriteRule, FavoritesFile};
use schemaic_core::format::{ColumnFormatRule, FormatsFile};

use crate::persist;

/// One store's save closure: the file named once, the value rebuilt per call,
/// and the `Saving` passed straight through to
/// [`persist::write_json_store`].
///
/// `build` is re-run on every save rather than capturing a snapshot, because
/// these signals are written by the UI between saves — a captured value would
/// persist whatever the store held when `wire` ran.
fn saver<F: Serialize + 'static>(
    file: &'static str,
    build: impl Fn() -> F + 'static,
) -> Rc<dyn Fn(persist::Saving)> {
    Rc::new(move |saving| persist::write_json_store(file, &build(), saving))
}

/// The three stores, as `app_view` receives them. Every field goes into the `Ui`
/// bundle unchanged; nothing here is consumed on this side except by the
/// connection-deletion prune.
pub(crate) struct UiStores {
    /// Per-column display formatters, keyed by connection+table+column; read and
    /// upserted by the results grid's "Format as" menu.
    pub(crate) formats: RwSignal<Vec<ColumnFormatRule>>,
    pub(crate) save_formats: Rc<dyn Fn(persist::Saving)>,
    /// Identity colour per database — keyed by connection+database, shown as a
    /// dot on the DB node, the active-DB selector and that database's query
    /// tabs.
    pub(crate) db_colors: RwSignal<Vec<DbColorRule>>,
    /// Identity colour per table — keyed by connection+database+display name,
    /// shown as a dot on the table row and as a tint on the table's ER-diagram
    /// card header.
    pub(crate) table_colors: RwSignal<Vec<TableColorRule>>,
    /// **One save for the pair.** See the module doc.
    pub(crate) save_db_colors: Rc<dyn Fn(persist::Saving)>,
    /// Favourited (bookmarked) databases — a gold star, sorted to the top of the
    /// tree.
    pub(crate) db_favorites: RwSignal<Vec<FavoriteRule>>,
    pub(crate) save_db_favorites: Rc<dyn Fn(persist::Saving)>,
}

/// Load all three and build their savers.
pub(crate) fn wire() -> UiStores {
    let formats = RwSignal::new(persist::load_json::<FormatsFile>(FORMATS_FILE).rules);
    let save_formats = saver(FORMATS_FILE, move || FormatsFile {
        rules: formats.get_untracked(),
    });

    // Both halves out of one read, as they go back in one write.
    let colors = persist::load_json::<DbColorsFile>(COLORS_FILE);
    let db_colors = RwSignal::new(colors.rules);
    let table_colors = RwSignal::new(colors.tables);
    let save_db_colors = saver(COLORS_FILE, move || DbColorsFile {
        rules: db_colors.get_untracked(),
        tables: table_colors.get_untracked(),
    });

    let db_favorites = RwSignal::new(persist::load_json::<FavoritesFile>(FAVORITES_FILE).rules);
    let save_db_favorites = saver(FAVORITES_FILE, move || FavoritesFile {
        rules: db_favorites.get_untracked(),
    });

    UiStores {
        formats,
        save_formats,
        db_colors,
        table_colors,
        save_db_colors,
        db_favorites,
        save_db_favorites,
    }
}

const FORMATS_FILE: &str = "format.json";
const COLORS_FILE: &str = "db_colors.json";
const FAVORITES_FILE: &str = "favorites.json";

#[cfg(test)]
mod tests {
    /// Each file is named **once**, by its const.
    ///
    /// Weaker than its siblings' gates on purpose, and the doc comment above
    /// says why: with every mutation in `schemaic-ui`, the only thing this side
    /// can promise is that a store has one name and one saver. The rule that
    /// matters — a mutation followed by a save — is asserted in the crate where
    /// the mutations are, by `schemaic_ui::persisted_store_gate`. **If that gate
    /// is ever deleted, this one is not a substitute for it.**
    #[test]
    fn each_store_is_named_once() {
        let raw = include_str!("ui_stores.rs");
        let src = schemaic_ui::source_gate::production_code(raw);
        for (file, ext) in [
            ("format", ".json"),
            ("db_colors", ".json"),
            ("favorites", ".json"),
        ] {
            let needle = [file, ext].concat();
            assert_eq!(
                src.matches(&needle).count(),
                1,
                "`{needle}` should appear once in this module's code, as its const"
            );
        }
        // The floor: one `saver` call per store. A store dropped from `wire`
        // would otherwise leave this passing on the two that remain, since each
        // surviving name would still be named exactly once.
        //
        // Three, not four: the definition is `saver<F: …>(` and does not match
        // this needle. Counted wrong on the first draft, which is the gate
        // catching its author before it caught anyone else.
        assert_eq!(
            src.matches("saver(").count(),
            3,
            "one `saver` call site per store"
        );
    }
}
