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
    #[error("`{0}` is a view — a proposal can only change a table")]
    NotATable(String),
    #[error("no table called `{0}` — call list_schema to see what exists")]
    NoSuchTable(String),
    #[error("the {what} `{text}` isn't a single SQL expression")]
    NotAnExpression { what: String, text: String },
    /// A `type` field that is not a bare column type.
    ///
    /// Separate from [`ProposeError::NotAnExpression`] because a type is not an
    /// expression and the two are checked by different gates — and because the
    /// message a model gets back has to name what it actually did wrong.
    #[error(
        "`{text}` isn't a column type — give the type alone, with no default, no NOT NULL, and nothing after it"
    )]
    NotAType { text: String },
    #[error("`{0}` isn't a foreign-key action")]
    UnknownAction(String),
}

/// The referential actions a foreign key may state, as both engines spell them.
///
/// A closed vocabulary, because [`crate::ddl`] writes the string straight after
/// `ON DELETE ` — so an author who could put anything there could close the
/// constraint and open another `alter_option`.
const FK_ACTIONS: [&str; 5] = [
    "CASCADE",
    "SET NULL",
    "SET DEFAULT",
    "RESTRICT",
    "NO ACTION",
];

/// Every string this op contributes to a statement **verbatim**, checked before
/// a single op is applied.
///
/// `DEFAULT <text>`, `GENERATED ALWAYS AS (<text>)` and `CHECK (<text>)` splice
/// the author's SQL in unquoted, and that is deliberate: a default has to be
/// able to say `CURRENT_TIMESTAMP` or `nextval('s')`, and quoting one would
/// break every real one. **What is new here is the author.** The designer's
/// fields are typed by the user who then consents to the preview; a proposal's
/// are written by a remote model that has itself been reading database content
/// somebody else may control, so `'x', DROP COLUMN placed_at` is a shape this
/// path has to refuse rather than merely describe.
///
/// The check is structural — [`crate::intel::is_single_expression`] and
/// [`crate::intel::is_column_type`], the project's AST authority — not a search
/// for suspicious characters, so a legitimate default with a comma inside a call
/// (`concat('a', 'b')`) passes and a trailing clause does not.
///
/// **`type` is on this list, and for a long time it wasn't.** A column type is
/// spliced by [`crate::schema::ColumnInfo::definition_sql`] exactly as verbatim
/// as a default is — twice, on PostgreSQL — and it is the field a live
/// proposal used to drop a column under a change list that read only
/// *Rename column qty to qty2*. Every field this function does not name is a
/// hole of the same shape, so a new one on `NewColumn` or `ColumnPatch` belongs
/// here before it belongs in the emitter.
fn check_free_sql(op: &ProposedOp, dialect: SqlDialect) -> Result<(), ProposeError> {
    let expr = |what: &str, text: &Option<String>| -> Result<(), ProposeError> {
        match text {
            Some(t) if !crate::intel::is_single_expression(t, dialect) => {
                Err(ProposeError::NotAnExpression {
                    what: what.to_string(),
                    text: t.clone(),
                })
            }
            _ => Ok(()),
        }
    };
    let column_type = |text: &str| -> Result<(), ProposeError> {
        if crate::intel::is_column_type(text, dialect) {
            Ok(())
        } else {
            Err(ProposeError::NotAType {
                text: text.to_string(),
            })
        }
    };
    let action = |a: &Option<String>| -> Result<(), ProposeError> {
        match a {
            Some(a) if !FK_ACTIONS.iter().any(|k| k.eq_ignore_ascii_case(a.trim())) => {
                Err(ProposeError::UnknownAction(a.clone()))
            }
            _ => Ok(()),
        }
    };
    match op {
        ProposedOp::AddColumn(c) => {
            column_type(&c.type_name)?;
            expr("default", &c.default)?;
            expr("generated expression", &c.generated)?;
        }
        ProposedOp::AlterColumn(p) => {
            if let Some(t) = &p.type_name {
                column_type(t)?;
            }
            expr("default", &p.default)?;
        }
        ProposedOp::AddCheck(ck) => {
            expr("check expression", &Some(ck.expression.clone()))?;
        }
        ProposedOp::AddForeignKey(fk) => {
            action(&fk.on_delete)?;
            action(&fk.on_update)?;
        }
        _ => {}
    }
    Ok(())
}

/// The table a proposal is about, resolved out of an introspected schema.
///
/// **One resolver, because the two ends of a proposal have to agree.** The tool
/// checks the ops against a table and reports "Valid. Nothing has run."; the
/// card then builds the plan the user consents to. They used to read the same
/// JSON by different rules — the tool ignored `proposal.schema` and the card
/// required it to match exactly — so on a database with `public.orders` *and*
/// `sales.orders` the model was told its change was valid for one table and the
/// user was offered it against the other, and the qualified `sales.orders` form
/// the tool's own description asks for dead-ended at the card.
///
/// Every form the listings can print resolves, in the order a caller means them:
/// an explicit `schema` field wins, then a qualifier written into `table`, then
/// the bare name — which [`DbSchema::find_table`] already resolves preferring
/// `public`, so the ordinary PostgreSQL case needs no separate default.
///
/// [`DbSchema::find_table`]: crate::schema::DbSchema::find_table
///
/// **A view is refused**, and that is the other half of this being one function.
/// `DbSchema::tables` holds views too, and every earlier caller of these lookups
/// arrived from a tree row whose kind the user had already seen. A proposal's
/// table name comes from an untrusted party, and a view laid under
/// `TableDraft::from_table` emits `ALTER TABLE` — which PostgreSQL *accepts* for
/// a rename, under a modal that says "Rename the table".
pub fn resolve_target<'a>(
    schema: &'a crate::schema::DbSchema,
    proposal: &Proposal,
) -> Result<&'a TableInfo, ProposeError> {
    let found = match &proposal.schema {
        Some(ns) => schema.find_table(Some(ns), &proposal.table),
        None => proposal
            .table
            .split_once('.')
            .and_then(|(ns, name)| schema.find_table(Some(ns), name))
            .or_else(|| schema.find_table(None, &proposal.table)),
    };
    match found {
        Some(t) if t.is_view => Err(ProposeError::NotATable(display_target(proposal))),
        Some(t) => Ok(t),
        None => Err(ProposeError::NoSuchTable(display_target(proposal))),
    }
}

/// The table as the proposal named it, for an error the user reads.
fn display_target(proposal: &Proposal) -> String {
    crate::schema::display_name(proposal.schema.as_deref(), &proposal.table)
}

/// Does `proposal` name the table `current` is, decomposed the way
/// [`resolve_target`] decomposes it?
///
/// **[`apply`]'s guard has to agree with the resolver, or a form that resolves
/// dead-ends one line later.** Both callers resolve and then hand the resolved
/// `TableInfo` straight to `apply`, so a guard that compared the *raw*
/// `proposal.table` to the *bare* `current.name` refused every qualified
/// `sales.orders` — the form `propose_table_change`'s own description asks the
/// model to write, on exactly the databases (more than one namespace) where the
/// listing prints it that way.
///
/// The guard is not dropped, because it still protects the case it was written
/// for: a caller that opened a table itself and never resolved anything. So the
/// namespace is enforced **where the resolver enforces it** and nowhere else —
/// an explicit `schema` field is an exact lookup there, while a qualifier
/// written into `table` is a first attempt the resolver falls back from, and on
/// MySQL `mydb.orders` legitimately lands on the unqualified `orders`.
fn names_the_same_table(proposal: &Proposal, current: &TableInfo) -> bool {
    let (namespace, bare) = match &proposal.schema {
        Some(ns) => (Some(ns.as_str()), proposal.table.as_str()),
        None => match proposal.table.split_once('.') {
            // A qualifier here is a hint, not a claim — see above.
            Some((_, name)) => (None, name),
            None => (None, proposal.table.as_str()),
        },
    };
    if !bare.eq_ignore_ascii_case(&current.name) {
        return false;
    }
    match namespace {
        Some(ns) => current
            .schema
            .as_deref()
            .is_some_and(|cur| ns.eq_ignore_ascii_case(cur)),
        None => true,
    }
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
    if !names_the_same_table(proposal, current) {
        return Err(ProposeError::WrongTable {
            proposed: display_target(proposal),
            actual: crate::schema::display_name(current.schema.as_deref(), &current.name),
        });
    }
    if proposal.ops.is_empty() {
        return Err(ProposeError::NoOps);
    }

    // Before anything is applied: every string this proposal would put into a
    // statement verbatim has to be the *shape* it claims to be. See
    // [`check_free_sql`] for why this path checks and the designer's doesn't.
    for op in &proposal.ops {
        check_free_sql(op, dialect)?;
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

    /// **One resolver, or the tool validates one table and the card targets
    /// another.** Every form the listings can print has to land on the same
    /// table from both ends: an explicit `schema`, a qualified `sales.orders`
    /// (which is what the tool's own description asks the model to write), and a
    /// bare name, which resolves preferring `public`.
    #[test]
    fn a_proposal_resolves_the_same_table_from_either_end() {
        let in_ns = |ns: &str| TableInfo {
            schema: Some(ns.into()),
            ..orders()
        };
        let schema = crate::schema::DbSchema {
            tables: vec![in_ns("public"), in_ns("sales")],
            ..Default::default()
        };
        let target = |table: &str, ns: Option<&str>| {
            resolve_target(
                &schema,
                &Proposal {
                    table: table.into(),
                    schema: ns.map(str::to_string),
                    ..Default::default()
                },
            )
            .map(|t| t.schema.clone().unwrap_or_default())
        };
        assert_eq!(target("orders", Some("sales")).unwrap(), "sales");
        assert_eq!(target("sales.orders", None).unwrap(), "sales");
        assert_eq!(target("orders", None).unwrap(), "public");
        assert!(matches!(
            target("nope", None),
            Err(ProposeError::NoSuchTable(_))
        ));
    }

    /// **Resolving is not enough: the table has to survive `apply` too.** Both
    /// real callers (`app/mcp.rs`'s `propose_table_change` and
    /// `ui/ddl_preview.rs`'s card) do `resolve_target` and then hand the very
    /// same `TableInfo` to `apply`, so the property "every form the listings can
    /// print lands on the same table" is a property of the *composition*. The
    /// resolver test above stops one line short of it, which is how the
    /// qualified `sales.orders` form — the one the tool's own description asks
    /// the model to write — went on dead-ending at ``the proposal is for table
    /// `sales.orders`, but `orders` is open`` after the resolver had already
    /// accepted it.
    #[test]
    fn every_naming_form_that_resolves_also_applies() {
        let in_ns = |ns: &str| TableInfo {
            schema: Some(ns.into()),
            ..orders()
        };
        let schema = crate::schema::DbSchema {
            tables: vec![in_ns("public"), in_ns("sales")],
            ..Default::default()
        };
        let round_trip = |table: &str, ns: Option<&str>| {
            let p = Proposal {
                table: table.into(),
                schema: ns.map(str::to_string),
                ops: vec![ProposedOp::DropColumn {
                    name: "placed_at".into(),
                }],
                ..Default::default()
            };
            let info = resolve_target(&schema, &p)?;
            let found = info.schema.clone().unwrap_or_default();
            apply(info, &p, SqlDialect::Postgres).map(|_| found)
        };
        assert_eq!(round_trip("orders", Some("sales")).unwrap(), "sales");
        assert_eq!(round_trip("sales.orders", None).unwrap(), "sales");
        assert_eq!(round_trip("orders", None).unwrap(), "public");
    }

    /// The over-reach side: the guard still exists, and it still refuses a
    /// `TableInfo` a caller opened without resolving. A namespace stated in the
    /// **`schema` field** is enforced, because that is the one `resolve_target`
    /// enforces exactly; a qualifier written into `table` is not, because
    /// `resolve_target` falls back from it — on MySQL `mydb.orders` resolves to
    /// the unqualified `orders`, and refusing it here would be the same
    /// dead-end by another route.
    #[test]
    fn a_qualifier_does_not_let_a_proposal_reach_another_table() {
        let ops = || {
            vec![ProposedOp::DropColumn {
                name: "placed_at".into(),
            }]
        };
        let public_orders = TableInfo {
            schema: Some("public".into()),
            ..orders()
        };
        // Wrong bare name, qualified — still the wrong table.
        assert!(matches!(
            apply(
                &public_orders,
                &Proposal {
                    table: "sales.customers".into(),
                    ops: ops(),
                    ..Default::default()
                },
                SqlDialect::Postgres,
            ),
            Err(ProposeError::WrongTable { .. })
        ));
        // Right name, but an explicit `schema` the open table doesn't carry.
        assert!(matches!(
            apply(
                &public_orders,
                &Proposal {
                    table: "orders".into(),
                    schema: Some("sales".into()),
                    ops: ops(),
                    ..Default::default()
                },
                SqlDialect::Postgres,
            ),
            Err(ProposeError::WrongTable { .. })
        ));
        // …and on MySQL, where the model may still write `db.table`, the
        // qualifier is a hint the resolver drops, so `apply` drops it too.
        assert!(
            apply(
                &orders(),
                &Proposal {
                    table: "mydb.orders".into(),
                    ops: ops(),
                    ..Default::default()
                },
                SqlDialect::MySql,
            )
            .is_ok()
        );
    }

    /// **A view is not a table, and this is the one caller whose name comes from
    /// an untrusted party.** `DbSchema::tables` holds views too; laid under
    /// `TableDraft::from_table` a view emits `ALTER TABLE` — which PostgreSQL
    /// *accepts* for a rename, under a modal reading "Rename the table".
    #[test]
    fn a_view_is_refused_before_any_op_is_applied() {
        let schema = crate::schema::DbSchema {
            tables: vec![TableInfo {
                name: "v".into(),
                is_view: true,
                ..orders()
            }],
            ..Default::default()
        };
        assert!(matches!(
            resolve_target(
                &schema,
                &Proposal {
                    table: "v".into(),
                    ..Default::default()
                }
            ),
            Err(ProposeError::NotATable(_))
        ));
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

    /// **A default is free SQL, and this author is not the user.**
    ///
    /// `DEFAULT <text>` splices the string in unquoted — which is right, and has
    /// to stay right — so `'x', DROP COLUMN placed_at` was a legal MySQL
    /// `alter_option` list the moment it landed in the statement, while the
    /// change list said `Add column note text` and the risk block said nothing.
    /// The shape is refused here, before a single op is applied.
    #[test]
    fn a_default_that_closes_the_clause_is_refused() {
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            let smuggle = |op| {
                assert!(
                    matches!(
                        apply(&orders(), &proposal(vec![op]), dialect),
                        Err(ProposeError::NotAnExpression { .. })
                    ),
                    "{dialect:?}"
                );
            };
            smuggle(ProposedOp::AddColumn(NewColumn {
                name: "note".into(),
                type_name: "text".into(),
                nullable: true,
                default: Some("'x', DROP COLUMN placed_at".into()),
                ..Default::default()
            }));
            smuggle(ProposedOp::AddColumn(NewColumn {
                name: "n".into(),
                type_name: "int".into(),
                nullable: true,
                generated: Some("1) , DROP COLUMN placed_at, ADD COLUMN z int".into()),
                ..Default::default()
            }));
            smuggle(ProposedOp::AlterColumn(ColumnPatch {
                name: "placed_at".into(),
                default: Some("now(); DROP TABLE customers; --".into()),
                ..Default::default()
            }));
            smuggle(ProposedOp::AddCheck(NewCheck {
                name: None,
                expression: "id > 0) , DROP COLUMN placed_at, ADD CHECK (1=1".into(),
            }));
        }
    }

    /// **A column's `type` is free SQL too, and nothing checked it.**
    ///
    /// Reproduced live on MariaDB 10.11.14: this exact two-op proposal was
    /// applied under a change list reading only `Rename column qty to qty2`,
    /// with an empty destructive block and a `Primary`-coloured Apply, and
    /// `orders` afterwards was `id`, `qty2`, `pad` — `placed_at` gone. The
    /// `/*M!…*/` wrapper is what makes the payload balance; see
    /// [`crate::intel::is_single_expression`] for why the client reads a comment
    /// where MariaDB reads code.
    #[test]
    fn a_type_that_closes_the_clause_is_refused() {
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            let smuggle = |op| {
                assert!(
                    matches!(
                        apply(&orders(), &proposal(vec![op]), dialect),
                        Err(ProposeError::NotAType { .. })
                    ),
                    "{dialect:?}"
                );
            };
            // The payload that was executed, verbatim.
            smuggle(ProposedOp::AlterColumn(ColumnPatch {
                name: "qty".into(),
                type_name: Some(
                    "int(11 /*M!100000 ), DROP COLUMN placed_at, ADD COLUMN pad int(1 */)".into(),
                ),
                ..Default::default()
            }));
            // …and the plainer shapes the same field admits.
            smuggle(ProposedOp::AlterColumn(ColumnPatch {
                name: "qty".into(),
                type_name: Some("int, DROP COLUMN placed_at".into()),
                ..Default::default()
            }));
            smuggle(ProposedOp::AddColumn(NewColumn {
                name: "note".into(),
                type_name: "text) , DROP COLUMN placed_at, ADD COLUMN z int".into(),
                nullable: true,
                ..Default::default()
            }));
            // A type is not a place to put a column constraint either: the field
            // is spliced where only a type belongs.
            smuggle(ProposedOp::AddColumn(NewColumn {
                name: "note".into(),
                type_name: "int DEFAULT 0".into(),
                nullable: true,
                ..Default::default()
            }));
            smuggle(ProposedOp::AddColumn(NewColumn {
                name: "note".into(),
                type_name: String::new(),
                nullable: true,
                ..Default::default()
            }));
        }
    }

    /// **MariaDB's executable comment is the one form where the client sees a
    /// comment and the server sees code.** sqlparser 0.62's `MySqlDialect`
    /// special-cases `/*!` and not `/*M!`, so `Parser::peek_token` skipped the
    /// whole run as whitespace and `is_single_expression` said yes.
    ///
    /// Live: the `default` below dropped `placed_at` on MariaDB 10.11.14 and was
    /// inert on MySQL 8.4.11. MariaDB expands `/*M!nnnnnn …*/` whenever its
    /// version is at least `nnnnnn`, so `/*M!010000` fires on every MariaDB the
    /// app supports.
    #[test]
    fn an_executable_comment_in_a_models_free_sql_is_refused() {
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            let smuggle = |op| {
                assert!(
                    matches!(
                        apply(&orders(), &proposal(vec![op]), dialect),
                        Err(ProposeError::NotAnExpression { .. })
                    ),
                    "{dialect:?}"
                );
            };
            smuggle(ProposedOp::AddColumn(NewColumn {
                name: "note".into(),
                type_name: "int".into(),
                nullable: true,
                default: Some("1 /*M!100000 , DROP COLUMN placed_at */".into()),
                ..Default::default()
            }));
            smuggle(ProposedOp::AddColumn(NewColumn {
                name: "note".into(),
                type_name: "int".into(),
                nullable: true,
                default: Some("1 /*!50000 , DROP COLUMN placed_at */".into()),
                ..Default::default()
            }));
            // The `CHECK` arm the same wrapper reaches.
            smuggle(ProposedOp::AddCheck(NewCheck {
                name: None,
                expression: "1 /*M!100000 ) , DROP COLUMN placed_at, ADD CHECK (1 */".into(),
            }));
            // A block comment in a *type* is refused by the type gate.
            assert!(
                matches!(
                    apply(
                        &orders(),
                        &proposal(vec![ProposedOp::AlterColumn(ColumnPatch {
                            name: "qty".into(),
                            type_name: Some("int /*M!100000 , DROP COLUMN placed_at */".into()),
                            ..Default::default()
                        })]),
                        dialect
                    ),
                    Err(ProposeError::NotAType { .. })
                ),
                "{dialect:?}"
            );
        }
    }

    /// …and the happy path of the **type** gate. An over-strict one refuses
    /// `enum('a','b')`, `numeric(10,2)` and every PostgreSQL array, which is a
    /// worse failure than the hole: it breaks proposals that were never
    /// hostile, silently, on engines nobody was testing.
    #[test]
    fn an_ordinary_type_is_still_accepted() {
        let shared = [
            "int",
            "text",
            "varchar(255)",
            "numeric(10,2)",
            "numeric(10, 2)",
            "char(1)",
            "blob",
            "double precision",
            "timestamp",
            "  int  ",
        ];
        let per_dialect: [(SqlDialect, &[&str]); 3] = [
            (
                SqlDialect::MySql,
                &[
                    "int(11)",
                    "enum('a','b')",
                    "set('a','b')",
                    "tinyint(1)",
                    "datetime(6)",
                    "longtext",
                    "decimal(10, 2) unsigned",
                    "bit(3)",
                ],
            ),
            (
                SqlDialect::Postgres,
                &[
                    "text[]",
                    "varchar(255)[]",
                    "numeric(10,2)[]",
                    "jsonb",
                    "uuid",
                    "interval",
                    "timestamp with time zone",
                    "time without time zone",
                    "character varying(10)",
                    "citext",
                ],
            ),
            (
                SqlDialect::Sqlite,
                &["integer", "real", "blob", "varchar(20)", "numeric"],
            ),
        ];
        for (dialect, extra) in per_dialect {
            for t in shared.iter().chain(extra.iter()) {
                let p = proposal(vec![ProposedOp::AddColumn(NewColumn {
                    name: "note".into(),
                    type_name: (*t).into(),
                    nullable: true,
                    ..Default::default()
                })]);
                assert!(
                    apply(&orders(), &p, dialect).is_ok(),
                    "refused a legitimate {dialect:?} type: {t}"
                );
            }
        }
    }

    /// …and the happy path is untouched. A default is genuinely arbitrary SQL,
    /// so a guard that refused these would be worse than the hole.
    ///
    /// **All three dialects**, because the guard is dialect-parameterised and
    /// pinning its accept side on PostgreSQL alone is what let the MySQL-family
    /// bypass through.
    #[test]
    fn an_ordinary_default_is_still_accepted() {
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            for text in [
                "CURRENT_TIMESTAMP",
                "0",
                "'draft'",
                "nextval('s')",
                "concat('a', 'b')",
                "-1",
                "(1 + 2)",
                "NULL",
                "true",
            ] {
                let p = proposal(vec![ProposedOp::AddColumn(NewColumn {
                    name: "note".into(),
                    type_name: "text".into(),
                    nullable: true,
                    default: Some(text.into()),
                    ..Default::default()
                })]);
                assert!(
                    apply(&orders(), &p, dialect).is_ok(),
                    "{dialect:?} refused a legitimate default: {text}"
                );
            }
        }
    }

    /// A referential action is a closed vocabulary, and [`crate::ddl`] writes it
    /// straight after `ON DELETE ` — so anything else could close the constraint
    /// and open another `alter_option`.
    #[test]
    fn a_foreign_key_action_outside_the_vocabulary_is_refused() {
        let fk = |action: &str| {
            proposal(vec![ProposedOp::AddForeignKey(NewForeignKey {
                name: Some("fk_c".into()),
                columns: vec!["customer_id".into()],
                ref_table: "customers".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some(action.into()),
                ..Default::default()
            })])
        };
        assert!(matches!(
            apply(
                &orders(),
                &fk("CASCADE, DROP COLUMN placed_at"),
                SqlDialect::MySql
            ),
            Err(ProposeError::UnknownAction(_))
        ));
        // The real ones, in either casing.
        assert!(apply(&orders(), &fk("cascade"), SqlDialect::MySql).is_ok());
        assert!(apply(&orders(), &fk("SET NULL"), SqlDialect::MySql).is_ok());
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
