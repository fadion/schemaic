//! Reading one binary cell's bytes back — `Db::fetch_blob`, on the wire.
//!
//! **This seam has no pure test and cannot have one.** The bytes of a `BLOB`
//! never reach a `ResultSet` on any engine, so nothing in the pure tier ever
//! holds a real one: `core::blob` can be told what a PNG header looks like, and
//! the SQLite suite can round-trip a blob through a file-less database, but only
//! a server can answer whether MySQL's `SUBSTRING` is byte-indexed on a binary
//! string and whether PostgreSQL's `bytea` survives the simple-query protocol's
//! hex text intact. Both were assumptions when this was written.
//!
//! The bytes are `DEADBEEF` because [`crate::cases`] already uses them for the
//! binary type case on both engines — `X'DEADBEEF'` and `'\xdeadbeef'` are the
//! same four bytes, so one expectation covers every leg. Every one of them is
//! outside ASCII, which is the point: this is the exact class of value the
//! `<n bytes>` placeholder exists to stop being rendered as mojibake, and a
//! decoder that lost the high bit would pass a test written with `"hello"`.

use schemaic_core::blob::{BlobRef, blob_source, sniff};
use schemaic_core::model::Value;
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// The four bytes every case in this module stores.
const DEADBEEF: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];

/// This server's spelling of a raw-bytes column, and a literal of [`DEADBEEF`]
/// in it.
///
/// Read off [`crate::cases`] rather than branched on the engine, for the reason
/// that module's own doc gives: a `match` on the engine here compiles cleanly
/// while sorting a fourth server onto whichever side it happens to fall, and
/// the spellings are already written down once.
fn binary_case(target: &'static Target) -> (&'static str, &'static str) {
    let case = target
        .type_cases()
        .find(|c| c.display.is_some_and(|d| d.starts_with('<')))
        .unwrap_or_else(|| {
            panic!(
                "{}: no binary type case — this module needs one to build its column",
                target.name
            )
        });
    (case.sql_type, case.literal)
}

/// Create `t (id, payload)` with one row per `(id, literal)` given.
async fn seed(scratch: &Scratch, table: &str, ty: &str, rows: &[(i64, &str)]) {
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, payload {ty})",
            scratch.qualified(table)
        ))
        .await;
    for (id, literal) in rows {
        scratch
            .exec(&format!(
                "INSERT INTO {} (id, payload) VALUES ({id}, {literal})",
                scratch.qualified(table)
            ))
            .await;
    }
}

fn blob_ref(scratch: &Scratch, table: &str, id: i64) -> BlobRef {
    BlobRef {
        database: scratch.database.clone(),
        schema: scratch.namespace.map(str::to_string),
        table: table.to_string(),
        column: "payload".to_string(),
        key: vec![("id".to_string(), Value::Int(id))],
    }
}

/// **The bytes come back exactly as stored.**
///
/// The claim the whole panel rests on: what the grid could only call
/// `<4 bytes>` is re-read byte-for-byte, and the length reported is the whole
/// value's.
pub async fn a_blob_reads_back_byte_for_byte(target: &'static Target) {
    let scratch = Scratch::create(target, "blobread").await;
    let (ty, literal) = binary_case(target);
    seed(&scratch, "b", ty, &[(1, literal)]).await;

    let got = scratch
        .db
        .fetch_blob(&blob_ref(&scratch, "b", 1), CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: the fetch failed: {e}", target.name))
        .unwrap_or_else(|| panic!("{}: the row has bytes and reported none", target.name));

    assert_eq!(got.bytes, DEADBEEF, "{}: bytes", target.name);
    assert_eq!(got.len, 4, "{}: reported length", target.name);
    assert!(
        !got.truncated(),
        "{}: four bytes is not truncated",
        target.name
    );

    scratch.teardown().await;
}

/// **The key selects the row it names.**
///
/// The fetch ends in `LIMIT 1`, so a `WHERE` that failed to narrow would still
/// return a row — a plausible-looking blob from whichever row the server
/// happened to hand over first. Three rows with distinguishable bytes is the
/// only shape in which that is visible.
pub async fn a_blob_fetch_lands_on_the_row_its_key_names(target: &'static Target) {
    let scratch = Scratch::create(target, "blobkey").await;
    let (ty, _) = binary_case(target);
    // One byte apiece, so which row answered is unmistakable. Spelled per
    // dialect from the same source the case list uses.
    let lit = |hex: &str| match scratch.dialect() {
        schemaic_core::intel::SqlDialect::Postgres => format!("'\\x{hex}'"),
        _ => format!("X'{hex}'"),
    };
    let (l1, l2, l3) = (lit("aa"), lit("bb"), lit("cc"));
    seed(&scratch, "b", ty, &[(1, &l1), (2, &l2), (3, &l3)]).await;

    for (id, byte) in [(1i64, 0xaau8), (2, 0xbb), (3, 0xcc)] {
        let got = scratch
            .db
            .fetch_blob(&blob_ref(&scratch, "b", id), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{}: row {id} failed: {e}", target.name))
            .unwrap_or_else(|| panic!("{}: row {id} reported no bytes", target.name));
        assert_eq!(
            got.bytes,
            [byte],
            "{}: row {id} returned another row's bytes",
            target.name
        );
    }

    scratch.teardown().await;
}

/// A NULL cell reports nothing — not an error, and not an empty blob.
pub async fn a_null_blob_reports_nothing(target: &'static Target) {
    let scratch = Scratch::create(target, "blobnull").await;
    let (ty, _) = binary_case(target);
    seed(&scratch, "b", ty, &[(1, "NULL")]).await;

    let got = scratch
        .db
        .fetch_blob(&blob_ref(&scratch, "b", 1), CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: the fetch failed: {e}", target.name));
    assert!(got.is_none(), "{}: a NULL cell has no bytes", target.name);

    // And a key that matches nothing is the same answer, from the other cause.
    let gone = scratch
        .db
        .fetch_blob(&blob_ref(&scratch, "b", 404), CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: the fetch failed: {e}", target.name));
    assert!(
        gone.is_none(),
        "{}: a missing row has no bytes",
        target.name
    );

    scratch.teardown().await;
}

/// **The whole path, from the result the grid holds to the bytes behind it.**
///
/// The two halves are tested apart everywhere else — `blob_source` against a
/// hand-built `EditModel`, `fetch_blob` against a hand-built `BlobRef` — and the
/// seam between them is exactly where this feature could be wrong while both
/// halves pass: `blob_source` deliberately does not ask through
/// `EditModel::table_index`, and one written against the write model's own
/// lookup returned `None` here for as long as C2 kept a binary column out of
/// `col_table`. This runs a real `SELECT`, derives the reference from its
/// provenance, and fetches with it.
pub async fn a_binary_cell_in_a_real_result_resolves_and_fetches(target: &'static Target) {
    let scratch = Scratch::create(target, "blobend2end").await;
    let (ty, literal) = binary_case(target);
    seed(&scratch, "b", ty, &[(7, literal)]).await;

    let (rs, model) = scratch
        .edit_model(&format!(
            "SELECT id, payload FROM {}",
            scratch.qualified("b")
        ))
        .await;

    // The premise: the grid shows a placeholder, so the column takes no *text*
    // write — the bytes themselves go in through the blob panel.
    assert_eq!(
        rs.cell(0, 1).map(|c| c.display().to_string()).as_deref(),
        Some("<4 bytes>"),
        "{}: the grid should hold a placeholder, not bytes",
        target.name
    );
    assert!(
        !model.text_editable(1),
        "{}: the placeholder must not be typeable back over the value",
        target.name
    );
    assert!(
        model.editable(1),
        "{}: but the column itself takes a write",
        target.name
    );

    let bref = blob_source(&model, &rs, 0, 1)
        .unwrap_or_else(|| panic!("{}: the binary cell resolved no source", target.name));
    assert_eq!(bref.column, "payload", "{}", target.name);
    assert_eq!(
        bref.key,
        vec![("id".to_string(), Value::Int(7))],
        "{}: keyed by its own row",
        target.name
    );

    let got = scratch
        .db
        .fetch_blob(&bref, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: the fetch failed: {e}", target.name))
        .unwrap_or_else(|| panic!("{}: reported no bytes", target.name));
    assert_eq!(got.bytes, DEADBEEF, "{}", target.name);

    scratch.teardown().await;
}

/// A stored PNG is recognized as one, over the wire.
///
/// `sniff` has a table of pure tests, so what this adds is the *transport*: a
/// header of NULs, CR/LF and bytes above `0x7f` is precisely what a text round
/// trip mangles, and a preview that opened on mangled bytes would draw nothing
/// at all rather than fail.
pub async fn a_stored_png_still_sniffs_as_one_after_the_round_trip(target: &'static Target) {
    let scratch = Scratch::create(target, "blobpng").await;
    let (ty, _) = binary_case(target);
    const PNG_HEX: &str = "89504e470d0a1a0a0000000d49484452";
    let literal = match scratch.dialect() {
        schemaic_core::intel::SqlDialect::Postgres => format!("'\\x{PNG_HEX}'"),
        _ => format!("X'{PNG_HEX}'"),
    };
    // The case's own type may be too narrow for sixteen bytes (MySQL's is
    // `VARBINARY(4)`), so widen it where it is parameterised.
    let ty = if ty.eq_ignore_ascii_case("VARBINARY(4)") {
        "VARBINARY(64)"
    } else {
        ty
    };
    seed(&scratch, "b", ty, &[(1, &literal)]).await;

    let got = scratch
        .db
        .fetch_blob(&blob_ref(&scratch, "b", 1), CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: the fetch failed: {e}", target.name))
        .unwrap_or_else(|| panic!("{}: reported no bytes", target.name));

    assert_eq!(got.len, 16, "{}: length", target.name);
    assert_eq!(
        sniff(&got.bytes),
        schemaic_core::blob::BlobKind::Png,
        "{}: a PNG header did not survive the round trip: {:02x?}",
        target.name,
        got.bytes
    );

    scratch.teardown().await;
}

/// **The write half, on the wire.** Stage bytes the way the blob panel does,
/// commit, and read them back through the same `fetch_blob` the panel uses.
///
/// This seam has no pure test on these two engines for the reason the module
/// header gives about the read: only a server can answer whether MySQL binds a
/// `MyValue::Bytes` as octets rather than as the characters they lossily decode
/// to, and whether PostgreSQL's `decode('…','hex')` — a *literal*, since that
/// module builds its writes as SQL text rather than as parameters — reaches a
/// `bytea` column as the bytes it names. Both were assumptions when this was
/// written, and the second one is a wager on `standard_conforming_strings` that
/// the `'\x…'::bytea` spelling would have lost.
///
/// The payload is four bytes because the column is (`VARBINARY(4)` on the MySQL
/// leg — [`crate::cases`] owns the width), and it is deliberately **not**
/// [`DEADBEEF`]: `de 00 be ff` differs from what was seeded in two positions, so
/// a write that did nothing fails; it carries an interior NUL, so a byte string
/// truncated at the first one fails on length; and `0xde`/`0xff` are not valid
/// UTF-8 on their own, so a value that went out through `String::from_utf8_lossy`
/// arrives as replacement characters and fails on both.
pub async fn staged_bytes_reach_the_column_as_bytes(target: &'static Target) {
    use schemaic_core::edit::{DirtyCells, build_edits};
    use schemaic_core::model::{CellEdit, GridWrite};

    let scratch = Scratch::create(target, "blobwrite").await;
    let (ty, literal) = binary_case(target);
    seed(&scratch, "b", ty, &[(7, literal)]).await;

    let (rs, model) = scratch
        .edit_model(&format!(
            "SELECT id, payload FROM {}",
            scratch.qualified("b")
        ))
        .await;

    // The narrowed C2, over a real result: writable, but not by typing.
    assert!(
        model.editable(1) && !model.text_editable(1),
        "{}: a binary column must take a bytes write and no text one",
        target.name
    );

    let payload: Vec<u8> = vec![0xde, 0x00, 0xbe, 0xff];
    let dirty: DirtyCells = [((0usize, 1usize), CellEdit::bytes(payload.clone()))]
        .into_iter()
        .collect();
    let write = GridWrite {
        updates: build_edits(&model, &rs, &dirty),
        ..Default::default()
    };
    assert_eq!(
        write.updates.len(),
        1,
        "{}: one staged cell, one UPDATE",
        target.name
    );

    let n = scratch
        .db
        .commit_writes(&write, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: the commit failed: {e}", target.name));
    assert_eq!(n, 1, "{}: the 1-row safety net", target.name);

    let bref = blob_source(&model, &rs, 0, 1)
        .unwrap_or_else(|| panic!("{}: the binary cell resolved no source", target.name));
    let got = scratch
        .db
        .fetch_blob(&bref, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: the re-read failed: {e}", target.name))
        .unwrap_or_else(|| panic!("{}: the written cell read back as empty", target.name));
    assert_eq!(
        got.bytes, payload,
        "{}: the column holds something other than the bytes staged",
        target.name
    );
    assert_eq!(
        got.len,
        payload.len() as u64,
        "{}: the column's length disagrees with its bytes",
        target.name
    );

    scratch.teardown().await;
}
