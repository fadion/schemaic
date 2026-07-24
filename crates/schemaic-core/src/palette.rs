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
//! bare `indent` alongside `indent width`). The longest-match below then can't
//! mis-resolve a name the user is still typing toward.

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

/// Whether `name` is a whole-word prefix of `s` (case-insensitive): either equal,
/// or `s` continues with a space right after `name`. Both are ASCII.
fn is_word_prefix(s_lower: &str, name_lower: &str) -> bool {
    s_lower == name_lower
        || s_lower
            .strip_prefix(name_lower)
            .is_some_and(|rest| rest.starts_with(' '))
}

/// Parse a search-box `input` given the set of argument-taking command names
/// (canonical, lowercase, e.g. `"toggle panel"`). Instant commands aren't passed —
/// they're resolved by the UI's list filtering, not here.
pub fn parse(input: &str, arg_commands: &[&str]) -> Parsed {
    let Some(rest) = input.strip_prefix('>') else {
        return Parsed::Search(input.trim().to_string());
    };
    // Allow "> history" (a space after the sigil) the same as ">history".
    let s = rest.trim_start();
    let lower = s.to_ascii_lowercase();
    // Longest argument-command whose full name is a word-prefix of the input wins
    // (defensive — the no-nesting invariant means at most one ever matches).
    let mut best: Option<&str> = None;
    for &name in arg_commands {
        if is_word_prefix(&lower, name) && best.is_none_or(|b| name.len() > b.len()) {
            best = Some(name);
        }
    }
    match best {
        // `name` matched case-insensitively over ASCII, so its byte length indexes
        // `s` (original case) at the same boundary.
        Some(name) => Parsed::Command {
            name: name.to_string(),
            arg: s[name.len()..].trim_start().to_string(),
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
        "ai",
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
            parse("> ai how do i join", ARGS),
            Parsed::Command {
                name: "ai".into(),
                arg: "how do i join".into()
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
