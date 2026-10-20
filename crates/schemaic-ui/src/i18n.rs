//! The interface's string catalogue: Fluent, through `i18n-embed`.
//!
//! **A label is spelled `tr!("message-id")`**, or `tr!("id", name = value)` with
//! arguments. It is `fl!` against [`LOADER`]: the id and its argument names are
//! checked against the `en-GB` catalogue at compile time, so a missing key is a
//! build error rather than a blank label. The catalogues live in
//! `crates/schemaic-ui/i18n/<language>/schemaic.ftl`, embedded in the binary.
//!
//! **The language is chosen once, at launch** ([`init`]), from
//! `schemaic_core::locale::resolve`, and never changes while the app runs: a
//! label is built with its text, and nothing re-renders when the setting moves.
//! Until `init` runs — and in every test — the loader holds `en-GB`, so English
//! output does not depend on the machine.
//!
//! **Fluent's isolation marks are off.** By default it wraps every placeable in
//! U+2068/U+2069 so a right-to-left value cannot reorder the sentence around
//! it. The interface has no right-to-left language in this phase, and the
//! marks would make every English string that takes an argument differ from
//! the literal it replaced — which several hundred assertions pin, and which
//! the user would copy out of a message with two invisible characters in it.

use std::borrow::Cow;
use std::sync::{LazyLock, OnceLock};

use i18n_embed::LanguageLoader;
use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use i18n_embed::unic_langid::LanguageIdentifier;
use rust_embed::RustEmbed;
use schemaic_core::locale::{self, UiLanguage};
use schemaic_core::text::{self, NumberMarks};

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Catalogues;

/// The loader every [`tr!`] reads. `en-GB` until [`init`] chooses otherwise.
///
/// Public for the app crate's own `tr!`, which is `fl!` against this same
/// loader, its ids checked through an `i18n.toml` pointing at these catalogues
/// — `fl!` reads the calling crate's, so a macro exported from here would not
/// carry the check across.
pub static LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = fluent_language_loader!();
    configure(&loader, UiLanguage::EnGb);
    loader
});

/// A catalogue message, by id — `fl!` against [`LOADER`].
macro_rules! tr {
    ($($args:tt)*) => {
        i18n_embed_fl::fl!($crate::i18n::LOADER, $($args)*)
    };
}
pub(crate) use tr;

/// What [`init`] was told at launch, so Settings can ask what a choice
/// *would* draw without asking the OS again.
struct Launch {
    env: Option<String>,
    os: Vec<String>,
    active: UiLanguage,
}

static LAUNCH: OnceLock<Launch> = OnceLock::new();

/// Choose the interface language, once, before any view is built.
///
/// `env` is `SCHEMAIC_LOCALE`, `os` the system's preferred languages and
/// `setting` the persisted *Language* key — the app reads all three, so this
/// crate never touches the environment. A second call is ignored: the language
/// a running app draws in does not change.
pub fn init(env: Option<String>, os: Vec<String>, setting: &str) -> UiLanguage {
    let active = locale::resolve(env.as_deref(), setting, &os);
    if LAUNCH.set(Launch { env, os, active }).is_ok() {
        configure(&LOADER, active);
    }
    self::active()
}

/// The language the interface is drawn in.
pub(crate) fn active() -> UiLanguage {
    LAUNCH.get().map_or(UiLanguage::EnGb, |l| l.active)
}

/// The marks the interface writes a figure with — English's until [`init`],
/// and in every test.
pub fn number_marks() -> NumberMarks {
    active().number_marks()
}

/// A figure core printed in English (`1,234.5`, `1.5 KiB`, `1.2s`, `~4.2m`),
/// in the interface's marks. **Every figure a view shows goes through this or
/// through a grouper given [`number_marks`]**; core's own printers stay English
/// for the CLI, MCP and the clipboard. A figure handed to [`tr!`] as a string
/// argument goes through it too — a plural selects on the raw number, never on
/// this.
pub fn figure(english: &str) -> String {
    text::remark(english, number_marks())
}

/// `text::human_count` in the interface's marks: `1.25k`, German `1,25k` —
/// the form the go-to-row box reads back in the same marks.
pub fn count(n: usize) -> String {
    figure(&text::human_count(n))
}

/// `stats::format_bytes` in the interface's marks: `1.5 KiB`, German `1,5 KiB`.
pub fn bytes(n: u64) -> String {
    figure(&schemaic_core::stats::format_bytes(n))
}

/// `transcript::elapsed_text` in the interface's marks: `1.2s`, German `1,2s`.
pub fn elapsed(ms: u64) -> String {
    figure(&schemaic_core::transcript::elapsed_text(ms))
}

/// `format::contrasting_bytes` in the interface's marks — two sizes told
/// apart, for a refusal whose point is that they differ.
pub fn contrasting_bytes(a: u64, b: u64) -> (String, String) {
    let (a, b) = schemaic_core::format::contrasting_bytes(a, b);
    (figure(&a), figure(&b))
}

/// The language a *Language* setting of `key` would draw, on this machine —
/// what Settings compares with [`active`] to say a restart is needed.
pub(crate) fn would_draw(key: &str) -> UiLanguage {
    match LAUNCH.get() {
        Some(l) => locale::resolve(l.env.as_deref(), key, &l.os),
        None => locale::resolve(None, key, &[]),
    }
}

/// The *Language* setting as Settings offers it: follow the system, or one of
/// the shipped languages. Never the pseudo-locale, and never a language whose
/// catalogue is not complete (`locale::SHIPPED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageChoice {
    System,
    Fixed(UiLanguage),
}

impl LanguageChoice {
    /// What Settings lists, system first.
    pub fn options() -> Vec<LanguageChoice> {
        std::iter::once(LanguageChoice::System)
            .chain(locale::SHIPPED.iter().map(|l| LanguageChoice::Fixed(*l)))
            .collect()
    }

    /// The persisted key.
    pub fn key(self) -> &'static str {
        match self {
            LanguageChoice::System => locale::SYSTEM_KEY,
            LanguageChoice::Fixed(l) => l.key(),
        }
    }

    /// A persisted key as Settings shows it. A key it does not offer — one a
    /// newer build wrote, a hand edit, a catalogue not shipped — reads as the
    /// system's, which is also what [`locale::resolve`] makes of it.
    pub fn from_key(key: &str) -> LanguageChoice {
        UiLanguage::from_key(key)
            .filter(|l| locale::SHIPPED.contains(l))
            .map_or(LanguageChoice::System, LanguageChoice::Fixed)
    }

    /// The entry's label: a language by its own name, and the system entry
    /// with the language it resolves to on this machine.
    pub fn label(self) -> String {
        match self {
            LanguageChoice::System => tr!(
                "settings-language-system",
                language = would_draw(locale::SYSTEM_KEY).endonym()
            ),
            LanguageChoice::Fixed(l) => l.endonym().to_string(),
        }
    }

    /// Whether choosing this needs a restart to show: it would draw another
    /// language than the one on screen. Choosing *English* on an English
    /// system changes the setting and nothing else, and says nothing.
    pub fn needs_restart(self) -> bool {
        would_draw(self.key()) != active()
    }
}

// ── Core's words, in the interface's language ───────────────────────────────
// Core names things in English for every surface — the CLI, the MCP server and
// the AI prompts, which stay English. Where core's word is a property of a
// typed value rather than finished prose, the interface draws its own here
// instead of translating core's text, and English agrees with core byte for
// byte (`english_names_every_object_kind_as_core_does`).

/// What an object kind is called in a sentence — `ObjectKind::label`'s word:
/// "sequence", "procedure".
pub(crate) fn object_kind(kind: schemaic_core::ddl::ObjectKind) -> String {
    use schemaic_core::ddl::ObjectKind;
    match kind {
        ObjectKind::Enum => tr!("kind-type"),
        ObjectKind::Domain => tr!("kind-domain"),
        ObjectKind::Sequence => tr!("kind-sequence"),
        ObjectKind::Function => tr!("kind-function"),
        ObjectKind::Procedure => tr!("kind-procedure"),
        ObjectKind::Event => tr!("kind-event"),
        ObjectKind::Synonym => tr!("kind-synonym"),
        ObjectKind::AliasType => tr!("kind-alias-type"),
        ObjectKind::XmlSchemaCollection => tr!("kind-xml-schema-collection"),
        ObjectKind::Extension => tr!("kind-extension"),
    }
}

/// The heading a set of them is listed under — `ObjectKind::group_label`'s
/// word: "Sequences", "Procedures".
pub(crate) fn object_kind_group(kind: schemaic_core::ddl::ObjectKind) -> String {
    use schemaic_core::ddl::ObjectKind;
    match kind {
        ObjectKind::Enum => tr!("kind-group-type"),
        ObjectKind::Domain => tr!("kind-group-domain"),
        ObjectKind::Sequence => tr!("kind-group-sequence"),
        ObjectKind::Function => tr!("kind-group-function"),
        ObjectKind::Procedure => tr!("kind-group-procedure"),
        ObjectKind::Event => tr!("kind-group-event"),
        ObjectKind::Synonym => tr!("kind-group-synonym"),
        ObjectKind::AliasType => tr!("kind-group-alias-type"),
        ObjectKind::XmlSchemaCollection => tr!("kind-group-xml-schema-collection"),
        ObjectKind::Extension => tr!("kind-group-extension"),
    }
}

/// "5m ago" — `history::relative_time` in the interface's language, from the
/// interval core works out (`history::ago`).
pub(crate) fn relative_time(ts: u64, now: u64) -> String {
    use schemaic_core::history::{Ago, ago};
    match ago(ts, now) {
        Ago::JustNow => tr!("ago-just-now"),
        Ago::Minutes(n) => tr!("ago-minutes", n = n),
        Ago::Hours(n) => tr!("ago-hours", n = n),
        Ago::Days(n) => tr!("ago-days", n = n),
        Ago::Weeks(n) => tr!("ago-weeks", n = n),
    }
}

/// A history row's outcome line — `history::outcome_line`'s composition, with
/// the row count worded here.
pub(crate) fn outcome_line(entry: &schemaic_core::history::HistoryEntry) -> Option<String> {
    schemaic_core::history::outcome_line_with(entry, number_marks(), |n, capped| {
        if capped {
            tr!("outcome-rows-capped", n = n)
        } else {
            tr!("outcome-rows", n = n)
        }
    })
}

// ── Core's typed messages ───────────────────────────────────────────────────
// Phase 4: where core says *why* in a typed value rather than a finished
// sentence, the interface words it — one arm per variant, its fields as the
// message's arguments — and core's own rendering stays the English for the
// CLI, MCP and the core tests, held equal to the interface's by a test per
// type. Each renderer lives beside the code that shows it
// (`alias_rename::refusal_message`, the app's `install_cli`); here only what
// another file or crate calls.

/// A connection's environment as the form's picker and the top bar's badge
/// name it — `connection::Environment::label` in the interface's language.
pub(crate) fn environment_label(env: schemaic_core::connection::Environment) -> String {
    use schemaic_core::connection::Environment;
    match env {
        Environment::None => tr!("env-none"),
        Environment::Local => tr!("env-local"),
        Environment::Development => tr!("env-development"),
        Environment::Testing => tr!("env-testing"),
        Environment::Staging => tr!("env-staging"),
        Environment::Production => tr!("env-production"),
    }
}

/// A connection's AI data access level — `connection::AiData::label`.
pub(crate) fn ai_data_label(level: schemaic_core::connection::AiData) -> String {
    use schemaic_core::connection::AiData;
    match level {
        AiData::SchemaOnly => tr!("ai-data-schema-only"),
        AiData::OnRequest => tr!("ai-data-on-request"),
        AiData::Full => tr!("ai-data-full"),
    }
}

/// What a level lets leave the machine — `connection::AiData::hint`.
pub(crate) fn ai_data_hint(level: schemaic_core::connection::AiData) -> String {
    use schemaic_core::connection::AiData;
    match level {
        AiData::SchemaOnly => tr!("ai-data-schema-only-hint"),
        AiData::OnRequest => tr!("ai-data-on-request-hint"),
        AiData::Full => tr!("ai-data-full-hint"),
    }
}

/// The sentence a finished export says — `export::export_note` in the
/// interface's language: the rows and where they went, then each loss joined
/// as core joins them (` — ` before the first, `; ` between).
pub(crate) fn export_note(note: &schemaic_core::export::ExportNote) -> String {
    use schemaic_core::export::{Caveat, XLSX_MAX_CELL_CHARS};
    let n = note.rows as usize;
    let head = tr!(
        "export-note-rows",
        rows = count(n),
        n = n,
        name = note.name.as_str()
    );
    let caveats: Vec<String> = note
        .caveats()
        .map(|(caveat, cols)| {
            let joined = cols.join(", ");
            match caveat {
                Caveat::Withheld => tr!("export-note-withheld", cols = joined, n = cols.len()),
                Caveat::Blanked => tr!("export-note-blanked", cols = joined),
                Caveat::Cut => tr!(
                    "export-note-cut",
                    cols = joined,
                    limit = XLSX_MAX_CELL_CHARS.to_string()
                ),
            }
        })
        .collect();
    if caveats.is_empty() {
        head
    } else {
        tr!(
            "export-note-with-caveats",
            head = head,
            caveats = caveats.join("; ")
        )
    }
}

/// What a failed export says — `export::export_failure_note` in the
/// interface's language. `message` already says it failed; this adds where the
/// rows went when the write had begun.
pub(crate) fn export_failure_note(message: &str, partial: Option<&str>) -> String {
    match partial {
        Some(name) => tr!(
            "export-failure-partial",
            message = message,
            name = name,
            part = schemaic_core::export::part_path(name)
        ),
        None => message.to_string(),
    }
}

/// What a cancelled export says — `export::export_cancel_note` in the
/// interface's language.
pub(crate) fn export_cancel_note(name: &str, partial: bool) -> String {
    if partial {
        tr!(
            "export-cancelled-partial",
            name = name,
            part = schemaic_core::export::part_path(name)
        )
    } else {
        tr!("export-cancelled-nothing", name = name)
    }
}

/// A statement timeout in words — `persist::statement_timeout_label` in the
/// interface's language. Public for the app, whose timed-out message quotes
/// it in the same words as the Settings and connection dropdowns.
pub fn statement_timeout(secs: u64) -> String {
    use schemaic_core::persist::{TimeoutSpan, statement_timeout_span};
    match statement_timeout_span(secs) {
        TimeoutSpan::Off => tr!("timeout-off"),
        TimeoutSpan::Hours(n) => tr!("timeout-hours", n = n),
        TimeoutSpan::Minutes(n) => tr!("timeout-minutes", n = n),
        TimeoutSpan::Seconds(n) => tr!("timeout-seconds", n = n),
    }
}

/// Why a run holds for its bind parameters — `ParamError`'s `Display` in the
/// interface's language. Public for the app, which raises the hold.
pub fn param_error(e: &schemaic_core::params::ParamError) -> String {
    use schemaic_core::params::ParamError;
    match e {
        ParamError::Missing(names) => {
            let names = names
                .iter()
                .map(|n| format!(":{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            tr!("params-missing", names = names)
        }
        ParamError::NotANumber { name, text } => {
            tr!(
                "params-not-a-number",
                name = name.as_str(),
                text = text.as_str()
            )
        }
    }
}

/// What a run cost its transaction — core's [`TxNote`] in the interface's
/// language.
///
/// [`TxNote`]: schemaic_core::tx::TxNote
fn tx_note_text(note: schemaic_core::tx::TxNote) -> String {
    use schemaic_core::tx::TxNote;
    match note {
        TxNote::CommittedBeforeDdl => tr!("tx-committed-before-ddl"),
        TxNote::RolledBackByServer => tr!("tx-rolled-back-by-server"),
        TxNote::Doomed => tr!("tx-doomed"),
        TxNote::AfterOwnCommit => tr!("tx-after-own-commit"),
        TxNote::LostOnCommit { stmts } => tr!("tx-lost-on-commit", stmts = stmts),
        TxNote::LostOnImplicitCommit { stmts } => {
            tr!("tx-lost-on-implicit-commit", stmts = stmts)
        }
        TxNote::LostRolledBack { stmts, mysql: true } => {
            tr!("tx-lost-rolled-back-mysql", stmts = stmts)
        }
        TxNote::LostRolledBack {
            stmts,
            mysql: false,
        } => {
            tr!("tx-lost-rolled-back", stmts = stmts)
        }
    }
}

/// `message` and, after a blank line, `note`'s words — as core's
/// `tx::failed_message` and its siblings put theirs together.
fn with_tx_note(message: &str, note: Option<schemaic_core::tx::TxNote>) -> String {
    match note {
        Some(note) => format!("{message}\n\n{}", tx_note_text(note)),
        None => message.to_string(),
    }
}

/// Core's `tx::run_failed_message` in the interface's language: `message`
/// (the server's, or the app's own) and what the failure cost the
/// transaction. Public for the app's run paths.
pub fn run_failed_message(
    message: &str,
    stmt: schemaic_core::tx::StmtOutcome,
    before: schemaic_core::tx::TxState,
    engine: schemaic_core::tx::TxEngine,
    sql: &str,
) -> String {
    with_tx_note(
        message,
        schemaic_core::tx::run_failure_note(stmt, before, engine, sql),
    )
}

/// Core's `tx::run_cancelled_message` in the interface's language, or `None`
/// when *Cancelled* is the whole truth. Public for the app's run paths.
pub fn run_cancelled_message(
    stmt: Option<schemaic_core::tx::StmtOutcome>,
    before: schemaic_core::tx::TxState,
    engine: schemaic_core::tx::TxEngine,
    sql: &str,
) -> Option<String> {
    schemaic_core::tx::run_cancelled_note(stmt, before, engine, sql)
        .map(|note| with_tx_note(&tr!("tx-cancelled"), Some(note)))
}

/// Core's `tx::not_sent_message` in the interface's language. Public for the
/// app's run paths.
pub fn not_sent_message(timed_out: bool) -> String {
    if timed_out {
        tr!("tx-not-sent-timeout")
    } else {
        tr!("tx-not-sent-cancelled")
    }
}

/// What the `TRUNCATE` confirmation asks about `label` — core's
/// `stats::truncate_prompt` in the interface's language; the figure is
/// core's `RowCount::label`, as the properties panel prints it.
pub(crate) fn truncate_prompt(label: &str, rows: Option<schemaic_core::stats::RowCount>) -> String {
    match schemaic_core::stats::confirm_figure(rows) {
        Some(r) => tr!(
            "tree-truncate-rows",
            rows = figure(&r.label()),
            table = label
        ),
        None => tr!("tree-truncate-every-row", table = label),
    }
}

/// What an operation that finds no SSH tunnel says — core's
/// `TunnelFailures::refusal` in the interface's language, the failure's own
/// text inside it as written. Public for the app, whose connection lookup and
/// terminal say it.
pub fn tunnel_refusal(
    failures: &schemaic_core::connection::TunnelFailures,
    conn_id: u64,
) -> String {
    use schemaic_core::connection::TunnelRefusal;
    match failures.refusal_of(conn_id) {
        TunnelRefusal::StillOpening => tr!("tunnel-still-opening"),
        TunnelRefusal::Tunnel(why) => tr!("tunnel-could-not-open", error = why),
        TunnelRefusal::Server { shown, .. } => tr!("tunnel-server-failed", error = shown),
    }
}

/// Why a kill is refused on a read-only connection — core's
/// `activity::read_only_refusal` in the interface's language. Public for the
/// app, whose kill action says it.
pub fn activity_read_only_refusal(kind: schemaic_core::activity::KillKind) -> String {
    use schemaic_core::activity::KillKind;
    match kind {
        KillKind::Query => tr!("activity-read-only-cancel"),
        KillKind::Session => tr!("activity-read-only-terminate"),
    }
}

/// Why two schemas cannot be compared — core's `compare::comparable` refusal
/// in the interface's language. The engines' names are product names. Public
/// for the app, which asks before reading either schema.
pub fn compare_incomparable(
    left: schemaic_core::intel::SqlDialect,
    right: schemaic_core::intel::SqlDialect,
) -> String {
    tr!(
        "compare-incomparable",
        left = left.engine_label(),
        right = right.engine_label()
    )
}

/// Why the grid will not re-read over staged work — core's
/// `filter::HoldsStagedWork`, whose English is `filter::STAGED_REQUERY_MSG`.
pub(crate) fn staged_requery_note() -> String {
    tr!("grid-staged-requery")
}

/// A re-read that landed over edits staged while it ran —
/// `filter::STAGED_LANDING_MSG` in the interface's language. Public for the
/// app, whose landing says it.
pub fn staged_landing_note() -> String {
    tr!("grid-staged-landing")
}

/// A commit's re-read that dropped the filter and sort —
/// `filter::REREAD_WITHOUT_FILTER_MSG` in the interface's language. Public
/// for the app, whose post-commit re-read says it.
pub fn reread_without_filter_note() -> String {
    tr!("grid-reread-without-filter")
}

/// Why a pasted string is not a connection URL — core's
/// [`UrlError`](schemaic_core::conn_import::UrlError) in the interface's
/// language. **Unredacted**, so reached only through [`skip_reason_shown`],
/// which redacts the sentence it sits in.
fn url_error_text(e: &schemaic_core::conn_import::UrlError) -> String {
    use schemaic_core::conn_import::UrlError;
    match e {
        UrlError::Empty => tr!("conn-import-url-empty"),
        UrlError::NoScheme => tr!("conn-import-url-no-scheme"),
        UrlError::UnknownScheme(s) => tr!("conn-import-url-unknown-scheme", scheme = s.as_str()),
        UrlError::NoHost => tr!("conn-import-url-no-host"),
        UrlError::BadPort(p) => tr!("conn-import-url-bad-port", port = p.as_str()),
        UrlError::NoPath => tr!("conn-import-url-no-path"),
        UrlError::Transport(s) => tr!("conn-import-url-transport", server = s.as_str()),
        UrlError::UserinfoInHost => tr!("conn-import-url-userinfo"),
        UrlError::NotSqlServer => tr!("conn-import-url-not-sql-server"),
    }
}

/// Why a connection-import entry was not imported, in the interface's
/// language and **redacted** as core's
/// [`SkipReason::message`](schemaic_core::conn_import::SkipReason::message)
/// is: the reason carries text a parser chose out of the entry, which may be
/// a password (`conn_import::redact_shown`). Public for the app's paste field,
/// and said by the import modal's skipped sentence.
pub fn skip_reason_shown(reason: &schemaic_core::conn_import::SkipReason) -> String {
    use schemaic_core::conn_import::{SkipReason, redact_shown};
    let words = match reason {
        SkipReason::UnsupportedEngine(d) => {
            tr!("conn-import-reason-unsupported-engine", driver = d.as_str())
        }
        SkipReason::NoServer => tr!("conn-import-reason-no-server"),
        SkipReason::NotPublished => tr!("conn-import-reason-not-published"),
        SkipReason::BadUrl(e) => tr!("conn-import-reason-unreadable", error = url_error_text(e)),
        SkipReason::NotAnExport(e) => {
            tr!("conn-import-reason-not-an-export", error = e.as_str())
        }
        &SkipReason::NewerExport(version) => {
            tr!("conn-import-reason-newer-export", version = version)
        }
        SkipReason::Unreadable(e) => tr!("conn-import-reason-unreadable", error = e.as_str()),
    };
    redact_shown(&words)
}

/// A startup notice — core's [`Notice::message`](schemaic_core::persist::Notice::message)
/// in the interface's language; a parser's or keyring's own text inside it
/// as written. Public for the app, which shows a save's answer directly.
pub fn notice_text(notice: &schemaic_core::persist::Notice) -> String {
    use schemaic_core::persist::Notice;
    match notice {
        Notice::Unreadable { name, error, swept } => {
            let mut text = format!(
                "{}\n{}",
                tr!(
                    "notice-unreadable",
                    name = name.as_str(),
                    error = error.as_str()
                ),
                tr!("notice-unreadable-kept", name = name.as_str())
            );
            if *swept {
                text.push('\n');
                text.push_str(&tr!("notice-unreadable-swept"));
            }
            text
        }
        Notice::Missing { name, from } => {
            // Bound first: `tr!` pastes `.into()` after an argument, so `*from`
            // would read as `*(from.into())`.
            let from: &str = from;
            format!(
                "{}\n{}",
                tr!("notice-missing", name = name.as_str()),
                tr!("notice-missing-restored", name = name.as_str(), from = from)
            )
        }
        Notice::KeyringUnreadable { secrets, error } => {
            let n = *secrets;
            let first = match error {
                Some(e) => tr!("notice-keyring-unread-why", n = n, error = e.as_str()),
                None => tr!("notice-keyring-unread", n = n),
            };
            format!("{first}\n{}", tr!("notice-keyring-unread-kept"))
        }
        Notice::KeyringSave {
            in_the_clear,
            undeleted,
        } => {
            let mut parts = Vec::new();
            if *in_the_clear > 0 {
                let n = *in_the_clear;
                parts.push(format!(
                    "{}\n{}",
                    tr!("notice-in-the-clear", n = n),
                    tr!("notice-in-the-clear-where")
                ));
            }
            if *undeleted > 0 {
                let n = *undeleted;
                parts.push(format!(
                    "{}\n{}",
                    tr!("notice-undeleted", n = n),
                    tr!("notice-undeleted-after")
                ));
            }
            parts.join("\n\n")
        }
        Notice::Said(text) => text.clone(),
    }
}

/// Why the import cannot read a file, in Schemaic's own words — core's
/// [`ReadRefusal::message`](schemaic_core::import::ReadRefusal::message) in the
/// interface's language, sizes as core prints them. Public for the app, whose
/// probe refuses a workbook too big to open before reading it.
pub fn read_refusal_text(r: &schemaic_core::import::ReadRefusal) -> String {
    use schemaic_core::import::{ReadRefusal, XLSX_MAX_BYTES, XLSX_MAX_COLS};
    let human_bytes = |n: i64| figure(&schemaic_core::format::human_bytes(n));
    let max = human_bytes(XLSX_MAX_BYTES as i64);
    match r {
        ReadRefusal::NotObjects => tr!("import-refuse-not-objects"),
        ReadRefusal::Oversize { file_bytes: None } => {
            tr!("import-refuse-oversize-unsized", max = max)
        }
        &ReadRefusal::Oversize {
            file_bytes: Some(n),
        } => tr!(
            "import-refuse-oversize",
            size = human_bytes(n as i64),
            max = max
        ),
        &ReadRefusal::Inflated {
            file_bytes,
            unpacked,
            ceiling,
        } => tr!(
            "import-refuse-inflated",
            size = human_bytes(file_bytes as i64),
            unpacked = human_bytes(unpacked as i64),
            ceiling = human_bytes(ceiling as i64)
        ),
        ReadRefusal::NoSuchSheet { sheet, sheets } => tr!(
            "import-refuse-no-such-sheet",
            sheet = sheet.as_str(),
            sheets = sheets.join(", ")
        ),
        ReadRefusal::NoSheets => tr!("import-refuse-no-sheets"),
        &ReadRefusal::TooWide { width } => tr!(
            "import-refuse-too-wide",
            width = width.to_string(),
            max = XLSX_MAX_COLS.to_string()
        ),
    }
}

/// Why a file could not be read or planned — core's
/// [`ImportError`](schemaic_core::import::ImportError)'s `Display` in the
/// interface's language, a library's own text inside it as written. Public
/// for the app, whose probe, check and load report it.
pub fn import_error_text(e: &schemaic_core::import::ImportError) -> String {
    use schemaic_core::import::ImportError;
    match e {
        ImportError::Read(text) => tr!("import-read-failed", error = text.as_str()),
        ImportError::Refused(r) => tr!("import-read-failed", error = read_refusal_text(r)),
        ImportError::NoColumnsMapped => tr!("import-no-columns-mapped"),
        &ImportError::PreviewRecordTooLarge { cap } => tr!(
            "import-preview-record-too-large",
            mib = (cap / (1024 * 1024)).to_string()
        ),
    }
}

/// Why the run guard refused a run or asks first — core's
/// [`GuardReason::message`](schemaic_core::sql::GuardReason::message) in the
/// interface's language. Public for the app's run guard bar; the script
/// modal's error bar says it too.
pub fn guard_reason_text(r: &schemaic_core::sql::GuardReason) -> String {
    use schemaic_core::sql::GuardReason;
    match r {
        GuardReason::ReadOnly => tr!("guard-read-only"),
        GuardReason::ReadOnlyRefused(r) => {
            tr!("guard-read-only-refused", reason = read_only_reason_text(r))
        }
        GuardReason::NoDatabase => tr!("guard-no-database"),
        GuardReason::Unsafe(u) => unsafe_text(u),
        GuardReason::Writes { several: false } => tr!("guard-writes-one"),
        GuardReason::Writes { several: true } => tr!("guard-writes-several"),
        GuardReason::Script { file } => tr!("guard-script", file = file.as_str()),
        GuardReason::Staged { after, changes } => {
            // Bound first: `tr!` pastes `.into()` after an argument.
            let n = *changes;
            let note = tr!("guard-staged", n = n);
            match after {
                Some(m) => format!("{} {note}", guard_reason_text(m)),
                None => note,
            }
        }
    }
}

/// What is wrong with a field — `import::IssueKind::message`'s words in the
/// interface's language; core keeps its English for the CLI. Here because the
/// import modal's problem list and a load's backstop ([`row_error_text`]) both
/// say it.
pub(crate) fn issue_kind_message(kind: schemaic_core::import::IssueKind) -> String {
    use schemaic_core::import::IssueKind;
    match kind {
        IssueKind::NotAnInteger => tr!("import-issue-not-an-integer"),
        IssueKind::NotANumber => tr!("import-issue-not-a-number"),
        IssueKind::NotABoolean => tr!("import-issue-not-a-boolean"),
        IssueKind::CellError => tr!("import-issue-cell-error"),
        IssueKind::NullInNotNull => tr!("import-issue-null-in-not-null"),
        IssueKind::FieldCount { expected, found } => {
            tr!(
                "import-issue-field-count",
                expected = expected,
                found = found
            )
        }
    }
}

/// Why the rows being written could not be read on — core's
/// `import::RowError::message` in the interface's language. The CSV reader's
/// and a copy's read's own words pass through as they came.
fn row_error_text(e: &schemaic_core::import::RowError) -> String {
    use schemaic_core::copy_table::CopyStop;
    use schemaic_core::import::RowError;
    match e {
        RowError::Issue(i) => {
            let (line, column) = (i.line, i.column.as_str());
            let problem = issue_kind_message(i.kind);
            if i.text.is_empty() {
                tr!(
                    "write-row-issue-empty",
                    line = line,
                    column = column,
                    problem = problem
                )
            } else {
                tr!(
                    "write-row-issue",
                    line = line,
                    column = column,
                    problem = problem,
                    text = i.text.as_str()
                )
            }
        }
        RowError::Read(m) => m.clone(),
        RowError::Copy(CopyStop::TooLong { column: Some(c) }) => {
            tr!("write-copy-too-long", column = c.as_str())
        }
        RowError::Copy(CopyStop::TooLong { column: None }) => tr!("write-copy-too-long-unnamed"),
        RowError::Copy(CopyStop::Bytes { column }) => {
            tr!("write-copy-bytes", column = column.as_str())
        }
        RowError::Copy(CopyStop::NotItsForm { column: Some(c) }) => {
            tr!("write-copy-not-its-form", column = c.as_str())
        }
        RowError::Copy(CopyStop::NotItsForm { column: None }) => {
            tr!("write-copy-not-its-form-unnamed")
        }
    }
}

/// A statement the server refused, `error` its own words — `schemaic-db`'s
/// `DbError::Query`, *query failed:* in English. Public for the app, which
/// holds the `DbError`; this crate does not.
pub fn db_query_failed(error: String) -> String {
    tr!("db-query-failed", error = error)
}

/// A connection the server or the network refused — `DbError::Connect`,
/// *connection failed:* in English.
pub fn db_connection_failed(error: String) -> String {
    tr!("db-connection-failed", error = error)
}

/// A statement the user or a timeout stopped — `DbError::Cancelled`.
pub fn db_query_cancelled() -> String {
    tr!("db-query-cancelled")
}

/// A DDL plan that stopped at its `at`th statement (0-based) with `applied`
/// earlier ones in effect — `schemaic-db`'s `DdlError`, *statement N
/// failed:* in English, `cause` already worded by the app.
pub fn ddl_failed(at: usize, applied: usize, cause: String) -> String {
    let n = at + 1;
    let failed = tr!("ddl-statement-failed", n = n, error = cause);
    if applied == 0 {
        failed
    } else {
        tr!("ddl-already-applied", failed = failed, n = applied)
    }
}

/// The user's Stop of a DDL plan — `DdlCause::Cancelled`.
pub fn ddl_cancelled() -> String {
    tr!("ddl-cancelled")
}

/// SQLite's refusal of a plan that leaves `row` pointing at nothing —
/// `DdlCause::DanglingForeignKey`.
pub fn ddl_dangling_foreign_key(row: String) -> String {
    tr!("ddl-dangling-foreign-key", row = row)
}

/// SQLite's refusal of a plan that breaks `view` — `DdlCause::BreaksView`,
/// `why` the engine's words.
pub fn ddl_breaks_view(view: String, why: String) -> String {
    tr!("ddl-breaks-view", view = view, why = why)
}

/// What the refresh after a SQL Server plan's commit could not do — core's
/// `ddl::ElsewhereNote` in the interface's language, the server's words in
/// it as they came. Public for the app, which runs the refresh.
pub fn elsewhere_note_text(note: &schemaic_core::ddl::ElsewhereNote) -> String {
    use schemaic_core::ddl::{Capped, ElsewhereNote};
    let list = |c: &Capped| {
        tr!(
            "ddl-elsewhere-list",
            items = c.shown.join(", "),
            more = c.more
        )
    };
    match note {
        ElsewhereNote::Failed { table, error } => tr!(
            "ddl-elsewhere-failed",
            table = table.as_str(),
            error = error.as_str()
        ),
        ElsewhereNote::Unrefreshed { table, views } => tr!(
            "ddl-elsewhere-unrefreshed",
            count = views.len(),
            table = table.as_str(),
            views = list(views)
        ),
        ElsewhereNote::Unsearched { table, databases } => tr!(
            "ddl-elsewhere-unsearched",
            count = databases.len(),
            table = table.as_str(),
            databases = list(databases)
        ),
    }
}

/// A server-level plan on SQLite — `DdlCause::NoDatabases`.
pub fn ddl_no_databases() -> String {
    tr!("ddl-no-databases")
}

/// A plan's failure `cause`, already worded, whose rollback then failed with
/// the engine's words `rollback` — `DdlCause::RollbackFailed`.
pub fn ddl_rollback_failed(cause: String, rollback: String) -> String {
    tr!("ddl-rollback-failed", error = cause, rollback = rollback)
}

/// A write path's failure — core's
/// [`WriteFailure::message`](schemaic_core::model::WriteFailure::message), what
/// `DbError::Write` displays, in the interface's language. Public for the
/// app's grid Commit, import and copy, which word it before it reaches a
/// modal. The server's words inside stay the server's.
pub fn write_failure_text(f: &schemaic_core::model::WriteFailure) -> String {
    use schemaic_core::model::{Rollback, WriteAction, WriteCause};
    let cause = match &f.cause {
        WriteCause::Server(m) | WriteCause::Refused(m) => m.clone(),
        WriteCause::Row(e) => row_error_text(e),
        WriteCause::OneRow(miss) => {
            let (table, affected) = (miss.table.as_str(), miss.affected);
            match miss.action {
                WriteAction::Delete => {
                    tr!("write-one-row-delete", table = table, affected = affected)
                }
                WriteAction::Update => {
                    tr!("write-one-row-update", table = table, affected = affected)
                }
                WriteAction::Insert => {
                    tr!("write-one-row-insert", table = table, affected = affected)
                }
            }
        }
        &WriteCause::Batch {
            rows,
            wrote,
            affected: false,
        } => tr!("write-batch-inserted", rows = rows, wrote = wrote),
        &WriteCause::Batch { rows, wrote, .. } => {
            tr!("write-batch-affected", rows = rows, wrote = wrote)
        }
        WriteCause::CommitCancelled => tr!("write-commit-cancelled"),
        WriteCause::ImportCancelled => tr!("write-import-cancelled"),
    };
    let note = match f.undone {
        None => String::new(),
        Some(Rollback::Complete) => tr!("write-rollback-complete"),
        Some(Rollback::Incomplete) => tr!("write-rollback-incomplete"),
        Some(Rollback::Unknown) => tr!("write-rollback-unknown"),
    };
    let error = format!("{cause}{note}");
    if f.cause.is_statements() {
        db_query_failed(error)
    } else {
        error
    }
}

/// What the read-only gate refused a statement for — core's
/// `sql::ReadOnlyReason::message` in the interface's language, said inside the
/// guard's read-only refusal. The keywords and names inside stay as SQL; the
/// heads are joined with `/` as core joins them.
fn read_only_reason_text(r: &schemaic_core::sql::ReadOnlyReason) -> String {
    use schemaic_core::sql::ReadOnlyReason;
    match r {
        ReadOnlyReason::Several => tr!("read-only-several"),
        ReadOnlyReason::NotARead { heads } => {
            tr!("read-only-not-a-read", heads = heads.join("/"))
        }
        ReadOnlyReason::Denied { word } => tr!("read-only-denied", word = word.as_str()),
        ReadOnlyReason::AdvancesSequence => tr!("read-only-advances-sequence"),
        ReadOnlyReason::UnlistedCall { name } => {
            tr!("read-only-unlisted-call", name = name.as_str())
        }
        &ReadOnlyReason::Locks { clause } => tr!("read-only-locks", clause = clause),
    }
}

/// A destructive statement's warning — core's `sql::Unsafe::message` in the
/// interface's language. The SQL keywords inside stay as the statement spelt
/// them.
fn unsafe_text(u: &schemaic_core::sql::Unsafe) -> String {
    use schemaic_core::sql::Unsafe;
    match *u {
        Unsafe::TruncatePartitionAll => tr!("guard-truncate-partition-all"),
        Unsafe::Truncate => tr!("guard-truncate"),
        Unsafe::NoWhere { kind } => tr!("guard-no-where", kind = kind),
        Unsafe::DropTable => tr!("guard-drop-table"),
        Unsafe::DropContainer { obj: "DATABASE" } => tr!("guard-drop-database"),
        Unsafe::DropContainer { .. } => tr!("guard-drop-schema"),
        Unsafe::DropOwned => tr!("guard-drop-owned"),
        Unsafe::DropCascade { obj } => tr!("guard-drop-cascade", obj = obj),
        Unsafe::ReplaceTable => tr!("guard-replace-table"),
        Unsafe::ReplaceContainer { obj: "DATABASE" } => tr!("guard-replace-database"),
        Unsafe::ReplaceContainer { .. } => tr!("guard-replace-schema"),
        Unsafe::TruncatePartition => tr!("guard-truncate-partition"),
        Unsafe::DropPartition => tr!("guard-drop-partition"),
    }
}

/// What a harness's own tools are kept to this session — `schemaic-ai`'s
/// `Constraint::notice` in the interface's language, asking the harness the
/// same questions. `None` where that says nothing.
pub fn constraint_notice_text(
    c: schemaic_ai::harness::Constraint,
    h: schemaic_ai::harness::Harness,
) -> Option<String> {
    use schemaic_ai::harness::Constraint;
    let cli = h.label();
    let mcp = match h.isolates_mcp_servers() {
        true => "isolated",
        false => "shared",
    };
    Some(match c {
        Constraint::Sealed => return None,
        Constraint::Restricted if h.restricted_means_sandbox() => {
            tr!("ai-constraint-sandboxed", cli = cli, mcp = mcp)
        }
        Constraint::Restricted if h.restricts_by_workspace_rules() => {
            tr!("ai-constraint-workspace-rules", cli = cli, mcp = mcp)
        }
        Constraint::Restricted if h.seals_by_flag() => tr!("ai-constraint-denylist", cli = cli),
        Constraint::Restricted => tr!("ai-constraint-unrestricted", cli = cli),
        Constraint::Unknown => tr!("ai-constraint-unknown", cli = cli),
    })
}

/// Why an agent CLI was not started — `schemaic-ai`'s `SpawnRefusal` in the
/// interface's language.
pub fn spawn_refusal_text(r: &schemaic_ai::SpawnRefusal) -> String {
    use schemaic_ai::{ArgUnit, Levers, SpawnRefusal};
    match r {
        SpawnRefusal::Oversize {
            harness,
            total,
            unit,
            limit,
            levers,
        } => {
            let unit = match unit {
                ArgUnit::Characters => tr!("ai-unit-characters"),
                ArgUnit::Bytes => tr!("ai-unit-bytes"),
            };
            let levers = match levers {
                Levers::SchemaAndQuery => tr!("ai-levers-schema-and-query"),
                Levers::Sample => tr!("ai-levers-sample"),
            };
            // As plain digits, which is how core writes them.
            let (total, limit) = (total.to_string(), limit.to_string());
            tr!(
                "ai-oversize",
                cli = harness.label(),
                total = total,
                unit = unit,
                limit = limit,
                levers = levers
            )
        }
        SpawnRefusal::BatchShim { harness, bin } => {
            tr!("ai-batch-shim", cli = harness.label(), bin = bin.as_str())
        }
    }
}

/// A failed agent CLI's reason — `schemaic-ai`'s `CliFailure` in the
/// interface's language, what the CLI said passing through as it said it.
pub fn cli_failure_text(f: &schemaic_ai::CliFailure) -> String {
    use schemaic_ai::CliFailure;
    match f {
        CliFailure::Said(s) => s.clone(),
        CliFailure::Exited { harness, code } => {
            let code = code.to_string();
            tr!("ai-cli-exited", cli = harness.label(), code = code)
        }
        CliFailure::Signalled { harness } => tr!("ai-cli-signalled", cli = harness.label()),
    }
}

/// Why Seed rows could not use the model's reply — core's `seed::SeedError`
/// in the interface's language, the JSON parser's own text passing through.
pub fn seed_error_text(e: &schemaic_core::seed::SeedError) -> String {
    use schemaic_core::seed::SeedError;
    match e {
        SeedError::Empty => tr!("ai-seed-empty"),
        SeedError::NotJson(why) => tr!("ai-seed-not-json", error = why.as_str()),
        SeedError::NotArrayOfObjects => tr!("ai-seed-not-rows"),
    }
}

/// Load `lang`'s catalogue (with `en-GB` behind it, message by message) into
/// `loader` and set it up. Split from [`init`] so a test can do it to a loader
/// of its own rather than to the one every other test reads.
fn configure(loader: &FluentLanguageLoader, lang: UiLanguage) {
    let tag: LanguageIdentifier = lang
        .catalogue()
        .parse()
        .expect("a catalogue tag is a valid language identifier");
    // Only the fallback can fail to load (a missing language is skipped), and
    // it is embedded: a failure here is a broken build, not a broken machine.
    loader
        .load_languages(&Catalogues, &[tag])
        .expect("the en-GB catalogue is embedded");
    // After the load, not before: a load swaps in fresh bundles, and these are
    // settings of the bundles.
    loader.set_use_isolating(false);
    if lang == UiLanguage::Pseudo {
        loader.with_bundles_mut(|bundle| bundle.set_transform(Some(pseudo)));
    }
}

/// The pseudo-locale's transform, applied to every text piece of a message —
/// never to the values put into it. Accented and lengthened by
/// `fluent-pseudo` (doubled vowels, roughly 30–40% longer, which is the length
/// German and Finnish run to), then bracketed, so a box that cuts either end
/// shows which end it cut.
fn pseudo(s: &str) -> Cow<'_, str> {
    if s.trim().is_empty() {
        return Cow::Borrowed(s);
    }
    Cow::Owned(format!("[{}]", fluent_pseudo::transform(s, false, true)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// **Every run guard reason reads in English exactly as core's
    /// `GuardReason::message`** — the refusals, every destructive statement's
    /// warning with each keyword it carries, the write confirmation over one
    /// statement and several, a script, and the staged-edits question alone
    /// and after another, over one change and several.
    #[test]
    fn english_says_every_guard_reason_as_core_does() {
        use schemaic_core::sql::{GuardReason, Unsafe};
        let mut reasons = vec![
            GuardReason::ReadOnly,
            GuardReason::ReadOnlyRefused(schemaic_core::sql::ReadOnlyReason::Several),
            GuardReason::NoDatabase,
            GuardReason::Writes { several: false },
            GuardReason::Writes { several: true },
            GuardReason::Script {
                file: "seed.sql".into(),
            },
        ];
        let unsafe_ones = [
            Unsafe::TruncatePartitionAll,
            Unsafe::Truncate,
            Unsafe::NoWhere { kind: "DELETE" },
            Unsafe::NoWhere { kind: "UPDATE" },
            Unsafe::DropTable,
            Unsafe::DropContainer { obj: "DATABASE" },
            Unsafe::DropContainer { obj: "SCHEMA" },
            Unsafe::DropOwned,
            Unsafe::DropCascade { obj: "TYPE" },
            Unsafe::DropCascade { obj: "EXTENSION" },
            Unsafe::ReplaceTable,
            Unsafe::ReplaceContainer { obj: "DATABASE" },
            Unsafe::ReplaceContainer { obj: "SCHEMA" },
            Unsafe::TruncatePartition,
            Unsafe::DropPartition,
        ];
        reasons.extend(unsafe_ones.into_iter().map(GuardReason::Unsafe));
        for changes in [1, 3] {
            reasons.push(GuardReason::Staged {
                after: None,
                changes,
            });
            for after in [
                GuardReason::Writes { several: false },
                GuardReason::Unsafe(Unsafe::NoWhere { kind: "DELETE" }),
            ] {
                reasons.push(GuardReason::Staged {
                    after: Some(Box::new(after)),
                    changes,
                });
            }
        }
        for r in reasons {
            assert_eq!(guard_reason_text(&r), r.message(), "{r:?}");
        }
    }

    /// **Every agent CLI's refusal and failure reads in English exactly as
    /// `schemaic-ai` says it** — each grade's notice on every harness, an
    /// oversize context in both units and with both levers, a batch shim, and
    /// a failure the CLI explained, exited from and was killed in — and each
    /// way Seed rows could not use the reply.
    #[test]
    fn english_says_every_agent_refusal_as_schemaic_ai_does() {
        use schemaic_ai::harness::{Constraint, Harness};
        use schemaic_ai::{ArgUnit, CliFailure, Levers, SpawnRefusal};
        for h in Harness::ALL {
            for c in [
                Constraint::Unknown,
                Constraint::Restricted,
                Constraint::Sealed,
            ] {
                assert_eq!(constraint_notice_text(c, h), c.notice(h), "{h:?} {c:?}");
            }
            let mut refusals = vec![SpawnRefusal::BatchShim {
                harness: h,
                bin: "opencode.cmd".into(),
            }];
            for unit in [ArgUnit::Characters, ArgUnit::Bytes] {
                for levers in [Levers::SchemaAndQuery, Levers::Sample] {
                    refusals.push(SpawnRefusal::Oversize {
                        harness: h,
                        total: 40_007,
                        unit,
                        limit: 30_000,
                        levers,
                    });
                }
            }
            for r in refusals {
                assert_eq!(spawn_refusal_text(&r), r.to_string(), "{r:?}");
            }
            for f in [
                CliFailure::Said("Failed to authenticate".into()),
                CliFailure::Exited {
                    harness: h,
                    code: 2,
                },
                CliFailure::Signalled { harness: h },
            ] {
                assert_eq!(cli_failure_text(&f), f.to_string(), "{f:?}");
            }
        }
        use schemaic_core::seed::SeedError;
        for e in [
            SeedError::Empty,
            SeedError::NotJson("expected value at line 1 column 1".into()),
            SeedError::NotArrayOfObjects,
        ] {
            assert_eq!(seed_error_text(&e), e.to_string(), "{e:?}");
        }
    }

    /// **Every write path's failure reads in English exactly as core's
    /// `WriteFailure::message`** — what `DbError::Write` displays: each cause,
    /// every miss of the 1-row net with and without a schema, each batch
    /// count, and each with every rollback outcome and none.
    #[test]
    fn english_says_every_write_failure_as_core_does() {
        use schemaic_core::model::{OneRowMiss, Rollback, WriteAction, WriteCause, WriteFailure};
        let mut causes = vec![
            WriteCause::Server("Duplicate entry '1' for key 'PRIMARY'".into()),
            WriteCause::CommitCancelled,
            WriteCause::ImportCancelled,
            WriteCause::Refused("Column `name` cannot hold an empty string".into()),
        ];
        for action in [
            WriteAction::Delete,
            WriteAction::Update,
            WriteAction::Insert,
        ] {
            for (table, affected) in [("db.city", 0), ("db.public.city", 2), ("db.t", 1)] {
                causes.push(WriteCause::OneRow(OneRowMiss {
                    action,
                    table: table.into(),
                    affected,
                }));
            }
        }
        {
            use schemaic_core::copy_table::CopyStop;
            use schemaic_core::import::{Issue, IssueKind, RowError};
            for kind in [
                IssueKind::NotAnInteger,
                IssueKind::NotANumber,
                IssueKind::NotABoolean,
                IssueKind::CellError,
                IssueKind::NullInNotNull,
                IssueKind::FieldCount {
                    expected: 3,
                    found: 2,
                },
            ] {
                for text in ["", "nope"] {
                    causes.push(WriteCause::Row(RowError::Issue(Issue {
                        line: 12,
                        column: "qty".into(),
                        text: text.into(),
                        kind,
                    })));
                }
            }
            causes.push(WriteCause::Row(RowError::Read(
                "CSV error: record 3 (line 4, byte 30): found record with 2 fields".into(),
            )));
            for stop in [
                CopyStop::TooLong {
                    column: Some("body".into()),
                },
                CopyStop::TooLong { column: None },
                CopyStop::Bytes {
                    column: "img".into(),
                },
                CopyStop::NotItsForm {
                    column: Some("img".into()),
                },
                CopyStop::NotItsForm { column: None },
            ] {
                causes.push(WriteCause::Row(RowError::Copy(stop)));
            }
        }
        for (rows, wrote) in [(500, 499), (1, 0), (2, 1)] {
            for affected in [false, true] {
                causes.push(WriteCause::Batch {
                    rows,
                    wrote,
                    affected,
                });
            }
        }
        for cause in causes {
            for undone in [
                None,
                Some(Rollback::Complete),
                Some(Rollback::Incomplete),
                Some(Rollback::Unknown),
            ] {
                let f = WriteFailure {
                    cause: cause.clone(),
                    undone,
                };
                assert_eq!(write_failure_text(&f), f.message(), "{f:?}");
            }
        }
    }

    /// **Every reason the read-only gate refuses with reads in English
    /// exactly as core's `ReadOnlyReason::message`**, on its own and inside
    /// the guard's read-only refusal — over each engine's list of heads.
    #[test]
    fn english_says_every_read_only_reason_as_core_does() {
        use schemaic_core::intel::SqlDialect;
        use schemaic_core::sql::{GuardReason, ReadOnlyReason, read_only_heads};
        let mut reasons = vec![
            ReadOnlyReason::Several,
            ReadOnlyReason::Denied {
                word: "SLEEP".into(),
            },
            ReadOnlyReason::AdvancesSequence,
            ReadOnlyReason::UnlistedCall {
                name: "my_func".into(),
            },
            ReadOnlyReason::Locks {
                clause: "FOR UPDATE",
            },
        ];
        for d in [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::MsSql,
        ] {
            reasons.push(ReadOnlyReason::NotARead {
                heads: read_only_heads(d),
            });
        }
        for r in reasons {
            assert_eq!(read_only_reason_text(&r), r.message(), "{r:?}");
            let g = GuardReason::ReadOnlyRefused(r);
            assert_eq!(guard_reason_text(&g), g.message(), "{g:?}");
        }
    }

    /// **A file the import cannot read is refused in English exactly as core
    /// says it** — every `ReadRefusal` on its own (as `xlsx_size_refusal`
    /// hands one over) and inside an `ImportError`, beside a library's text,
    /// nothing mapped and a JSON record past the preview.
    #[test]
    fn english_refuses_every_unreadable_file_as_core_does() {
        use schemaic_core::import::{ImportError, ReadRefusal};
        let refusals = [
            ReadRefusal::NotObjects,
            ReadRefusal::Oversize { file_bytes: None },
            ReadRefusal::Oversize {
                file_bytes: Some(300 * 1024 * 1024),
            },
            ReadRefusal::Inflated {
                file_bytes: 4 * 1024 * 1024,
                unpacked: 3 * 1024 * 1024 * 1024,
                ceiling: 2 * 1024 * 1024 * 1024,
            },
            ReadRefusal::NoSuchSheet {
                sheet: "Q3".into(),
                sheets: vec!["Q1".into(), "Q2".into()],
            },
            ReadRefusal::NoSheets,
            ReadRefusal::TooWide { width: 20_000 },
        ];
        for r in refusals {
            assert_eq!(read_refusal_text(&r), r.message(), "{r:?}");
            let e = ImportError::Refused(r);
            assert_eq!(import_error_text(&e), e.to_string(), "{e:?}");
        }
        for e in [
            ImportError::Read("CSV error: record 3 has 2 fields".into()),
            ImportError::NoColumnsMapped,
            ImportError::PreviewRecordTooLarge {
                cap: 16 * 1024 * 1024,
            },
        ] {
            assert_eq!(import_error_text(&e), e.to_string(), "{e:?}");
        }
    }

    /// **Every startup notice reads in English exactly as core's
    /// `persist::Notice::message`** — a file that did not parse (the
    /// credential file, whose copy is swept, and another), one that came back
    /// off either sibling, the keyring failing one secret and several with and
    /// without its reason, a save it refused one way, the other and both, and
    /// text its caller already worded.
    #[test]
    fn english_says_every_startup_notice_as_core_does() {
        use schemaic_core::persist::Notice;
        let mut notices = vec![
            Notice::Unreadable {
                name: "connections.json".into(),
                error: "EOF while parsing".into(),
                swept: true,
            },
            Notice::Unreadable {
                name: "ui.json".into(),
                error: "expected value".into(),
                swept: false,
            },
            Notice::Missing {
                name: "ui.json".into(),
                from: ".bak",
            },
            Notice::Missing {
                name: "tabs.json".into(),
                from: ".tmp",
            },
            Notice::Said("Already worded.".into()),
        ];
        for secrets in [1, 3] {
            for error in [None, Some("keyring locked".to_string())] {
                notices.push(Notice::KeyringUnreadable { secrets, error });
            }
        }
        for (in_the_clear, undeleted) in [(1, 0), (2, 0), (0, 1), (0, 4), (3, 1)] {
            notices.push(Notice::KeyringSave {
                in_the_clear,
                undeleted,
            });
        }
        for n in notices {
            assert_eq!(notice_text(&n), n.message(), "{n:?}");
        }
    }

    /// **A failed, cancelled or never-sent run reads in English exactly as
    /// core's `tx` messages say it** — every outcome, over every transaction
    /// state, engine and kind of statement that picks a different note, the
    /// counts 0, 1 and several among them.
    #[test]
    fn english_says_every_transaction_note_as_core_does() {
        use schemaic_core::tx::{self, StmtOutcome::*, TxEngine, TxState};
        let outcomes = [
            Ok,
            Failed,
            FailedIsolated,
            Cancelled,
            CancelledIsolated,
            FailedAndCommitted,
            FailedAndRolledBack,
            FailedAndDoomed,
            FailedAfterCommit,
            OkAndClosed,
            Untouched,
            ConnectionLost,
            NotSent,
        ];
        let befores = [
            TxState::Idle,
            TxState::Lost,
            TxState::Open { stmts: 0 },
            TxState::Open { stmts: 1 },
            TxState::Open { stmts: 2 },
            TxState::Poisoned { stmts: 1 },
            TxState::Poisoned { stmts: 1_234 },
        ];
        let engines = [TxEngine::MySql, TxEngine::Postgres, TxEngine::MsSql];
        let sqls = [
            "SELECT 1",
            "COMMIT",
            "ALTER TABLE t ADD c INT",
            "UPDATE t SET a = 1; COMMIT",
        ];
        for o in outcomes {
            for b in befores {
                for e in engines {
                    for s in sqls {
                        let at = format!("{o:?} {b:?} {e:?} {s}");
                        assert_eq!(
                            run_failed_message("boom", o, b, e, s),
                            tx::run_failed_message("boom", o, b, e, s),
                            "{at}"
                        );
                        assert_eq!(
                            run_cancelled_message(Some(o), b, e, s),
                            tx::run_cancelled_message(Some(o), b, e, s),
                            "{at}"
                        );
                    }
                }
            }
        }
        for timed_out in [true, false] {
            assert_eq!(not_sent_message(timed_out), tx::not_sent_message(timed_out));
        }
        assert_eq!(tr!("tx-cancelled"), tx::CANCELLED);
    }

    /// **The after-commit refresh note reads in English exactly as core's** —
    /// the batch's failure, one view and several unrefreshed, one database
    /// and several unsearched, with and without more beyond the eight shown.
    #[test]
    fn english_says_every_refresh_note_as_core_does() {
        use schemaic_core::ddl::{ElsewhereRefresh, elsewhere_notes, elsewhere_report};
        let view = |i: usize| ElsewhereRefresh {
            database: format!("db{i}"),
            module: Some(format!("dbo.v{i}")),
            error: Some("Invalid column name 'qty'.".into()),
        };
        let database = |i: usize, error: bool| ElsewhereRefresh {
            database: format!("other{i}"),
            module: None,
            error: error.then(|| "The server principal is not able to access it.".into()),
        };
        let mut cases: Vec<(Vec<ElsewhereRefresh>, Option<&str>)> =
            vec![(vec![], Some("query failed: timeout"))];
        for n in [1, 3, 11] {
            cases.push(((0..n).map(view).collect(), None));
            cases.push(((0..n).map(|i| database(i, i % 2 == 0)).collect(), None));
        }
        cases.push((vec![view(0), database(0, false)], None));
        for (rows, failed) in cases {
            let worded: Vec<String> = elsewhere_notes("dbo.orders", &rows, failed)
                .iter()
                .map(elsewhere_note_text)
                .collect();
            assert_eq!(
                Some(worded.join("\n")),
                elsewhere_report("dbo.orders", &rows, failed),
                "{rows:?}"
            );
        }
    }

    /// **The truncate confirmation reads in English exactly as core's** —
    /// with a figure worth naming, exact or estimated, and without one.
    #[test]
    fn english_asks_the_truncate_question_as_core_does() {
        use schemaic_core::stats::{self, RowCount};
        for rows in [
            None,
            Some(RowCount::Exact(0)),
            Some(RowCount::Exact(1)),
            Some(RowCount::Exact(4_213_551)),
            Some(RowCount::Estimate(12)),
            Some(RowCount::Estimate(4_200_000)),
        ] {
            assert_eq!(
                truncate_prompt("shop.orders", rows),
                stats::truncate_prompt("shop.orders", rows),
                "{rows:?}"
            );
        }
    }

    /// **The tunnel refusal reads in English exactly as core's** — still
    /// opening, an SSH failure and a server's, a closing full stop on the
    /// failure taken off as core takes it.
    #[test]
    fn english_says_the_tunnel_refusal_as_core_does() {
        use schemaic_core::connection::{LoadFailure, TunnelFailures};
        let mut f = TunnelFailures::default();
        f.record(1, LoadFailure::Tunnel("Host key mismatch.".into()));
        let denied = "connection failed: Access denied for user 'u'";
        f.record(
            2,
            LoadFailure::Server {
                text: denied.into(),
                shown: denied.into(),
            },
        );
        for id in [0, 1, 2] {
            assert_eq!(tunnel_refusal(&f, id), f.refusal(id), "{id}");
        }
    }

    /// **The window says a server's failure as the window worded it**, and
    /// core's refusal — the English — as the error said it.
    #[test]
    fn the_tunnel_refusal_says_the_servers_failure_as_shown() {
        use schemaic_core::connection::{LoadFailure, TunnelFailures};
        let mut f = TunnelFailures::default();
        f.record(
            2,
            LoadFailure::Server {
                text: "connection failed: Access denied".into(),
                shown: "Verbindung fehlgeschlagen: Access denied".into(),
            },
        );
        assert!(tunnel_refusal(&f, 2).contains("Verbindung fehlgeschlagen: Access denied"));
        assert!(f.refusal(2).contains("connection failed: Access denied"));
    }

    /// **A refused kill and an incomparable pair read in English exactly as
    /// core says them** — both kill kinds, and every pair of engines.
    #[test]
    fn english_refuses_a_kill_and_a_comparison_as_core_does() {
        use schemaic_core::activity::{self, KillKind};
        use schemaic_core::intel::SqlDialect;
        for kind in [KillKind::Query, KillKind::Session] {
            assert_eq!(
                activity_read_only_refusal(kind),
                activity::read_only_refusal(kind),
                "{kind:?}"
            );
        }
        let all = [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::MsSql,
        ];
        for l in all {
            for r in all.into_iter().filter(|r| *r != l) {
                assert_eq!(
                    Err(compare_incomparable(l, r)),
                    schemaic_core::compare::comparable(l, r),
                    "{l:?} {r:?}"
                );
            }
        }
    }

    /// **The grid's three filter notices read in English exactly as core's
    /// constants.**
    #[test]
    fn english_says_every_filter_notice_as_core_does() {
        use schemaic_core::filter;
        assert_eq!(staged_requery_note(), filter::STAGED_REQUERY_MSG);
        assert_eq!(staged_landing_note(), filter::STAGED_LANDING_MSG);
        assert_eq!(
            reread_without_filter_note(),
            filter::REREAD_WITHOUT_FILTER_MSG
        );
    }

    /// **Every URL error and skip reason reads in English exactly as core's
    /// `message` says it** — redaction included, so a password a parser
    /// carried into a reason stays hidden in the interface's wording.
    #[test]
    fn english_says_every_import_refusal_as_core_does() {
        use schemaic_core::conn_import::{SkipReason, UrlError};
        let errors = [
            UrlError::Empty,
            UrlError::NoScheme,
            UrlError::UnknownScheme("postgersql".into()),
            UrlError::NoHost,
            UrlError::BadPort("54x2".into()),
            UrlError::NoPath,
            UrlError::Transport("np:\\\\.\\pipe\\sql".into()),
            UrlError::UserinfoInHost,
            UrlError::NotSqlServer,
        ];
        for e in &errors {
            assert_eq!(url_error_text(e), e.message(), "{e:?}");
        }
        let mut reasons: Vec<SkipReason> = errors.into_iter().map(SkipReason::BadUrl).collect();
        reasons.extend([
            SkipReason::UnsupportedEngine("oracle.jdbc.OracleDriver".into()),
            SkipReason::NoServer,
            SkipReason::NotPublished,
            SkipReason::NotAnExport("expected value at line 1 column 1".into()),
            SkipReason::NewerExport(2),
            SkipReason::Unreadable("could not parse postgres://admin:hunter2@h/d".into()),
        ]);
        for r in &reasons {
            assert_eq!(skip_reason_shown(r), r.message(), "{r:?}");
        }
        let hidden = SkipReason::Unreadable("could not parse postgres://admin:hunter2@h/d".into());
        assert!(!skip_reason_shown(&hidden).contains("hunter2"));
    }

    fn loader(lang: UiLanguage) -> FluentLanguageLoader {
        let l = fluent_language_loader!();
        configure(&l, lang);
        l
    }

    fn system(l: &FluentLanguageLoader, language: &str) -> String {
        l.get_args(
            "settings-language-system",
            HashMap::from([("language", language)]),
        )
    }

    /// **English output is byte-identical to the literal it replaces.** Fluent
    /// wraps each placeable in U+2068/U+2069 unless told not to, so every
    /// message with an argument would differ from its old literal by two
    /// invisible characters.
    #[test]
    fn a_message_with_an_argument_carries_no_isolation_marks() {
        assert_eq!(
            system(&loader(UiLanguage::EnGb), "English"),
            "System (English)"
        );
        assert_eq!(
            system(&loader(UiLanguage::De), "Deutsch"),
            "System (Deutsch)"
        );
    }

    #[test]
    fn the_global_loader_is_english_until_init() {
        assert_eq!(tr!("settings-language"), "Language");
    }

    #[test]
    fn german_reads_its_own_catalogue() {
        assert_eq!(loader(UiLanguage::De).get("settings-language"), "Sprache");
    }

    /// German shortcut labels use the German keyboard's legends — the ones
    /// `shortcuts`' tests take as their German namer.
    #[test]
    fn german_names_keys_by_its_keyboard_legends() {
        let de = loader(UiLanguage::De);
        for (id, legend) in [
            ("key-ctrl", "Strg"),
            ("key-shift", "Umschalt"),
            ("key-enter", "Eingabe"),
            ("key-del", "Entf"),
            ("key-home", "Pos1"),
            ("key-end", "Ende"),
            ("key-space", "Leertaste"),
        ] {
            assert_eq!(de.get(id), legend, "{id}");
        }
    }

    /// The pseudo-locale changes every text piece — so a string left as a
    /// literal is the one that reads plain — and leaves the values put into a
    /// message alone, since those are data and not the interface's words.
    #[test]
    fn the_pseudo_locale_marks_the_words_and_spares_the_values() {
        let l = loader(UiLanguage::Pseudo);
        let word = l.get("settings-language");
        assert!(word.starts_with('[') && word.ends_with(']'), "{word}");
        assert_ne!(word, "Language");
        assert!(
            word.chars().count() > "Language".chars().count() + 2,
            "longer: {word}"
        );
        assert!(
            system(&l, "English").contains("English"),
            "the value is spared"
        );
        assert!(
            !system(&l, "English").contains("System"),
            "the words are not"
        );
    }

    /// Settings offers the system and the shipped catalogues, and reads a key
    /// it does not offer as the system — never as the pseudo-locale, and never
    /// as a catalogue still being written.
    #[test]
    fn the_language_setting_offers_only_what_ships() {
        let options = LanguageChoice::options();
        assert_eq!(options[0], LanguageChoice::System);
        assert!(options.contains(&LanguageChoice::Fixed(UiLanguage::EnGb)));
        assert!(!options.contains(&LanguageChoice::Fixed(UiLanguage::Pseudo)));
        for o in &options {
            assert_eq!(LanguageChoice::from_key(o.key()), *o);
        }
        assert_eq!(LanguageChoice::from_key("pseudo"), LanguageChoice::System);
        assert_eq!(LanguageChoice::from_key("xx"), LanguageChoice::System);
        assert_eq!(
            LanguageChoice::from_key("de"),
            if locale::SHIPPED.contains(&UiLanguage::De) {
                LanguageChoice::Fixed(UiLanguage::De)
            } else {
                LanguageChoice::System
            }
        );
    }

    /// Without `init` (every test), every choice draws English — so none of
    /// them needs a restart, and the system entry says it is English.
    #[test]
    fn a_choice_that_draws_what_is_on_screen_needs_no_restart() {
        for o in LanguageChoice::options() {
            assert!(!o.needs_restart(), "{o:?}");
        }
        assert_eq!(LanguageChoice::System.label(), "System (English)");
        assert_eq!(LanguageChoice::Fixed(UiLanguage::EnGb).label(), "English");
    }

    /// **A connection's environment and AI data access read in English
    /// exactly as core's labels and hints**, every value.
    #[test]
    fn english_names_every_environment_and_data_level_as_core_does() {
        use schemaic_core::connection::{AiData, Environment};
        for env in Environment::ALL {
            assert_eq!(environment_label(env), env.label(), "{env:?}");
        }
        for level in AiData::ALL {
            assert_eq!(ai_data_label(level), level.label(), "{level:?}");
            assert_eq!(ai_data_hint(level), level.hint(), "{level:?}");
        }
    }

    /// **An export's note reads in English exactly as core's** — the head's
    /// singular and plural, every combination of the three losses (so every
    /// seam), one column and several.
    #[test]
    fn english_says_every_export_note_as_core_does() {
        use schemaic_core::export::{ExportTally, export_said};
        let cols = |n: usize| -> Vec<String> { (1..=n).map(|i| format!("c{i}")).collect() };
        for rows in [0, 1, 16_384, 2_000_000] {
            for mask in 0..8u8 {
                for width in [1, 2] {
                    let pick = |bit: u8| if mask & bit != 0 { cols(width) } else { vec![] };
                    let tally = ExportTally {
                        rows,
                        withheld: pick(1),
                        blanked: pick(2),
                        cut: pick(4),
                    };
                    let note = export_said(&tally, "out.csv", true).expect("streaming");
                    assert_eq!(export_note(&note), note.to_string(), "{note:?}");
                }
            }
        }
    }

    /// The same for a failed and a cancelled export, with and without the
    /// `.part` sibling.
    #[test]
    fn english_says_every_failed_or_cancelled_export_as_core_does() {
        use schemaic_core::export as core;
        let message = "Export failed: No space left on device (os error 28)";
        for partial in [None, Some("orders.csv")] {
            assert_eq!(
                export_failure_note(message, partial),
                core::export_failure_note(message, partial)
            );
        }
        for partial in [false, true] {
            assert_eq!(
                export_cancel_note("orders.csv", partial),
                core::export_cancel_note("orders.csv", partial)
            );
        }
    }

    /// **A statement timeout reads in English exactly as core's label**, in
    /// every unit and both numbers of each.
    #[test]
    fn english_says_every_statement_timeout_as_core_does() {
        for secs in [0, 1, 2, 45, 60, 90, 120, 900, 3_600, 7_200, 5_400] {
            assert_eq!(
                statement_timeout(secs),
                schemaic_core::persist::statement_timeout_label(secs),
                "{secs}"
            );
        }
    }

    /// A name quoted inside a message takes the language's own quotation
    /// marks, which are the catalogue's text, not the argument's.
    #[test]
    fn german_quotes_a_conflicting_alias_name_its_own_way() {
        let de = loader(UiLanguage::De);
        let args = HashMap::from([("name", "t2")]);
        assert!(de.get_args("alias-rename-conflict", args).contains("„t2“"));
    }

    /// **A typed message reads in English exactly as core's own rendering**
    /// (phase 4). The names list is joined here (`:a, :b`), as core's
    /// `Display` joins it, and handed in whole.
    #[test]
    fn english_says_every_parameter_error_as_core_does() {
        use schemaic_core::params::ParamError;
        for e in [
            ParamError::Missing(vec!["id".into()]),
            ParamError::Missing(vec!["a".into(), "b".into(), "c".into()]),
            ParamError::NotANumber {
                name: "n".into(),
                text: "1 OR 1=1".into(),
            },
        ] {
            assert_eq!(param_error(&e), e.to_string(), "{e:?}");
        }
    }

    /// **English names every object kind exactly as core does**, so the
    /// interface's words and the CLI's, the MCP server's and the AI prompts'
    /// — which stay core's — agree byte for byte in English. Asked over
    /// `ObjectKind::ALL`, so a new kind with no message fails here as well as
    /// at the exhaustive match.
    #[test]
    fn english_names_every_object_kind_as_core_does() {
        for kind in schemaic_core::ddl::ObjectKind::ALL {
            assert_eq!(object_kind(kind), kind.label(), "{kind:?}");
            assert_eq!(object_kind_group(kind), kind.group_label(), "{kind:?}");
        }
        let de = loader(UiLanguage::De);
        assert_eq!(de.get("kind-procedure"), "Prozedur");
        assert_eq!(de.get("kind-group-procedure"), "Prozeduren");
    }

    /// **Plurals that replaced a `match` or a `text::plural` read as before**,
    /// for the editors and panels whose files hold no tests of their own: the
    /// schema editors' change count (`[0]` included — "No changes"), the
    /// properties panel's structure counts, and its vacuum note.
    #[test]
    fn the_shared_counts_read_as_they_did() {
        use schemaic_core::text::plural;
        assert_eq!(tr!("editor-change-count", n = 0), "No changes");
        assert_eq!(tr!("editor-change-count", n = 1), "1 change");
        assert_eq!(tr!("editor-change-count", n = 7), "7 changes");
        for n in [1usize, 2, 30] {
            assert_eq!(
                tr!("props-count-columns", n = n),
                format!("{n} {}", plural(n, "column", "columns"))
            );
            assert_eq!(
                tr!("props-count-indexes", n = n),
                format!("{n} {}", plural(n, "index", "indexes"))
            );
            assert_eq!(
                tr!("props-count-keys", n = n),
                format!("{n} {}", plural(n, "key", "keys"))
            );
        }
        // The copy modal's "Copied 1 row." / "Copied 3 rows.".
        assert_eq!(tr!("copy-table-copied", n = 1), "Copied 1 row.");
        assert_eq!(tr!("copy-table-copied", n = 3), "Copied 3 rows.");
        assert_eq!(tr!("copy-table-copied", n = 0), "Copied 0 rows.");
        assert_eq!(
            tr!("props-vacuum-note", pct = "23"),
            "23% of this table's row versions are dead and not yet reclaimed. A VACUUM \
             would return that space."
        );
    }

    /// English says how long ago, and how many rows, exactly as core does.
    #[test]
    fn english_says_ago_and_rows_as_core_does() {
        let now = 10_000_000_000u64;
        for back in [0u64, 61_000, 7_200_000, 90_000_000, 2_000_000_000] {
            assert_eq!(
                relative_time(now - back, now),
                schemaic_core::history::relative_time(now - back, now)
            );
        }
        // The count words, through the same composition core's English uses.
        let words = |n: u64, capped: bool| {
            if capped {
                tr!("outcome-rows-capped", n = n)
            } else {
                tr!("outcome-rows", n = n)
            }
        };
        for (n, capped) in [(0u64, false), (1, false), (100, false), (200_000, true)] {
            let english = if capped {
                format!("{n}+ rows")
            } else {
                format!(
                    "{n} {}",
                    schemaic_core::text::plural(n as usize, "row", "rows")
                )
            };
            assert_eq!(words(n, capped), english);
        }
    }

    /// **The script import's sentences read as the `format!`s they replaced**,
    /// each with a plural (or two) inside, for one statement and for several —
    /// `script_view` holds no tests of its own.
    #[test]
    fn the_script_import_sentences_read_as_they_did() {
        use schemaic_core::text::plural;
        for n in [1usize, 3] {
            let st = plural(n, "statement", "statements");
            assert_eq!(
                tr!("script-done", n = n, file = "a.sql"),
                format!("Ran {n} {st} from a.sql.")
            );
            assert_eq!(
                tr!("script-stopped", n = n),
                format!("Stopped after {n} {st}.")
            );
            assert_eq!(
                tr!("script-failed", message = "boom", n = n),
                format!("Failed: boom — {n} {st} ran before it.")
            );
            assert_eq!(
                tr!(
                    "script-failed-at",
                    line = 12,
                    file = "a.sql",
                    message = "boom",
                    n = n
                ),
                format!("Failed at line 12 of a.sql: boom — {n} {st} ran before it.")
            );
            let count = format!("{n}+");
            assert_eq!(
                tr!(
                    "script-probe-counted",
                    count = count.clone(),
                    n = n,
                    size = "8 MB"
                ),
                format!("{count} {st} read from the first 8 MB.")
            );
            assert_eq!(
                tr!("script-destroys", count = count.clone(), n = n),
                format!(
                    "{count} {st} in this file {} data — DROP, TRUNCATE or DELETE. That cannot \
                     be undone from here.",
                    plural(n, "destroys", "destroy")
                )
            );
            assert_eq!(
                tr!("script-unqualified", count = count.clone(), n = n),
                format!(
                    "{count} {st} in this file {} on every row of a table — a DELETE, UPDATE or \
                     TRUNCATE with no WHERE. That cannot be undone from here.",
                    plural(n, "acts", "act")
                )
            );
        }
    }

    #[test]
    fn pseudo_leaves_whitespace_alone() {
        assert_eq!(pseudo(" "), " ");
        assert_eq!(pseudo(""), "");
    }

    /// The catalogues, as embedded, parsed **strictly**. At run time a parse
    /// error is only logged and the broken messages fall back to English, and
    /// `fl!` checks only the `en-GB` file — so a typo in German would ship.
    #[test]
    fn every_catalogue_parses_cleanly() {
        let mut seen = 0;
        for path in Catalogues::iter() {
            let file = Catalogues::get(&path).expect("listed");
            let text = std::str::from_utf8(&file.data).expect("utf-8");
            if let Err((_, errors)) = fluent_syntax::parser::parse(text) {
                panic!("{path}: {errors:?}");
            }
            seen += 1;
        }
        assert!(seen >= 2, "found only {seen} catalogues");
    }

    fn ids(path: &str) -> Vec<String> {
        let file = Catalogues::get(path).expect(path);
        let text = std::str::from_utf8(&file.data).expect("utf-8").to_string();
        let res = fluent_syntax::parser::parse(text).expect("parses");
        res.body
            .iter()
            .filter_map(|e| match e {
                fluent_syntax::ast::Entry::Message(m) => Some(m.id.name.clone()),
                _ => None,
            })
            .collect()
    }

    /// A translation of a message English no longer has is dead weight that
    /// reads as live — and usually a renamed id whose new name is untranslated.
    #[test]
    fn no_translation_outlives_its_english_message() {
        let english = ids("en-GB/schemaic.ftl");
        for path in Catalogues::iter().filter(|p| !p.starts_with("en-GB/")) {
            for id in ids(&path) {
                assert!(
                    english.contains(&id),
                    "{path} translates `{id}`, which en-GB has not"
                );
            }
        }
    }

    /// **No view shows a figure core printed in English unmarked.** Core's
    /// printers write English's marks for the CLI, MCP and the clipboard; a
    /// view calling one directly showed `1.5 KiB` to a German reader. So
    /// outside this file every call to one in the two view crates sits on a
    /// line that hands it to [`figure`] or is given [`number_marks`] — or goes
    /// through [`count`], [`bytes`], [`elapsed`] and [`contrasting_bytes`],
    /// which do. A `figure(` ending the line above counts, as `cargo fmt`
    /// splits one there; a call split further, or a printer imported under
    /// another name, is not seen.
    #[test]
    fn every_core_figure_a_view_shows_takes_the_interface_s_marks() {
        const PRINTERS: [&str; 7] = [
            "human_count",
            "format_bytes",
            "human_bytes",
            "contrasting_bytes",
            "group_digits",
            "format_duration",
            "elapsed_text",
        ];
        // `::name` as a whole path segment — called, or passed as a function
        // (`.map(stats::format_bytes)`), which is how one was missed.
        let names = |line: &str, prefix: &str, p: &str| {
            let needle = format!("{prefix}{p}");
            line.match_indices(&needle).any(|(at, _)| {
                let next = line[at + needle.len()..].chars().next();
                !next.is_some_and(|c| c.is_alphanumeric() || c == '_')
            })
        };
        let files = crate::source_gate::crate_sources();
        let mut seen = 0usize;
        let mut offenders = Vec::new();
        for (name, code) in files.iter().filter(|(n, _)| n != "i18n.rs") {
            let lines: Vec<&str> = code.lines().collect();
            for (i, &line) in lines.iter().enumerate() {
                // `cargo fmt` breaks `figure(` + the call over two lines.
                let above = i.checked_sub(1).map_or("", |j| lines[j]);
                let imports = line.trim_start().starts_with("use schemaic_core");
                for p in PRINTERS {
                    // Core's, by its path; a view's own wrapper is unqualified,
                    // and this file's are `i18n::`.
                    if imports || !names(line, "::", p) || names(line, "i18n::", p) {
                        continue;
                    }
                    seen += 1;
                    let marked = |l: &str| l.contains("figure(") || l.contains("number_marks()");
                    if !(marked(line) || above.trim_end().ends_with("figure(")) {
                        offenders.push(format!("{name}:{}: {}", i + 1, line.trim()));
                    }
                }
                let imported = PRINTERS.iter().any(|p| names(line, "", p));
                if imports && imported {
                    offenders.push(format!("{name}:{}: {}", i + 1, line.trim()));
                }
            }
        }
        assert!(
            seen >= 5,
            "only {seen} core printer calls found; this gate has gone blind"
        );
        assert!(
            offenders.is_empty(),
            "a figure core printed in English, shown without the interface's \
             marks (wrap it in `i18n::figure`, or use `i18n::count`/`bytes`):\n{}",
            offenders.join("\n")
        );
    }
}
