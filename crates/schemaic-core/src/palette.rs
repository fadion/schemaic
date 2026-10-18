//! Command-palette query parsing for the global search box.
//!
//! The search box has two modes. Without a leading `>` it's the ordinary
//! table/column search ([`Parsed::Search`]). With a `>` it's command mode: the
//! text after the sigil either still *filters* the command list
//! ([`Parsed::Filter`]) or has resolved to an argument-taking command plus its
//! argument ([`Parsed::Command`]).
//!
//! The tricky bit — deciding when typing stops filtering and becomes the
//! argument — lives here, pure and tested. The rule: on the space after a
//! recognised argument-command's full name, the remainder is that command's
//! argument. Instant (no-argument) commands are never resolved here; they stay in
//! filter mode and the UI runs the highlighted one on Enter.
//!
//! Invariant the caller must uphold: **no argument-command name may be a
//! word-prefix of another** (e.g. `indent style` and `indent width`, but never a
//! bare `indent` alongside `indent width`) — across every name passed, which is
//! a command's translated label and its English name together. The
//! longest-match below then can't mis-resolve a name the user is still typing
//! toward.

/// The parse of a search-box query. `Command`/`Filter` only arise in command mode
/// (a leading `>`); `Search` is the default table search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    /// No `>` prefix — the ordinary table/column search with this (trimmed) query.
    Search(String),
    /// Command mode, still choosing a command: filter the command list by this
    /// text (may be empty — `>` alone lists everything).
    Filter(String),
    /// Command mode resolved to an argument-command (`name`) plus its argument
    /// text (`arg`, may be empty when the name was just completed).
    Command { name: String, arg: String },
}

/// What follows `name` in `s` when `name` is a whole-word prefix of it,
/// case-insensitively: `Some("")` when they are equal, `Some(rest)` when `s`
/// continues with a space, `None` otherwise.
///
/// Compared a character at a time and cut at `s`'s own byte offset, because a
/// name is a translated label (`größe ändern`) and the input is whatever case
/// Tab or the user left it in. Lowercasing the whole input first only works for
/// ASCII — `to_ascii_lowercase` leaves `Ä` alone, and a full `to_lowercase` can
/// change a string's byte length, so its offsets no longer index `s`.
fn after_word_prefix<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let mut chars = s.char_indices();
    for n in name.chars() {
        let (_, c) = chars.next()?;
        if !c.to_lowercase().eq(n.to_lowercase()) {
            return None;
        }
    }
    let rest = chars.next().map_or("", |(at, _)| &s[at..]);
    (rest.is_empty() || rest.starts_with(' ')).then_some(rest)
}

/// Parse a search-box `input` given the set of argument-taking command names
/// (lowercase, e.g. `"toggle panel"`). A command may be passed under more than
/// one name — its label in the interface's language and its English one — and
/// [`Parsed::Command`] carries the name that matched. Instant commands aren't
/// passed — they're resolved by the UI's list filtering, not here.
pub fn parse(input: &str, arg_commands: &[&str]) -> Parsed {
    let Some(rest) = input.strip_prefix('>') else {
        return Parsed::Search(input.trim().to_string());
    };
    // Allow "> history" (a space after the sigil) the same as ">history".
    let s = rest.trim_start();
    // Longest argument-command whose full name is a word-prefix of the input wins
    // (defensive — the no-nesting invariant means at most one ever matches).
    let mut best: Option<(&str, &str)> = None;
    for &name in arg_commands {
        if let Some(arg) = after_word_prefix(s, name)
            && best.is_none_or(|(b, _)| name.chars().count() > b.chars().count())
        {
            best = Some((name, arg));
        }
    }
    match best {
        Some((name, arg)) => Parsed::Command {
            name: name.to_string(),
            arg: arg.trim_start().to_string(),
        },
        None => Parsed::Filter(s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A representative slice, including the deliberately non-nesting
    // `indent style` / `indent width` pair.
    const ARGS: &[&str] = &[
        "toggle panel",
        "go to line",
        "history",
        "ask ai",
        "terminal",
        "ui theme",
        "editor theme",
        "font size",
        "indent style",
        "indent width",
        "switch connection",
    ];

    #[test]
    fn no_sigil_is_table_search() {
        assert_eq!(parse("employees", ARGS), Parsed::Search("employees".into()));
        // Trimmed.
        assert_eq!(parse("  emp  ", ARGS), Parsed::Search("emp".into()));
    }

    #[test]
    fn bare_sigil_filters_everything() {
        assert_eq!(parse(">", ARGS), Parsed::Filter(String::new()));
    }

    #[test]
    fn partial_command_name_still_filters() {
        assert_eq!(parse(">tog", ARGS), Parsed::Filter("tog".into()));
        // A shared first word of two commands stays in filter mode.
        assert_eq!(parse(">toggle", ARGS), Parsed::Filter("toggle".into()));
    }

    #[test]
    fn exact_command_name_resolves_with_empty_arg() {
        assert_eq!(
            parse(">toggle panel", ARGS),
            Parsed::Command {
                name: "toggle panel".into(),
                arg: String::new()
            }
        );
    }

    #[test]
    fn command_with_argument() {
        assert_eq!(
            parse(">toggle panel ai", ARGS),
            Parsed::Command {
                name: "toggle panel".into(),
                arg: "ai".into()
            }
        );
        assert_eq!(
            parse(">history sales", ARGS),
            Parsed::Command {
                name: "history".into(),
                arg: "sales".into()
            }
        );
    }

    #[test]
    fn nested_names_resolve_to_the_right_one() {
        assert_eq!(
            parse(">indent width 4", ARGS),
            Parsed::Command {
                name: "indent width".into(),
                arg: "4".into()
            }
        );
        assert_eq!(
            parse(">indent style tabs", ARGS),
            Parsed::Command {
                name: "indent style".into(),
                arg: "tabs".into()
            }
        );
        // The shared "indent" word alone is not a command → keeps filtering, so a
        // user can still reach either sibling.
        assert_eq!(parse(">indent", ARGS), Parsed::Filter("indent".into()));
    }

    #[test]
    fn case_insensitive_and_arg_trimmed() {
        assert_eq!(
            parse(">HISTORY   Sales", ARGS),
            Parsed::Command {
                name: "history".into(),
                arg: "Sales".into()
            }
        );
    }

    #[test]
    fn space_after_sigil_is_allowed() {
        assert_eq!(
            parse("> ask ai how do i join", ARGS),
            Parsed::Command {
                name: "ask ai".into(),
                arg: "how do i join".into()
            }
        );
    }

    /// Tab completes the command's *label*, so what lands in the box is
    /// title-cased ("Ask AI", "Editor Theme") and must resolve to the same
    /// command as the lower-case name — including the argument's own case, which
    /// is a prompt or a shell command and must survive verbatim.
    #[test]
    fn a_completed_label_resolves_to_its_command() {
        assert_eq!(
            parse(">Ask AI Why is this JOIN slow?", ARGS),
            Parsed::Command {
                name: "ask ai".into(),
                arg: "Why is this JOIN slow?".into()
            }
        );
        assert_eq!(
            parse(">Editor Theme One Dark Pro", ARGS),
            Parsed::Command {
                name: "editor theme".into(),
                arg: "One Dark Pro".into()
            }
        );
    }

    #[test]
    fn instant_command_text_stays_in_filter() {
        // "run" isn't an argument-command, so it never captures a following word;
        // the UI resolves/executes it from the filtered list instead.
        assert_eq!(parse(">run", ARGS), Parsed::Filter("run".into()));
        assert_eq!(
            parse(">clear history", ARGS),
            Parsed::Filter("clear history".into())
        );
    }

    /// **A translated name matches whatever its case**, umlauts included: a
    /// German label Tab-completes as `Größe Ändern`, and the parser compared
    /// ASCII-lowercased input with the name, which leaves `Ä` as it is.
    #[test]
    fn a_name_beyond_ascii_matches_case_insensitively() {
        let args = &["größe ändern", "verlauf"];
        assert_eq!(
            parse(">Größe Ändern 3", args),
            Parsed::Command {
                name: "größe ändern".into(),
                arg: "3".into()
            }
        );
        assert_eq!(
            parse(">VERLAUF Umsätze", args),
            Parsed::Command {
                name: "verlauf".into(),
                arg: "Umsätze".into()
            }
        );
        // Still a whole word: `größe änderung` is not `größe ändern`.
        assert_eq!(
            parse(">größe änderung", args),
            Parsed::Filter("größe änderung".into())
        );
    }

    #[test]
    fn go_to_line_takes_a_number() {
        assert_eq!(
            parse(">go to line 42", ARGS),
            Parsed::Command {
                name: "go to line".into(),
                arg: "42".into()
            }
        );
    }
}
