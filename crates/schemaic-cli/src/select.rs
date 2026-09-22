//! Which saved connection a CLI invocation means, and whether it may have it.
//!
//! **One entry point, and it gates.** [`select`] resolves the name-or-id *and*
//! applies [`Connection::cli_access`], rather than returning a connection for a
//! caller to remember to check. That is the same rule the run guard is written
//! to — a check the caller has to remember is a check one `return` can delete —
//! and it is why there is no `find` here that hands back an ungated connection.

use schemaic_core::connection::Connection;

/// Why a CLI invocation gets no connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoConnection {
    /// Nothing saved under that id or name.
    Unknown(String),
    /// The string names more than one saved connection. Refused rather than
    /// resolved by some tie-break, because the tie-break is invisible in a
    /// script and the two candidates may be a staging box and a production one.
    Ambiguous { want: String, matches: Vec<String> },
    /// Found, but the user has not granted it to the CLI.
    ///
    /// Distinct from [`NoConnection::Unknown`] on purpose: "you have not
    /// enabled this one" is an instruction, and "no such connection" sends the
    /// reader looking for a typo they will not find.
    NotExposed(String),
}

impl NoConnection {
    /// What goes to stderr.
    pub fn message(&self) -> String {
        match self {
            NoConnection::Unknown(want) => {
                format!("no saved connection named or numbered '{want}'")
            }
            NoConnection::Ambiguous { want, matches } => format!(
                "'{want}' names {} connections ({}); use the id instead",
                matches.len(),
                matches.join(", ")
            ),
            NoConnection::NotExposed(name) => format!(
                "connection '{name}' is not available to the CLI; \
                 enable CLI access for it in Schemaic's connection settings"
            ),
        }
    }
}

/// Does `want` name this connection — by id, or by name ignoring case?
///
/// Both spellings, because a human types the name and a script wants the id's
/// stability. `id` never changes and is never reissued; a name can be edited
/// out from under a script.
fn names(conn: &Connection, want: &str) -> bool {
    conn.name.eq_ignore_ascii_case(want) || want.parse::<u64>() == Ok(conn.id)
}

/// Every connection the CLI may see, in saved order.
///
/// What `schemaic list` prints. A connection the user has not exposed is not in
/// here — `list --all` is what reports those, with their status, so a human can
/// see *why* a connection they expected is unreachable.
pub fn listed(conns: &[Connection]) -> Vec<&Connection> {
    conns.iter().filter(|c| c.cli_access).collect()
}

/// The one connection `want` names, if the user has exposed it to the CLI.
///
/// Ambiguity is judged over **every** saved connection, not only the exposed
/// ones: otherwise toggling CLI access on a second connection would silently
/// change which one an existing script resolves to.
pub fn select<'a>(conns: &'a [Connection], want: &str) -> Result<&'a Connection, NoConnection> {
    let hits: Vec<&Connection> = conns.iter().filter(|c| names(c, want)).collect();
    match hits.as_slice() {
        [] => Err(NoConnection::Unknown(want.to_string())),
        [one] => {
            if one.cli_access {
                Ok(one)
            } else {
                Err(NoConnection::NotExposed(one.name.clone()))
            }
        }
        many => Err(NoConnection::Ambiguous {
            want: want.to_string(),
            matches: many.iter().map(|c| c.name.clone()).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built through serde rather than as a struct literal: every field here
    /// carries a serde default, so this helper survives the next field added to
    /// [`Connection`] instead of being one more literal to go and fix.
    fn conn(id: u64, name: &str, cli_access: bool) -> Connection {
        let mut c: Connection = serde_json::from_str(
            r#"{"id":0,"name":"","host":"h","port":3306,"user":"u","password":""}"#,
        )
        .expect("a minimal saved connection parses");
        c.id = id;
        c.name = name.to_string();
        c.cli_access = cli_access;
        c
    }

    #[test]
    fn a_connection_resolves_by_its_name() {
        let cs = [conn(1, "local", true), conn(2, "prod", true)];
        assert_eq!(select(&cs, "prod").unwrap().id, 2);
    }

    /// The id is what a script should carry, so it has to resolve even when the
    /// connection has since been renamed.
    #[test]
    fn a_connection_resolves_by_its_id() {
        let cs = [conn(1, "local", true), conn(2, "renamed later", true)];
        assert_eq!(select(&cs, "2").unwrap().id, 2);
    }

    /// Typing the name of a connection is not an exercise in capitalisation.
    #[test]
    fn a_name_matches_regardless_of_case() {
        let cs = [conn(1, "Prod Backup", true)];
        assert_eq!(select(&cs, "prod backup").unwrap().id, 1);
    }

    /// **The gate.** A connection the user has not exposed is refused even
    /// though it exists, resolves, and is the only match — and the refusal says
    /// which one, so the fix is obvious.
    #[test]
    fn an_unexposed_connection_is_refused_by_name_and_by_id() {
        let cs = [conn(7, "prod", false)];
        assert_eq!(
            select(&cs, "prod"),
            Err(NoConnection::NotExposed("prod".to_string()))
        );
        assert_eq!(
            select(&cs, "7"),
            Err(NoConnection::NotExposed("prod".to_string()))
        );
    }

    /// Absent is not the same as forbidden, and the message must not send the
    /// reader hunting for a typo when the answer is a toggle.
    #[test]
    fn a_missing_connection_and_a_forbidden_one_refuse_differently() {
        let cs = [conn(1, "prod", false)];
        assert!(
            select(&cs, "nope")
                .unwrap_err()
                .message()
                .contains("no saved connection")
        );
        assert!(
            select(&cs, "prod")
                .unwrap_err()
                .message()
                .contains("enable CLI access")
        );
    }

    /// Two connections answering to one word is refused, never tie-broken: the
    /// pair is as likely to be staging and production as it is to be duplicates.
    #[test]
    fn an_ambiguous_name_is_refused_rather_than_resolved() {
        let cs = [conn(1, "backup", true), conn(2, "backup", true)];
        let err = select(&cs, "backup").unwrap_err();
        assert!(matches!(err, NoConnection::Ambiguous { .. }));
        assert!(err.message().contains("use the id instead"));
    }

    /// **Ambiguity is judged before the gate, over every saved connection.**
    /// Judged over the exposed ones only, this pair would quietly resolve to
    /// whichever one happened to be enabled — so enabling the second connection
    /// later would silently repoint an existing script.
    #[test]
    fn ambiguity_counts_connections_the_cli_cannot_use() {
        let cs = [conn(1, "backup", true), conn(2, "backup", false)];
        assert!(matches!(
            select(&cs, "backup"),
            Err(NoConnection::Ambiguous { .. })
        ));
    }

    /// A name that happens to be a number must not be shadowed by an id.
    #[test]
    fn a_numeric_name_colliding_with_an_id_is_ambiguous_not_silently_one_of_them() {
        let cs = [conn(1, "2", true), conn(2, "two", true)];
        assert!(matches!(
            select(&cs, "2"),
            Err(NoConnection::Ambiguous { .. })
        ));
    }

    #[test]
    fn listing_shows_only_what_the_user_exposed() {
        let cs = [
            conn(1, "local", true),
            conn(2, "prod", false),
            conn(3, "staging", true),
        ];
        let names: Vec<&str> = listed(&cs).iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["local", "staging"]);
    }

    #[test]
    fn listing_an_empty_book_is_empty_rather_than_an_error() {
        assert!(listed(&[]).is_empty());
    }
}
