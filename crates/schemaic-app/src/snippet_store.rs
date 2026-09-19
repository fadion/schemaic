//! The snippet library's store: the persisted user list and the nine closures
//! that write it.
//!
//! The second cut of the `app_view` split, and it is the **same invariant** as
//! [`crate::history_store`]'s — which is why the two are separate modules with
//! the same shape rather than one "stores" grab-bag. Every mutation is followed
//! by a save, and which save is policy: `Erasing` from the two paths that
//! *remove* — a snippet deleted from its row's menu, and a deleted connection's
//! connection-scoped snippets — and `Replacing` from the seven edits. A snippet
//! is the user's own
//! SQL, written against a named server, and an ordinary save copies the
//! pre-delete file — body and all — to `snippets.json.bak` at the moment the
//! modal says *"This can't be undone."* On a library nobody edits again, that
//! copy is forever.
//!
//! **The seam is the store against the editor, not the panel against the app.**
//! What lives here writes `snippets` and saves; what stays in `app_view` is
//! everything that reads a *tab* or raises a *modal*:
//!
//! * `insert_snippet` writes the active tab's `insert_req` and then calls
//!   [`SnippetStore::record_use`] — the editor half cannot move, because the
//!   mounted editor owns the document.
//! * `save_snippet_current` reads the selection through `snippet::snippet_text`
//!   and names the snippet from the tab's title, then calls
//!   [`SnippetStore::create`].
//! * `remove_snippet` raises the confirm and calls [`SnippetStore::remove`] only
//!   on yes — the same split `delete_conn`/`delete_conn_now` already has in
//!   `app_view`, and the reason [`SnippetStore::remove`] takes no confirm of its
//!   own: **asking is the UI's job, erasing is this module's.**
//! * `duplicate_snippet` looks the source up in the *merged* library memo —
//!   Duplicate exists mainly to get an editable copy of a built-in, which is not
//!   in the user's list to be found — and hands it to
//!   [`SnippetStore::duplicate_from`].
//!
//! `snippet_library` and `can_save_snippet` stay too: both are memos over
//! signals this module does not own (`conn_dialect_memo`, the tab strip).

use std::rc::Rc;

use floem::prelude::*;

use schemaic_core::snippet::{Scope, Snippet, SnippetsFile};

use crate::persist;

/// The store's file. A `const` rather than a literal per call site, so
/// `the_remove_path_erases` can say "named once" and mean it.
const FILE: &str = "snippets.json";

/// **The one place the save policy is chosen.** See the module doc for why
/// `Erasing` is not interchangeable with `Replacing` here.
fn save(snippets: RwSignal<Vec<Snippet>>, saving: persist::Saving) {
    let file = SnippetsFile {
        snippets: snippets.get_untracked(),
    };
    match saving {
        persist::Saving::Erasing => persist::save_json_erasing(FILE, &file),
        persist::Saving::Replacing => persist::save_json(FILE, &file),
    }
}

/// Wall-clock millis, for "last used". The same reading `history_store`'s
/// `record` takes, and for the same reason: it is when the user did the thing.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The store, as `app_view` receives it.
///
/// `snippets` is the **user's list alone** — never the merged library. The
/// built-in pack is `snippet::builtins(dialect)` and is merged into a memo in
/// `app_view`, which is what keeps the pack in code where a later release can
/// fix one, instead of copied into everybody's `snippets.json`.
pub(crate) struct SnippetStore {
    pub(crate) snippets: RwSignal<Vec<Snippet>>,
    /// "This was used just now", for the row's `3d ago` and the recently-used
    /// sort. A no-op on a built-in — it has nothing to record, and without the
    /// gate every insert of a shipped snippet rewrote `snippets.json` for no
    /// change.
    pub(crate) record_use: Rc<dyn Fn(u64)>,
    /// Save the editor's contribution as a new snippet, returning its id. The
    /// caller supplies `body` and `name` because both come from the *tab*.
    pub(crate) create: Rc<dyn Fn(String, String, u64) -> u64>,
    pub(crate) rename: Rc<dyn Fn(u64, String)>,
    pub(crate) set_abbrev: Rc<dyn Fn(u64, Option<String>)>,
    pub(crate) set_body: Rc<dyn Fn(u64, String)>,
    pub(crate) set_scope: Rc<dyn Fn(u64, Scope)>,
    /// Copy a snippet the caller has already found in the **merged** library.
    pub(crate) duplicate_from: Rc<dyn Fn(Snippet)>,
    /// Delete a snippet, erasing. **Takes no confirm**: the caller raises it —
    /// see the module doc.
    pub(crate) remove: Rc<dyn Fn(u64)>,
    /// Drop a deleted connection's connection-scoped snippets.
    ///
    /// **The twelfth store, and the one that was missed** when the deletion
    /// block was first made to erase: a connection-scoped snippet is a query the
    /// user wrote against *this* connection, so both of that block's reasons
    /// apply to it — and the id-recycling one with force, since the next
    /// connection to take the freed id would have found them waiting under
    /// "THIS CONNECTION".
    pub(crate) clear_conn: Rc<dyn Fn(u64)>,
}

/// Load the library and build its writers.
pub(crate) fn wire() -> SnippetStore {
    let snippets = RwSignal::new(persist::load_json::<SnippetsFile>(FILE).snippets);

    let record_use: Rc<dyn Fn(u64)> = Rc::new(move |id: u64| {
        if schemaic_core::snippet::is_builtin(id) {
            return;
        }
        snippets.update(|v| schemaic_core::snippet::touch(v, id, now()));
        save(snippets, persist::Saving::Replacing);
    });

    let create: Rc<dyn Fn(String, String, u64) -> u64> =
        Rc::new(move |name: String, body: String, conn_id: u64| {
            let id = snippets.with_untracked(|v| schemaic_core::snippet::next_id(v));
            // The scope and the used-now stamp are `new_saved`'s, in core with
            // the test that composes them with the panel's grouping and sort —
            // both were a struct literal in `app_view`, and both shipped wrong
            // once.
            snippets.update(|v| {
                v.push(schemaic_core::snippet::new_saved(
                    id,
                    name,
                    body,
                    conn_id,
                    now(),
                ))
            });
            save(snippets, persist::Saving::Replacing);
            id
        });

    let rename: Rc<dyn Fn(u64, String)> = Rc::new(move |id: u64, name: String| {
        snippets.update(|v| {
            if let Some(s) = v.iter_mut().find(|s| s.id == id) {
                s.name = name.clone();
            }
        });
        save(snippets, persist::Saving::Replacing);
    });

    let set_abbrev: Rc<dyn Fn(u64, Option<String>)> = Rc::new(move |id: u64, abbrev| {
        snippets.update(|v| {
            if let Some(s) = v.iter_mut().find(|s| s.id == id) {
                s.abbrev = abbrev.clone().filter(|a: &String| !a.trim().is_empty());
            }
        });
        save(snippets, persist::Saving::Replacing);
    });

    let set_body: Rc<dyn Fn(u64, String)> = Rc::new(move |id: u64, body: String| {
        snippets.update(|v| {
            if let Some(s) = v.iter_mut().find(|s| s.id == id) {
                s.body = body.clone();
            }
        });
        save(snippets, persist::Saving::Replacing);
    });

    let set_scope: Rc<dyn Fn(u64, Scope)> = Rc::new(move |id: u64, scope: Scope| {
        snippets.update(|v| {
            if let Some(s) = v.iter_mut().find(|s| s.id == id) {
                s.scope = scope.clone();
            }
        });
        save(snippets, persist::Saving::Replacing);
    });

    let duplicate_from: Rc<dyn Fn(Snippet)> = Rc::new(move |src: Snippet| {
        let new_id = snippets.with_untracked(|v| schemaic_core::snippet::next_id(v));
        // The five things the copy does and does not inherit are
        // `snippet::duplicate`'s, with the tests: they were a struct literal in
        // `app_view`, where nothing could call them.
        snippets.update(|v| v.push(schemaic_core::snippet::duplicate(&src, new_id)));
        save(snippets, persist::Saving::Replacing);
    });

    let remove: Rc<dyn Fn(u64)> = Rc::new(move |id: u64| {
        snippets.update(|v| schemaic_core::snippet::remove(v, id));
        save(snippets, persist::Saving::Erasing);
    });

    let clear_conn: Rc<dyn Fn(u64)> = Rc::new(move |id: u64| {
        snippets.update(|v| schemaic_core::snippet::clear_conn(v, id));
        save(snippets, persist::Saving::Erasing);
    });

    SnippetStore {
        snippets,
        record_use,
        create,
        rename,
        set_abbrev,
        set_body,
        set_scope,
        duplicate_from,
        remove,
        clear_conn,
    }
}

#[cfg(test)]
mod tests {
    /// **The module's reason for existing, as a gate** — the twin of
    /// `history_store::the_removal_paths_erase`, and deliberately spelled the
    /// same way so the pair reads as one rule in two places.
    ///
    /// Exactly two erasing saves — `remove` and `clear_conn` — and seven
    /// ordinary ones. An edit that erased would delete a `.bak` the user might
    /// want; a *delete* that only replaced would leave the snippet's whole body
    /// in `snippets.json.bak` under a modal saying it cannot be undone, which is
    /// the bug the policy exists for.
    ///
    /// Reads its own source through `production_code`, which blanks comments and
    /// this module below it — the `source_gate` family's hazard being that a
    /// gate in the same file as its subject is its own first match. Needles are
    /// assembled from fragments for the same reason, and the counts are exact
    /// rather than floors so that a writer *added* without a save is a failure
    /// too.
    #[test]
    fn the_remove_path_erases() {
        let raw = include_str!("snippet_store.rs");
        let src = schemaic_ui::source_gate::production_code(raw);
        let body = src
            .split_once("pub(crate) fn wire()")
            .expect("`wire` is what this gate reads")
            .1;
        let erasing = ["Saving", "::", "Erasing"].concat();
        let replacing = ["Saving", "::", "Replacing"].concat();
        assert_eq!(
            body.matches(&erasing).count(),
            2,
            "exactly two erasing saves in `wire` — `remove`, behind the confirm \
             `app_view` raises, and `clear_conn` for a deleted connection"
        );
        assert_eq!(
            body.matches(&replacing).count(),
            7,
            "seven ordinary saves in `wire` — record_use, create, rename, \
             set_abbrev, set_body, set_scope, duplicate_from. A writer added \
             without one would leave the library unpersisted"
        );
        assert_eq!(
            src.matches(&["snippets", ".json"].concat()).count(),
            1,
            "`snippets.json` should appear once in this module's code, as `FILE`"
        );
    }
}
