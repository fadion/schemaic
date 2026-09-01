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
//! **One suite, run per server.** [`suite`] and [`editable`] hold the
//! assertions, and the `live_suite!` macro below expands them into a module per
//! leg, so a failure reads `mysql::introspection_finds_the_seeded_table` rather
//! than a loop that stopped at the first server and never reached the other two.
//! The macro takes them grouped by module because the group a test belongs to is
//! the one thing its name does not say.

mod cases;
mod ddl;
mod editable;
mod endpoint;
mod runtime;
mod scratch;
mod suite;
mod writeback;

/// Expand each named [`suite`] function into one test per server.
///
/// A leg left out of `SCHEMAIC_IT_ENGINES` returns without asserting, and says
/// so on stderr. That is the *only* thing in this tier that does not run: a
/// missing server is a failure, because a suite that quietly passes when it
/// could not connect is worth less than no suite at all.
macro_rules! live_suite {
    ($($module:ident: [$($test:ident),+ $(,)?]),+ $(,)?) => {
        live_suite!(@leg mariadb, MARIADB, $($module: [$($test),+]),+);
        live_suite!(@leg mysql, MYSQL, $($module: [$($test),+]),+);
        live_suite!(@leg pg, POSTGRES, $($module: [$($test),+]),+);
    };
    (@leg $leg:ident, $target:ident, $($module:ident: [$($test:ident),+]),+) => {
        mod $leg {
            $($(
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
                    crate::$module::$test(target).await;
                }
            )+)+
        }
    };
}

live_suite!(
    suite: [
        a_ping_reaches_the_server,
        a_seeded_table_round_trips_through_a_query,
        introspection_finds_the_seeded_table,
        a_scratch_database_is_gone_once_torn_down,
        every_type_renders_as_the_grid_shows_it,
        the_text_the_grid_shows_writes_back_unchanged,
    ],
    editable: [
        a_select_star_carries_each_columns_provenance,
        an_alias_does_not_hide_the_real_column,
        an_expression_column_has_no_provenance,
        a_join_attributes_each_column_to_its_own_table,
        a_primary_key_becomes_the_write_key,
        a_not_null_unique_index_is_the_fallback_key,
        a_nullable_unique_index_is_no_key_at_all,
        a_table_with_no_key_is_read_only,
        a_key_left_out_of_the_select_makes_the_result_read_only,
        the_same_column_twice_refuses_the_whole_table,
        a_binary_column_is_read_only_inside_an_editable_row,
        one_table_offers_itself_as_the_insert_target,
    ],
    ddl: [
        an_introspected_table_diffs_to_nothing_against_its_own_draft,
        an_added_column_lands_and_reads_back_as_drafted,
        a_dropped_column_goes_and_the_rest_stays,
        a_renamed_column_keeps_its_data,
        a_retyped_column_reads_back_as_the_new_type,
        a_refused_plan_says_where_it_stopped,
    ],
    runtime: [
        a_script_runs_every_statement_in_order,
        a_script_holds_one_connection_so_session_state_carries,
        a_refused_statement_stops_the_run_and_names_its_line,
        an_empty_script_finishes_having_run_nothing,
        an_import_loads_every_row,
        a_reader_error_rolls_the_whole_import_back,
        a_refused_row_rolls_the_whole_import_back,
        a_manual_transaction_is_invisible_until_it_commits,
        a_rolled_back_manual_transaction_leaves_nothing,
        a_cancelled_query_stops_at_the_server,
    ],
    writeback: [
        a_staged_update_writes_exactly_the_row_it_names,
        an_update_to_an_unchanged_value_still_counts_as_one_row,
        a_staged_insert_lands_with_defaults_for_what_it_omits,
        a_staged_delete_removes_exactly_its_row,
        a_staged_null_is_written_as_a_null,
        deletes_run_before_inserts_so_a_unique_key_can_be_reused,
        a_key_that_matches_no_row_fails_the_batch_and_undoes_the_rest,
        a_key_that_matches_two_rows_fails_the_batch_and_undoes_the_rest,
        a_failed_batch_says_what_the_rollback_actually_undid,
        an_empty_batch_writes_nothing,
    ],
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
