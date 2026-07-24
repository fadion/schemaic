//! Headless memory + timing diagnostic for the columnar [`ResultSet`] refactor.
//!
//! Builds a representative 200k×50 result and reports, for the columnar layout
//! vs the old row-major `Vec<Vec<Value>>`:
//!   * **held heap** — exact live bytes, via a counting global allocator (so it
//!     measures real allocations incl. `Vec`/`String` over-capacity, with no GPU
//!     stack or allocator noise — this is the number the refactor set out to move);
//!   * **build time** — constructing each layout from the same generated rows;
//!   * **numeric-sort time** — the one plausible regression: columnar sort parses
//!     an `f64` out of the arena text on every comparison, where the row-major
//!     path reads it straight from the typed `Value`.
//!
//! Not a unit test and not wired into CI — a one-shot, run manually:
//!   cargo run --release --example result_footprint -p schemaic-core
//!   cargo run --release --example result_footprint -p schemaic-core -- 50000 30
//!
//! It only uses the crate's public API, so it doubles as a check that the
//! columnar storage is measurable/buildable from outside the crate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use schemaic_core::model::{CellRef, CellTag, Column, ResultBuilder, ResultSet, Value};

// ── Counting allocator: tracks live (allocated − freed) bytes ────────────────

static LIVE: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: delegates every call to the system allocator unchanged, only adding
// atomic byte bookkeeping around it.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let np = unsafe { System.realloc(p, l, new) };
        if !np.is_null() {
            // realloc frees the old block and returns a `new`-sized one.
            LIVE.fetch_add(new, Ordering::Relaxed);
            LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        }
        np
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

// ── Representative dataset generation (deterministic, no `rand`) ──────────────

/// SplitMix64 step — a fast deterministic generator so runs are reproducible
/// without pulling in `rand`.
fn mix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const WORDS: &[&str] = &[
    "alpha",
    "bravo",
    "charlie",
    "delta",
    "echo",
    "foxtrot",
    "golf",
    "hotel",
    "india",
    "juliet",
    "kilo",
    "lima",
    "customer",
    "invoice",
    "shipment",
    "pending",
    "settled",
    "region-west",
];

/// One column's category, chosen by column index — a realistic mix of an id, a
/// few plain ints, many varchars, datetimes, decimals, and some nullable text.
#[derive(Clone, Copy)]
enum Kind {
    Id,
    Int,
    Varchar,
    Datetime,
    Decimal,
    Nullable,
}

fn kind_of(col: usize) -> Kind {
    match col {
        0 => Kind::Id,
        1..=7 => Kind::Int,
        8..=27 => Kind::Varchar,
        28..=37 => Kind::Datetime,
        38..=43 => Kind::Decimal,
        _ => Kind::Nullable,
    }
}

fn gen_cell(kind: Kind, row: usize, col: usize, rng: &mut u64) -> Value {
    match kind {
        Kind::Id => Value::UInt(row as u64 + 1),
        Kind::Int => Value::Int((mix(rng) % 1_000_000) as i64 - 500_000),
        Kind::Varchar => {
            let a = WORDS[(mix(rng) as usize) % WORDS.len()];
            let b = WORDS[(mix(rng) as usize) % WORDS.len()];
            Value::Str(format!("{a}_{b}{}", col))
        }
        Kind::Datetime => {
            let d = 1 + (mix(rng) % 28);
            let h = mix(rng) % 24;
            Value::Str(format!("2021-06-{d:02} {h:02}:34:56"))
        }
        Kind::Decimal => {
            let w = mix(rng) % 100_000;
            let f = mix(rng) % 100;
            Value::Str(format!("{w}.{f:02}"))
        }
        Kind::Nullable => {
            if mix(rng) % 10 < 3 {
                Value::Null
            } else {
                Value::Str(WORDS[(mix(rng) as usize) % WORDS.len()].to_string())
            }
        }
    }
}

fn gen_row(row: usize, ncols: usize) -> Vec<Value> {
    // Seed per row so the data is deterministic and independent of build order.
    let mut rng = (row as u64).wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0xDEAD_BEEF;
    (0..ncols)
        .map(|c| gen_cell(kind_of(c), row, c, &mut rng))
        .collect()
}

fn columns(ncols: usize) -> Vec<Column> {
    (0..ncols)
        .map(|c| Column {
            name: format!("col_{c}"),
            type_name: match kind_of(c) {
                Kind::Id => "BIGINT UNSIGNED",
                Kind::Int => "INT",
                Kind::Varchar | Kind::Nullable => "VARCHAR",
                Kind::Datetime => "DATETIME",
                Kind::Decimal => "DECIMAL",
            }
            .to_string(),
            origin: None,
        })
        .collect()
}

// ── Sort comparators (mirrors of grid.rs cmp_cell / the old cmp_value) ────────

fn cell_num(c: CellRef) -> Option<f64> {
    match c.tag {
        CellTag::Int | CellTag::UInt | CellTag::Float => c.text().parse::<f64>().ok(),
        _ => None,
    }
}

fn cmp_cell(a: CellRef, b: CellRef) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.is_null(), b.is_null()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => match (cell_num(a), cell_num(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => a.display().cmp(b.display()),
        },
    }
}

fn value_num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn cmp_value(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Greater,
        (_, Value::Null) => Ordering::Less,
        _ => match (value_num(a), value_num(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => a.display().cmp(&b.display()),
        },
    }
}

/// Best of `iters` runs (min = least scheduler/GC noise) of `f`, in ms.
fn best_ms(iters: u32, mut f: impl FnMut()) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..iters {
        let t = Instant::now();
        f();
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    best
}

fn main() {
    let mut args = std::env::args().skip(1);
    let nrows: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(200_000);
    let ncols: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(50);
    let cells = nrows * ncols;

    println!("=== ResultSet footprint: {nrows} rows × {ncols} cols ({cells} cells) ===");
    println!(
        "size_of::<Value>() = {} bytes\n",
        std::mem::size_of::<Value>()
    );

    // Generate the rows once, up front, so build timing/footprint excludes it.
    let data: Vec<Vec<Value>> = (0..nrows).map(|r| gen_row(r, ncols)).collect();
    let cols = columns(ncols);

    // ── Held heap: columnar vs row-major ─────────────────────────────────────
    // Build columnar via the incremental builder (the loader's path): each source
    // row is copied into the arena, so only the finished ResultSet is held.
    let base = live();
    let rs: ResultSet = {
        let mut b = ResultBuilder::with_capacity(cols.clone(), nrows);
        for row in &data {
            b.push_row(row);
        }
        b.finish()
    };
    let columnar_bytes = live() - base;

    // Row-major: a clone of the generated rows plus the columns, held live.
    let base2 = live();
    let row_major: Vec<Vec<Value>> = data.clone();
    let cols_hold = cols.clone();
    let rowmajor_bytes = live() - base2;
    black_box(&cols_hold);

    println!("held heap");
    println!(
        "  row-major Vec<Vec<Value>> : {:>8.1} MB  ({} bytes, {:.1} B/cell)",
        mb(rowmajor_bytes),
        rowmajor_bytes,
        rowmajor_bytes as f64 / cells as f64
    );
    println!(
        "  columnar   ResultSet      : {:>8.1} MB  ({} bytes, {:.1} B/cell)",
        mb(columnar_bytes),
        columnar_bytes,
        columnar_bytes as f64 / cells as f64
    );
    println!(
        "  reduction                 : {:>8.2}×  (−{:.1} MB, −{:.0}%)\n",
        rowmajor_bytes as f64 / columnar_bytes as f64,
        mb(rowmajor_bytes.saturating_sub(columnar_bytes)),
        (1.0 - columnar_bytes as f64 / rowmajor_bytes as f64) * 100.0
    );

    // ── Build time ───────────────────────────────────────────────────────────
    let build_col = best_ms(3, || {
        let mut b = ResultBuilder::with_capacity(cols.clone(), nrows);
        for row in &data {
            b.push_row(row);
        }
        black_box(b.finish());
    });
    let build_row = best_ms(3, || {
        black_box(data.clone());
    });
    println!("build time (best of 3)");
    println!("  row-major clone           : {build_row:>8.1} ms");
    println!("  columnar  builder         : {build_col:>8.1} ms\n");

    // ── Numeric sort time (the regression to rule out) ───────────────────────
    // Sort display indices by column 1 (an INT column): columnar parses f64 per
    // comparison, row-major reads the typed Value directly.
    const SORT_COL: usize = 1;
    let sort_col = best_ms(3, || {
        let mut idx: Vec<usize> = (0..nrows).collect();
        idx.sort_by(
            |&a, &b| match (rs.cell(a, SORT_COL), rs.cell(b, SORT_COL)) {
                (Some(x), Some(y)) => cmp_cell(x, y),
                _ => std::cmp::Ordering::Equal,
            },
        );
        black_box(idx);
    });
    let sort_row = best_ms(3, || {
        let mut idx: Vec<usize> = (0..nrows).collect();
        idx.sort_by(|&a, &b| cmp_value(&row_major[a][SORT_COL], &row_major[b][SORT_COL]));
        black_box(idx);
    });
    // Decorate-sort: parse each numeric key once up front, then sort — this is
    // what `grid::compute_order` actually does.
    let sort_col_decorated = best_ms(3, || {
        let keys: Vec<(bool, Option<f64>, &str)> = (0..nrows)
            .map(|r| match rs.cell(r, SORT_COL) {
                Some(cell) => (cell.is_null(), cell_num(cell), cell.text()),
                None => (true, None, ""),
            })
            .collect();
        let mut idx: Vec<usize> = (0..nrows).collect();
        idx.sort_by(|&a, &b| {
            let (ka, kb) = (&keys[a], &keys[b]);
            match (ka.0, kb.0) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => match (ka.1, kb.1) {
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                    _ => ka.2.cmp(kb.2),
                },
            }
        });
        black_box(idx);
    });
    println!("numeric sort time, col {SORT_COL} INT (best of 3)");
    println!("  row-major (Value direct)  : {sort_row:>8.1} ms");
    println!(
        "  columnar  (parse per cmp) : {sort_col:>8.1} ms  ({:.2}× row-major)",
        sort_col / sort_row.max(f64::MIN_POSITIVE)
    );
    println!(
        "  columnar  (decorate-sort) : {sort_col_decorated:>8.1} ms  ({:.2}× row-major) ← grid::compute_order\n",
        sort_col_decorated / sort_row.max(f64::MIN_POSITIVE)
    );

    // Keep everything alive until the end so the footprint numbers are honest.
    black_box((&rs, &row_major));
}
