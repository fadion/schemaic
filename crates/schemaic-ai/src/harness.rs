//! Which agent CLI Schemaic is driving, what it can do, and how it is constrained.
//!
//! **Ask a capability, never a harness.** The predicates below —
//! [`Harness::supports_effort`], [`supports_resume`](Harness::supports_resume),
//! [`streams_deltas`](Harness::streams_deltas) — exist so callers stop at "does
//! this harness do X" instead of `== Harness::Claude`, which compiles cleanly
//! while sorting a fourth CLI onto whichever side it happens to fall.
//!
//! # The constraint is the point
//!
//! `docs/architecture.md` states the seal as *the session gets exactly the tools
//! Schemaic hands it, and nothing else*. That was one CLI's three flags. It is
//! now a question each harness answers its own way, and — this is the part that
//! is easy to wish away — **they do not answer it equally well**:
//!
//! - `claude` accepts `--tools ""`, which empties the *built-in* set outright.
//!   Nothing but the allow-listed MCP tools remains. This is a true seal.
//! - `codex` has no such flag. Its lever is a sandbox — `read-only` stops writes
//!   and command side effects, but the model can still *read* the filesystem it
//!   was launched in. Note the lever is the **config key**
//!   (`-c sandbox_mode="read-only"`), not only the `--sandbox` flag: the flag is
//!   absent from `codex exec resume`, so a flag-only constraint would either
//!   kill every resumed turn on an unknown option or leave it unconstrained.
//! - `antigravity` is `--sandbox`, and a measured session lists 56 built-in
//!   tools with it set — `run_command` and `write_to_file` among them. Its
//!   filesystem readers are auto-approved in headless mode: a measured turn ran
//!   `list_dir` and `view_file` unprompted.
//!
//! **Codex needs a per-tool approval too, and for the same reason Antigravity
//! does.** `codex exec` runs with approval policy `never` — there is nobody to
//! prompt — and an MCP tool with no standing approval is refused outright
//! (*"MCP tool call requires approval, but approval policy is never"*, measured).
//! The lever is `mcp_servers.schemaic.tools.<name>.approval_mode = "approve"`,
//! set per tool by [`codex_mcp_overrides`] from the connection's own allow-list,
//! so a schema-only connection never approves `run_query`. Unlike Antigravity's,
//! this one costs no global state: it rides in the same `-c` override as the
//! server itself and vanishes with the process.
//!
//! **Antigravity additionally needs a standing allow-rule, or its tools are
//! refused.** Headless mode cannot prompt, so a tool with no rule is auto-denied
//! (*"user denied permission for mcp(schemaic/list_schema)"*) and the turn still
//! reports `"status":"SUCCESS"` with an empty response. The rule is per-tool —
//! `permissions.allow: ["mcp(schemaic/list_schema)", …]` in its `settings.json`
//! — so the four Schemaic tools can be allowed **without** granting
//! `run_command`; the alternative it suggests, `--dangerously-skip-permissions`,
//! approves everything and must never be passed. Measured end to end: with the
//! four rules in place the same prompt returned real rows. The app layer must
//! therefore manage *two* pieces of that CLI's global state — the MCP server
//! registration and this allow-list — and remove both when the session ends.
//!
//! **And Antigravity has no MCP isolation at all, which is the asymmetry this
//! list would otherwise omit.** Claude gets `--strict-mcp-config` and Codex has
//! the whole `mcp_servers` table assigned out from under it
//! ([`codex_mcp_overrides`], and [`codex_isolation_only`] when even our own
//! server cannot be configured) — both of which *displace* whatever servers the
//! user has registered globally. `agy` has no counterpart: `agy mcp add` appends
//! to the user's own MCP config, so an Antigravity session sees every server
//! they have registered alongside ours. The per-tool `permissions.allow` rules
//! cover only the four `mcp(schemaic/…)` names, so a user whose own settings
//! already allow their own servers' tools has those live inside Schemaic's SQL
//! assistant. No flag was found that closes this, which is why it is written
//! down rather than fixed: an undocumented gap is the one that gets assumed
//! shut.
//!
//! So [`Constraint`] is graded rather than boolean, and the grade is shown to the
//! user rather than averaged away. Reporting "sealed" for all three would be the
//! comfortable lie: it is exactly the shape of the bug the denylist era already
//! shipped once, where the guard looked total and left nineteen tools live.
//!
//! **An unestablished constraint is refused, not downgraded.** If the probe
//! cannot tell us a harness accepts its constraining flags, the answer is
//! [`Constraint::Unknown`] and the caller does not spawn. The tempting direction
//! — assume it is fine and carry on — is how a read-only promise becomes a
//! session with a shell.

/// Which agent CLI is producing the stream.
///
/// **A dialect, not a vendor.** The variants name the wire format a binary
/// speaks, which is what decoding needs to know; who wrote it, what it costs and
/// which models it serves are all questions this type deliberately cannot
/// answer. A fork that still speaks its parent's JSONL is that parent here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Harness {
    /// `claude` — bidirectional `stream-json` over a persistent stdin/stdout pair.
    Claude,
    /// `codex exec --json` — thread/turn/item events, one process per turn.
    Codex,
    /// `agy -p --output-format stream-json` — `event`-tagged init/step_update/
    /// result, one process per turn.
    ///
    /// It also advertises a bidirectional mode (`--input-format stream-json`
    /// "reads one NDJSON message per line from stdin and runs a turn for each"),
    /// which would make it persistent like Claude. That mode is **not** what we
    /// drive, because it is not what was measured: the per-turn `-p` path is.
    Antigravity,
}

impl Harness {
    /// Every harness, in the order the settings UI offers them.
    pub const ALL: [Harness; 3] = [Harness::Claude, Harness::Codex, Harness::Antigravity];

    /// The stable string form, matched back by [`Harness::from_key`].
    ///
    /// This is what `UiState::ai_harness` stores. Nothing in the UI *sets* it
    /// yet — hand-editing `ui_state.json` is the only way to choose a harness
    /// today — but the app reads it at startup and writes it back on save, so a
    /// key that changes here silently retargets everyone's settings file.
    pub fn key(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Antigravity => "antigravity",
        }
    }

    /// Display name for the settings dropdown, and for every notice that has to
    /// name the CLI it is talking about.
    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Codex => "Codex",
            Harness::Antigravity => "Antigravity",
        }
    }

    /// The executable looked for on `PATH` when no override path is set.
    pub fn bin(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            // Not "antigravity" — the binary is `agy`.
            Harness::Antigravity => "agy",
        }
    }

    /// Parse a persisted key. An unknown one is **not** silently coerced to a
    /// working harness: `None` leaves that decision with the caller, which can
    /// say so. Deciding it here would substitute a different CLI than the one
    /// the settings file names, with nothing on screen to reveal it — the same
    /// failure `AiModel::from_cli` shipped, where any unrecognised model string
    /// quietly ran Haiku.
    pub fn from_key(s: &str) -> Option<Harness> {
        Harness::ALL.into_iter().find(|h| h.key() == s)
    }
}

/// How well a harness's built-in tools are shut off for this session.
///
/// Ordered worst-to-best deliberately: a caller comparing grades gets the
/// intuitive direction, and [`Constraint::is_runnable`] names the floor rather
/// than leaving each call site to invent one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Constraint {
    /// We could not establish that the binary accepts its constraining flags.
    /// **Do not spawn.** See the module docs for why this is not a warning.
    Unknown,
    /// Side effects are blocked (no writes, no commands), but the harness keeps
    /// built-in tools that can *read* the machine it runs on.
    Restricted,
    /// The built-in tool set is empty. Only the MCP tools we allow-list remain.
    Sealed,
}

impl Constraint {
    /// Whether a session may be started at all under this grade.
    pub fn is_runnable(self) -> bool {
        self > Constraint::Unknown
    }

    /// One line for the AI panel, stating what the user is actually getting.
    ///
    /// Returns `None` for [`Constraint::Sealed`]: the strongest grade is the one
    /// the app has always silently provided, and a banner on every Claude
    /// session would train the user to ignore the two that matter.
    pub fn notice(self, h: Harness) -> Option<String> {
        match self {
            Constraint::Sealed => None,
            // **The grade is shared; the lever behind it is not, and the notice
            // must describe the lever.** For Codex and Antigravity `Restricted`
            // is a *sandbox* — side effects really are blocked, and "cannot write
            // files or run commands" is a claim the OS is enforcing. For Claude
            // it means only that `--tools` was absent from `--help`, so the seal
            // fell back to `DISALLOWED_TOOLS`: a denylist, which the module docs
            // above open by describing as the guard that looked total and left
            // nineteen built-ins live — `Artifact`, `SendMessage`, `Workflow`,
            // `CronCreate` among them, several of which executed with no
            // permission request at all. Printing the sandbox sentence there
            // would be the comfortable lie one grade down, and shown to the user
            // as a positive assurance.
            //
            // Asked per harness rather than per capability because this *is* a
            // harness question: two different mechanisms reach one grade, and the
            // difference between them is the whole content of the sentence.
            Constraint::Restricted if h == Harness::Claude => Some(format!(
                "This {} build does not accept the flag that empties its built-in \
                 tools, so they are held back by a denylist instead — weaker, and \
                 not something Schemaic can guarantee. Updating the CLI restores \
                 the full seal.",
                h.label()
            )),
            Constraint::Restricted => Some(format!(
                "{} runs read-only: it cannot write files or run commands, but its \
                 built-in tools can still read this machine.",
                h.label()
            )),
            Constraint::Unknown => Some(format!(
                "Schemaic could not confirm that this {} binary accepts the flags that \
                 restrict it, so the assistant is disabled. Check the path in \
                 Settings → AI.",
                h.label()
            )),
        }
    }
}

impl Harness {
    /// Does `--model` take an arbitrary id on this harness?
    ///
    /// True everywhere today, and still a predicate: it is the question the
    /// settings field asks, and a harness that pins its own model would answer
    /// it differently without any call site changing shape.
    pub fn supports_model_choice(self) -> bool {
        matches!(
            self,
            Harness::Claude | Harness::Codex | Harness::Antigravity
        )
    }

    /// Model ids worth offering as one-click suggestions for this harness.
    ///
    /// **Suggestions, not the permitted set.** The model field takes any string
    /// the CLI accepts, because the alternative is what this app shipped for a
    /// year: a closed three-variant enum, so a model released after the build
    /// could not be selected at all, and a settings file naming one silently ran
    /// Haiku instead. These are the aliases each CLI documents as stable, which
    /// is what a menu should contain; anything dated, private, or newer than
    /// this build is typed in and passed through untouched.
    ///
    /// An empty list is meaningful — it means "we have nothing useful to
    /// suggest, let them type" — so callers must render the field regardless.
    pub fn suggested_models(self) -> &'static [&'static str] {
        match self {
            Harness::Claude => &["haiku", "sonnet", "opus", "opusplan"],
            Harness::Codex => &["gpt-5.4", "gpt-5.4-codex", "o3"],
            Harness::Antigravity => &[],
        }
    }

    /// Does the harness take a reasoning-effort level?
    ///
    /// Claude's `--effort` and Antigravity's. The settings row hides itself for
    /// the rest rather than showing a control that silently does nothing.
    pub fn supports_effort(self) -> bool {
        !self.effort_levels().is_empty()
    }

    /// The effort levels this harness's flag actually accepts.
    ///
    /// **The vocabularies differ, and the wider one is Claude's.** Claude takes
    /// a fourth level, `xhigh`; Antigravity's `--effort` is documented
    /// `low|medium|high`. Offering the union would hand `agy` an `xhigh` it
    /// never advertised — the same shape as a closed model list, one CLI's
    /// vocabulary applied to another, which is the bug this whole change exists
    /// to stop repeating.
    ///
    /// Empty means the harness has no such flag, which is what
    /// [`Harness::supports_effort`] *computes* its answer from rather than
    /// restating as a second list that could disagree with this one.
    pub fn effort_levels(self) -> &'static [&'static str] {
        match self {
            Harness::Claude => &["low", "medium", "high", "xhigh"],
            Harness::Antigravity => &["low", "medium", "high"],
            Harness::Codex => &[],
        }
    }

    /// The effort level to actually send, given what the user has selected.
    ///
    /// **`supports_effort` is not enough, and the gap was live.** The app asked
    /// the boolean and then passed the selected level through unchanged, so
    /// picking Claude's `xhigh` and switching to Antigravity — which keeps the
    /// setting, because only the path and the model are cleared on a switch —
    /// sent `agy --effort xhigh`, a level that flag never advertised. That is the
    /// same shape as a closed model list: one CLI's vocabulary applied to
    /// another.
    ///
    /// `None` means send no flag: either the harness has none, or the level it
    /// was handed is not one this harness's own list contains. Returning a
    /// `&'static str` **from that list** rather than echoing the caller's string
    /// is deliberate — the value cannot be anything the harness did not
    /// advertise, whatever is passed in.
    pub fn effort_arg(self, requested: &str) -> Option<&'static str> {
        self.effort_levels()
            .iter()
            .copied()
            .find(|l| *l == requested)
    }

    /// Does a turn stream *incremental* text?
    ///
    /// Claude sends deltas, and so does Antigravity (`step_update.text_delta`).
    /// Codex restates a message cumulatively, which is what
    /// [`crate::stream::StreamParser`] coalesces; the panel uses this only to
    /// decide whether a first token means "it has started".
    pub fn streams_deltas(self) -> bool {
        matches!(self, Harness::Claude | Harness::Antigravity)
    }

    /// Does one process serve the whole conversation?
    ///
    /// Claude holds a bidirectional `stream-json` pipe, so a turn is a line on
    /// stdin. The others exit after each turn and are resumed by id, which is
    /// why [`crate::StreamEvent::SessionStarted`] exists.
    pub fn is_persistent(self) -> bool {
        matches!(self, Harness::Claude)
    }

    /// Can a previous turn be continued by id?
    pub fn supports_resume(self) -> bool {
        !self.is_persistent()
    }
}

/// Read a non-Claude harness's constraint out of its `--help`.
///
/// Same failure direction as [`crate::seal_from_help`], for the same reason: an
/// unreadable probe is not evidence of absence. But the *answer* differs — there
/// is no safe "pass every flag" fallback here, because passing `--sandbox` to a
/// binary that has never heard of it kills the spawn. So an unreadable probe
/// yields [`Constraint::Unknown`] and the session is refused, which is the
/// conservative direction when the flag cannot be assumed either way.
pub fn constraint_from_help(h: Harness, help: &str) -> Constraint {
    if !crate::looks_like_help(help) {
        return Constraint::Unknown;
    }
    match h {
        Harness::Claude => {
            if crate::mentions_flag(help, "--tools") {
                Constraint::Sealed
            } else {
                Constraint::Restricted
            }
        }
        Harness::Codex => {
            if crate::mentions_flag(help, "--sandbox") {
                Constraint::Restricted
            } else {
                Constraint::Unknown
            }
        }
        // `--sandbox` is "Run in a sandbox with terminal restrictions enabled".
        // Never `Sealed`: a measured print-mode session lists 56 built-in tools
        // — `run_command`, `write_to_file`, `execute_browser_javascript` among
        // them — *with* `--sandbox` passed, gated only by a permission mode.
        // There is no flag that empties that set.
        Harness::Antigravity => {
            if crate::mentions_flag(help, "--sandbox") {
                Constraint::Restricted
            } else {
                Constraint::Unknown
            }
        }
    }
}

/// Does this Codex binary accept `--ignore-user-config`?
///
/// Read off the same `--help` probe as [`constraint_from_help`], and answered
/// `false` for an unreadable one: the flag is then simply not passed, which
/// costs isolation but cannot kill the spawn. That is the opposite direction
/// from the *grade*, and deliberately so — a missing grade means we do not know
/// whether side effects are blocked and must refuse, whereas a missing
/// isolation flag is a known, lesser, still-runnable state.
pub fn codex_isolates_config(help: &str) -> bool {
    crate::looks_like_help(help) && crate::mentions_flag(help, "--ignore-user-config")
}

/// Everything a single turn needs, independent of harness.
///
/// One struct rather than eight positional arguments because three harnesses
/// build their command lines from overlapping subsets of it, and a positional
/// list is how `--model`'s value ends up in `--effort`'s slot.
#[derive(Clone, Debug, Default)]
pub struct TurnSpec {
    /// The user's prompt for this turn.
    pub prompt: String,
    /// Schema outline and house rules, appended to the system prompt.
    pub system: String,
    /// Model id, passed through verbatim. Empty = the harness's own default.
    pub model: String,
    /// Reasoning effort, where [`Harness::supports_effort`] holds.
    pub effort: String,
    /// Session/thread id from a previous turn, where
    /// [`Harness::supports_resume`] holds.
    pub resume: Option<String>,
    /// Path to the MCP config this harness reads, when one was written.
    pub mcp_config: Option<String>,
    /// `-c` overrides carrying the MCP server, for harnesses configured on the
    /// command line rather than by file. Never a credential — see
    /// [`codex_mcp_overrides`].
    pub mcp_overrides: Vec<String>,
    /// Pass Codex's `--ignore-user-config`, when the probe saw it.
    ///
    /// This is Codex's `--strict-mcp-config` *and* `--setting-sources user`
    /// rolled into one, and its help text is explicit that it keeps the user
    /// logged in: *"Do not load `$CODEX_HOME/config.toml`; auth still uses
    /// `CODEX_HOME`"*. Without it the user's own `config.toml` loads into
    /// Schemaic's SQL assistant — their MCP servers (tools nobody here
    /// allow-listed) and their hooks, which are commands.
    ///
    /// Conditional for the same reason Claude's two isolating flags are: a
    /// binary that has never heard of it dies on the unknown flag rather than
    /// running degraded. It does **not** decide the [`Constraint`] grade — the
    /// sandbox does — exactly as Claude's grade turns only on `--tools`.
    pub isolate_config: bool,
}

/// The argv for one turn on a non-persistent harness.
///
/// Claude's is [`crate::build_session_args`]: it is spawned once per
/// *conversation*, not per turn, and its prompt arrives later on stdin. These
/// two are spawned per turn with the prompt in argv.
pub fn turn_args(h: Harness, spec: &TurnSpec) -> Vec<String> {
    match h {
        // Claude does not take a per-turn command line.
        Harness::Claude => Vec::new(),
        Harness::Codex => {
            let mut a: Vec<String> = vec!["exec".into()];
            // Resume is a sub-subcommand and must follow `exec` immediately.
            let resuming = spec.resume.as_deref().filter(|s| !s.is_empty());
            if let Some(id) = resuming {
                a.push("resume".into());
                a.push(id.into());
            }
            a.push("--json".into());
            if resuming.is_none() {
                // Defence in depth, and only where it is taken: `--sandbox`
                // exists on `codex exec` but **not** on `codex exec resume`
                // (measured against the installed binary), so passing it on a
                // resumed turn kills the spawn on an unknown option.
                a.push("--sandbox".into());
                a.push("read-only".into());
            }
            // The isolation. See `TurnSpec::isolate_config`.
            if spec.isolate_config {
                a.push("--ignore-user-config".into());
            }
            // Codex requires a git repo unless told otherwise, and the session
            // cwd is a private app directory that is not one.
            a.push("--skip-git-repo-check".into());
            if !spec.model.is_empty() {
                a.push("--model".into());
                a.push(spec.model.clone());
            }
            // Caller-supplied overrides go *before* the constraint below.
            for o in &spec.mcp_overrides {
                a.push("-c".into());
                a.push(o.clone());
            }
            // **The constraint, last among the `-c`s and never only a flag.**
            //
            // Not only a flag, because `--sandbox` is absent from `exec resume`
            // (above) and dropping it there would leave every turn after the
            // first unconstrained — the worse half of that trade. The config key
            // is accepted on both paths, so it is the primary mechanism.
            //
            // Last, because `-c` precedence is positional: Codex splices root
            // overrides to the front expressly "so they have lower precedence
            // than command-specific flags parsed after a subcommand", i.e. a
            // later `-c` wins. Emitted before `mcp_overrides`, a caller passing
            // its own `sandbox_mode` would silently outrank the constraint. Ours
            // goes last so nothing a caller adds can displace it.
            a.push("-c".into());
            a.push("sandbox_mode=\"read-only\"".into());
            // Prompt last of all: it is the positional argument.
            a.push(prefixed_prompt(turn_system(spec), &spec.prompt));
            a
        }
        Harness::Antigravity => {
            // `-p <prompt>` takes the prompt as its value, not as a positional,
            // so unlike Codex the prompt is not last.
            let mut a: Vec<String> = vec![
                "-p".into(),
                prefixed_prompt(turn_system(spec), &spec.prompt),
                "--output-format".into(),
                "stream-json".into(),
                // The constraint: terminal restrictions. It does not empty the
                // tool set — nothing here can — so the grade stays `Restricted`.
                "--sandbox".into(),
                // Print mode expands slash commands and skills by default; a
                // prompt is user text and must not be able to invoke either.
                "--disable-slash-commands".into(),
            ];
            if !spec.model.is_empty() {
                a.push("--model".into());
                a.push(spec.model.clone());
            }
            if !spec.effort.is_empty() {
                a.push("--effort".into());
                a.push(spec.effort.clone());
            }
            if let Some(id) = spec.resume.as_deref().filter(|s| !s.is_empty()) {
                a.push("--conversation".into());
                a.push(id.into());
            }
            a
        }
    }
}

/// Fold the system context into the prompt for harnesses with no
/// `--append-system-prompt`.
///
/// Kept separate and tested because the join is load-bearing: run together
/// without the blank line, the schema outline's last table name reads as the
/// first word of the user's question.
fn prefixed_prompt(system: &str, prompt: &str) -> String {
    if system.trim().is_empty() {
        return prompt.to_string();
    }
    format!("{system}\n\n{prompt}")
}

/// The system context this turn should carry, given whether it resumes a thread.
///
/// **Once per thread, not once per turn.** With no `--append-system-prompt`,
/// these harnesses take the outline folded into the prompt — and a resumed turn
/// makes the CLI replay the whole prior thread, every turn of which already
/// carries its own copy. Sending it again put the schema into the model's
/// context N times on turn N: a ten-turn conversation against a large catalogue
/// paid for the outline ten times, and the panel's own input-token count would
/// show it climbing against a catalogue that never changed. Claude has the flag
/// and sends it once at spawn; this is the same thing for the harnesses that do
/// not.
///
/// It lives here, beside the argv it shapes, rather than at the call site that
/// fills in `TurnSpec` — the caller cannot then forget it, and the rule gets a
/// test that runs the composition rather than the predicate.
fn turn_system(spec: &TurnSpec) -> &str {
    match spec.resume.as_deref().filter(|s| !s.is_empty()) {
        Some(_) => "",
        None => &spec.system,
    }
}

/// `-c` overrides that give Codex our MCP server and **only** ours.
///
/// Two things are deliberate.
///
/// **The whole `mcp_servers` table is replaced, not extended.** Setting
/// `mcp_servers.schemaic.command` alone would merge ours alongside whatever the
/// user has in `~/.codex/config.toml`, loading tools nobody here allow-listed
/// into Schemaic's SQL assistant. Assigning the table wholesale is this
/// harness's `--strict-mcp-config`.
///
/// **The endpoint is not here.** It carries the database credentials, and `-c`
/// overrides are argv — visible to every process listing on the machine. It
/// travels in a file instead, named by `--endpoint-file`, whose *path* is not a
/// secret. This is the same rule the Claude path follows by putting the endpoint
/// in its temp config's `env` rather than on the command line (review C6).
/// **Each tool is approved by name, or the model cannot call it.** Measured: with
/// the server registered but no approval set, `codex exec` refuses every call
/// with *"MCP tool call requires approval, but approval policy is never"* —
/// `exec` has nobody to prompt, so the default `auto` denies. The lever is
/// per-tool (`tools.<name>.approval_mode = "approve"`), which is what makes it
/// safe: `allowed` carries only the tools this connection's access level offers,
/// so a schema-only connection approves `list_schema` and `describe_table` and
/// leaves `run_query` unapproved rather than trusting the server alone to refuse
/// it. Same rule the Claude path states with `--allowedTools`.
pub fn codex_mcp_overrides(exe: &str, endpoint_file: &str, allowed: &[&str]) -> Vec<String> {
    let tools = allowed
        .iter()
        .map(|t| format!("{}={{approval_mode=\"approve\"}}", bare_tool_name(t)))
        .collect::<Vec<_>>()
        .join(",");
    vec![format!(
        "mcp_servers={{schemaic={{command={},args=[{},{},{}],tools={{{tools}}}}}}}",
        toml_str(exe),
        toml_str("--mcp-serve"),
        toml_str("--endpoint-file"),
        // The path, never the endpoint itself.
        toml_str(endpoint_file),
    )]
}

/// The isolation half of [`codex_mcp_overrides`], with no server of our own.
///
/// **Losing our tools must not mean gaining somebody else's.** Assigning the
/// whole `mcp_servers` table is this harness's `--strict-mcp-config`: it is the
/// only thing that displaces the user's own server table. When the endpoint file
/// cannot be written the session has no database tools either way — but emitting
/// *no* override left every server in `~/.codex/config.toml` loaded into
/// Schemaic's SQL assistant, tools nobody here allow-listed, reachable by the
/// route `codex exec`'s approval policy does not cover. An empty table is the
/// honest version of "no database tools": ours absent, and nobody else's in its
/// place.
pub fn codex_isolation_only() -> Vec<String> {
    vec!["mcp_servers={}".to_string()]
}

/// `mcp__schemaic__run_query` → `run_query`.
///
/// Codex keys its per-tool config by the name the *server* advertises, while the
/// app's allow-list holds the fully-qualified name the transcript and Claude's
/// `--allowedTools` both speak. Deriving one from the other keeps a single source
/// of truth: a second hand-written list of bare names is one rename away from
/// approving a tool that no longer exists while refusing one that does.
fn bare_tool_name(qualified: &str) -> &str {
    qualified.rsplit("__").next().unwrap_or(qualified)
}

/// A TOML basic string: quoted, with backslashes and quotes escaped.
///
/// Windows paths are the reason this cannot be `format!("\"{s}\"")` — every
/// separator in `C:\Users\…\schemaic.exe` is a TOML escape introducer, and an
/// unescaped one turns the override into a parse error or, worse, a different
/// path.
fn toml_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The `permissions.allow` rules Antigravity needs to let our tools run.
///
/// One per tool, from the connection's own allow-list — so a schema-only
/// connection never grants `run_query`, and `run_command` is never granted at
/// all. Shaped `mcp(<server>/<tool>)`, which is the form its own refusal message
/// names.
pub fn antigravity_allow_rules(allowed: &[&str]) -> Vec<String> {
    allowed
        .iter()
        .map(|t| format!("mcp(schemaic/{})", bare_tool_name(t)))
        .collect()
}

/// Add `rules` to an Antigravity `settings.json`, preserving everything else.
///
/// **Merged, never rewritten.** This is the user's file: it holds their
/// `trustedWorkspaces` and whatever else that CLI keeps there, and Antigravity
/// rewrites it itself (it reordered the keys the moment it read ours). So the
/// document is parsed, the rules are unioned into `permissions.allow`, and every
/// other key is handed back untouched.
///
/// `None` when the text is not a JSON object — a settings file we cannot parse
/// is one we must not overwrite, and the cost of declining is that Antigravity
/// gets no database tools this session rather than that the user loses a file.
///
/// Idempotent: adding a rule that is already there changes nothing, so a crashed
/// session that left rules behind does not accumulate duplicates.
pub fn antigravity_settings_with_rules(current: &str, rules: &[String]) -> Option<String> {
    let mut doc = parse_settings(current)?;
    let allow = doc
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()?
        .entry("allow")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let arr = allow.as_array_mut()?;
    for r in rules {
        let v = serde_json::Value::String(r.clone());
        if !arr.contains(&v) {
            arr.push(v);
        }
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(doc)).ok()
}

/// Remove exactly `rules` from an Antigravity `settings.json`.
///
/// **Only ours.** A rule the user added by hand — even for the same tool — is
/// indistinguishable from ours by value, which is a real limit worth stating:
/// this removes any rule matching one we would have added, so a user who granted
/// `mcp(schemaic/run_query)` themselves loses it when a Schemaic session ends.
/// The alternative, leaving rules behind on the chance one was theirs, means a
/// standing grant nobody remembers making — the worse of the two.
///
/// Empty `permissions`/`allow` containers are pruned so the file returns to the
/// shape it had before, rather than accumulating scaffolding.
pub fn antigravity_settings_without_rules(current: &str, rules: &[String]) -> Option<String> {
    let mut doc = parse_settings(current)?;
    if let Some(perms) = doc.get_mut("permissions").and_then(|p| p.as_object_mut()) {
        if let Some(arr) = perms.get_mut("allow").and_then(|a| a.as_array_mut()) {
            arr.retain(|v| !v.as_str().is_some_and(|s| rules.iter().any(|r| r == s)));
            if arr.is_empty() {
                perms.remove("allow");
            }
        }
        if perms.is_empty() {
            doc.remove("permissions");
        }
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(doc)).ok()
}

/// A settings document, or `None` if it is not a JSON object.
///
/// An **empty or whitespace-only** file is treated as an empty object rather
/// than a parse failure: that is what a fresh install looks like, and refusing
/// it would deny tools to exactly the users who have never configured anything.
fn parse_settings(current: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    if current.trim().is_empty() {
        return Some(serde_json::Map::new());
    }
    match serde_json::from_str::<serde_json::Value>(current) {
        Ok(serde_json::Value::Object(m)) => Some(m),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TurnSpec {
        TurnSpec {
            prompt: "count rows".into(),
            system: "tables: users(id)".into(),
            model: "gpt-5".into(),
            ..Default::default()
        }
    }

    #[test]
    fn an_unreadable_probe_refuses_rather_than_assuming() {
        for h in Harness::ALL {
            for probe in ["", "   ", "npm ERR! missing node"] {
                assert_eq!(
                    constraint_from_help(h, probe),
                    Constraint::Unknown,
                    "{h:?} on {probe:?}"
                );
                assert!(!constraint_from_help(h, probe).is_runnable());
            }
        }
    }

    #[test]
    fn a_harness_missing_its_constraining_flag_is_not_runnable() {
        // Reads like help, but the flag that restricts it is absent.
        let codex = "Usage: codex [options]\n  --model <m>\n  --help\n";
        assert_eq!(
            constraint_from_help(Harness::Codex, codex),
            Constraint::Unknown
        );
        let agy = "Usage: agy [options]\n  --model <m>\n  --help\n";
        assert_eq!(
            constraint_from_help(Harness::Antigravity, agy),
            Constraint::Unknown
        );
    }

    #[test]
    fn only_claude_can_reach_the_sealed_grade() {
        let claude = "Usage: claude\n  --tools <t>\n  --help\n";
        assert_eq!(
            constraint_from_help(Harness::Claude, claude),
            Constraint::Sealed
        );

        let codex = "Usage: codex\n  --sandbox <s>\n  --help\n";
        assert_eq!(
            constraint_from_help(Harness::Codex, codex),
            Constraint::Restricted
        );
        let agy = "Usage: agy\n  --sandbox\n  --help\n";
        assert_eq!(
            constraint_from_help(Harness::Antigravity, agy),
            Constraint::Restricted
        );
    }

    #[test]
    fn a_claude_without_the_tools_flag_drops_to_restricted_not_sealed() {
        let help = "Usage: claude\n  --model <m>\n  --help\n";
        assert_eq!(
            constraint_from_help(Harness::Claude, help),
            Constraint::Restricted
        );
    }

    /// **Only a sandbox may claim a sandbox.** `Restricted` is reached two
    /// different ways: by a real OS-level sandbox on Codex and Antigravity, and
    /// on Claude merely by `--tools` being missing — which leaves the denylist,
    /// the guard this module's docs describe as having left nineteen built-ins
    /// live. The notice for the second must not read like the notice for the
    /// first.
    #[test]
    fn a_denylisted_claude_is_not_described_as_unable_to_write_or_run() {
        let claude = Constraint::Restricted
            .notice(Harness::Claude)
            .expect("a notice");
        assert!(
            !claude.contains("cannot write files or run commands"),
            "a denylist is claiming what only a sandbox delivers: {claude}"
        );
        // It says what is actually true, and what the user can do about it.
        assert!(claude.contains("denylist"), "{claude}");
        assert!(claude.to_lowercase().contains("updat"), "{claude}");

        // The sandboxed harnesses keep the stronger sentence, which they earn.
        for h in [Harness::Codex, Harness::Antigravity] {
            let n = Constraint::Restricted.notice(h).expect("a notice");
            assert!(
                n.contains("cannot write files or run commands"),
                "{h:?}: {n}"
            );
        }
    }

    #[test]
    fn the_weaker_grades_say_so_and_the_strongest_stays_quiet() {
        assert_eq!(Constraint::Sealed.notice(Harness::Claude), None);
        let r = Constraint::Restricted
            .notice(Harness::Codex)
            .expect("a notice");
        assert!(r.contains("read"), "{r}");
        // **It says what *this* harness gives you, and stops there.** Naming the
        // stronger harness turned a statement of fact into a pitch for the one
        // the user had just declined, in the one place they were exercising the
        // choice. Every grade's notice is about the binary in the dropdown.
        for h in Harness::ALL {
            for g in [Constraint::Restricted, Constraint::Unknown] {
                let n = g.notice(h).expect("a notice");
                assert!(
                    h == Harness::Claude || !n.contains("Claude"),
                    "{h:?}/{g:?} advertises another harness: {n}"
                );
            }
        }
        let u = Constraint::Unknown
            .notice(Harness::Codex)
            .expect("a notice");
        assert!(u.contains("disabled"), "{u}");
    }

    #[test]
    fn grades_are_ordered_and_only_unknown_blocks_a_spawn() {
        assert!(Constraint::Sealed > Constraint::Restricted);
        assert!(Constraint::Restricted > Constraint::Unknown);
        assert!(Constraint::Sealed.is_runnable());
        assert!(Constraint::Restricted.is_runnable());
        assert!(!Constraint::Unknown.is_runnable());
    }

    #[test]
    fn gemini_is_gone_and_its_persisted_key_resolves_to_nothing() {
        // Google withdrew OAuth for personal accounts, so `gemini` now needs an
        // API key to authenticate at all, and the CLI itself points at
        // Antigravity as the successor — which this build does drive. The
        // harness was never runnable here (`spawn_refusal` turned it away), so
        // removing it costs nobody a working session.
        //
        // The migration is the one `from_key` was built for: a settings file
        // still saying "gemini" resolves to `None`, and `main.rs` warns and
        // falls back to Claude rather than coercing it to a neighbour.
        assert!(Harness::from_key("gemini").is_none());
        for h in Harness::ALL {
            assert_ne!(h.key(), "gemini");
            assert_ne!(h.bin(), "gemini");
            assert!(!h.label().contains("Gemini"), "{h:?}");
        }
    }

    #[test]
    fn suggestions_are_a_menu_and_never_a_permitted_set() {
        // Every harness takes an arbitrary id; the suggestion list is only what
        // a menu offers. An empty one (Antigravity, whose model names were not
        // measured) must not be read as "no models allowed".
        for h in Harness::ALL {
            assert!(
                h.supports_model_choice(),
                "{h:?} would make the field pointless"
            );
            for m in h.suggested_models() {
                assert!(!m.is_empty(), "{h:?} suggests an empty id");
                assert!(
                    !m.contains(' '),
                    "{h:?} suggests {m:?}, which needs quoting"
                );
            }
        }
        assert!(Harness::Antigravity.suggested_models().is_empty());
        assert!(!Harness::Claude.suggested_models().is_empty());
    }

    #[test]
    fn a_model_id_no_build_has_heard_of_still_reaches_the_command_line() {
        // The whole point of opening the field: a model released after this
        // binary was compiled, or a dated snapshot, is passed through verbatim.
        let mut s = spec();
        s.model = "claude-opus-5-20260901".into();
        let a = turn_args(Harness::Codex, &s);
        let i = a.iter().position(|x| x == "--model").expect("--model");
        assert_eq!(a[i + 1], "claude-opus-5-20260901");
    }

    #[test]
    fn effort_is_offered_only_where_the_flag_exists() {
        assert!(Harness::Claude.supports_effort());
        assert!(Harness::Antigravity.supports_effort());
        assert!(!Harness::Codex.supports_effort());
    }

    /// The rule the list exists for, enforced on the value that is actually
    /// sent rather than on the list alone. `xhigh` is Claude's fourth level and
    /// the one that leaked: the app checked `supports_effort()` — true for both
    /// Claude and Antigravity — and passed the selection straight through.
    #[test]
    fn a_level_one_harness_takes_is_not_sent_to_another_that_does_not() {
        assert_eq!(Harness::Claude.effort_arg("xhigh"), Some("xhigh"));
        assert_eq!(Harness::Antigravity.effort_arg("xhigh"), None);
        // A harness with no flag at all sends nothing, whatever it is handed.
        for lvl in ["low", "medium", "high", "xhigh"] {
            assert_eq!(Harness::Codex.effort_arg(lvl), None, "{lvl}");
        }
        // Junk and empties are the same non-answer, not a passed-through string.
        for h in Harness::ALL {
            for junk in ["", " ", "HIGH", "extreme"] {
                assert_eq!(h.effort_arg(junk), None, "{h:?} on {junk:?}");
            }
            // Whatever comes back is always one of this harness's own levels —
            // the property, rather than a list of cases.
            for lvl in ["low", "medium", "high", "xhigh"] {
                if let Some(out) = h.effort_arg(lvl) {
                    assert!(h.effort_levels().contains(&out), "{h:?} sent {out}");
                }
            }
        }
    }

    /// One CLI's vocabulary must not be handed to another. Claude takes a fourth
    /// level, `xhigh`; Antigravity's flag is documented `low|medium|high`, so a
    /// shared list would send it a level it never advertised.
    #[test]
    fn each_harness_offers_only_the_effort_levels_its_own_flag_takes() {
        assert!(Harness::Claude.effort_levels().contains(&"xhigh"));
        assert!(!Harness::Antigravity.effort_levels().contains(&"xhigh"));
        // The shared floor still holds, so the common levels mean the same thing.
        for level in ["low", "medium", "high"] {
            for h in [Harness::Claude, Harness::Antigravity] {
                assert!(h.effort_levels().contains(&level), "{h:?} lacks {level}");
            }
        }
    }

    /// The two answers are computed from one list rather than restated, so they
    /// cannot drift into disagreeing about the same harness.
    #[test]
    fn supports_effort_agrees_with_the_levels_it_would_offer() {
        for h in Harness::ALL {
            assert_eq!(
                h.supports_effort(),
                !h.effort_levels().is_empty(),
                "{h:?} says one thing and offers another"
            );
        }
    }

    #[test]
    fn resume_is_the_complement_of_a_persistent_pipe() {
        for h in Harness::ALL {
            assert_eq!(
                h.supports_resume(),
                !h.is_persistent(),
                "{h:?} needs one or the other, never both"
            );
        }
    }

    #[test]
    fn codex_turn_is_constrained_before_it_is_given_a_prompt() {
        let a = turn_args(Harness::Codex, &spec());
        assert_eq!(a[0], "exec");
        let sandbox = a.iter().position(|s| s == "--sandbox").expect("--sandbox");
        assert_eq!(a[sandbox + 1], "read-only");
        assert!(a.contains(&"--json".to_string()));
        assert!(a.contains(&"--skip-git-repo-check".to_string()));
        // The prompt is the positional and must be last, or a later flag reads
        // as part of it.
        assert!(a.last().expect("a prompt").contains("count rows"));
    }

    #[test]
    fn codex_isolation_is_passed_only_when_the_probe_saw_the_flag() {
        let mut s = spec();
        s.isolate_config = true;
        assert!(
            turn_args(Harness::Codex, &s).contains(&"--ignore-user-config".to_string()),
            "the user's own MCP servers and hooks would load"
        );

        s.isolate_config = false;
        assert!(
            !turn_args(Harness::Codex, &s).contains(&"--ignore-user-config".to_string()),
            "an unknown flag kills the spawn outright"
        );
    }

    #[test]
    fn the_isolation_probe_needs_real_help_and_the_real_flag() {
        // Verbatim from the installed binary's `codex exec --help`.
        let real = "Options:\n  -s, --sandbox <SANDBOX_MODE>\n      --ignore-user-config\n          \
                    Do not load `$CODEX_HOME/config.toml`; auth still uses `CODEX_HOME`\n  \
                    -h, --help\n";
        assert!(codex_isolates_config(real));

        // Readable help, flag absent.
        assert!(!codex_isolates_config(
            "Usage: codex\n  --sandbox <s>\n  --help\n"
        ));
        // Unreadable probe: not passed, but still runnable — unlike the grade.
        assert!(!codex_isolates_config(""));
        assert!(!codex_isolates_config("npm ERR! missing node"));
    }

    #[test]
    fn isolation_does_not_decide_the_grade() {
        // Parallel to Claude, whose grade turns only on `--tools` and not on its
        // two isolating flags.
        let no_isolation = "Usage: codex\n  --sandbox <s>\n  --help\n";
        assert_eq!(
            constraint_from_help(Harness::Codex, no_isolation),
            Constraint::Restricted
        );
        assert!(!codex_isolates_config(no_isolation));
    }

    /// `-c` precedence is positional and a later one wins, so the constraint has
    /// to be emitted after anything a caller supplies. Otherwise an override
    /// arriving through `mcp_overrides` — a channel whose only producer today is
    /// ours, but which is a plain `Vec<String>` — silently outranks it.
    #[test]
    fn a_caller_supplied_override_cannot_displace_the_constraint() {
        let mut s = spec();
        s.mcp_overrides = vec![
            "mcp_servers={schemaic={command=\"x\"}}".into(),
            // The hostile case, whether by bug or by malice.
            "sandbox_mode=\"danger-full-access\"".into(),
        ];
        let a = turn_args(Harness::Codex, &s);

        let ours = a
            .iter()
            .rposition(|x| x == "sandbox_mode=\"read-only\"")
            .expect("the constraint override");
        let theirs = a
            .iter()
            .position(|x| x == "sandbox_mode=\"danger-full-access\"")
            .expect("the caller's override");
        assert!(ours > theirs, "the constraint must come last to win: {a:?}");
    }

    #[test]
    fn codex_is_never_handed_a_bypass_flag() {
        let mut s = spec();
        s.isolate_config = true;
        let a = turn_args(Harness::Codex, &s).join(" ");
        for danger in [
            "--dangerously-bypass-approvals-and-sandbox",
            "--dangerously-bypass-hook-trust",
            "--approve-for-me",
            "workspace-write",
            "danger-full-access",
            "--add-dir",
        ] {
            assert!(
                !a.contains(danger),
                "{danger} reached the command line: {a}"
            );
        }
    }

    #[test]
    fn codex_resume_follows_exec_immediately() {
        let mut s = spec();
        s.resume = Some("th_9".into());
        let a = turn_args(Harness::Codex, &s);
        assert_eq!(
            &a[..3],
            &["exec".to_string(), "resume".into(), "th_9".into()]
        );
    }

    /// `codex exec` takes `--sandbox`; `codex exec resume` does **not** (measured
    /// against the installed binary). So a resumed turn passing the flag dies on
    /// an unknown option, and one passing nothing runs unconstrained. Every turn
    /// must carry the constraint by a route its own path accepts.
    #[test]
    fn every_codex_turn_is_constrained_including_a_resumed_one() {
        let key = "sandbox_mode=\"read-only\"".to_string();

        let first = turn_args(Harness::Codex, &spec());
        assert!(first.contains(&key), "first turn unconstrained: {first:?}");

        let mut s = spec();
        s.resume = Some("th_9".into());
        let again = turn_args(Harness::Codex, &s);
        assert!(
            again.contains(&key),
            "resumed turn unconstrained: {again:?}"
        );
        // …and not via a flag that subcommand would reject.
        assert!(
            !again.contains(&"--sandbox".to_string()),
            "`--sandbox` is not accepted by `exec resume`: {again:?}"
        );
    }

    #[test]
    fn an_empty_resume_id_is_not_passed_as_a_session() {
        let mut s = spec();
        s.resume = Some(String::new());
        let a = turn_args(Harness::Codex, &s);
        assert!(!a.contains(&"resume".to_string()), "{a:?}");
    }

    #[test]
    fn an_empty_model_lets_the_harness_keep_its_own_default() {
        // **Every** harness, because this is what the settings modal relies on
        // when a harness switch clears the field: empty must mean "the CLI's own
        // default" on whichever one the user just picked, not `--model ""`.
        let mut s = spec();
        s.model = String::new();
        for h in Harness::ALL {
            let a = turn_args(h, &s);
            assert!(!a.contains(&"--model".to_string()), "{h:?}: {a:?}");
        }
    }

    /// The composition, not the predicate: what matters is whether the *argv*
    /// carries the outline, and on which turns.
    #[test]
    fn the_schema_outline_rides_the_first_turn_of_a_thread_and_not_the_rest() {
        for h in [Harness::Codex, Harness::Antigravity] {
            let mut first = spec();
            first.system = "tables: users(id)".into();
            first.resume = None;
            let a = turn_args(h, &first);
            assert!(
                a.iter().any(|x| x.contains("users(id)")),
                "{h:?} dropped the outline from the opening turn: {a:?}"
            );

            // The resumed turn: the CLI replays the thread, which already holds
            // the outline from the turn above.
            let mut later = first.clone();
            later.resume = Some("thread_1".into());
            let b = turn_args(h, &later);
            assert!(
                !b.iter().any(|x| x.contains("users(id)")),
                "{h:?} sent the outline again on a resumed turn: {b:?}"
            );
            // …and the question itself always survives.
            assert!(
                b.iter().any(|x| x.contains(&first.prompt)),
                "{h:?} lost the prompt: {b:?}"
            );
        }
    }

    /// The fallback still displaces the user's servers. Both overrides assign
    /// the *whole* table, which is the property that isolates — asserted here
    /// against the same prefix rather than against two hand-copied strings.
    #[test]
    fn losing_our_server_does_not_hand_the_session_somebody_elses() {
        let full = codex_mcp_overrides("/usr/bin/schemaic", "/tmp/ep.json", &["mcp__s__run_query"]);
        let bare = codex_isolation_only();
        for o in full.iter().chain(bare.iter()) {
            assert!(
                o.starts_with("mcp_servers="),
                "an override that does not assign the table isolates nothing: {o}"
            );
        }
        // The fallback names no server at all — not ours, and not a leftover.
        assert_eq!(bare, vec!["mcp_servers={}".to_string()]);
        assert!(!bare[0].contains("schemaic"), "{bare:?}");
        // …while the full one does carry ours.
        assert!(full[0].contains("schemaic"), "{full:?}");
    }

    /// An empty `resume` is not a resume — the same non-answer the argv builders
    /// already treat it as, so it must not silently drop the outline.
    #[test]
    fn an_empty_resume_id_still_carries_the_outline() {
        let mut s = spec();
        s.system = "tables: users(id)".into();
        s.resume = Some(String::new());
        for h in [Harness::Codex, Harness::Antigravity] {
            let a = turn_args(h, &s);
            assert!(a.iter().any(|x| x.contains("users(id)")), "{h:?}: {a:?}");
        }
    }

    #[test]
    fn the_system_context_is_separated_from_the_question() {
        let out = prefixed_prompt("tables: users(id)", "count rows");
        assert!(
            out.contains("users(id)\n\ncount rows"),
            "outline ran into the question: {out}"
        );
        // Nothing to prepend leaves the prompt untouched.
        assert_eq!(prefixed_prompt("", "count rows"), "count rows");
        assert_eq!(prefixed_prompt("   ", "count rows"), "count rows");
    }

    /// The access level decides which tools are approved, not just which the
    /// server answers. Measured: an unapproved tool is refused by Codex with
    /// "requires approval, but approval policy is never" — so approving the
    /// whole set on a schema-only connection would hand `run_query` an approval
    /// it must not have.
    #[test]
    fn codex_approves_only_the_tools_the_access_level_offers() {
        let read_only = codex_mcp_overrides(
            "/usr/bin/schemaic",
            "/tmp/ep.json",
            &[
                "mcp__schemaic__list_schema",
                "mcp__schemaic__describe_table",
            ],
        );
        assert!(read_only[0].contains("list_schema={approval_mode=\"approve\"}"));
        assert!(read_only[0].contains("describe_table={approval_mode=\"approve\"}"));
        assert!(
            !read_only[0].contains("run_query"),
            "a schema-only connection approved the query tool: {}",
            read_only[0]
        );

        let full = codex_mcp_overrides(
            "/usr/bin/schemaic",
            "/tmp/ep.json",
            &["mcp__schemaic__run_query"],
        );
        assert!(full[0].contains("run_query={approval_mode=\"approve\"}"));
    }

    #[test]
    fn the_bare_tool_name_is_derived_rather_than_restated() {
        assert_eq!(bare_tool_name("mcp__schemaic__run_query"), "run_query");
        assert_eq!(bare_tool_name("run_query"), "run_query");
        // Never empty, whatever it is handed.
        assert_eq!(bare_tool_name(""), "");
    }

    #[test]
    fn codex_overrides_replace_the_whole_server_table() {
        let o = codex_mcp_overrides("/usr/bin/schemaic", "/tmp/ep.json", &[]);
        assert_eq!(o.len(), 1);
        // `mcp_servers=` and not `mcp_servers.schemaic=`: the user's own servers
        // must be displaced, not joined.
        assert!(o[0].starts_with("mcp_servers={"), "{}", o[0]);
        assert!(!o[0].starts_with("mcp_servers.schemaic"), "{}", o[0]);
    }

    #[test]
    fn the_endpoint_never_reaches_a_codex_command_line() {
        // The whole point of the endpoint *file*: argv is world-readable.
        let secret = "mysql://root:hunter2@10.0.0.5:3306/prod";
        let o = codex_mcp_overrides("/usr/bin/schemaic", "/tmp/ep.json", &[]);
        for arg in &o {
            assert!(!arg.contains(secret), "{arg}");
            assert!(!arg.contains("hunter2"), "{arg}");
            assert!(!arg.contains("mysql://"), "{arg}");
        }
        assert!(o[0].contains("--endpoint-file"), "{}", o[0]);
        assert!(o[0].contains("/tmp/ep.json"), "{}", o[0]);
    }

    #[test]
    fn a_windows_path_survives_the_toml_override_intact() {
        // Every separator is a TOML escape introducer; unescaped, the override
        // is a parse error or silently a different path.
        let o = codex_mcp_overrides(r"C:\Users\a b\schemaic.exe", r"C:\tmp\ep.json", &[]);
        assert!(o[0].contains(r"C:\\Users\\a b\\schemaic.exe"), "{}", o[0]);
        assert!(o[0].contains(r"C:\\tmp\\ep.json"), "{}", o[0]);
        // No lone separator survives: every backslash is part of a doubled pair.
        let body = o[0].strip_prefix("mcp_servers=").expect("the key");
        let bytes: Vec<char> = body.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '\\' {
                assert_eq!(
                    bytes.get(i + 1),
                    Some(&'\\'),
                    "lone backslash at {i}: {body}"
                );
                i += 2;
            } else {
                i += 1;
            }
        }
    }

    #[test]
    fn a_quote_in_a_path_cannot_break_out_of_the_override() {
        let o = codex_mcp_overrides(r#"/opt/we"ird/schemaic"#, "/tmp/ep.json", &[]);
        assert!(o[0].contains(r#"we\"ird"#), "{}", o[0]);
    }

    // ---- Antigravity settings surgery ------------------------------------

    /// Verbatim shape of the file this was measured against.
    const AGY_SETTINGS: &str = r#"{
  "trustedWorkspaces": [
    "C:\\Users\\jonid"
  ]
}"#;

    #[test]
    fn the_rules_name_each_tool_and_follow_the_access_level() {
        let full =
            antigravity_allow_rules(&["mcp__schemaic__list_schema", "mcp__schemaic__run_query"]);
        assert_eq!(
            full,
            vec![
                "mcp(schemaic/list_schema)".to_string(),
                "mcp(schemaic/run_query)".to_string()
            ]
        );
        // Nothing outside our server is ever granted.
        assert!(!full.iter().any(|r| r.contains("run_command")));
    }

    #[test]
    fn adding_rules_preserves_every_other_setting() {
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        let out = antigravity_settings_with_rules(AGY_SETTINGS, &rules).expect("merged");
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        // The user's own key survives untouched — this is their file.
        assert_eq!(v["trustedWorkspaces"][0], "C:\\Users\\jonid");
        assert_eq!(v["permissions"]["allow"][0], "mcp(schemaic/list_schema)");
    }

    #[test]
    fn adding_the_same_rule_twice_does_not_duplicate_it() {
        // A crashed session leaves rules behind; the next one must not stack.
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        let once = antigravity_settings_with_rules(AGY_SETTINGS, &rules).expect("merged");
        let twice = antigravity_settings_with_rules(&once, &rules).expect("merged");
        let v: serde_json::Value = serde_json::from_str(&twice).expect("json");
        assert_eq!(
            v["permissions"]["allow"].as_array().expect("array").len(),
            1
        );
    }

    #[test]
    fn removing_our_rules_restores_the_original_shape() {
        let rules =
            antigravity_allow_rules(&["mcp__schemaic__list_schema", "mcp__schemaic__run_query"]);
        let added = antigravity_settings_with_rules(AGY_SETTINGS, &rules).expect("merged");
        let back = antigravity_settings_without_rules(&added, &rules).expect("removed");
        let v: serde_json::Value = serde_json::from_str(&back).expect("json");
        assert_eq!(v["trustedWorkspaces"][0], "C:\\Users\\jonid");
        // The scaffolding is gone, not left behind empty.
        assert!(v.get("permissions").is_none(), "{back}");
    }

    #[test]
    fn a_rule_we_did_not_add_is_left_alone() {
        let mine = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        let with_theirs =
            antigravity_settings_with_rules(AGY_SETTINGS, &["mcp(other/their_tool)".to_string()])
                .expect("merged");
        let both = antigravity_settings_with_rules(&with_theirs, &mine).expect("merged");
        let back = antigravity_settings_without_rules(&both, &mine).expect("removed");
        let v: serde_json::Value = serde_json::from_str(&back).expect("json");
        let allow = v["permissions"]["allow"].as_array().expect("array");
        assert_eq!(allow.len(), 1);
        assert_eq!(allow[0], "mcp(other/their_tool)");
    }

    #[test]
    fn an_unparseable_settings_file_is_declined_rather_than_overwritten() {
        // Losing the user's file is worse than losing this session's DB tools.
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        assert_eq!(antigravity_settings_with_rules("{not json", &rules), None);
        assert_eq!(antigravity_settings_with_rules("[1,2,3]", &rules), None);
        assert_eq!(
            antigravity_settings_without_rules("{not json", &rules),
            None
        );
    }

    #[test]
    fn a_fresh_install_with_an_empty_file_still_gets_its_rules() {
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        for empty in ["", "   ", "\n"] {
            let out = antigravity_settings_with_rules(empty, &rules).expect("merged");
            let v: serde_json::Value = serde_json::from_str(&out).expect("json");
            assert_eq!(v["permissions"]["allow"][0], "mcp(schemaic/list_schema)");
        }
    }
}
