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
//! construction rather than by a runtime check that could be got wrong —
//! **for that spelling**. `--all-features` supplies the feature and runs the
//! whole tier; see the manifest's note on why that is left as it is. See
//! [`endpoint`] for how servers are named, and [`scratch`] for the namespace
//! guard that keeps the tier away from any database it did not create.
//!
//! **One suite, run per server.** [`suite`] and [`editable`] hold the
//! assertions, and the `live_suite!` macro below expands them into a module per
//! leg, so a failure reads `mysql::introspection_finds_the_seeded_table` rather
//! than a loop that stopped at the first server and never reached the other two.
//! The macro takes them grouped by module because the group a test belongs to is
//! the one thing its name does not say.
//!
//! **[`pg_catalog`] and [`mariadb_catalog`] are the modules outside it**, and
//! their own docs say why: each guards one engine's builtin-function catalog
//! rather than a claim about the DB layer, so there is no version of it the
//! other legs could answer — the oracle is a system view only that engine has.
//! Their tests carry their own `enabled()` check, which is what the macro would
//! otherwise have given them.

mod blob;
mod cases;
mod ddl;
mod editable;
mod endpoint;
mod mariadb_catalog;
mod namespaces;
mod pg_catalog;
mod routines;
mod runtime;
mod scratch;
mod streaming;
mod suite;
mod triggers;
mod users;
mod views;
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
                        crate::endpoint::note_skipped(target);
                        return;
                    }
                    crate::$module::$test(target).await;
                }
            )+)+
        }
    };
}

live_suite!(
    blob: [
        a_blob_reads_back_byte_for_byte,
        a_blob_fetch_lands_on_the_row_its_key_names,
        a_null_blob_reports_nothing,
        an_empty_blob_is_a_value_not_a_null,
        a_binary_cell_in_a_real_result_resolves_and_fetches,
        a_stored_png_still_sniffs_as_one_after_the_round_trip,
        staged_bytes_reach_the_column_as_bytes,
    ],
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
        a_join_with_one_unkeyed_side_still_offers_no_insert_target,
        a_primary_key_becomes_the_write_key,
        a_write_built_from_the_resolved_key_lands_on_that_row,
        a_composite_key_names_one_row_and_writes_only_it,
        a_not_null_unique_index_is_the_fallback_key,
        a_nullable_unique_index_is_no_key_at_all,
        a_table_with_no_key_is_read_only,
        a_key_left_out_of_the_select_makes_the_result_read_only,
        the_same_column_twice_refuses_the_whole_table,
        a_binary_column_is_read_only_inside_an_editable_row,
        one_table_offers_itself_as_the_insert_target,
        an_include_column_is_not_part_of_the_write_key,
    ],
    routines: [
        a_pg_redefinition_keeps_a_functions_planner_attributes,
        a_mariadb_sequence_is_not_read_as_a_base_table,
    ],
    ddl: [
        an_introspected_table_diffs_to_nothing_against_its_own_draft,
        an_added_column_lands_and_reads_back_as_drafted,
        a_reordered_column_lands_where_it_was_put,
        a_dropped_column_goes_and_the_rest_stays,
        a_renamed_column_keeps_its_data,
        a_renamed_column_keeps_its_indexs_kind,
        a_retyped_column_reads_back_as_the_new_type,
        a_refused_plan_says_where_it_stopped,
        a_partly_read_index_says_so_and_is_emitted_whole,
        a_switched_off_index_is_not_silently_brought_back,
        a_functional_index_does_not_stop_the_schema_being_read,
        clearing_a_generated_expression_keeps_the_column_values,
        a_column_inserted_in_the_middle_lands_there,
        an_added_index_lands_as_the_index_drafted,
        a_dropped_foreign_key_goes_and_the_column_stays,
        an_added_check_is_enforced_by_the_server,
        a_renamed_table_keeps_its_rows_and_its_keys,
        a_table_comment_lands_and_reads_back,
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
        a_typo_in_a_manual_transaction_names_what_the_server_refused,
        a_cancelled_query_stops_at_the_server,
        a_cancelled_script_stops_at_the_server_and_reports_what_ran,
        a_cancelled_import_rolls_back_and_says_so,
        a_cancelled_import_on_a_non_transactional_table_says_the_rows_remain,
        a_refused_write_says_which_value_the_server_refused,
    ],
    streaming: [
        a_streamed_export_delivers_every_row,
        a_cancelled_export_tells_the_writer_it_stopped,
        a_statement_with_no_rows_to_export_is_refused,
        a_failed_export_reports_the_failure_to_the_writer,
    ],
    namespaces: [
        same_named_tables_in_two_namespaces_stay_distinct,
        a_result_names_the_namespace_it_read_from,
        an_edit_lands_in_the_namespace_it_was_read_from,
        a_sequence_cannot_be_owned_across_namespaces,
        a_join_across_namespaces_stays_two_tables,
        generated_ddl_lands_in_the_namespace_it_was_drafted_from,
    ],
    views: [
        an_introspected_view_diffs_to_nothing_against_its_own_draft,
        an_edited_view_body_lands_and_settles,
        a_view_that_drops_a_column_takes_the_destructive_arm_where_it_must,
        a_recreated_view_keeps_the_triggers_the_drop_took,
        a_renamed_view_lands_under_the_new_name,
        a_view_is_introspected_as_a_view,
        a_view_is_never_writable_through_a_key_that_does_not_identify_a_row,
    ],
    triggers: [
        an_introspected_trigger_diffs_to_nothing_against_its_own_draft,
        an_added_trigger_lands_and_fires,
        a_dropped_trigger_stops_firing,
        a_renamed_trigger_still_fires,
        one_of_two_triggers_can_be_dropped_without_the_other,
    ],
    writeback: [
        a_spliced_row_is_the_row_a_fresh_select_would_show,
        a_staged_update_writes_exactly_the_row_it_names,
        an_update_to_an_unchanged_value_still_counts_as_one_row,
        a_staged_insert_lands_with_defaults_for_what_it_omits,
        a_staged_delete_removes_exactly_its_row,
        a_staged_null_is_written_as_a_null,
        deletes_run_before_inserts_so_a_unique_key_can_be_reused,
        a_key_that_matches_no_row_fails_the_batch_and_undoes_the_rest,
        a_key_that_matches_two_rows_fails_the_batch_and_undoes_the_rest,
        a_failed_batch_says_what_the_rollback_actually_undid,
        a_cancelled_commit_on_a_non_transactional_table_says_the_rows_remain,
        a_refused_write_in_a_transaction_undoes_only_itself,
        an_empty_batch_writes_nothing,
    ],
    users: [
        the_account_we_connected_as_is_in_the_list,
        an_accounts_grants_come_back_as_grant_statements,
        a_grant_list_says_which_database_it_covers_when_it_covers_only_one,
        a_grant_list_with_no_database_says_it_is_covering_none,
        no_password_material_survives_the_fetch,
        a_created_account_is_one_the_server_then_lists,
        an_account_created_at_a_host_is_listed_and_dropped_at_it,
        a_created_account_can_log_in_with_the_password_it_was_given,
        a_reset_password_replaces_the_one_the_account_had,
        a_role_the_server_made_is_never_offered_a_password_reset,
        a_created_role_is_one_the_server_accepts,
        a_granted_privilege_comes_back_and_a_revoke_takes_it_off,
        a_grant_at_every_level_reads_back_naming_that_object,
        a_granted_role_comes_back_and_a_revoke_takes_it_off,
        a_dropped_account_is_gone_from_the_list,
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

/// The skip notice is the whole mitigation for the one silent-green exception
/// this tier allows, so its *spelling* is load-bearing.
///
/// libtest captures a test's `print!`/`eprint!` and prints it only for failing
/// tests — and a skipped leg is a passing one, so the notice was swallowed for
/// years of runs. `endpoint::note_skipped` writes to the locked handle instead,
/// which is past the macro's capture-aware path.
///
/// A source assertion because it has to be: libtest's capture is a property of
/// the harness, not something a `#[test]` can observe about its own run. Same
/// argument every source gate in this workspace makes.
mod skip_notice {
    #[test]
    fn no_skip_notice_goes_through_the_captured_macro() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("live");
        // **The whole tier, not two names.** The corpus was the literal list
        // `["main.rs", "endpoint.rs"]` — the two files that already answered
        // correctly — while `routines.rs` held two live violations the day the
        // gate landed. A gate whose corpus is narrower than the rule it states
        // is the shape this tier keeps producing; the fix is to read the
        // directory rather than a list somebody has to remember to extend.
        //
        // **Two exemptions, both named.** `scratch.rs` and `users.rs` report a
        // *leak* — scratch state the teardown could not remove — on a path that
        // is about to fail the test anyway, so libtest will print it. Anything
        // else is a notice on a passing test, which is the silent green.
        let exempt = ["scratch.rs", "users.rs"];
        let mut seen = 0usize;
        for entry in std::fs::read_dir(&dir).expect("the live tier's directory") {
            let path = entry.expect("a directory entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let file = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a file name")
                .to_string();
            seen += 1;
            if exempt.contains(&file.as_str()) {
                continue;
            }
            let src =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {file}: {e}"));
            let code: String = src
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            // Split so this assertion is not itself a match.
            let macro_call = format!("eprint{}!(", "ln");
            assert!(
                !code.contains(&macro_call),
                "{file} reports through the macro libtest hides for passing \
                 tests; a notice about a leg that asserted nothing must go to \
                 the locked stderr handle (`endpoint::note_skipped` or \
                 `endpoint::note_no_op`), or it becomes a silent green"
            );
        }
        // The floor that notices the corpus going empty — a `read_dir` that
        // matched nothing is a gate that passes on everything.
        assert!(
            seen >= 10,
            "only {seen} files in the live tier were scanned; this gate has \
             stopped seeing the directory it is written about"
        );
    }
}
