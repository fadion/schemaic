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
    schemaic_core::history::outcome_line_with(entry, |n, capped| {
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
        rows = schemaic_core::text::human_count(n),
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
}
