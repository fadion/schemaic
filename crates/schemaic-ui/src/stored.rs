//! A persisted store the UI can read freely and write only by saving.
//!
//! `formats`, `db_colors`, `table_colors` and `db_favorites` used to reach the
//! UI as a raw `RwSignal` beside a separate save closure, so writing the signal
//! and forgetting the save compiled — and looked right until a restart: the
//! swatch painted, the star turned gold, and the choice was gone next launch.
//! The only thing between that and a shipped bug was a source gate matching a
//! `persist::Saving::` within 700 bytes of each core mutator.
//!
//! [`Stored`] makes the pairing unspellable instead. Its signal and its saver
//! are private to this module, so the only ways to change the store are
//! [`Stored::update`] and [`Stored::erase`], and both save. Which kind of save
//! is still the caller's to say — an upsert keeps the previous file as `.bak`,
//! a deletion must not — but *whether* to save no longer is.
//!
//! It is `Copy`, like the `RwSignal` it replaces, because the saver sits in a
//! signal of its own rather than a bare `Rc`: the grid's `GridState` and every
//! closure that captured the old field keep their shape.

use std::rc::Rc;

use floem::prelude::*;
use floem::reactive::ReadSignal;
use schemaic_core::persist::Saving;

/// See the module doc.
pub struct Stored<T: 'static> {
    signal: RwSignal<T>,
    save: RwSignal<Rc<dyn Fn(Saving)>>,
}

impl<T: 'static> Clone for Stored<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Stored<T> {}

impl<T: 'static> Stored<T> {
    /// A store over `signal`, saved by `save`.
    ///
    /// Takes the signal rather than the value because two stores can share one
    /// file — `db_colors` and `table_colors` are both `db_colors.json` — and the
    /// shared saver has to read both signals before either store exists. Whoever
    /// builds a `Stored` should not keep the `RwSignal`; the app's
    /// `ui_stores::wire` is the one place that does, and only to build the savers.
    pub fn new(signal: RwSignal<T>, save: Rc<dyn Fn(Saving)>) -> Self {
        Stored {
            signal,
            save: RwSignal::new(save),
        }
    }

    /// Change the store and save it, keeping the previous file as `.bak`.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        self.write(f, Saving::Replacing);
    }

    /// Change the store because something is **gone** — a deleted connection's
    /// rules — and save with [`Saving::Erasing`], so no `.bak` keeps a copy of
    /// what the user was told cannot be recovered.
    pub fn erase(&self, f: impl FnOnce(&mut T)) {
        self.write(f, Saving::Erasing);
    }

    fn write(&self, f: impl FnOnce(&mut T), saving: Saving) {
        self.signal.update(f);
        // The `Rc` is cloned out rather than called inside a borrow of its
        // signal: a saver reads the store's own signal (and, for the colour
        // pair, its sibling's), and nothing should be held while it runs.
        let save = self.save.get_untracked();
        save(saving);
    }

    /// A read-only handle, for a struct that only ever reads the store.
    pub fn read_only(&self) -> ReadSignal<T> {
        self.signal.read_only()
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.signal.with(f)
    }

    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.signal.with_untracked(f)
    }
}

impl<T: Clone + 'static> Stored<T> {
    pub fn get(&self) -> T {
        self.signal.get()
    }

    pub fn get_untracked(&self) -> T {
        self.signal.get_untracked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Each save: its kind, and what the store held when it ran.
    type SaveLog = Rc<RefCell<Vec<(Saving, Vec<i32>)>>>;

    /// A store over a `Vec<i32>` whose saver records each save.
    fn recorded() -> (Stored<Vec<i32>>, SaveLog) {
        let signal = RwSignal::new(vec![1]);
        let log: SaveLog = Rc::default();
        let save = {
            let log = log.clone();
            Rc::new(move |saving| log.borrow_mut().push((saving, signal.get_untracked())))
        };
        (Stored::new(signal, save), log)
    }

    #[test]
    fn update_changes_the_store_and_saves_keeping_a_backup() {
        let (store, log) = recorded();
        store.update(|v| v.push(2));
        assert_eq!(store.get_untracked(), vec![1, 2]);
        assert_eq!(*log.borrow(), vec![(Saving::Replacing, vec![1, 2])]);
    }

    #[test]
    fn erase_changes_the_store_and_saves_without_a_backup() {
        let (store, log) = recorded();
        store.erase(|v| v.clear());
        assert_eq!(store.get_untracked(), Vec::<i32>::new());
        assert_eq!(*log.borrow(), vec![(Saving::Erasing, vec![])]);
    }

    #[test]
    fn the_save_runs_after_the_change_not_before() {
        // A saver that ran first would write the previous generation and lose
        // the choice just made — the exact bug this type exists to rule out.
        let (store, log) = recorded();
        store.update(|v| v[0] = 9);
        assert_eq!(log.borrow()[0].1, vec![9]);
    }

    #[test]
    fn every_write_saves_exactly_once() {
        let (store, log) = recorded();
        store.update(|v| v.push(2));
        store.update(|v| v.push(3));
        store.erase(|v| v.pop().map(drop).unwrap_or_default());
        let kinds: Vec<Saving> = log.borrow().iter().map(|(s, _)| *s).collect();
        assert_eq!(
            kinds,
            vec![Saving::Replacing, Saving::Replacing, Saving::Erasing]
        );
    }

    #[test]
    fn reads_see_the_store_and_save_nothing() {
        let (store, log) = recorded();
        assert_eq!(store.get(), vec![1]);
        assert_eq!(store.with(|v| v.len()), 1);
        assert_eq!(store.with_untracked(|v| v[0]), 1);
        assert_eq!(store.read_only().get_untracked(), vec![1]);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn a_copy_is_the_same_store() {
        let (store, log) = recorded();
        let copy = store;
        copy.update(|v| v.push(2));
        assert_eq!(store.get_untracked(), vec![1, 2]);
        assert_eq!(log.borrow().len(), 1);
    }

    #[test]
    fn two_stores_sharing_a_saver_both_save_the_whole_file() {
        // `db_colors` and `table_colors`: one file, one saver reading both.
        let a = RwSignal::new(vec![1]);
        let b = RwSignal::new(vec![10]);
        type PairLog = Rc<RefCell<Vec<(Vec<i32>, Vec<i32>)>>>;
        let log: PairLog = Rc::default();
        let save: Rc<dyn Fn(Saving)> = {
            let log = log.clone();
            Rc::new(move |_| {
                log.borrow_mut()
                    .push((a.get_untracked(), b.get_untracked()))
            })
        };
        let (sa, sb) = (Stored::new(a, save.clone()), Stored::new(b, save));
        sa.update(|v| v.push(2));
        sb.update(|v| v.push(20));
        assert_eq!(
            *log.borrow(),
            vec![(vec![1, 2], vec![10]), (vec![1, 2], vec![10, 20])]
        );
    }
}
