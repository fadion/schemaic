# `app/deepseek.rs`

`deepseek.rs` — the `DSH_HOME` a DeepSeek Harness run uses, and the `--patch` overlay inside it
that seals the run, signs it in, picks its model and gives it our server.
`DshHome::write(exe, Some(endpoint_file), model, effort)` writes `harness::dsh_overlay_yaml` into
`instance_root("deepseek")` as `schemaic-overlay.yml`, our server inserted; `write(exe, None, …)`
writes the same overlay with no server row; `write_inline(model, effort)` writes it, never with a
server, under `instance_root("deepseek-inline")`. `overlay()` is the file `--patch` names (carried
on `TurnSpec::mcp_config` for a turn and on `InlineSpec::overlay` for a one-shot), `env()` the
three variables the child gets — `DSH_HOME`, `DSH_PERMISSION_MODE=read-only` and
`DSH_TELEMETRY_MODE=DISABLED` — and `env_remove()` the two it must not inherit. `dump_args(overlay)`
is the `--profile headless --patch <overlay> --dump-config` that `ai::dsh_seal_refusal` runs before
every turn of a session and before every one-shot (under `app/overview.md`). `sweep` collects the
homes of instances that are gone, through `opencode::sweep_instances`, called from `main.rs` after
`copilot::sweep` on the same background thread.
**The seal is the overlay; the isolation is this directory, and they are two answers because a
patch cannot reach backwards.** A dsh run is a composition of plugins and its built-in tools are
plugins like any other, so the overlay disables every one of them by id (`harness::DSH_TOOL_ROWS`;
what that was measured to do, and the check by plugin *name* that backs a list of ids, are under
`harness.rs`). What a `--patch` cannot do is stop the layers that compose before it: the user's own
profile patch and home patch, under their `~/.dsh`, apply first and could insert a tool plugin the
overlay has never named. `DSH_HOME` pointed at a directory Schemaic owns means those layers do not
exist for the run — OpenCode's `XDG_CONFIG_HOME` and Copilot's `COPILOT_HOME` again, and like them
it edits nothing of the user's. **The model and the effort ride in the overlay too**, because
headless takes neither as a flag, which is why `write` is handed them and the file is rewritten per
session rather than once per instance.
**The sign-in is read where it lives, never copied — the opposite of Copilot's `carry_login`.**
Headless ships with the credential store and the account plugins off, and under a private home the
store would look for its file there, so the overlay switches `credentials`, `authorization` and
`deepseek-account` back on and points the store at the user's own `.credentials.yaml` by path
(`user_dsh_home`: their `DSH_HOME`, else `.dsh` under `USERPROFILE` on Windows and `HOME`
elsewhere, `the_sign_in_is_read_from_where_the_user_s_dsh_keeps_it`). A second reader of a grant
is safe only if it cannot rotate the grant from under the first, and the account grant has no
expiry or refresh flow (its plugin's README); measured, a run signed in that way left the file
untouched. With no home to be found at all the path is one inside our own root, which holds no
store, so an API-key user is unaffected and an account user meets dsh's own "no credential" error
rather than a refusal. **`DEEPSEEK_API_KEY` picks the route** (`provider`): set, the overlay names
`deepseek-official`; unset or blank, `deepseek-account`
(`an_exported_key_signs_in_by_key_and_none_by_account`).
**`env_remove()` is written to the variables**, each because it reopens something the overlay
closed. `DSH_TOOLS_MODE` switches the tool layer to "PTC mode", whose one tool is `run_code` — a
code runner the plugin denylist cannot see, since it is a mode of the `tools` row and not a plugin
of its own. `DSH_TELEMETRY_OTLP_URL` redirects the telemetry exporter; with the mode `DISABLED`
nothing is built to use it, and a shell profile's value has no business in a SQL assistant's
environment regardless. `DEEPSEEK_API_KEY` is deliberately **not** cleared, for the reason
Copilot's token variables are not: it is how a user signs in, and clearing it would log the run out
rather than isolate it. `ai::inherited_env_to_clear` chains this list after OpenCode's and
Copilot's, and every harness's child is cleared of all three.
`the_variable_that_turns_on_a_code_runner_is_cleared` holds `DSH_TOOLS_MODE` in and the key out,
and `the_home_is_the_lever_and_is_never_also_cleared` holds every variable `env()` sets out of the
list — a variable on both would be cleared after being set, or the other way round, by an order
nobody chose. The chain itself is held by `ai.rs`'s `no_seal_variable_is_also_cleared`, which
asserts `DSH_TOOLS_MODE` is in it and `DSH_HOME`, the other two variables `env()` sets and the
launch's `ELECTRON_RUN_AS_NODE` are not. Telemetry is off because by default the CLI uploads a session only after in-app
feedback a headless run cannot give, and `DISABLED` constructs no transport at all; the read-only
permission preset has no shell left to govern once the overlay has run, but it is what was
measured.
**Reused within an instance, because the session lives in it.** A turn resumes the previous one
with `--session-id`, which adopts a session from the log dsh keeps under `DSH_HOME`'s `sessions/`,
so a home per *turn* would forget every conversation after one question. The root is per instance,
from `opencode::instance_root`, for that function's reason — the overlay carries the endpoint
file's path, which differs per connection, and a second window must not re-point the first one's
next turn — and with its consequence: the directory outlives the session, so the overlay names the
per-session endpoint file by path and the credential never enters it. It is written plainly, since
it names files by path and holds no secret. `deepseek-inline` is a separate root for Copilot's
reason, so a session's overlay and a one-shot's are never one file whichever wrote it last.
`the_dump_is_of_the_same_overlay_the_run_is_given` holds `dump_args` to the path `--patch` is
handed, since a check of one file and a run of another would vouch for nothing.
**`write` returning `None` refuses the session — OpenCode's shape, not Copilot's — because here the
file that is missing is the seal**: a headless run with no overlay has every built-in tool. So
`ai::isolation_refusal` answers `ai-refused-no-deepseek-home` (through `refuse_every_turn`, like the
other non-spawning returns), and `ai::inline_isolation` refuses a one-shot whose `write_inline`
fails (`ai-inline-no-deepseek-home`). Where only the *endpoint file* could not be written the
session is not refused: the overlay is still written, with no server row, and the session says it
has no database tools — Cursor's shape.
