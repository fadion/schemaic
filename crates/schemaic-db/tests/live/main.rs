//! The live engine tier: the DB layer against real MySQL, MariaDB and
//! PostgreSQL servers.
//!
//! It exists because `schemaic-db`'s pure suite can only reach the decisions,
//! never the wire — SQLite is the one backend tested directly, and the two
//! engines that ship most were covered by hand alone. Everything here needs a
//! server, so it is off unless the `live-tests` feature is on:
//!
//! ```text
//! cargo test -p schemaic-db --features live-tests
//! cargo test -p schemaic-db --features live-tests -- --nocapture mariadb::
//! ```
//!
//! `cargo test --workspace` does not build this target at all (the manifest
//! declares the feature `required-features`), so the pure tier stays pure by
//! construction rather than by a runtime check that could be got wrong. See
//! [`endpoint`] for how servers are named, and [`scratch`] for the namespace
//! guard that keeps the tier away from any database it did not create.
//!
//! **One suite, run per server.** [`suite`] holds the assertions, and the
//! `live_suite!` macro below expands them into a module per leg, so a failure
//! reads `mysql::introspection_finds_the_seeded_table` rather than a loop that
//! stopped at the first server and never reached the other two.

mod cases;
mod endpoint;
mod scratch;
mod suite;

/// Expand each named [`suite`] function into one test per server.
///
/// A leg left out of `SCHEMAIC_IT_ENGINES` returns without asserting, and says
/// so on stderr. That is the *only* thing in this tier that does not run: a
/// missing server is a failure, because a suite that quietly passes when it
/// could not connect is worth less than no suite at all.
macro_rules! live_suite {
    ($($test:ident),+ $(,)?) => {
        live_suite!(@leg mariadb, MARIADB, $($test),+);
        live_suite!(@leg mysql, MYSQL, $($test),+);
        live_suite!(@leg pg, POSTGRES, $($test),+);
    };
    (@leg $leg:ident, $target:ident, $($test:ident),+) => {
        mod $leg {
            $(
                // Multi-threaded: the drivers spawn their connection tasks onto
                // the runtime, and the teardown guard blocks a thread of its own.
                #[tokio::test(flavor = "multi_thread")]
                async fn $test() {
                    let target = &crate::endpoint::$target;
                    if !target.enabled() {
                        eprintln!(
                            "live: {} is not in SCHEMAIC_IT_ENGINES — this test asserted nothing",
                            target.name
                        );
                        return;
                    }
                    crate::suite::$test(target).await;
                }
            )+
        }
    };
}

live_suite!(
    a_ping_reaches_the_server,
    a_seeded_table_round_trips_through_a_query,
    introspection_finds_the_seeded_table,
    a_scratch_database_is_gone_once_torn_down,
    every_type_renders_as_the_grid_shows_it,
    the_text_the_grid_shows_writes_back_unchanged,
);

/// The name guard needs no server, and is here rather than in `schemaic-core`
/// because what it protects is this binary: the one place in the workspace that
/// issues `DROP DATABASE` against a machine somebody is using.
mod name_guard {
    use crate::scratch::assert_scratch_name;

    #[test]
    fn a_generated_name_passes() {
        assert_scratch_name("schemaic_it_1234_mariadb_roundtrip");
    }

    #[test]
    #[should_panic(expected = "refusing")]
    fn a_name_without_the_prefix_is_refused() {
        assert_scratch_name("sakila");
    }

    #[test]
    #[should_panic(expected = "refusing")]
    fn a_prefix_in_the_middle_is_not_the_prefix() {
        assert_scratch_name("real_schemaic_it_data");
    }

    #[test]
    #[should_panic(expected = "refusing")]
    fn a_name_carrying_a_quote_is_refused() {
        // Belt and braces: every name is quoted before it reaches a statement,
        // so this is the second lock rather than the first.
        assert_scratch_name("schemaic_it_1`; DROP DATABASE sakila; --");
    }
}
