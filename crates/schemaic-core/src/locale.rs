//! Which language the interface is drawn in — decided once, at launch.
//!
//! The GUI is translated; nothing else is. The CLI, the MCP server and
//! everything sent to an AI harness stay English, because scripts parse the
//! first and a model reads the second (`docs/architecture.md`, *The string
//! catalogue*). So this module answers a question only the GUI asks, and
//! nothing in core renders through it.
//!
//! **Three sources, in order:** the `SCHEMAIC_LOCALE` environment variable, the
//! *Language* setting, and the operating system's preferred languages. The
//! variable is for development — it reaches a catalogue that is not shipped yet,
//! when there is one — so it is honoured whatever it names. The setting and
//! the OS reach only [`SHIPPED`] catalogues: a language whose catalogue is half
//! written would draw a half-translated interface for everyone whose system is
//! set to it, which is worse than English.
//!
//! The answer is read once and never changes while the app runs. A label is
//! built with its text and does not re-render when the setting moves; the
//! setting says so where it is changed.

/// A language the interface can be drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiLanguage {
    /// British English — the source catalogue, and the answer whenever nothing
    /// else matches.
    EnGb,
    /// German.
    De,
    /// Albanian.
    Sq,
    /// Italian.
    It,
    /// Dutch.
    Nl,
    /// French.
    Fr,
    /// Spanish.
    Es,
    /// Romanian.
    Ro,
    /// Bulgarian — the one catalogue in Cyrillic, which the bundled IBM Plex
    /// covers.
    Bg,
}

/// The catalogues a user may choose, or be given by their OS. A language joins
/// this list when its catalogue is complete, not when it is started. English
/// first, since Settings lists them in this order.
pub const SHIPPED: &[UiLanguage] = &[
    UiLanguage::EnGb,
    UiLanguage::De,
    UiLanguage::Sq,
    UiLanguage::It,
    UiLanguage::Nl,
    UiLanguage::Fr,
    UiLanguage::Es,
    UiLanguage::Ro,
    UiLanguage::Bg,
];

/// The setting's value meaning "follow the operating system".
pub const SYSTEM_KEY: &str = "system";

impl UiLanguage {
    /// Every language, shipped or not.
    pub const ALL: &'static [UiLanguage] = &[
        UiLanguage::EnGb,
        UiLanguage::De,
        UiLanguage::Sq,
        UiLanguage::It,
        UiLanguage::Nl,
        UiLanguage::Fr,
        UiLanguage::Es,
        UiLanguage::Ro,
        UiLanguage::Bg,
    ];

    /// The persisted key — also what `SCHEMAIC_LOCALE` takes. The catalogue's
    /// tag, every language having a catalogue of its own.
    pub fn key(self) -> &'static str {
        self.catalogue()
    }

    /// The catalogue's language tag — the directory its `.ftl` lives in.
    pub fn catalogue(self) -> &'static str {
        match self {
            UiLanguage::EnGb => "en-GB",
            UiLanguage::De => "de",
            UiLanguage::Sq => "sq",
            UiLanguage::It => "it",
            UiLanguage::Nl => "nl",
            UiLanguage::Fr => "fr",
            UiLanguage::Es => "es",
            UiLanguage::Ro => "ro",
            UiLanguage::Bg => "bg",
        }
    }

    /// The language's name **in itself** — what a picker lists it as, in every
    /// language, so a reader lost in the wrong one can still find their own.
    /// Which is why these are not in the catalogue.
    pub fn endonym(self) -> &'static str {
        match self {
            UiLanguage::EnGb => "English",
            UiLanguage::De => "Deutsch",
            UiLanguage::Sq => "Shqip",
            UiLanguage::It => "Italiano",
            UiLanguage::Nl => "Nederlands",
            UiLanguage::Fr => "Français",
            UiLanguage::Es => "Español",
            UiLanguage::Ro => "Română",
            UiLanguage::Bg => "Български",
        }
    }

    /// The marks the language writes a figure with.
    pub fn number_marks(self) -> crate::text::NumberMarks {
        match self {
            UiLanguage::EnGb => crate::text::NumberMarks::ENGLISH,
            UiLanguage::De | UiLanguage::It | UiLanguage::Nl | UiLanguage::Es | UiLanguage::Ro => {
                crate::text::NumberMarks {
                    decimal: ',',
                    group: '.',
                }
            }
            // A space between groups, one that never breaks a figure across
            // lines — French's narrow one, as CLDR now has it.
            UiLanguage::Fr => crate::text::NumberMarks {
                decimal: ',',
                group: '\u{202F}',
            },
            UiLanguage::Bg | UiLanguage::Sq => crate::text::NumberMarks {
                decimal: ',',
                group: '\u{A0}',
            },
        }
    }

    /// A key as the setting or the variable spells it. Case-insensitive, and a
    /// region on a language the app has only one of is ignored (`de-AT` is
    /// German; `en-US` is the English there is).
    pub fn from_key(key: &str) -> Option<Self> {
        // The primary subtag: `de-AT`, `de_AT` and POSIX's `de_AT.UTF-8` alike.
        let primary = key.split(['-', '_', '.', '@']).next().unwrap_or("");
        match primary.to_ascii_lowercase().as_str() {
            "en" => Some(UiLanguage::EnGb),
            "de" => Some(UiLanguage::De),
            "sq" => Some(UiLanguage::Sq),
            "it" => Some(UiLanguage::It),
            "nl" => Some(UiLanguage::Nl),
            "fr" => Some(UiLanguage::Fr),
            "es" => Some(UiLanguage::Es),
            "ro" => Some(UiLanguage::Ro),
            "bg" => Some(UiLanguage::Bg),
            _ => None,
        }
    }
}

/// The language to draw the interface in.
///
/// `env` is `SCHEMAIC_LOCALE`, `setting` the persisted *Language* key
/// ([`SYSTEM_KEY`] or a language's [`UiLanguage::key`]), and `os` the system's
/// preferred languages, most preferred first, as BCP 47 tags.
pub fn resolve(env: Option<&str>, setting: &str, os: &[String]) -> UiLanguage {
    resolve_in(env, setting, os, SHIPPED)
}

/// [`resolve`] against a given shipped list, so the order can be tested with
/// a list other than the one that ships today.
fn resolve_in(
    env: Option<&str>,
    setting: &str,
    os: &[String],
    shipped: &[UiLanguage],
) -> UiLanguage {
    if let Some(lang) = env.and_then(UiLanguage::from_key) {
        return lang;
    }
    let offered = |lang: &UiLanguage| shipped.contains(lang);
    if let Some(lang) = UiLanguage::from_key(setting).filter(offered) {
        return lang;
    }
    os.iter()
        .filter_map(|tag| UiLanguage::from_key(tag))
        .find(offered)
        .unwrap_or(UiLanguage::EnGb)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn german_swaps_both_marks_and_english_keeps_core_s() {
        use crate::text::NumberMarks;
        assert_eq!(UiLanguage::EnGb.number_marks(), NumberMarks::ENGLISH);
        assert_eq!(
            UiLanguage::De.number_marks(),
            NumberMarks {
                decimal: ',',
                group: '.'
            }
        );
    }

    /// Every translation after German writes a decimal comma; they part on the
    /// group mark — a dot, or the space French, Bulgarian and Albanian set
    /// between groups, which must not break (French's CLDR one, U+202F).
    #[test]
    fn each_translation_writes_its_own_group_mark_after_a_decimal_comma() {
        use crate::text::NumberMarks;
        let comma = |group| NumberMarks {
            decimal: ',',
            group,
        };
        for (lang, marks) in [
            (UiLanguage::It, comma('.')),
            (UiLanguage::Nl, comma('.')),
            (UiLanguage::Es, comma('.')),
            (UiLanguage::Ro, comma('.')),
            (UiLanguage::Fr, comma('\u{202F}')),
            (UiLanguage::Bg, comma('\u{A0}')),
            (UiLanguage::Sq, comma('\u{A0}')),
        ] {
            assert_eq!(lang.number_marks(), marks, "{lang:?}");
        }
    }

    /// [`UiLanguage::ALL`] is every variant: the `match` makes a new one a
    /// compile error here until it joins the list.
    #[test]
    fn all_names_every_language_once() {
        for lang in UiLanguage::ALL {
            match lang {
                UiLanguage::EnGb
                | UiLanguage::De
                | UiLanguage::Sq
                | UiLanguage::It
                | UiLanguage::Nl
                | UiLanguage::Fr
                | UiLanguage::Es
                | UiLanguage::Ro
                | UiLanguage::Bg => {}
            }
            let once = UiLanguage::ALL.iter().filter(|l| *l == lang).count();
            assert_eq!(once, 1, "{lang:?}");
        }
        assert_eq!(UiLanguage::ALL.len(), 9);
    }

    #[test]
    fn keys_round_trip_and_tolerate_case_and_region() {
        for &lang in UiLanguage::ALL {
            assert_eq!(UiLanguage::from_key(lang.key()), Some(lang));
        }
        for (tag, lang) in [
            ("sq-AL", UiLanguage::Sq),
            ("sq_XK", UiLanguage::Sq),
            ("it-CH", UiLanguage::It),
            ("nl-BE", UiLanguage::Nl),
            ("fr-FR", UiLanguage::Fr),
            ("fr_CA.UTF-8", UiLanguage::Fr),
            ("es-MX", UiLanguage::Es),
            ("ro-MD", UiLanguage::Ro),
            ("bg-BG", UiLanguage::Bg),
        ] {
            assert_eq!(UiLanguage::from_key(tag), Some(lang), "{tag}");
        }
        assert_eq!(UiLanguage::from_key("EN-gb"), Some(UiLanguage::EnGb));
        assert_eq!(UiLanguage::from_key("en-US"), Some(UiLanguage::EnGb));
        assert_eq!(UiLanguage::from_key("en_US.UTF-8"), Some(UiLanguage::EnGb));
        assert_eq!(UiLanguage::from_key("de-AT"), Some(UiLanguage::De));
        // The pseudo-locales are gone; a setting or variable naming one is
        // a language the app does not know.
        assert_eq!(UiLanguage::from_key("pseudo"), None);
        assert_eq!(UiLanguage::from_key("pseudo-en"), None);
        assert_eq!(UiLanguage::from_key("ja-JP"), None);
        assert_eq!(UiLanguage::from_key(""), None);
        assert_eq!(UiLanguage::from_key(SYSTEM_KEY), None);
    }

    /// The variable is a developer's, so it overrides the setting and the
    /// system, and would reach a catalogue not shipped yet.
    #[test]
    fn the_environment_wins_and_reaches_unshipped_catalogues() {
        assert_eq!(
            resolve(Some("bg"), "en-GB", &os(&["de-DE"])),
            UiLanguage::Bg
        );
        assert_eq!(
            resolve(Some("de"), "en-GB", &os(&["en-GB"])),
            UiLanguage::De
        );
        // One it does not know falls through rather than failing the launch.
        assert_eq!(
            resolve(Some("klingon"), "system", &os(&["en-US"])),
            UiLanguage::EnGb
        );
    }

    /// **Every catalogue ships** — all nine are complete, and shipping them
    /// was the user's call (2026-10-21). A language added later starts off
    /// this list: drop it from the expectation here until its catalogue is
    /// complete.
    #[test]
    fn every_catalogue_ships() {
        for lang in UiLanguage::ALL {
            assert!(SHIPPED.contains(lang), "{lang:?}");
        }
        assert_eq!(SHIPPED.len(), UiLanguage::ALL.len());
        assert_eq!(SHIPPED[0], UiLanguage::EnGb, "English heads the picker");
    }

    /// **The OS reaches only shipped catalogues.** A catalogue still being
    /// written must not give a system set to its language a half-translated
    /// interface — nor may the setting.
    #[test]
    fn the_system_never_reaches_an_unshipped_catalogue() {
        let english = [UiLanguage::EnGb];
        assert_eq!(
            resolve_in(None, SYSTEM_KEY, &os(&["de-DE", "en-US"]), &english),
            UiLanguage::EnGb
        );
        assert_eq!(
            resolve_in(None, "de", &os(&["de-DE"]), &english),
            UiLanguage::EnGb,
            "nor the setting"
        );
    }

    /// With every catalogue shipped, the public `resolve` takes the system's
    /// language and the setting's.
    #[test]
    fn a_system_set_to_a_shipped_language_gets_it() {
        assert_eq!(
            resolve(None, SYSTEM_KEY, &os(&["fr-CA", "en-US"])),
            UiLanguage::Fr
        );
        assert_eq!(resolve(None, "bg", &os(&["en-GB"])), UiLanguage::Bg);
    }

    #[test]
    fn the_setting_picks_a_shipped_language_over_the_system() {
        assert_eq!(resolve(None, "en-GB", &os(&["fr-FR"])), UiLanguage::EnGb);
        // A key a newer build wrote, or a hand edit, is the system's choice.
        assert_eq!(resolve(None, "xx", &os(&["en-GB"])), UiLanguage::EnGb);
    }

    /// A settings file a pseudo-locale build wrote reads as the system's
    /// choice, and so does the variable naming one.
    #[test]
    fn a_retired_pseudo_locale_key_is_the_system_s_choice() {
        assert_eq!(resolve(None, "pseudo", &os(&["en-GB"])), UiLanguage::EnGb);
        assert_eq!(
            resolve(Some("pseudo-en"), "en-GB", &os(&["en-GB"])),
            UiLanguage::EnGb
        );
    }

    /// The first preferred language the app has wins; an unknown one is passed
    /// over rather than ending the search; none at all is English.
    #[test]
    fn the_system_is_read_in_preference_order() {
        let both = [UiLanguage::EnGb, UiLanguage::De];
        let pick = |tags: &[&str]| resolve_in(None, SYSTEM_KEY, &os(tags), &both);
        assert_eq!(pick(&["de-DE", "en-US"]), UiLanguage::De);
        assert_eq!(pick(&["en-US", "de-DE"]), UiLanguage::EnGb);
        assert_eq!(
            pick(&["ja-JP", "de-CH"]),
            UiLanguage::De,
            "an unknown one is passed over"
        );
        assert_eq!(pick(&["ja-JP"]), UiLanguage::EnGb);
        assert_eq!(
            pick(&["fr-FR"]),
            UiLanguage::EnGb,
            "nor is a known one that has not shipped"
        );
        assert_eq!(pick(&[]), UiLanguage::EnGb);
    }

    /// A shipped setting reaches its language even on an English system, and
    /// English on a German one.
    #[test]
    fn a_shipped_setting_overrides_the_system_either_way() {
        let both = [UiLanguage::EnGb, UiLanguage::De];
        assert_eq!(
            resolve_in(None, "de", &os(&["en-GB"]), &both),
            UiLanguage::De
        );
        assert_eq!(
            resolve_in(None, "en-GB", &os(&["de-DE"]), &both),
            UiLanguage::EnGb
        );
        assert_eq!(
            resolve_in(None, SYSTEM_KEY, &os(&["de-DE"]), &both),
            UiLanguage::De
        );
    }
}
