# `core/jsontree.rs`

`jsontree.rs` — the editable JSON tree the row panel uses for a `json`/`jsonb` column.
`JsonNode::parse` walks the document as `RawValue`s rather than going through
`serde_json::Value`, for two reasons: object key order survives, duplicates included (a
PostgreSQL `json` column really can hold `{"a":1,"a":2}`), and **a number keeps its source text
byte for byte** — going through `f64` turned `10.00` into `10` and a 21-digit integer into a
different integer, silently, the moment an unrelated leaf edit re-serialised the tree.
`PathSeg::{Member(i),Index(i)}` addresses by *position*, not key, so duplicate keys get distinct
independently-editable rows; `set_leaf` reparses arbitrary JSON, so editing a leaf may change
its type. `rows`/`TreeRow` is the flattened outline the UI renders.
