//! A table change as the AI proposes it — a **patch**, applied to the current
//! table to produce a [`TableDraft`] the user reviews.
//!
//! The model never writes SQL here and never writes a [`crate::ddl::ChangeSet`]:
//! it writes the ops it wants, this module lays them over
//! [`TableDraft::from_table`], and the result goes through `ddl::diff` → `emit`
//! → the preview modal like any draft the designer produced. That keeps
//! `ddl::diff` the only differ, and it keeps the write behind the same Apply
//! click.
//!
//! **Why a patch and not the whole table.** The obvious wire shape is "here is
//! the table you should have", and it is wrong in a way that only shows up in
//! use: `diff` compares field by field, so a model that re-types `varchar(255)`
//! as `VARCHAR(255)` proposes a `MODIFY COLUMN` on a column nobody asked about,
//! and a model that omits a column it was never asked about proposes to
//! **drop** it. Both are then true statements about the draft it sent, so
//! nothing downstream can tell them from an intended change. A patch cannot
//! express either by accident: what it does not name, it does not touch.

use serde::Deserialize;

use crate::ddl::{CheckDraft, ColumnDraft, ForeignKeyDraft, IndexDraft, TableDraft};
use crate::intel::SqlDialect;
use crate::schema::{CheckInfo, ColumnInfo, ForeignKeyInfo, IndexInfo, TableInfo};

/// A proposed change to one table, as the model writes it in JSON.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// The table the ops address. Checked against the table actually loaded, so
    /// a proposal that drifted onto the wrong table is refused rather than
    /// applied to whatever happens to be open.
    pub table: String,
    #[serde(default)]
    pub schema: Option<String>,
    /// The model's one-line account of what it is proposing, shown above the
    /// preview. Not used to build the draft.
    #[serde(default)]
    pub summary: Option<String>,
    pub ops: Vec<ProposedOp>,
}

/// One requested change. Externally tagged, so the JSON reads
/// `{"add_column": {…}}` — the shape a model produces most reliably.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposedOp {
    AddColumn(NewColumn),
    /// Change an existing column. Every field is optional: what is absent is
    /// left as it is, which is the whole point of a patch.
    AlterColumn(ColumnPatch),
    DropColumn {
        name: String,
    },
    RenameColumn {
        from: String,
        to: String,
    },
    /// Replace the primary key with these columns, in key order. An empty list
    /// drops the key.
    SetPrimaryKey {
        columns: Vec<String>,
    },
    AddIndex(NewIndex),
    DropIndex {
        name: String,
    },
    AddForeignKey(NewForeignKey),
    DropForeignKey {
        name: String,
    },
    AddCheck(NewCheck),
    DropCheck {
        name: String,
    },
    RenameTable {
        to: String,
    },
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NewColumn {
    pub name: String,
    /// The full SQL type, parameters included — `varchar(255)`, `numeric(10,2)`.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Defaults to nullable, which is the only choice that is always legal to
    /// add to a table with rows in it.
    #[serde(default = "nullable_default")]
    pub nullable: bool,
    /// SQL text, ready to follow `DEFAULT ` — `'draft'`, `0`, `CURRENT_TIMESTAMP`.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub auto_increment: bool,
    /// A generated column's expression, without the `AS (…)` wrapper.
    #[serde(default)]
    pub generated: Option<String>,
}

fn nullable_default() -> bool {
    true
}

/// An edit to an existing column: `None` means "leave it alone".
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ColumnPatch {
    pub name: String,
    #[serde(rename = "type", default)]
    pub type_name: Option<String>,
    #[serde(default)]
    pub nullable: Option<bool>,
    /// Set the column's `DEFAULT`. To remove one, use `drop_default` — a JSON
    /// `null` here is indistinguishable from an absent field, so it cannot mean
    /// "no default" without also meaning "don't touch it".
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub drop_default: bool,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NewIndex {
    /// Omitted, the name is generated from the table and its key columns.
    #[serde(default)]
    pub name: Option<String>,
    pub columns: Vec<String>,
    #[serde(default)]
    pub unique: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NewForeignKey {
    #[serde(default)]
    pub name: Option<String>,
    pub columns: Vec<String>,
    #[serde(default)]
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    #[serde(default)]
    pub on_delete: Option<String>,
    #[serde(default)]
    pub on_update: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NewCheck {
    #[serde(default)]
    pub name: Option<String>,
    /// The predicate, without the wrapping `CHECK (…)`.
    pub expression: String,
}

/// Why a proposal could not be turned into a draft.
///
/// Every one of these is the model being wrong about the table, so the message
/// is written to be shown to the *user* — they are the one who has to decide
/// whether to ask again.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProposeError {
    #[error("the proposal is for table `{proposed}`, but `{actual}` is open")]
    WrongTable { proposed: String, actual: String },
    #[error("the proposal asks for no changes")]
    NoOps,
    #[error("`{0}` has no column named `{1}`")]
    NoSuchColumn(String, String),
    #[error("`{0}` already has a column named `{1}`")]
    ColumnExists(String, String),
    #[error("`{0}` has no index named `{1}`")]
    NoSuchIndex(String, String),
    #[error("`{0}` already has an index named `{1}`")]
    IndexExists(String, String),
    #[error("`{0}` has no foreign key named `{1}`")]
    NoSuchForeignKey(String, String),
    #[error("`{0}` has no check constraint named `{1}`")]
    NoSuchCheck(String, String),
    #[error("a foreign key needs as many referenced columns as referencing ones")]
    KeyWidthMismatch,
    #[error("an index needs at least one column")]
    EmptyIndex,
    #[error("the proposed table wouldn't be valid: {0}")]
    Invalid(String),
}

/// Lay `proposal` over `current` and return the draft it describes.
///
/// The draft is validated the same way the designer's is before it is returned,
/// so a proposal that names a column in an index and drops it in the next op
/// fails here rather than at the server.
pub fn apply(
    current: &TableInfo,
    proposal: &Proposal,
    dialect: SqlDialect,
) -> Result<TableDraft, ProposeError> {
    if !proposal.table.eq_ignore_ascii_case(&current.name) {
        return Err(ProposeError::WrongTable {
            proposed: proposal.table.clone(),
            actual: current.name.clone(),
        });
    }
    if proposal.ops.is_empty() {
        return Err(ProposeError::NoOps);
    }

    let mut draft = TableDraft::from_table(current);
    for op in &proposal.ops {
        apply_op(&mut draft, op)?;
    }

    let problems = draft.validate(dialect);
    if let Some(first) = problems.first() {
        return Err(ProposeError::Invalid(first.clone()));
    }
    Ok(draft)
}

/// The index of the column `name` addresses, matched case-insensitively because
/// a model routinely writes `Email` for a column the server spells `email`.
fn column_index(draft: &TableDraft, name: &str) -> Option<usize> {
    draft
        .columns
        .iter()
        .position(|c| c.info.name.eq_ignore_ascii_case(name))
}

fn apply_op(draft: &mut TableDraft, op: &ProposedOp) -> Result<(), ProposeError> {
    let table = draft.name.clone();
    let missing = |name: &str| ProposeError::NoSuchColumn(table.clone(), name.to_string());

    match op {
        ProposedOp::AddColumn(c) => {
            if column_index(draft, &c.name).is_some() {
                return Err(ProposeError::ColumnExists(table, c.name.clone()));
            }
            draft.columns.push(ColumnDraft::new(ColumnInfo {
                name: c.name.clone(),
                type_name: c.type_name.clone(),
                nullable: c.nullable,
                default: c.default.clone(),
                comment: c.comment.clone(),
                auto_increment: c.auto_increment,
                generated: c.generated.clone(),
                ..Default::default()
            }));
        }
        ProposedOp::AlterColumn(p) => {
            let idx = column_index(draft, &p.name).ok_or_else(|| missing(&p.name))?;
            let info = &mut draft.columns[idx].info;
            if let Some(ty) = &p.type_name {
                info.type_name = ty.clone();
            }
            if let Some(n) = p.nullable {
                info.nullable = n;
            }
            if p.drop_default {
                info.default = None;
            } else if let Some(d) = &p.default {
                info.default = Some(d.clone());
            }
            if let Some(c) = &p.comment {
                info.comment = Some(c.clone());
            }
        }
        ProposedOp::DropColumn { name } => {
            let idx = column_index(draft, name).ok_or_else(|| missing(name))?;
            draft.remove_column(idx);
        }
        ProposedOp::RenameColumn { from, to } => {
            let idx = column_index(draft, from).ok_or_else(|| missing(from))?;
            if let Some(clash) = column_index(draft, to)
                && clash != idx
            {
                return Err(ProposeError::ColumnExists(table, to.clone()));
            }
            // Through the draft's own rename, which carries every index, key and
            // foreign key that names the column along with it.
            draft.rename_column(idx, to);
        }
        ProposedOp::SetPrimaryKey { columns } => {
            // Spelled as the draft spells them: the key names *draft* columns, so
            // a key written in the server's casing has to land on the draft's.
            let mut key = Vec::with_capacity(columns.len());
            for name in columns {
                let idx = column_index(draft, name).ok_or_else(|| missing(name))?;
                key.push(draft.columns[idx].info.name.clone());
            }
            draft.primary_key = key;
        }
        ProposedOp::AddIndex(ix) => {
            if ix.columns.is_empty() {
                return Err(ProposeError::EmptyIndex);
            }
            let mut columns = Vec::with_capacity(ix.columns.len());
            for name in &ix.columns {
                let idx = column_index(draft, name).ok_or_else(|| missing(name))?;
                columns.push(draft.columns[idx].info.name.clone());
            }
            let name = match &ix.name {
                Some(n) => n.clone(),
                None => generated_index_name(draft, &columns),
            };
            if draft
                .indexes
                .iter()
                .any(|d| d.info.name.eq_ignore_ascii_case(&name))
            {
                return Err(ProposeError::IndexExists(table, name));
            }
            draft
                .indexes
                .push(IndexDraft::new(IndexInfo::plain(name, columns, ix.unique)));
        }
        ProposedOp::DropIndex { name } => {
            let at = draft
                .indexes
                .iter()
                .position(|d| d.info.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| ProposeError::NoSuchIndex(table, name.clone()))?;
            draft.indexes.remove(at);
        }
        ProposedOp::AddForeignKey(fk) => {
            if fk.columns.len() != fk.ref_columns.len() || fk.columns.is_empty() {
                return Err(ProposeError::KeyWidthMismatch);
            }
            let mut columns = Vec::with_capacity(fk.columns.len());
            for name in &fk.columns {
                let idx = column_index(draft, name).ok_or_else(|| missing(name))?;
                columns.push(draft.columns[idx].info.name.clone());
            }
            let name = match &fk.name {
                Some(n) => n.clone(),
                None => generated_fk_name(draft, &columns),
            };
            draft
                .foreign_keys
                .push(ForeignKeyDraft::new(ForeignKeyInfo {
                    name,
                    columns,
                    ref_schema: fk.ref_schema.clone(),
                    ref_table: fk.ref_table.clone(),
                    ref_columns: fk.ref_columns.clone(),
                    on_delete: fk.on_delete.clone(),
                    on_update: fk.on_update.clone(),
                }));
        }
        ProposedOp::DropForeignKey { name } => {
            let at = draft
                .foreign_keys
                .iter()
                .position(|d| d.info.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| ProposeError::NoSuchForeignKey(table, name.clone()))?;
            draft.foreign_keys.remove(at);
        }
        ProposedOp::AddCheck(ck) => {
            let name = match &ck.name {
                Some(n) => n.clone(),
                None => unique_name(&format!("chk_{}", draft.name), |candidate| {
                    draft
                        .check_constraints
                        .iter()
                        .any(|d| d.info.name.eq_ignore_ascii_case(candidate))
                }),
            };
            draft.check_constraints.push(CheckDraft::new(CheckInfo {
                name,
                expression: ck.expression.clone(),
                enforced: true,
                validated: true,
                inherited: false,
                column_level: false,
            }));
        }
        ProposedOp::DropCheck { name } => {
            let at = draft
                .check_constraints
                .iter()
                .position(|d| d.info.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| ProposeError::NoSuchCheck(table, name.clone()))?;
            draft.check_constraints.remove(at);
        }
        ProposedOp::RenameTable { to } => {
            draft.name = to.clone();
        }
    }
    Ok(())
}

/// `idx_<table>_<col>_<col>`, suffixed until it is free.
fn generated_index_name(draft: &TableDraft, columns: &[String]) -> String {
    let stem = format!("idx_{}_{}", draft.name, columns.join("_"));
    unique_name(&stem, |candidate| {
        draft
            .indexes
            .iter()
            .any(|d| d.info.name.eq_ignore_ascii_case(candidate))
    })
}

/// `fk_<table>_<col>_<col>`, suffixed until it is free.
fn generated_fk_name(draft: &TableDraft, columns: &[String]) -> String {
    let stem = format!("fk_{}_{}", draft.name, columns.join("_"));
    unique_name(&stem, |candidate| {
        draft
            .foreign_keys
            .iter()
            .any(|d| d.info.name.eq_ignore_ascii_case(candidate))
    })
}

/// `stem`, or `stem_2`, `stem_3`… — the first that `taken` doesn't claim.
fn unique_name(stem: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(stem) {
        return stem.to_string();
    }
    (2..)
        .map(|n| format!("{stem}_{n}"))
        .find(|c| !taken(c))
        .unwrap_or_else(|| stem.to_string())
}

/// The fenced-code tag a proposal arrives under.
///
/// Its own tag rather than plain `json`, because a model discussing a schema
/// prints example JSON all the time and none of it is a request to change the
/// table. An offer to edit the user's database has to be something the model
/// asked for on purpose.
pub const FENCE_TAG: &str = "schemaic-proposal";

/// Does this fenced block's language tag mark it as a proposal?
///
/// The renderer finds the blocks — it is already parsing the reply as markdown,
/// and a second scanner here would be a second answer to "is this a proposal"
/// that could drift from the first. This is the one place that knows the tag.
pub fn is_proposal_tag(lang: &str) -> bool {
    lang.trim().eq_ignore_ascii_case(FENCE_TAG)
}

/// Read a proposal block's body.
///
/// The error is the message shown to the user, so it says what serde says: a
/// model that invents a field is told which one, and the user can see that the
/// change wasn't quietly dropped.
pub fn parse(json: &str) -> Result<Proposal, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::IndexInfo;

    fn col(name: &str, ty: &str, nullable: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            nullable,
            ..Default::default()
        }
    }

    /// `orders(id PK, customer_id, placed_at)` with one index and one FK.
    fn orders() -> TableInfo {
        TableInfo {
            name: "orders".into(),
            columns: vec![
                ColumnInfo {
                    primary_key: true,
                    auto_increment: true,
                    ..col("id", "int(11)", false)
                },
                col("customer_id", "int(11)", false),
                col("placed_at", "datetime", true),
            ],
            indexes: vec![
                IndexInfo::plain("PRIMARY", vec!["id"], true),
                IndexInfo::plain("idx_customer", vec!["customer_id"], false),
            ],
            foreign_keys: vec![ForeignKeyInfo {
                name: "fk_orders_customer".into(),
                columns: vec!["customer_id".into()],
                ref_table: "customers".into(),
                ref_columns: vec!["id".into()],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn proposal(ops: Vec<ProposedOp>) -> Proposal {
        Proposal {
            table: "orders".into(),
            ops,
            ..Default::default()
        }
    }

    fn applied(ops: Vec<ProposedOp>) -> TableDraft {
        apply(&orders(), &proposal(ops), SqlDialect::MySql).expect("proposal applies")
    }

    #[test]
    fn untouched_table_produces_no_changes() {
        // The floor the whole design rests on: a patch that names nothing must
        // diff clean, or every proposal carries collateral changes.
        let draft = applied(vec![ProposedOp::AddColumn(NewColumn {
            name: "deleted_at".into(),
            type_name: "datetime".into(),
            nullable: true,
            ..Default::default()
        })]);
        let changes = crate::ddl::diff(&orders(), &draft, SqlDialect::MySql);
        assert_eq!(
            changes.len(),
            1,
            "only the added column: {:?}",
            changes.changes
        );
    }

    #[test]
    fn adds_a_column_and_an_index_over_it() {
        let draft = applied(vec![
            ProposedOp::AddColumn(NewColumn {
                name: "deleted_at".into(),
                type_name: "datetime".into(),
                nullable: true,
                ..Default::default()
            }),
            ProposedOp::AddIndex(NewIndex {
                name: None,
                columns: vec!["deleted_at".into()],
                unique: false,
            }),
        ]);
        assert_eq!(draft.columns.len(), 4);
        let ix = draft.indexes.last().expect("index added");
        assert_eq!(ix.info.name, "idx_orders_deleted_at");
        assert_eq!(ix.info.column_names().collect::<Vec<_>>(), ["deleted_at"]);
        assert!(ix.original.is_none(), "a new index, not an edited one");
    }

    #[test]
    fn a_new_column_defaults_to_nullable() {
        // Deserialized, not constructed — the default only exists in serde.
        let p: Proposal = serde_json::from_str(
            r#"{"table":"orders","ops":[{"add_column":{"name":"note","type":"text"}}]}"#,
        )
        .expect("parses");
        let draft = apply(&orders(), &p, SqlDialect::MySql).expect("applies");
        let added = draft.columns.last().expect("column added");
        assert!(
            added.info.nullable,
            "adding a NOT NULL column to a table with rows fails"
        );
    }

    #[test]
    fn generated_index_name_steps_around_a_collision() {
        let draft = applied(vec![ProposedOp::AddIndex(NewIndex {
            name: None,
            columns: vec!["customer_id".into()],
            unique: false,
        })]);
        // `idx_orders_customer_id` is free; the point is it doesn't reuse
        // `idx_customer`, which already stands on that column.
        assert_eq!(
            draft.indexes.last().unwrap().info.name,
            "idx_orders_customer_id"
        );
    }

    #[test]
    fn renaming_a_column_carries_its_index_and_key() {
        let draft = applied(vec![ProposedOp::RenameColumn {
            from: "customer_id".into(),
            to: "buyer_id".into(),
        }]);
        let ix = draft
            .indexes
            .iter()
            .find(|d| d.info.name == "idx_customer")
            .expect("index kept");
        assert_eq!(
            ix.info.column_names().collect::<Vec<_>>(),
            ["buyer_id"],
            "the index must follow the rename, or it diffs as a rebuild"
        );
        assert_eq!(
            draft.foreign_keys[0].info.columns,
            vec!["buyer_id".to_string()]
        );
    }

    #[test]
    fn column_names_match_case_insensitively() {
        let draft = applied(vec![ProposedOp::AlterColumn(ColumnPatch {
            name: "Placed_At".into(),
            nullable: Some(false),
            ..Default::default()
        })]);
        let c = draft
            .columns
            .iter()
            .find(|c| c.info.name == "placed_at")
            .unwrap();
        assert!(!c.info.nullable);
        assert_eq!(c.info.name, "placed_at", "the server's spelling is kept");
    }

    #[test]
    fn a_patch_leaves_unmentioned_fields_alone() {
        let draft = applied(vec![ProposedOp::AlterColumn(ColumnPatch {
            name: "placed_at".into(),
            default: Some("CURRENT_TIMESTAMP".into()),
            ..Default::default()
        })]);
        let c = draft
            .columns
            .iter()
            .find(|c| c.info.name == "placed_at")
            .unwrap();
        assert_eq!(c.info.default.as_deref(), Some("CURRENT_TIMESTAMP"));
        assert_eq!(c.info.type_name, "datetime", "type untouched");
        assert!(c.info.nullable, "nullability untouched");
    }

    #[test]
    fn drop_default_removes_it() {
        let mut table = orders();
        table.columns[2].default = Some("CURRENT_TIMESTAMP".into());
        let p = proposal(vec![ProposedOp::AlterColumn(ColumnPatch {
            name: "placed_at".into(),
            drop_default: true,
            ..Default::default()
        })]);
        let draft = apply(&table, &p, SqlDialect::MySql).expect("applies");
        assert!(draft.columns[2].info.default.is_none());
    }

    #[test]
    fn dropping_a_column_is_expressible_but_never_accidental() {
        // The op exists — the model can ask. What it can't do is *omit* a column
        // and have that read as a drop.
        let draft = applied(vec![ProposedOp::DropColumn {
            name: "placed_at".into(),
        }]);
        assert_eq!(draft.columns.len(), 2);
        let changes = crate::ddl::diff(&orders(), &draft, SqlDialect::MySql);
        assert!(
            !changes.destructive().is_empty(),
            "the preview must name the risk"
        );
    }

    #[test]
    fn set_primary_key_replaces_the_key() {
        let draft = applied(vec![ProposedOp::SetPrimaryKey {
            columns: vec!["id".into(), "customer_id".into()],
        }]);
        assert_eq!(
            draft.primary_key,
            vec!["id".to_string(), "customer_id".to_string()]
        );
    }

    #[test]
    fn adds_a_foreign_key_with_a_generated_name() {
        let draft = applied(vec![ProposedOp::AddForeignKey(NewForeignKey {
            columns: vec!["placed_at".into()],
            ref_table: "calendar".into(),
            ref_columns: vec!["day".into()],
            ..Default::default()
        })]);
        let fk = draft.foreign_keys.last().unwrap();
        assert_eq!(fk.info.name, "fk_orders_placed_at");
        assert_eq!(fk.info.ref_table, "calendar");
    }

    #[test]
    fn drops_index_foreign_key_and_check_by_name() {
        let draft = applied(vec![
            ProposedOp::DropIndex {
                name: "idx_customer".into(),
            },
            ProposedOp::DropForeignKey {
                name: "FK_ORDERS_CUSTOMER".into(),
            },
        ]);
        assert!(draft.indexes.iter().all(|d| d.info.name != "idx_customer"));
        assert!(draft.foreign_keys.is_empty(), "dropped case-insensitively");
    }

    #[test]
    fn a_check_gets_a_name_when_the_model_omits_one() {
        let draft = applied(vec![ProposedOp::AddCheck(NewCheck {
            name: None,
            expression: "`customer_id` > 0".into(),
        })]);
        assert_eq!(
            draft.check_constraints.last().unwrap().info.name,
            "chk_orders"
        );
    }

    #[test]
    fn wrong_table_is_refused() {
        let p = Proposal {
            table: "customers".into(),
            ops: vec![ProposedOp::DropColumn { name: "id".into() }],
            ..Default::default()
        };
        assert_eq!(
            apply(&orders(), &p, SqlDialect::MySql),
            Err(ProposeError::WrongTable {
                proposed: "customers".into(),
                actual: "orders".into(),
            })
        );
    }

    #[test]
    fn an_empty_proposal_is_refused() {
        assert_eq!(
            apply(&orders(), &proposal(vec![]), SqlDialect::MySql),
            Err(ProposeError::NoOps)
        );
    }

    #[test]
    fn a_column_that_isnt_there_is_refused() {
        let p = proposal(vec![ProposedOp::AlterColumn(ColumnPatch {
            name: "shipped_at".into(),
            nullable: Some(true),
            ..Default::default()
        })]);
        assert_eq!(
            apply(&orders(), &p, SqlDialect::MySql),
            Err(ProposeError::NoSuchColumn(
                "orders".into(),
                "shipped_at".into()
            ))
        );
    }

    #[test]
    fn adding_a_column_that_exists_is_refused() {
        let p = proposal(vec![ProposedOp::AddColumn(NewColumn {
            name: "ID".into(),
            type_name: "bigint".into(),
            ..Default::default()
        })]);
        assert!(matches!(
            apply(&orders(), &p, SqlDialect::MySql),
            Err(ProposeError::ColumnExists(_, _))
        ));
    }

    #[test]
    fn a_lopsided_foreign_key_is_refused() {
        let p = proposal(vec![ProposedOp::AddForeignKey(NewForeignKey {
            columns: vec!["customer_id".into()],
            ref_table: "customers".into(),
            ref_columns: vec!["id".into(), "tenant".into()],
            ..Default::default()
        })]);
        assert_eq!(
            apply(&orders(), &p, SqlDialect::MySql),
            Err(ProposeError::KeyWidthMismatch)
        );
    }

    #[test]
    fn dropping_a_column_takes_its_index_and_key_with_it() {
        // Ops compose through the draft's own mutators, so a drop cascades
        // exactly as it does in the designer — the proposal can't leave an
        // index standing on a column that is going away.
        let draft = applied(vec![ProposedOp::DropColumn {
            name: "customer_id".into(),
        }]);
        assert!(
            draft.indexes.iter().all(|d| d.info.name != "idx_customer"),
            "the index over the dropped column goes too"
        );
        assert!(draft.foreign_keys.is_empty(), "and so does the foreign key");
    }

    #[test]
    fn a_draft_that_wouldnt_validate_is_refused() {
        // The designer's own validation stands between the model and the
        // preview, so nonsense fails here rather than at the server.
        let p = proposal(vec![ProposedOp::RenameTable { to: "  ".into() }]);
        assert!(matches!(
            apply(&orders(), &p, SqlDialect::MySql),
            Err(ProposeError::Invalid(_))
        ));
    }

    #[test]
    fn unknown_json_fields_are_refused() {
        // A model inventing `"charset"` must fail loudly, not have it dropped:
        // a silently-ignored field is a change the user was told about and
        // didn't get.
        let err = serde_json::from_str::<Proposal>(
            r#"{"table":"orders","ops":[{"add_column":{"name":"a","type":"int","charset":"utf8"}}]}"#,
        );
        assert!(err.is_err(), "unknown field must not be ignored");
    }

    #[test]
    fn the_documented_json_shape_parses() {
        let p: Proposal = serde_json::from_str(
            r#"{
                "table": "orders",
                "summary": "soft-delete support",
                "ops": [
                    {"add_column": {"name": "deleted_at", "type": "datetime"}},
                    {"add_index": {"columns": ["deleted_at"]}}
                ]
            }"#,
        )
        .expect("the shape the tool advertises must parse");
        assert_eq!(p.ops.len(), 2);
        assert_eq!(p.summary.as_deref(), Some("soft-delete support"));
        apply(&orders(), &p, SqlDialect::MySql).expect("and apply");
    }

    #[test]
    fn only_the_proposal_tag_marks_a_block() {
        assert!(is_proposal_tag(FENCE_TAG));
        assert!(is_proposal_tag("  Schemaic-Proposal "));
        // The model prints example JSON constantly, and none of it is consent to
        // edit the user's table.
        assert!(!is_proposal_tag("json"));
        assert!(!is_proposal_tag(""));
        assert!(!is_proposal_tag("sql"));
    }

    #[test]
    fn a_block_body_parses_into_a_proposal() {
        let p = parse(r#"{"table":"orders","ops":[{"drop_index":{"name":"idx_customer"}}]}"#)
            .expect("parses");
        assert_eq!(p.table, "orders");
        assert!(matches!(&p.ops[0], ProposedOp::DropIndex { name } if name == "idx_customer"));
    }

    #[test]
    fn a_malformed_body_reports_why() {
        // Shown to the user, so it has to say something — silently dropping the
        // block would look exactly like the model ignoring them.
        let err = parse(r#"{"table": "orders","#).expect_err("refused");
        assert!(!err.is_empty());
    }
}
