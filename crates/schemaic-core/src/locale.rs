//! Which language the interface is drawn in — decided once, at launch.
//!
//! The GUI is translated; nothing else is. The CLI, the MCP server and
//! everything sent to an AI harness stay English, because scripts parse the
//! first and a model reads the second (`docs/roadmap.md`, *Interface
//! translation*). So this module answers a question only the GUI asks, and
//! nothing in core renders through it.
//!
//! **Three sources, in order:** the `SCHEMAIC_LOCALE` environment variable, the
//! *Language* setting, and the operating system's preferred languages. The
//! variable is for development — it reaches a catalogue that is not shipped yet,
//! and the pseudo-locale — so it is honoured whatever it names. The setting and
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
    /// The pseudo-locale: the English catalogue with every letter accented,
    /// ~40% longer and bracketed, so one run shows every box too narrow for a
    /// longer language and every string nobody moved into the catalogue (it is
    /// the one left plain). A development tool, never offered in Settings.
    Pseudo,
}

/// The catalogues a user may choose, or be given by their OS. A language joins
/// this list when its catalogue is complete, not when it is started.
pub const SHIPPED: &[UiLanguage] = &[UiLanguage::EnGb];

/// The setting's value meaning "follow the operating system".
pub const SYSTEM_KEY: &str = "system";

impl UiLanguage {
    /// The persisted key — also what `SCHEMAIC_LOCALE` takes.
    pub fn key(self) -> &'static str {
        match self {
            UiLanguage::EnGb => "en-GB",
            UiLanguage::De => "de",
            UiLanguage::Pseudo => "pseudo",
        }
    }

    /// The catalogue's language tag — the directory its `.ftl` lives in. The
    /// pseudo-locale is the English catalogue transformed, so it has none of
    /// its own.
    pub fn catalogue(self) -> &'static str {
        match self {
            UiLanguage::EnGb | UiLanguage::Pseudo => "en-GB",
            UiLanguage::De => "de",
        }
    }

    /// The language's name **in itself** — what a picker lists it as, in every
    /// language, so a reader lost in the wrong one can still find their own.
    /// Which is why these are not in the catalogue.
    pub fn endonym(self) -> &'static str {
        match self {
            UiLanguage::EnGb => "English",
            UiLanguage::De => "Deutsch",
            UiLanguage::Pseudo => "Pseudo",
        }
    }

    /// The marks the language writes a figure with. The pseudo-locale keeps
    /// English's: it tests the words' length, and its catalogue is English's.
    pub fn number_marks(self) -> crate::text::NumberMarks {
        match self {
            UiLanguage::EnGb | UiLanguage::Pseudo => crate::text::NumberMarks::ENGLISH,
            UiLanguage::De => crate::text::NumberMarks {
                decimal: ',',
                group: '.',
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
            "pseudo" => Some(UiLanguage::Pseudo),
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
/// more than the one catalogue that ships today.
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
    fn german_swaps_both_marks_and_the_pseudo_locale_keeps_english_s() {
        use crate::text::NumberMarks;
        assert_eq!(UiLanguage::EnGb.number_marks(), NumberMarks::ENGLISH);
        assert_eq!(UiLanguage::Pseudo.number_marks(), NumberMarks::ENGLISH);
        assert_eq!(
            UiLanguage::De.number_marks(),
            NumberMarks {
                decimal: ',',
                group: '.'
            }
        );
    }

    #[test]
    fn keys_round_trip_and_tolerate_case_and_region() {
        for lang in [UiLanguage::EnGb, UiLanguage::De, UiLanguage::Pseudo] {
            assert_eq!(UiLanguage::from_key(lang.key()), Some(lang));
        }
        assert_eq!(UiLanguage::from_key("EN-gb"), Some(UiLanguage::EnGb));
        assert_eq!(UiLanguage::from_key("en-US"), Some(UiLanguage::EnGb));
        assert_eq!(UiLanguage::from_key("en_US.UTF-8"), Some(UiLanguage::EnGb));
        assert_eq!(UiLanguage::from_key("de-AT"), Some(UiLanguage::De));
        assert_eq!(UiLanguage::from_key("Pseudo"), Some(UiLanguage::Pseudo));
        assert_eq!(UiLanguage::from_key("fr-FR"), None);
        assert_eq!(UiLanguage::from_key(""), None);
        assert_eq!(UiLanguage::from_key(SYSTEM_KEY), None);
    }

    /// The variable is a developer's, so it reaches what Settings cannot: a
    /// catalogue not shipped yet, and the pseudo-locale.
    #[test]
    fn the_environment_wins_and_reaches_unshipped_catalogues() {
        let de = os(&["de-DE"]);
        assert_eq!(resolve(Some("pseudo"), "en-GB", &de), UiLanguage::Pseudo);
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

    /// **The OS reaches only shipped catalogues.** German's is being written;
    /// a German system must not get a half-German interface meanwhile.
    #[test]
    fn the_system_never_reaches_an_unshipped_catalogue() {
        assert!(
            !SHIPPED.contains(&UiLanguage::De),
            "update this test when German ships"
        );
        assert_eq!(
            resolve(None, SYSTEM_KEY, &os(&["de-DE", "en-US"])),
            UiLanguage::EnGb
        );
        assert_eq!(
            resolve(None, "de", &os(&["de-DE"])),
            UiLanguage::EnGb,
            "nor the setting"
        );
    }

    #[test]
    fn the_setting_picks_a_shipped_language_over_the_system() {
        assert_eq!(resolve(None, "en-GB", &os(&["fr-FR"])), UiLanguage::EnGb);
        // A key a newer build wrote, or a hand edit, is the system's choice.
        assert_eq!(resolve(None, "xx", &os(&["en-GB"])), UiLanguage::EnGb);
    }

    /// Never offered, so never honoured from the setting: a hand-edited
    /// settings file is not a way into a developer tool.
    #[test]
    fn the_pseudo_locale_is_the_environment_s_alone() {
        assert_eq!(resolve(None, "pseudo", &os(&["en-GB"])), UiLanguage::EnGb);
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
            pick(&["fr-FR", "de-CH"]),
            UiLanguage::De,
            "an unknown one is passed over"
        );
        assert_eq!(pick(&["fr-FR"]), UiLanguage::EnGb);
        assert_eq!(pick(&[]), UiLanguage::EnGb);
    }

    /// Once German ships, the setting reaches it even on an English system,
    /// and English on a German one.
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
