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
//! - `opencode` empties its built-in set too, but by *configuration* rather than
//!   a flag: an agent definition whose `tools` map sets every documented name to
//!   `false`, selected with `--agent`. Measured against `opencode debug agent`,
//!   which resolves that agent to eleven disabled tools and one enabled — and
//!   the enabled one is `invalid`, the CLI's own handler for a malformed tool
//!   call, which it excludes from the set it offers the model. That is a true
//!   seal, the second one here.
//!
//! **The seal that is not a flag needs a different kind of probe, and a
//! different kind of care.** Claude's grade is read straight off `--help`,
//! because `--tools` either exists or does not. OpenCode's lives in a file
//! Schemaic writes and an environment variable it sets, neither of which
//! `--help` can confirm — so [`constraint_from_help`] greps for `--pure`
//! instead, which is real evidence the binary is the one these mechanisms were
//! measured against and is itself part of the constraint (it keeps the user's
//! external plugins, which are arbitrary code, out of the session).
//!
//! The rest of that seal is enforced where it is applied rather than where it is
//! graded: `ai::start_ai_session` refuses the session outright if
//! [`opencode_config_json`] cannot be written. This is the one harness where a
//! missing config does **not** fail closed — `--agent schemaic` naming an agent
//! that does not exist leaves the run on OpenCode's default `build` agent, which
//! has `bash` — so the refusal is the only thing keeping the grade honest.
//!
//! **Its MCP isolation is total, and costs no global state.** `XDG_CONFIG_HOME`
//! pointed at a directory Schemaic owns removes the user's own registered
//! servers from the resolved config entirely (measured). Note what does *not*
//! work: `OPENCODE_CONFIG` and `OPENCODE_CONFIG_CONTENT` both **merge** with the
//! user's file rather than replacing it, so a config naming only our server
//! resolved to ours alongside theirs. They are the obvious lever and the wrong
//! one. See [`crate::harness::opencode_config_json`] and the app's `opencode`
//! module.
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
//! user rather than averaged away. Reporting "sealed" for all four would be the
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
    /// `opencode run --format json` — `type`-tagged whole *parts*, one process
    /// per turn.
    ///
    /// **Its `text` events are not deltas, and that is structural rather than a
    /// sampling artefact.** The JSON printer emits a text part only once
    /// `time.end` is set — i.e. once the part is finished — so a 2964-character
    /// answer arrives as a single event. [`Harness::streams_deltas`] is false
    /// here for that reason, and none of the coalescing state Codex needs is
    /// wanted: nothing is ever restated.
    ///
    /// **It is also the only harness with no turn-completion event at all.** The
    /// printer stops when the session goes idle; there is no `turn.completed` to
    /// decode. The end of a turn is read off `step_finish.reason` instead — see
    /// [`crate::stream::StreamParser`] — because the alternative is a stream that
    /// simply stops, which the app reports as "ended unexpectedly".
    OpenCode,
}

impl Harness {
    /// Every harness, in the order the settings UI offers them.
    pub const ALL: [Harness; 4] = [
        Harness::Claude,
        Harness::Codex,
        Harness::Antigravity,
        Harness::OpenCode,
    ];

    /// The stable string form, matched back by [`Harness::from_key`].
    ///
    /// This is what `UiState::ai_harness` stores. Settings → AI writes it from
    /// the *Agent CLI* dropdown, the app reads it back at startup, and a
    /// `ChatMessage` stamps it to record which CLI answered a turn — so a key
    /// that changes here silently retargets everyone's settings file *and*
    /// unnames every reply already in their saved transcripts.
    ///
    /// (This used to say the UI could not set it and `ui_state.json` had to be
    /// hand-edited. That stopped being true when the dropdown shipped; it is
    /// noted because the sentence read as a live constraint long after it had
    /// become a description of the past.)
    pub fn key(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Antigravity => "antigravity",
            Harness::OpenCode => "opencode",
        }
    }

    /// Display name for the settings dropdown, and for every notice that has to
    /// name the CLI it is talking about.
    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Codex => "Codex",
            Harness::Antigravity => "Antigravity",
            Harness::OpenCode => "OpenCode",
        }
    }

    /// Who a reply in the transcript is *from*, as opposed to which product the
    /// settings dropdown is offering.
    ///
    /// The two differ in exactly one place and deliberately: [`Harness::label`]
    /// is "Claude Code", the thing you install and point a path at, while a
    /// header over an answer is naming a speaker and reads "CLAUDE". That is
    /// also the name that header carried before it learned to vary, so an
    /// existing transcript does not appear to change its mind about who wrote
    /// it.
    ///
    /// A method rather than a `match` at the call site, so the transcript and
    /// the settings box cannot drift into disagreeing about which harness is
    /// which — and so a new harness answers the question once.
    pub fn speaker_name(self) -> &'static str {
        match self {
            Harness::Claude => "Claude",
            // The rest name themselves the same way in both places; they are
            // spelled out rather than delegated to `label` so that a product
            // name gaining a suffix does not silently reach the transcript.
            Harness::Codex => "Codex",
            Harness::Antigravity => "Antigravity",
            Harness::OpenCode => "OpenCode",
        }
    }

    /// The arguments that print the help page this harness's grade is read from.
    ///
    /// **Codex hides half its own flags from the top-level page, and the half it
    /// hides is the isolation.** Measured against the installed binary:
    /// `codex --help` lists `--model`, `--sandbox`, `--help`, `--version` and
    /// *not* `--ignore-user-config`; `codex exec --help` lists all of them. The
    /// probe asked the top-level page, so [`codex_isolates_config`] answered
    /// `false` on every real machine, `--ignore-user-config` was never passed,
    /// and every Codex session loaded the user's own `~/.codex/config.toml` —
    /// their MCP servers, whose tools nobody here allow-listed, and their hooks,
    /// which are commands.
    ///
    /// It is a method rather than a literal at the call site because that is the
    /// seam the bug lived in: the pure decoder was tested with `codex exec
    /// --help` text pasted in by hand, while the caller fed it a different page
    /// entirely, and neither half was wrong on its own.
    ///
    /// `exec` is also where Codex's `--sandbox` lives, so one page still answers
    /// every question the probe asks.
    pub fn help_args(self) -> &'static [&'static str] {
        match self {
            Harness::Codex => &["exec", "--help"],
            Harness::Claude | Harness::Antigravity | Harness::OpenCode => &["--help"],
        }
    }

    /// The executable looked for on `PATH` when no override path is set.
    pub fn bin(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            // Not "antigravity" — the binary is `agy`.
            Harness::Antigravity => "agy",
            Harness::OpenCode => "opencode",
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
            // **Asked as a capability, because the sentence is a claim about a
            // mechanism.** This was `h == Harness::Claude`, with everything else
            // falling through to the sandbox wording — a harness-identity check
            // standing in for one, which is the shape `CLAUDE.md` names: it
            // compiles cleanly while sorting a fourth CLI onto whichever side it
            // happens to land. OpenCode is that fourth: its seal is a `tools`
            // map and it has no sandbox at all, so the fallthrough would have
            // promised the user an OS-enforced read-only that nothing enforces.
            Constraint::Restricted if h.restricted_means_sandbox() => Some(format!(
                "{} runs read-only: it cannot write files or run commands, but its \
                 built-in tools can still read this machine.",
                h.label()
            )),
            Constraint::Restricted if h.seals_by_flag() => Some(format!(
                "This {} build does not accept the flag that empties its built-in \
                 tools, so they are held back by a denylist instead — weaker, and \
                 not something Schemaic can guarantee. Updating the CLI restores \
                 the full seal.",
                h.label()
            )),
            // Neither a sandbox nor a flag we can name. Reached by no harness
            // today — OpenCode's grade is `Sealed` or `Unknown`, never this —
            // and worded to promise nothing rather than to be unreachable,
            // because "unreachable" is what the arm above assumed too.
            Constraint::Restricted => Some(format!(
                "Schemaic could not fully restrict {}'s built-in tools for this \
                 session, so it may be able to do more than answer questions about \
                 this database.",
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
            Harness::Claude | Harness::Codex | Harness::Antigravity | Harness::OpenCode
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
            // **`provider/model`, and the provider half is the point.** A bare
            // `claude-sonnet-5` is not a model id this CLI accepts; every entry
            // `opencode models` prints is qualified. These name the built-in
            // `opencode` provider, which is the one an install has without the
            // user adding credentials of their own — a user authenticated
            // straight to a vendor types `anthropic/…` or `openai/…` instead,
            // which the free-text field passes through untouched.
            Harness::OpenCode => &[
                "opencode/claude-sonnet-5",
                "opencode/claude-opus-5",
                "opencode/gpt-5",
                "opencode/gemini-3.1-pro",
            ],
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
            // `--variant`, whose help calls it "model variant (provider-specific
            // reasoning effort, e.g., high, max, minimal)". These are the three
            // that help text names, and deliberately not the union with anyone
            // else's: `medium` and `low` are Claude's and Antigravity's
            // vocabulary, and "provider-specific" means the accepted set is not
            // even constant across OpenCode's own models. `effort_arg` clamps to
            // this list, so a level carried over from another harness sends no
            // flag rather than an invented one.
            Harness::OpenCode => &["minimal", "high", "max"],
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

    /// When this harness is graded [`Constraint::Restricted`], is that an OS
    /// sandbox doing the restricting?
    ///
    /// The grade is shared; the mechanism behind it is not, and
    /// [`Constraint::notice`] has to describe the mechanism — "it cannot write
    /// files or run commands" is a claim the OS enforces for Codex and
    /// Antigravity and nothing enforces anywhere else.
    pub fn restricted_means_sandbox(self) -> bool {
        match self {
            Harness::Codex | Harness::Antigravity => true,
            // Claude's `Restricted` is the denylist fallback when `--tools` is
            // absent; OpenCode has no sandbox lever at all — it seals with a
            // `tools` map or not at all.
            Harness::Claude | Harness::OpenCode => false,
        }
    }

    /// Is this harness's seal a *flag* on its own command line, such that an
    /// older build missing that flag is the reason for a weaker grade?
    ///
    /// True only for Claude (`--tools`). OpenCode also reaches [`Constraint::Sealed`]
    /// but does it by configuration, so "updating the CLI restores the full seal"
    /// would be advice that fixes nothing there.
    pub fn seals_by_flag(self) -> bool {
        matches!(self, Harness::Claude)
    }

    /// Does a turn stream *incremental* text?
    ///
    /// Claude sends deltas, and so does Antigravity (`step_update.text_delta`).
    /// Codex restates a message cumulatively, which is what
    /// [`crate::stream::StreamParser`] coalesces; the panel uses this only to
    /// decide whether a first token means "it has started".
    ///
    /// **OpenCode is the one that sends neither.** Its printer emits a text part
    /// only after `time.end` is set, so the whole answer arrives in one event
    /// and there is no "it has started" moment to report — the panel's spinner
    /// runs until the text lands. That is a property of the CLI, not something
    /// coalescing can recover: no partial text is ever written to decode.
    pub fn streams_deltas(self) -> bool {
        matches!(self, Harness::Claude | Harness::Antigravity)
    }

    /// Does one process serve the whole conversation?
    ///
    /// Claude and Antigravity both hold a bidirectional `stream-json` pipe, so a
    /// turn is a line on stdin. The other two exit after each turn and are
    /// resumed by id, which is why [`crate::StreamEvent::SessionStarted`]
    /// exists.
    ///
    /// **Antigravity's was measured before it was driven**, which is what the
    /// flag's own help promises and not the same thing: `--input-format
    /// stream-json` reads one NDJSON message per line and runs a turn for each,
    /// and a two-turn probe against the installed binary kept one process alive,
    /// held the same `conversation_id`, counted `num_turns` up, and answered the
    /// second question from the first one's context.
    pub fn is_persistent(self) -> bool {
        matches!(self, Harness::Claude | Harness::Antigravity)
    }

    /// Can a previous turn be continued by id?
    ///
    /// **No longer the complement of [`Harness::is_persistent`], and the reason
    /// is [`Harness::session_interrupt`].** While Claude was the only persistent
    /// harness the two were the same question asked twice: a process that holds
    /// the conversation has no id to resume from. Antigravity holds the
    /// conversation *and* has no way to interrupt a turn in flight, so Stop ends
    /// the process — and the next turn has to pick the conversation back up by
    /// id, exactly as it did when every turn was its own process. It needs both
    /// answers, so the two can no longer be one.
    pub fn supports_resume(self) -> bool {
        match self {
            // The pipe is the continuity, and Stop is a control message that
            // leaves the process running.
            Harness::Claude => false,
            Harness::Codex | Harness::OpenCode | Harness::Antigravity => true,
        }
    }

    /// The stdin line that delivers one turn, for a [`Harness::is_persistent`]
    /// harness.
    ///
    /// Empty for the two that take their prompt in argv — there is no stdin
    /// protocol to encode for, and a caller reaching here for one is asking the
    /// wrong question.
    pub fn session_turn_line(self, text: &str) -> String {
        match self {
            Harness::Claude => crate::user_message_line(text),
            // **Its own envelope, not Claude's.** Measured: the shape is
            // `{"event":…}` mirroring what it *writes*, and Claude's
            // `{"type":"user"}` is refused with `stream input message is missing
            // the "event" field`. `role` is accepted but optional.
            Harness::Antigravity => {
                let v = serde_json::json!({
                    "event": "user",
                    "message": { "content": text }
                });
                format!("{v}\n")
            }
            Harness::Codex | Harness::OpenCode => String::new(),
        }
    }

    /// The stdin line that ends a turn in flight, or `None` where the only way
    /// to stop one is to end the process.
    ///
    /// **Antigravity has no such message, and that was measured rather than
    /// assumed**: an unrecognised event on its stdin is *silently ignored*, so a
    /// guessed `interrupt` would not error — it would hang, with the turn still
    /// running and the panel waiting on a stop that never came. `None` is the
    /// honest answer, and the caller kills the child instead.
    pub fn session_interrupt(self) -> Option<String> {
        match self {
            Harness::Claude => Some(crate::interrupt_line("stop")),
            Harness::Antigravity | Harness::Codex | Harness::OpenCode => None,
        }
    }

    /// Does the system context have to ride in the first turn's own text?
    ///
    /// Claude has `--append-system-prompt`; Antigravity has no such flag, and on
    /// the per-turn path [`turn_args`] folds the context into every prompt. One
    /// process holding the conversation only needs it once — repeating it would
    /// re-send the whole schema outline on every question, which is most of what
    /// persistence was for.
    pub fn session_system_in_first_turn(self) -> bool {
        matches!(self, Harness::Antigravity)
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
        // **The only harness whose seal is not a flag**, which is why this arm
        // greps for something other than the lever it depends on.
        //
        // OpenCode's built-in tools are emptied by an *agent definition* — a
        // `tools` map with every documented name set to `false`, verified
        // against `opencode debug agent`, which resolves it to eleven disabled
        // tools and nothing live. Its MCP isolation is an *environment variable*,
        // `XDG_CONFIG_HOME`, pointed at a directory Schemaic owns: measured, the
        // user's own globally-registered servers disappear from
        // `opencode debug config`. Neither of those appears in `--help`, so
        // there is nothing to grep for that would confirm the seal itself.
        //
        // `--pure` is greppable, is genuinely part of the constraint — it stops
        // the user's external plugins, which are arbitrary code, from loading
        // into the session — and is passed on every turn. So it stands as the
        // evidence that this binary is the OpenCode these mechanisms were
        // measured against. A binary without it is not one we can claim to have
        // sealed, and `Unknown` refuses the session rather than assuming.
        //
        // The grade is `Sealed` rather than `Restricted` because the tool set
        // really is empty, exactly as `claude --tools ""` empties it — not a
        // sandbox that leaves readers live. The other half of that promise is
        // enforced at the call site: `crate::harness::opencode_config_json` is
        // the only way the agent is defined, and `ai::start_ai_session` refuses
        // the turn outright if it cannot be written, rather than falling back to
        // a default agent that has every tool.
        Harness::OpenCode => {
            if crate::mentions_flag(help, "--pure") {
                Constraint::Sealed
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
    // **Trimmed, because the field it comes from is free text the user can
    // clear.** Typing a space and closing the settings modal leaves `" "`, which
    // `!is_empty()` waves through as `--model " "` — an unknown model, reported
    // to the user as "Couldn't launch the CLI", which is the one explanation
    // that is not the problem. `build_session_args` already trims on Claude's
    // path; these three did not, and that divergence was the whole bug.
    let model = spec.model.trim();
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
            if !model.is_empty() {
                a.push("--model".into());
                a.push(model.to_string());
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
        // Antigravity took a per-turn command line until its bidirectional mode
        // was measured; it is spawned once per conversation now, by
        // `session_args`. One harness, one spawn shape — a second one kept
        // "just in case" is a command line nothing builds and nobody re-measures.
        Harness::Antigravity => Vec::new(),
        Harness::OpenCode => {
            let mut a: Vec<String> = vec![
                "run".into(),
                // **Not a performance flag, though it is also that.** `--pure`
                // runs without external plugins, which is the third leg of the
                // seal (the config directory and the agent's empty `tools` map
                // being the other two): a plugin is arbitrary code the user
                // installed, and nothing else here would keep it out.
                //
                // It is *also* what makes the first turn survivable. A config
                // directory OpenCode has not seen before makes it bootstrap its
                // plugin runtime — a real npm install into that directory,
                // measured at over three minutes and producing not one line of
                // output before it finished. With `--pure` the same first turn
                // took four seconds. A user would have read the difference as a
                // hang.
                "--pure".into(),
                // The sealed agent from `opencode_config_json`. Naming it is
                // what selects the empty tool set; without it the session runs
                // as `build`, which has every built-in.
                "--agent".into(),
                OPENCODE_AGENT.into(),
                "--format".into(),
                "json".into(),
            ];
            if !model.is_empty() {
                a.push("--model".into());
                a.push(model.to_string());
            }
            if !spec.effort.is_empty() {
                a.push("--variant".into());
                a.push(spec.effort.clone());
            }
            if let Some(id) = spec.resume.as_deref().filter(|s| !s.is_empty()) {
                a.push("--session".into());
                a.push(id.into());
            }
            // **No `--auto`.** Its own help calls it dangerous, and it is not
            // needed: measured, a headless turn under this agent called its tool
            // and returned without ever asking for permission, because OpenCode's
            // default permission set allows rather than denies. That is the
            // opposite of Antigravity, where an unprompted tool is auto-*denied*
            // and the turn reports success with an empty answer — the failure
            // that made `AgyRegistration`'s allow-rules necessary. Passing
            // `--auto` here would buy nothing and pre-approve anything a future
            // build adds to the tool set.
            //
            // Prompt last: it is the positional argument.
            a.push(prefixed_prompt(turn_system(spec), &spec.prompt));
            a
        }
    }
}

/// The argv for a **persistent** session — one process for the whole
/// conversation, with turns arriving on stdin.
///
/// The counterpart of [`turn_args`], which is what the two per-turn harnesses
/// take instead; each returns an empty vector for a harness that uses the other,
/// so a caller that asks the wrong one gets nothing rather than a plausible
/// command line for the wrong shape.
///
/// **Antigravity carries no prompt here, and that is the whole difference.** Its
/// `-p` takes the prompt as the flag's *value*, so leaving the flag in with
/// nothing to give it makes the CLI read the next flag as the prompt — measured,
/// and it says so itself: *"-p took `--input-format` as its prompt"*. In
/// bidirectional mode the prompt comes from stdin, so the flag goes entirely.
pub fn session_args(
    h: Harness,
    spec: &TurnSpec,
    seal: crate::CliSeal,
    mcp_tools: &[&str],
) -> Vec<String> {
    let model = spec.model.trim();
    match h {
        Harness::Claude => crate::build_session_args(
            &spec.system,
            Some(model),
            Some(spec.effort.trim()),
            spec.mcp_config.as_deref(),
            mcp_tools,
            seal,
        ),
        Harness::Antigravity => {
            let mut a: Vec<String> = vec![
                "--input-format".into(),
                "stream-json".into(),
                // Its own help: `stream-json` on stdin *requires* this on stdout.
                "--output-format".into(),
                "stream-json".into(),
                "--sandbox".into(),
                "--disable-slash-commands".into(),
            ];
            if !model.is_empty() {
                a.push("--model".into());
                a.push(model.to_string());
            }
            if !spec.effort.trim().is_empty() {
                a.push("--effort".into());
                a.push(spec.effort.trim().to_string());
            }
            // **Only after a Stop.** The pipe is the continuity while the process
            // lives; this is how the conversation is picked back up once Stop has
            // had to end it, since there is no interrupt to send instead.
            if let Some(id) = spec.resume.as_deref().filter(|s| !s.is_empty()) {
                a.push("--conversation".into());
                a.push(id.into());
            }
            a
        }
        Harness::Codex | Harness::OpenCode => Vec::new(),
    }
}

/// The name of the agent [`opencode_config_json`] defines and
/// [`turn_args`] selects with `--agent`.
///
/// One constant because the two must agree: a definition nothing selects leaves
/// the session on OpenCode's own `build` agent, which has every built-in tool —
/// the seal silently absent rather than reported missing.
pub const OPENCODE_AGENT: &str = "schemaic";

/// What the transcript prints over one assistant turn, given the harness that
/// produced it.
///
/// **The turn's own harness, not the one selected now.** A conversation can span
/// several: the user switches CLI in Settings mid-thread and asks the next
/// question, which is exactly how the panel's hard-coded "CLAUDE" was found
/// sitting over an Antigravity answer. Reading the *live* setting would fix that
/// label and break every earlier one, retroactively attributing Claude's replies
/// to whichever CLI happens to be selected when the panel is next drawn — a
/// worse failure, because the transcript would then be wrong about history
/// rather than merely wrong about now. So the key is stamped on the message when
/// the turn starts and read back from there.
///
/// `None` and unrecognised both answer "ASSISTANT" rather than a guess.
///
/// `None` is a transcript persisted before the field existed — which means a
/// build shipping Claude, Codex and Antigravity, the three [`Harness::ALL`] held
/// when the field was added alongside [`Harness::OpenCode`]. Any of the three
/// could have written it and nothing on disk says which, so there is no safe
/// default; naming Claude would be [`Harness::from_key`]'s refusal-to-guess rule
/// broken at the one place the user can read the result.
///
/// An unrecognised key is the same problem from the other direction: a
/// transcript written by a *later* build, naming a harness this one does not
/// have.
pub fn speaker_label(harness_key: Option<&str>) -> String {
    harness_key
        .and_then(Harness::from_key)
        .map(|h| h.speaker_name().to_uppercase())
        .unwrap_or_else(|| "ASSISTANT".to_string())
}

/// The name Schemaic registers its MCP server under.
///
/// It exists because **one dialect has to decode a name it wrote itself.** Codex
/// reports `server` and `tool` as separate fields, so its decoder rebuilds the
/// qualified name from the event; OpenCode reports one flattened `tool`, and the
/// only way to read `schemaic_run_query` as *our* `run_query` is to know the
/// prefix we registered. A literal on both sides would be a rename away from a
/// transcript that labels every database call by its raw name.
///
/// Deliberately not [`OPENCODE_AGENT`], which happens to be the same string
/// today and answers a different question — the agent is the tool *set*, this is
/// the tool *source*. Collapsing them is how one rename silently becomes two.
pub const MCP_SERVER: &str = "schemaic";

/// The `opencode.json` that seals an OpenCode session and gives it our MCP
/// server.
///
/// This is OpenCode's answer to Claude's `--mcp-config` plus `--tools ""`, and
/// to Codex's `-c mcp_servers={…}`: one file, holding both halves.
///
/// **The seal is the `tools` map, and it must name every tool to close.** The
/// map is a denylist by omission — anything not listed stays enabled — so the
/// entries are the names `opencode agent create --permissions` documents.
/// Verified rather than assumed: `opencode debug agent schemaic`, run against
/// the JSON this function emits, reports every one of them disabled.
///
/// It reports one tool still enabled, `invalid`, and that is not a hole. Its own
/// description is *"Do not use"*, its `execute` returns nothing but a message
/// that the arguments were malformed, and the CLI excludes it from the set it
/// offers the model (`activeTools: …filter(m => m !== "invalid")`). It is the
/// fallback for a tool call that failed to parse, not a capability — which is
/// written down here because the alternative is discovering it again in a probe
/// and wondering whether the seal leaks.
///
/// **The endpoint is not here, for the reason it is not in Codex's `-c`
/// override.** It carries the database credentials, and this file outlives the
/// session: the directory is reused so the plugin bootstrap described in
/// [`turn_args`] is paid at most once, which means anything written here stays
/// on disk after the app closes. The credentials travel in the separate
/// `--endpoint-file` written per session and swept, and only its *path* appears
/// here — the same split the Codex path makes for a different reason.
///
/// **`mcp` is assigned, not merged into.** Whatever the user has registered
/// globally is displaced by pointing `XDG_CONFIG_HOME` at the directory holding
/// this file, so their servers are not in the resolved config at all. That is
/// this harness's `--strict-mcp-config`, and unlike Antigravity's it costs no
/// global state: nothing of the user's is edited, so nothing has to be put back.
pub fn opencode_config_json(exe: &str, endpoint_file: &str, allowed: &[&str]) -> String {
    // Every tool `opencode agent create --permissions` lists, all off.
    //
    // **Twelve are written and eleven come back, and the direction of that gap
    // is the whole point.** `opencode debug agent` resolves this map against the
    // build's own tool registry: `lsp` and `websearch` are documented by
    // `--permissions` but are not registered tools in the measured build, so
    // they are dropped; `question` is not written here and comes back disabled
    // anyway. The map is a **denylist by omission** — a name absent from it
    // stays *enabled* — so a name here that the CLI no longer has costs nothing,
    // while a name the CLI gains and this list has not heard of is live. That
    // asymmetry is why the list is deliberately over-inclusive, and why it sits
    // next to the probe that can confirm what actually resolved.
    let tools = sealed_tools();
    // The allow-list rides in the agent's description rather than a permission
    // rule: OpenCode allows MCP tools by default (see `turn_args`), so there is
    // no per-tool approval to set, and the server itself refuses anything this
    // connection's access level does not offer. Naming them keeps the model from
    // spending a turn discovering that.
    let offered = allowed
        .iter()
        .map(|t| t.rsplit("__").next().unwrap_or(t))
        .collect::<Vec<_>>()
        .join(", ");
    serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "mcp": {
            MCP_SERVER: {
                "type": "local",
                "enabled": true,
                "command": [exe, "--mcp-serve", "--endpoint-file", endpoint_file],
            }
        },
        "agent": {
            OPENCODE_AGENT: {
                "mode": "primary",
                "description": format!(
                    "Schemaic's SQL assistant. Database tools available: {offered}."
                ),
                "tools": tools,
            }
        }
    })
    .to_string()
}

/// Where a one-shot generation's reply comes back from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineOutput {
    /// The child's stdout **is** the reply, verbatim.
    Stdout,
    /// The reply is in the file whose path [`InlineSpec::last_message`] put in
    /// argv; stdout is the CLI's own human-facing rendering and is discarded.
    LastMessageFile,
}

/// Everything one inline (Ctrl+K / AI Fill / AI Seed) generation needs.
pub struct InlineSpec {
    /// What the user asked for.
    pub intent: String,
    /// Schema outline and house rules.
    pub system: String,
    /// Model id, verbatim. Empty = the harness's own default.
    pub model: String,
    /// Reasoning effort as the user set it, in whatever vocabulary the harness
    /// they set it under uses. **Clamped here** rather than by the caller — see
    /// [`inline_argv`].
    pub effort: String,
    /// Claude's seal flags, from the probe. Ignored by every other harness,
    /// whose constraint is a sandbox flag baked into the argv below.
    pub seal: crate::CliSeal,
    /// Pass Codex's `--ignore-user-config`, when the probe saw it.
    pub isolate_config: bool,
    /// Where Codex should write its last message. Ignored by the three
    /// harnesses whose [`inline_output`] is [`InlineOutput::Stdout`].
    pub last_message: String,
}

/// Where to read the reply for `h`.
pub fn inline_output(h: Harness) -> InlineOutput {
    match h {
        Harness::Codex => InlineOutput::LastMessageFile,
        Harness::Claude | Harness::Antigravity | Harness::OpenCode => InlineOutput::Stdout,
    }
}

/// The argv for one inline generation on any harness.
///
/// **The closed counterpart of [`turn_args`].** A chat turn is given a server
/// and may resume a thread; this is neither. Ctrl+K, AI Fill and AI Seed each
/// want one string back and read it with a parser — none has a surface that
/// could render a tool call, and each discards everything but the value it
/// parses out, so a tool call here would leave no trace anywhere. The whole
/// request is in the prompt, which is why no `--mcp-config`, no `-c` override
/// carrying a server, and no resume id appears below on any harness
/// (`no_inline_generation_is_given_a_server_or_a_session`).
///
/// **Claude's is [`crate::inline_args`], unchanged.** It was the only one of
/// these for as long as the other three spawned Claude regardless of the
/// picker; it is now one arm of four rather than the path all of them took.
pub fn inline_argv(h: Harness, spec: &InlineSpec) -> Vec<String> {
    // Trimmed for the reason `turn_args` trims: the field is free text the user
    // can clear, and `--model " "` dies as an unknown model under "couldn't
    // launch the CLI" — the one explanation that is not the problem.
    let model = spec.model.trim();
    // **Clamped here rather than by the caller**, unlike `turn_args`, whose
    // caller hands it an already-answered `effort_arg`. Three call sites reach
    // this one and each would have had to remember; the setting outlives a
    // harness switch, so a level from the previous harness's vocabulary is the
    // ordinary case rather than the exotic one.
    let effort = h.effort_arg(spec.effort.trim()).unwrap_or_default();
    match h {
        Harness::Claude => crate::inline_args(&spec.intent, &spec.system, model, effort, spec.seal),
        Harness::Codex => {
            let mut a: Vec<String> = vec![
                "exec".into(),
                // No session file for a request with nothing to resume.
                "--ephemeral".into(),
                // The session cwd is a private app directory, not a git repo.
                "--skip-git-repo-check".into(),
                "--sandbox".into(),
                "read-only".into(),
                // stdout is a human rendering here, not a stream we decode; keep
                // the escapes out of the file we do read.
                "--color".into(),
                "never".into(),
            ];
            if spec.isolate_config {
                a.push("--ignore-user-config".into());
            }
            if !model.is_empty() {
                a.push("--model".into());
                a.push(model.to_string());
            }
            // The constraint, as on the session path: `--sandbox` alone is not
            // enough, because the config key is the mechanism accepted on every
            // Codex path. Nothing a caller adds can displace it — there is no
            // caller-supplied override on this path at all.
            a.push("-c".into());
            a.push("sandbox_mode=\"read-only\"".into());
            // **The reply, and why it is a file.** `codex exec` without
            // `--json` prints for a person: measured, this build writes the
            // final message alone, but that is an observation about a
            // human-facing surface rather than a promise. `-o` is the promise —
            // its help says "file where the last message from the agent should
            // be written" — so the parser reads that and never the pipe.
            a.push("-o".into());
            a.push(spec.last_message.clone());
            // Prompt last: it is the positional argument.
            a.push(prefixed_prompt(&spec.system, &spec.intent));
            a
        }
        Harness::Antigravity => {
            let mut a: Vec<String> = vec![
                "-p".into(),
                prefixed_prompt(&spec.system, &spec.intent),
                // `text` is the default, and named anyway: the session path asks
                // this same binary for `stream-json`, and a default that moved
                // would put a JSONL envelope where a parser expects SQL.
                "--output-format".into(),
                "text".into(),
                "--sandbox".into(),
                "--disable-slash-commands".into(),
            ];
            if !model.is_empty() {
                a.push("--model".into());
                a.push(model.to_string());
            }
            if !effort.is_empty() {
                a.push("--effort".into());
                a.push(effort.to_string());
            }
            a
        }
        Harness::OpenCode => {
            let mut a: Vec<String> = vec![
                "run".into(),
                "--pure".into(),
                "--agent".into(),
                OPENCODE_AGENT.into(),
                // `default` is the formatted mode, and formatted is exactly what
                // it is not: measured, the decoration and the model banner go to
                // **stderr** and stdout carries the answer alone.
                "--format".into(),
                "default".into(),
            ];
            if !model.is_empty() {
                a.push("--model".into());
                a.push(model.to_string());
            }
            if !effort.is_empty() {
                a.push("--variant".into());
                a.push(effort.to_string());
            }
            a.push(prefixed_prompt(&spec.system, &spec.intent));
            a
        }
    }
}

/// The OpenCode config for an inline generation: the sealed agent, and no
/// server at all.
///
/// **The difference from [`opencode_config_json`] is the whole `mcp` block, and
/// it is deliberate.** That one registers Schemaic's server because a chat turn
/// is meant to reach the database; this one answers from the prompt alone, so
/// registering a server would hand a tool to the one path with nowhere to show
/// that it was called. The agent is still named and its tool map is still empty
/// — without the config the `--agent` in [`inline_argv`] selects nothing and
/// OpenCode falls back to `build`, which has every built-in.
pub fn opencode_inline_config_json() -> String {
    serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "agent": {
            OPENCODE_AGENT: {
                "mode": "primary",
                "description": "Schemaic's one-shot SQL generator. No tools; answer from the prompt.",
                "tools": sealed_tools(),
            }
        }
    })
    .to_string()
}

/// Every built-in `opencode agent create --permissions` lists, all off.
///
/// One list, shared by the session config and the inline one, so the two cannot
/// come to disagree about which built-ins are shut off — the same reason
/// `seal_args` is shared by Claude's two argv builders.
fn sealed_tools() -> serde_json::Map<String, serde_json::Value> {
    // **Twelve are written and eleven come back, and the direction of that gap
    // is the point.** The map is a denylist by omission — a name absent from it
    // stays *enabled* — so a name here the CLI no longer has costs nothing,
    // while a name the CLI gains and this list has not heard of is live.
    [
        "bash",
        "read",
        "edit",
        "write",
        "glob",
        "grep",
        "webfetch",
        "websearch",
        "task",
        "todowrite",
        "lsp",
        "skill",
    ]
    .iter()
    .map(|t| ((*t).to_string(), serde_json::Value::Bool(false)))
    .collect()
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

/// What one of the settings surgeries below asks its caller to do.
///
/// **The "nothing changed" answer has to come from here**, because nothing
/// outside can compute it. The caller used to decide by comparing the returned
/// text against the bytes it read, which is a comparison that never succeeds: a
/// `to_string_pretty` re-serialization carries no trailing newline, and with no
/// `preserve_order` feature in this workspace `serde_json::Map` is a `BTreeMap`,
/// so the user's keys come back sorted. Every Antigravity user therefore had
/// another vendor's configuration file truncated and rewritten on **every**
/// launch — including everyone who never opened the AI panel, since the startup
/// sweep withdraws rules that are usually not there.
///
/// Rewriting is not free even when the content is equivalent: it re-sorts keys,
/// strips the trailing newline, collapses CRLF, normalizes numbers and collapses
/// duplicate keys. It is a fair price for an edit the user asked for, and no
/// price at all is right for an edit that removes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsEdit {
    /// The document already says what it was asked to say. **Write nothing.**
    Unchanged,
    /// Replace the file with this text.
    Write(String),
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
/// Idempotent, and it says so: adding a rule that is already there answers
/// [`SettingsEdit::Unchanged`], so a crashed session that left rules behind
/// costs the next launch neither a duplicate nor a rewrite.
pub fn antigravity_settings_with_rules(current: &str, rules: &[String]) -> Option<SettingsEdit> {
    let mut doc = parse_settings(current)?;
    let mut added = false;
    {
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
                added = true;
            }
        }
    }
    if !added {
        return Some(SettingsEdit::Unchanged);
    }
    Some(SettingsEdit::Write(render_settings(doc)?))
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
/// shape it had before — but **only the ones this call emptied**. The pruning
/// used to test the *post-`retain`* state instead, so a user whose own file
/// already held `{"permissions":{"allow":[]}}` had that key deleted on a launch
/// where Schemaic had added nothing and withdrawn nothing.
pub fn antigravity_settings_without_rules(current: &str, rules: &[String]) -> Option<SettingsEdit> {
    let mut doc = parse_settings(current)?;
    let mut removed = false;
    if let Some(perms) = doc.get_mut("permissions").and_then(|p| p.as_object_mut()) {
        let mut emptied_allow = false;
        if let Some(arr) = perms.get_mut("allow").and_then(|a| a.as_array_mut()) {
            let before = arr.len();
            arr.retain(|v| !v.as_str().is_some_and(|s| rules.iter().any(|r| r == s)));
            removed = arr.len() != before;
            emptied_allow = removed && arr.is_empty();
        }
        if emptied_allow {
            perms.remove("allow");
            if perms.is_empty() {
                doc.remove("permissions");
            }
        }
    }
    if !removed {
        return Some(SettingsEdit::Unchanged);
    }
    Some(SettingsEdit::Write(render_settings(doc)?))
}

/// Serialize a settings document back out, with the trailing newline every
/// editor and CLI that writes this file leaves on it.
fn render_settings(doc: serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let mut s = serde_json::to_string_pretty(&serde_json::Value::Object(doc)).ok()?;
    s.push('\n');
    Some(s)
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

    /// **A sandbox is never a seal, however good the help page looks.**
    ///
    /// This was `only_claude_can_reach_the_sealed_grade`, and both halves of
    /// that name had stopped being true: OpenCode reaches `Sealed` too, and the
    /// test never asserted "only" in the first place — it checked three
    /// harnesses one at a time, so a fourth could have reached any grade at all
    /// without failing it. The name was the claim; nothing under it was.
    ///
    /// It now runs over `Harness::ALL`, so the grade of every harness is
    /// asserted and a new one has to be *decided* here rather than inheriting
    /// whatever `constraint_from_help` happens to return.
    #[test]
    fn each_harness_reaches_exactly_the_grade_its_mechanism_earns() {
        // Each harness's *best case*: a readable help page listing the flag its
        // own arm greps for.
        let best = |h: Harness| match h {
            Harness::Claude => "Usage: claude\n  --tools <t>\n  --help\n",
            Harness::Codex => "Usage: codex\n  --sandbox <s>\n  --help\n",
            Harness::Antigravity => "Usage: agy\n  --sandbox\n  --help\n",
            Harness::OpenCode => "Usage: opencode\n  --pure\n  --help\n",
        };
        for h in Harness::ALL {
            let got = constraint_from_help(h, best(h));
            let want = match h {
                // Empties the built-in set: `--tools ""` and an agent whose
                // `tools` map is all false.
                Harness::Claude | Harness::OpenCode => Constraint::Sealed,
                // A sandbox blocks side effects and leaves the readers live.
                // There is no flag on either that empties the tool set, so
                // neither can ever be `Sealed` — the claim this test exists to
                // hold.
                Harness::Codex | Harness::Antigravity => Constraint::Restricted,
            };
            assert_eq!(got, want, "{h:?}");
        }
    }

    /// The other direction, for every harness at once: no help page at all means
    /// no grade, and no session.
    #[test]
    fn an_unreadable_probe_refuses_every_harness() {
        for h in Harness::ALL {
            assert_eq!(constraint_from_help(h, ""), Constraint::Unknown, "{h:?}");
            assert_eq!(
                constraint_from_help(h, "segmentation fault"),
                Constraint::Unknown,
                "{h:?}"
            );
        }
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

    /// **This used to assert the complement, and that rule is deliberately
    /// gone.** It held while Claude was the only persistent harness — a process
    /// that owns the conversation has no id to resume from. Antigravity holds
    /// the conversation *and* cannot interrupt a turn, so Stop ends the process
    /// and the next turn resumes by id: it needs both answers. What survives is
    /// the weaker rule that actually matters, which is that every harness has at
    /// least one way to continue a conversation. See
    /// `persistence_and_resume_are_no_longer_complements`.
    #[test]
    fn every_harness_can_continue_a_conversation_somehow() {
        for h in Harness::ALL {
            assert!(
                h.supports_resume() || h.is_persistent(),
                "{h:?} can neither hold a conversation nor resume one"
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
        // The harnesses that still take a per-turn argv. Antigravity used to be
        // one of them and now holds the outline in the first stdin message
        // instead (`session_system_in_first_turn`).
        for h in [Harness::Codex, Harness::OpenCode] {
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
        for h in [Harness::Codex, Harness::OpenCode] {
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

    /// The text a surgery asked for, or a panic naming which answer came back.
    /// Every test below that asserts on *content* wants a real write; the ones
    /// that assert on `Unchanged` say so directly.
    fn written(edit: Option<SettingsEdit>) -> String {
        match edit {
            Some(SettingsEdit::Write(s)) => s,
            Some(SettingsEdit::Unchanged) => panic!("expected a write, got Unchanged"),
            None => panic!("expected a write, the document was declined"),
        }
    }

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
        let out = written(antigravity_settings_with_rules(AGY_SETTINGS, &rules));
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        // The user's own key survives untouched — this is their file.
        assert_eq!(v["trustedWorkspaces"][0], "C:\\Users\\jonid");
        assert_eq!(v["permissions"]["allow"][0], "mcp(schemaic/list_schema)");
    }

    #[test]
    fn adding_the_same_rule_twice_does_not_duplicate_it() {
        // A crashed session leaves rules behind; the next one must not stack.
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        let once = written(antigravity_settings_with_rules(AGY_SETTINGS, &rules));
        // The second grant has nothing to add, so it asks for no write at all —
        // and what it would have written still holds exactly one rule.
        assert_eq!(
            antigravity_settings_with_rules(&once, &rules),
            Some(SettingsEdit::Unchanged)
        );
        let v: serde_json::Value = serde_json::from_str(&once).expect("json");
        assert_eq!(
            v["permissions"]["allow"].as_array().expect("array").len(),
            1
        );
    }

    #[test]
    fn removing_our_rules_restores_the_original_shape() {
        let rules =
            antigravity_allow_rules(&["mcp__schemaic__list_schema", "mcp__schemaic__run_query"]);
        let added = written(antigravity_settings_with_rules(AGY_SETTINGS, &rules));
        let back = written(antigravity_settings_without_rules(&added, &rules));
        let v: serde_json::Value = serde_json::from_str(&back).expect("json");
        assert_eq!(v["trustedWorkspaces"][0], "C:\\Users\\jonid");
        // The scaffolding is gone, not left behind empty.
        assert!(v.get("permissions").is_none(), "{back}");
    }

    #[test]
    fn a_rule_we_did_not_add_is_left_alone() {
        let mine = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        let with_theirs = written(antigravity_settings_with_rules(
            AGY_SETTINGS,
            &["mcp(other/their_tool)".to_string()],
        ));
        let both = written(antigravity_settings_with_rules(&with_theirs, &mine));
        let back = written(antigravity_settings_without_rules(&both, &mine));
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

    /// **The launch sweep must not rewrite a file it took nothing out of.**
    /// `edit_settings` used to decide that by comparing the pure layer's
    /// `to_string_pretty` output against the bytes it read — which never matches
    /// a file an editor or a CLI wrote, because that output carries no trailing
    /// newline and (this workspace does not enable `preserve_order`) comes back
    /// with the user's keys sorted. So every Antigravity user had another
    /// vendor's settings truncated, re-sorted and rewritten at **every** launch,
    /// including the ones who never opened the AI panel. Only the pure layer can
    /// answer "did anything change", so it does.
    #[test]
    fn withdrawing_from_a_file_that_holds_none_of_our_rules_changes_nothing() {
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        for untouched in [
            AGY_SETTINGS,
            r#"{"permissions":{"allow":["mcp(other/their_tool)"]}}"#,
            "{}",
            "",
        ] {
            assert_eq!(
                antigravity_settings_without_rules(untouched, &rules),
                Some(SettingsEdit::Unchanged),
                "rewrote a file it removed nothing from: {untouched}"
            );
        }
    }

    /// The other half of the same rule: granting a rule that is already granted
    /// is not a reason to rewrite the user's file either. A crashed session
    /// leaves its rules behind, so this is the *common* case on the next launch.
    #[test]
    fn granting_a_rule_that_is_already_there_changes_nothing() {
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        let once = written(antigravity_settings_with_rules(AGY_SETTINGS, &rules));
        assert_eq!(
            antigravity_settings_with_rules(&once, &rules),
            Some(SettingsEdit::Unchanged)
        );
    }

    /// **Prune only what we emptied.** The pruning tested the *post-`retain`*
    /// state rather than whether `retain` removed anything, so a user who
    /// already had `{"permissions":{"allow":[]}}` — Schemaic having added
    /// nothing — lost the key on the next launch's sweep. Both the function's
    /// own doc and `docs/architecture.md` claim it prunes only containers it
    /// emptied.
    #[test]
    fn an_empty_container_we_did_not_empty_is_left_where_it_was() {
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        let theirs = r#"{"permissions":{"allow":[]},"trustedWorkspaces":["/home/x"]}"#;
        assert_eq!(
            antigravity_settings_without_rules(theirs, &rules),
            Some(SettingsEdit::Unchanged),
            "deleted a key the user's own file already had"
        );
        // And an `allow` that held only somebody else's rule keeps its
        // container even though ours is gone from it.
        let mixed = format!(
            r#"{{"permissions":{{"allow":["mcp(other/t)","{}"]}}}}"#,
            rules[0]
        );
        let out = written(antigravity_settings_without_rules(&mixed, &rules));
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(
            v["permissions"]["allow"].as_array().expect("array").len(),
            1
        );
    }

    #[test]
    fn a_fresh_install_with_an_empty_file_still_gets_its_rules() {
        let rules = antigravity_allow_rules(&["mcp__schemaic__list_schema"]);
        for empty in ["", "   ", "\n"] {
            let out = written(antigravity_settings_with_rules(empty, &rules));
            let v: serde_json::Value = serde_json::from_str(&out).expect("json");
            assert_eq!(v["permissions"]["allow"][0], "mcp(schemaic/list_schema)");
        }
    }
}

#[cfg(test)]
mod opencode_tests {
    use super::*;

    fn spec() -> TurnSpec {
        TurnSpec {
            prompt: "count rows".into(),
            system: "tables: users(id)".into(),
            ..Default::default()
        }
    }

    fn args_of(s: &TurnSpec) -> Vec<String> {
        turn_args(Harness::OpenCode, s)
    }

    fn flag_value(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .map(|i| args[i + 1].clone())
    }

    #[test]
    fn a_turn_runs_the_sealed_agent_and_asks_for_json() {
        let a = args_of(&spec());
        assert_eq!(a[0], "run");
        assert!(a.contains(&"--pure".to_string()), "{a:?}");
        assert_eq!(flag_value(&a, "--agent").as_deref(), Some(OPENCODE_AGENT));
        assert_eq!(flag_value(&a, "--format").as_deref(), Some("json"));
    }

    #[test]
    fn the_prompt_is_the_last_argument_and_carries_the_schema() {
        // It is positional, so anything appended after it would be read as more
        // prompt.
        let a = args_of(&spec());
        let last = a.last().expect("prompt");
        assert!(last.starts_with("tables: users(id)"), "{last}");
        assert!(last.ends_with("count rows"), "{last}");
    }

    #[test]
    fn a_resumed_turn_names_the_session_and_drops_the_schema() {
        // Same rule as the other per-turn harnesses: a resumed thread replays
        // every earlier turn, each already carrying the outline, so re-sending
        // it pays for the catalogue once per turn.
        let mut s = spec();
        s.resume = Some("ses_abc".into());
        let a = args_of(&s);
        assert_eq!(flag_value(&a, "--session").as_deref(), Some("ses_abc"));
        assert_eq!(a.last().map(String::as_str), Some("count rows"));
    }

    #[test]
    fn an_empty_resume_is_not_a_session_flag() {
        let mut s = spec();
        s.resume = Some(String::new());
        assert!(!args_of(&s).contains(&"--session".to_string()));
    }

    #[test]
    fn an_empty_model_sends_no_model_flag() {
        // `--model ""` is what a harness switch produces: the field is cleared
        // outright, and this harness's ids are `provider/model`, so an empty one
        // is not merely useless but unparseable.
        let s = spec();
        assert!(s.model.is_empty());
        assert!(!args_of(&s).contains(&"--model".to_string()));
    }

    #[test]
    fn a_model_is_passed_through_verbatim() {
        let mut s = spec();
        s.model = "anthropic/claude-sonnet-5".into();
        assert_eq!(
            flag_value(&args_of(&s), "--model").as_deref(),
            Some("anthropic/claude-sonnet-5")
        );
    }

    #[test]
    fn effort_rides_on_variant_not_effort() {
        // `--effort` is Antigravity's flag and Claude's; this CLI calls it
        // `--variant` and would die on an unknown option.
        let mut s = spec();
        s.effort = "high".into();
        let a = args_of(&s);
        assert!(!a.contains(&"--effort".to_string()), "{a:?}");
        assert_eq!(flag_value(&a, "--variant").as_deref(), Some("high"));
    }

    #[test]
    fn another_harness_effort_level_is_clamped_before_it_reaches_argv() {
        // The composition the app performs: `effort_arg` first, `turn_args`
        // second. Claude's `xhigh` and Antigravity's `medium` survive a harness
        // switch in settings, and neither is a variant this CLI advertises.
        for level in ["xhigh", "medium", "low"] {
            assert_eq!(Harness::OpenCode.effort_arg(level), None, "{level}");
            let mut s = spec();
            s.effort = Harness::OpenCode
                .effort_arg(level)
                .unwrap_or_default()
                .into();
            let a = args_of(&s);
            assert!(!a.contains(&"--variant".to_string()), "{level}: {a:?}");
        }
        assert_eq!(Harness::OpenCode.effort_arg("max"), Some("max"));
    }

    #[test]
    fn a_turn_never_passes_the_auto_approve_flag() {
        // Measured: a headless turn calls its tools without it, because this
        // CLI's default permissions allow rather than deny. Its own help calls
        // the flag dangerous, and it would pre-approve whatever a future build
        // adds to the tool set.
        let mut s = spec();
        s.effort = "max".into();
        s.model = "opencode/gpt-5".into();
        s.resume = Some("ses_1".into());
        assert!(!args_of(&s).contains(&"--auto".to_string()));
    }

    #[test]
    fn no_other_harness_flags_leak_into_the_argv() {
        // The failure this whole layer exists to stop: one CLI's vocabulary
        // reaching another. None of these is an OpenCode flag.
        let mut s = spec();
        s.model = "opencode/gpt-5".into();
        s.effort = "high".into();
        s.mcp_overrides = vec!["mcp_servers={}".into()];
        s.isolate_config = true;
        s.mcp_config = Some("/tmp/x.json".into());
        let a = args_of(&s);
        for foreign in [
            "--sandbox",
            "--ignore-user-config",
            "--skip-git-repo-check",
            "-c",
            "--mcp-config",
            "--conversation",
            "--output-format",
            "--disable-slash-commands",
            "-p",
            "exec",
        ] {
            assert!(!a.contains(&foreign.to_string()), "{foreign} in {a:?}");
        }
    }

    #[test]
    fn the_constraint_is_sealed_only_when_the_probe_read_a_help_page() {
        // `--pure` is the greppable evidence; an unreadable probe refuses rather
        // than assuming, because this harness's seal is a file we write and a
        // binary that is not OpenCode would ignore it entirely.
        // Trimmed from the real `opencode --help`, keeping the flag rows
        // verbatim. Both halves of the probe are exercised: `looks_like_help`
        // wants one of `--help`/`--version`/`--model`/`--print`, and the grade
        // wants `--pure`.
        let help = "\
Commands:
  opencode run [message..]     run opencode with a message
  opencode serve               starts a headless opencode server

Options:
  -h, --help          show help                                       [boolean]
  -v, --version       show version number                             [boolean]
      --pure          run without external plugins                    [boolean]
  -m, --model         model to use in the format of provider/model     [string]
";
        assert_eq!(
            constraint_from_help(Harness::OpenCode, help),
            Constraint::Sealed
        );
        // A readable help page for something that is not OpenCode: the seal
        // Schemaic writes would mean nothing to it, so the session is refused
        // rather than run on an assumption.
        assert_eq!(
            constraint_from_help(
                Harness::OpenCode,
                "Usage: other\n  --help  show help\n  --version  print version\n"
            ),
            Constraint::Unknown
        );
        assert_eq!(
            constraint_from_help(Harness::OpenCode, ""),
            Constraint::Unknown
        );
    }

    #[test]
    fn an_unknown_constraint_is_not_runnable() {
        assert!(!Constraint::Unknown.is_runnable());
        assert!(Constraint::Sealed.is_runnable());
        // Sealed shows no banner: it is the grade the app has always quietly
        // provided, and a notice on every turn trains the user past the two that
        // matter.
        assert_eq!(Constraint::Sealed.notice(Harness::OpenCode), None);
        assert!(
            Constraint::Unknown
                .notice(Harness::OpenCode)
                .is_some_and(|n| n.contains("OpenCode"))
        );
    }

    #[test]
    fn the_capability_answers_match_what_was_measured() {
        let h = Harness::OpenCode;
        assert!(!h.is_persistent());
        assert!(h.supports_resume());
        // The one that is false where two of the other three are true: its
        // printer emits a text part only once it is finished.
        assert!(!h.streams_deltas());
        assert!(h.supports_model_choice());
        assert!(h.supports_effort());
    }

    #[test]
    fn the_key_round_trips_and_is_distinct() {
        assert_eq!(Harness::from_key("opencode"), Some(Harness::OpenCode));
        for h in Harness::ALL {
            assert_eq!(Harness::from_key(h.key()), Some(h));
        }
        let keys: std::collections::HashSet<_> = Harness::ALL.iter().map(|h| h.key()).collect();
        assert_eq!(keys.len(), Harness::ALL.len());
    }

    #[test]
    fn every_suggested_model_is_provider_qualified() {
        // A bare alias is not an id this CLI accepts, and a suggestion chip that
        // fails the turn is worse than no chip.
        for m in Harness::OpenCode.suggested_models() {
            assert!(m.contains('/'), "{m}");
        }
    }
}

#[cfg(test)]
mod speaker_label_tests {
    use super::*;

    #[test]
    fn each_harness_is_named_by_its_own_speaker_name() {
        // The bug this exists for: the panel printed "CLAUDE" over every reply,
        // including ones a different CLI had just produced. Driven from
        // `Harness::ALL` so a new harness cannot be added without a name here.
        for h in Harness::ALL {
            let got = speaker_label(Some(h.key()));
            assert_eq!(got, h.speaker_name().to_uppercase(), "{h:?}");
        }
    }

    #[test]
    fn the_speaker_name_is_the_product_name_except_where_it_is_deliberately_not() {
        // Guards the one intentional divergence rather than leaving it to be
        // "tidied" into `label()` later: the settings box installs *Claude Code*,
        // the transcript is quoting *Claude*. Every other harness must agree, so
        // a second gratuitous difference fails here.
        assert_eq!(Harness::Claude.label(), "Claude Code");
        assert_eq!(Harness::Claude.speaker_name(), "Claude");
        for h in Harness::ALL {
            if h == Harness::Claude {
                continue;
            }
            assert_eq!(h.speaker_name(), h.label(), "{h:?}");
        }
    }

    #[test]
    fn no_speaker_name_collides_with_the_unknown_placeholder() {
        // "ASSISTANT" has to stay distinguishable from a harness that is named,
        // or the neutral fallback stops being readable as one.
        for h in Harness::ALL {
            assert_ne!(speaker_label(Some(h.key())), "ASSISTANT", "{h:?}");
        }
    }

    #[test]
    fn the_four_labels_are_distinct() {
        // A label that collides tells the user nothing, which is the state this
        // replaces.
        let names: std::collections::HashSet<String> = Harness::ALL
            .iter()
            .map(|h| speaker_label(Some(h.key())))
            .collect();
        assert_eq!(names.len(), Harness::ALL.len(), "{names:?}");
    }

    #[test]
    fn a_turn_from_before_this_field_existed_is_not_attributed_to_anyone() {
        // Transcripts persisted by an earlier build carry no harness, and by
        // then three CLIs could already have written them — so the honest answer
        // is the neutral one. Guessing "Claude" here is the same move
        // `AiModel::from_cli` made when it coerced every unknown model to Haiku:
        // a plausible name in place of an unknown, with nothing on screen to say
        // it was invented.
        assert_eq!(speaker_label(None), "ASSISTANT");
    }

    #[test]
    fn an_unrecognised_key_is_neutral_rather_than_guessed() {
        // A transcript written by a *later* build naming a harness this one does
        // not have. Same rule as `Harness::from_key`, which returns `None`
        // rather than picking a working harness.
        assert_eq!(speaker_label(Some("gemini")), "ASSISTANT");
        assert_eq!(speaker_label(Some("")), "ASSISTANT");
    }
}

#[cfg(test)]
mod review_fix_tests {
    use super::*;

    /// **The isolation probe has to read the page the flag is on.**
    ///
    /// Measured: `codex --help` lists `--model`/`--sandbox`/`--help`/`--version`
    /// and *not* `--ignore-user-config`; `codex exec --help` lists all of them.
    /// The probe asked the top-level page, so `codex_isolates_config` was false
    /// on every real machine and every Codex session loaded the user's own
    /// `~/.codex/config.toml` — their MCP servers and their hooks.
    ///
    /// The bug lived in the seam: `the_isolation_probe_needs_real_help_and_the_
    /// real_flag` fed the decoder `exec --help` text by hand while the caller
    /// fed it something else, so both halves passed alone. This pins the
    /// *argv* — the thing that was wrong.
    #[test]
    fn codex_is_probed_on_the_subcommand_that_documents_its_isolation() {
        assert_eq!(Harness::Codex.help_args(), &["exec", "--help"]);
        // The others have no subcommand to descend into.
        for h in Harness::ALL {
            if h == Harness::Codex {
                continue;
            }
            assert_eq!(h.help_args(), &["--help"], "{h:?}");
        }
        // Every harness's page must still be able to answer the grade question,
        // so the last argument is always the help flag itself.
        for h in Harness::ALL {
            assert_eq!(h.help_args().last(), Some(&"--help"), "{h:?}");
        }
    }

    /// The page Codex is now asked for carries *both* things read off it.
    #[test]
    fn the_codex_help_page_answers_the_grade_and_the_isolation_together() {
        // Trimmed from the real `codex exec --help`, flag rows verbatim.
        let exec_help = "\
Usage: codex exec [OPTIONS] [PROMPT]

Options:
  -m, --model <MODEL>              Model the agent should use
  -s, --sandbox <SANDBOX_MODE>     Select the sandbox policy
      --ignore-user-config         Do not load ~/.codex/config.toml
      --skip-git-repo-check        Allow running outside a Git repository
  -h, --help                       Print help
  -V, --version                    Print version
";
        assert_eq!(
            constraint_from_help(Harness::Codex, exec_help),
            Constraint::Restricted
        );
        assert!(codex_isolates_config(exec_help));

        // And the top-level page, which is what used to be read: the grade still
        // resolves, the isolation silently does not. Kept as a test so the
        // regression is legible rather than merely absent.
        let top_help = "\
Usage: codex [OPTIONS] [PROMPT]

Options:
  -m, --model <MODEL>       Model the agent should use
  -s, --sandbox <SANDBOX>   Select the sandbox policy
  -h, --help                Print help
  -V, --version             Print version
";
        assert_eq!(
            constraint_from_help(Harness::Codex, top_help),
            Constraint::Restricted
        );
        assert!(
            !codex_isolates_config(top_help),
            "the top-level page cannot establish the isolation — that is why \
             `help_args` descends into `exec`"
        );
    }

    /// A cleared-then-spaced model field must not reach any harness's argv.
    ///
    /// The field is free text. Claude's builder trimmed; the other three tested
    /// `!is_empty()`, so `" "` became `--model " "` — an unknown model, reported
    /// as "Couldn't launch the CLI", which is the one cause that is not it.
    #[test]
    fn a_whitespace_only_model_is_no_model_on_every_harness() {
        for h in Harness::ALL {
            for blank in ["", " ", "   ", "\t", "\n"] {
                let spec = TurnSpec {
                    prompt: "hi".into(),
                    model: blank.into(),
                    ..Default::default()
                };
                let args = turn_args(h, &spec);
                assert!(
                    !args.iter().any(|a| a == "--model"),
                    "{h:?} sent --model for {blank:?}: {args:?}"
                );
                // And nothing blank reached argv by another route.
                assert!(
                    !args.iter().any(|a| !a.is_empty() && a.trim().is_empty()),
                    "{h:?} passed a blank argument: {args:?}"
                );
            }
        }
    }

    /// A real model id still survives, untrimmed in the middle.
    #[test]
    fn a_model_with_surrounding_space_is_sent_trimmed() {
        for h in Harness::ALL {
            let spec = TurnSpec {
                prompt: "hi".into(),
                model: "  provider/some-model  ".into(),
                ..Default::default()
            };
            // Whichever spawn shape this harness has — the trim is a property of
            // the field, not of one builder, and both builders have to make it.
            let args = match h.is_persistent() {
                true => session_args(h, &spec, crate::CliSeal::ALL, &[]),
                false => turn_args(h, &spec),
            };
            let i = args.iter().position(|a| a == "--model").expect("--model");
            assert_eq!(args[i + 1], "provider/some-model", "{h:?}");
        }
    }

    /// **The `Restricted` notice describes a mechanism, so it must be asked as
    /// one.** The sandbox sentence is a claim the OS enforces; promising it for
    /// a harness with no sandbox would be a positive assurance nothing backs.
    #[test]
    fn only_a_sandboxed_harness_is_told_the_os_is_stopping_it() {
        for h in Harness::ALL {
            let notice = Constraint::Restricted
                .notice(h)
                .unwrap_or_else(|| panic!("{h:?} said nothing at Restricted"));
            let claims_sandbox = notice.contains("cannot write files or run commands");
            assert_eq!(
                claims_sandbox,
                h.restricted_means_sandbox(),
                "{h:?}: {notice}"
            );
            // Whatever it says, it names itself and no one else.
            assert!(notice.contains(h.label()), "{h:?}: {notice}");
            for other in Harness::ALL {
                if other != h && other.label() != h.label() {
                    assert!(!notice.contains(other.label()), "{h:?} named {other:?}");
                }
            }
        }
    }

    /// "Update the CLI and the seal comes back" is only true where the seal is a
    /// flag an older build could be missing.
    #[test]
    fn only_a_flag_sealed_harness_is_told_an_update_would_help() {
        for h in Harness::ALL {
            let notice = Constraint::Restricted.notice(h).expect("a notice");
            assert_eq!(
                notice.contains("Updating the CLI"),
                h.seals_by_flag(),
                "{h:?}: {notice}"
            );
        }
    }
}

/// The inline (one-shot) argv, which every harness now builds for itself.
///
/// The property under test throughout is the one the feature turns on: an
/// inline generation is a *closed* request — no server, no session to resume,
/// no tool the model could reach for — on whichever CLI the user picked. Before
/// this, three of the four spawned Claude regardless.
#[cfg(test)]
mod inline_tests {
    use super::*;
    use crate::CliSeal;

    fn spec() -> InlineSpec {
        InlineSpec {
            intent: "count the rows".into(),
            system: "tables: users(id)".into(),
            model: "some-model".into(),
            effort: String::new(),
            seal: CliSeal::ALL,
            isolate_config: true,
            last_message: "C:/tmp/last.txt".into(),
        }
    }

    fn flag_value(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .map(|i| args[i + 1].clone())
    }

    /// The invariant, on every harness at once: nothing here can reach a tool,
    /// a server or an earlier session.
    #[test]
    fn no_inline_generation_is_given_a_server_or_a_session() {
        // Flags that would hand the model a server, or resume a thread whose
        // contents this one-shot never sees.
        let forbidden = [
            "--mcp-config",
            "--allowedTools",
            "--conversation",
            "--session",
            "--resume",
            "resume",
            "--continue",
            "-c",
        ];
        for h in Harness::ALL {
            let a = inline_argv(h, &spec());
            assert!(!a.is_empty(), "{h:?} built no argv");
            for f in forbidden {
                // Codex's constraint rides on `-c sandbox_mode=…`, which is the
                // one `-c` that may appear: it closes the sandbox rather than
                // opening anything.
                if h == Harness::Codex && f == "-c" {
                    let bad = a
                        .iter()
                        .zip(a.iter().skip(1))
                        .any(|(k, v)| k == "-c" && !v.starts_with("sandbox_mode="));
                    assert!(!bad, "{h:?} passes a -c that is not the sandbox: {a:?}");
                    continue;
                }
                assert!(!a.contains(&f.to_string()), "{h:?} passes {f}: {a:?}");
            }
        }
    }

    /// Each harness asks for its own plain-text answer, never the streaming
    /// dialect the chat panel decodes — there is no parser on this path.
    #[test]
    fn every_inline_generation_asks_for_plain_text() {
        for h in Harness::ALL {
            let a = inline_argv(h, &spec());
            assert!(
                !a.iter().any(|s| s == "stream-json" || s == "--json"),
                "{h:?} asked for a stream: {a:?}"
            );
        }
        assert_eq!(
            flag_value(
                &inline_argv(Harness::Antigravity, &spec()),
                "--output-format"
            )
            .as_deref(),
            Some("text")
        );
        assert_eq!(
            flag_value(&inline_argv(Harness::OpenCode, &spec()), "--format").as_deref(),
            Some("default")
        );
    }

    /// Codex is the one harness whose reply is a file, because its stdout is a
    /// human-facing rendering rather than a contract.
    #[test]
    fn codex_writes_its_last_message_where_it_was_told_to() {
        assert_eq!(inline_output(Harness::Codex), InlineOutput::LastMessageFile);
        let a = inline_argv(Harness::Codex, &spec());
        assert_eq!(flag_value(&a, "-o").as_deref(), Some("C:/tmp/last.txt"));
        // Everything else reads the pipe it already has open.
        for h in [Harness::Claude, Harness::Antigravity, Harness::OpenCode] {
            assert_eq!(inline_output(h), InlineOutput::Stdout, "{h:?}");
        }
    }

    /// The sandbox is not optional on the two harnesses that only ever reach
    /// `Restricted`: it is the whole of their constraint.
    #[test]
    fn the_restricted_harnesses_still_carry_their_sandbox() {
        let cx = inline_argv(Harness::Codex, &spec());
        assert_eq!(flag_value(&cx, "--sandbox").as_deref(), Some("read-only"));
        assert!(
            cx.iter().any(|s| s == "sandbox_mode=\"read-only\""),
            "{cx:?}"
        );
        assert!(cx.contains(&"--skip-git-repo-check".to_string()), "{cx:?}");
        // Left no session file behind: a one-shot has nothing to resume.
        assert!(cx.contains(&"--ephemeral".to_string()), "{cx:?}");
        assert!(cx.contains(&"--ignore-user-config".to_string()), "{cx:?}");

        let ag = inline_argv(Harness::Antigravity, &spec());
        assert!(ag.contains(&"--sandbox".to_string()), "{ag:?}");
        // A prompt is user text and must not expand a slash command or skill.
        assert!(
            ag.contains(&"--disable-slash-commands".to_string()),
            "{ag:?}"
        );
    }

    /// OpenCode's seal is its agent, and the agent only exists in the config —
    /// naming it is what selects the empty tool map.
    #[test]
    fn opencode_runs_the_sealed_agent_with_no_plugins() {
        let a = inline_argv(Harness::OpenCode, &spec());
        assert_eq!(a[0], "run");
        assert!(a.contains(&"--pure".to_string()), "{a:?}");
        assert_eq!(flag_value(&a, "--agent").as_deref(), Some(OPENCODE_AGENT));
    }

    /// The inline config defines the same sealed agent as a session's, and
    /// **no** `mcp` block: a one-shot is answered from the prompt alone.
    #[test]
    fn the_inline_opencode_config_defines_the_agent_and_no_server() {
        let v: serde_json::Value =
            serde_json::from_str(&opencode_inline_config_json()).expect("valid json");
        assert!(
            v.get("mcp").is_none(),
            "inline config registers a server: {v}"
        );
        let tools = v["agent"][OPENCODE_AGENT]["tools"]
            .as_object()
            .expect("the sealed agent's tool map");
        assert!(!tools.is_empty());
        assert!(
            tools.values().all(|b| b == &serde_json::Value::Bool(false)),
            "a built-in is left on: {tools:?}"
        );
    }

    /// The prompt carries the schema outline on the three harnesses with no
    /// `--append-system-prompt`, and Claude keeps its own flag.
    #[test]
    fn the_system_context_reaches_every_harness() {
        for h in Harness::ALL {
            let a = inline_argv(h, &spec());
            let joined = a.join("\u{1}");
            assert!(
                joined.contains("tables: users(id)"),
                "{h:?} dropped the system context: {a:?}"
            );
            assert!(
                joined.contains("count the rows"),
                "{h:?} dropped the intent: {a:?}"
            );
        }
        assert!(
            inline_argv(Harness::Claude, &spec()).contains(&"--append-system-prompt".to_string())
        );
    }

    /// A one-shot honours the effort setting too — it never did on Claude's
    /// path, so the setting silently applied to the chat panel alone.
    #[test]
    fn effort_reaches_each_harness_in_its_own_flag() {
        for h in Harness::ALL {
            let s = InlineSpec {
                effort: "high".into(),
                ..spec()
            };
            let a = inline_argv(h, &s);
            match h {
                // No such flag on `codex exec`, so nothing to send.
                Harness::Codex => assert!(
                    !a.contains(&"--effort".to_string()),
                    "Codex was sent an effort it has no flag for: {a:?}"
                ),
                Harness::OpenCode => {
                    assert_eq!(
                        flag_value(&a, "--variant").as_deref(),
                        Some("high"),
                        "{a:?}"
                    )
                }
                _ => assert_eq!(flag_value(&a, "--effort").as_deref(), Some("high"), "{a:?}"),
            }
        }
    }

    /// **Clamped to the harness's own vocabulary**, the same rule `effort_arg`
    /// exists for: the setting survives a harness switch, so Claude's `xhigh`
    /// is still selected when the picker moves to Antigravity, whose `--effort`
    /// never advertised it.
    #[test]
    fn an_effort_the_harness_never_advertised_is_not_sent() {
        for h in Harness::ALL {
            let s = InlineSpec {
                effort: "xhigh".into(),
                ..spec()
            };
            let a = inline_argv(h, &s);
            if h == Harness::Claude {
                // Claude's own fourth level, and it does travel.
                assert_eq!(flag_value(&a, "--effort").as_deref(), Some("xhigh"));
                continue;
            }
            assert!(
                !a.contains(&"--effort".to_string()) && !a.contains(&"--variant".to_string()),
                "{h:?} was handed another harness's level: {a:?}"
            );
        }
    }

    /// The same trim `turn_args` needed: the field is free text the user can
    /// clear, and `--model " "` is an unknown model reported as a launch
    /// failure — the one cause that is not the problem.
    #[test]
    fn a_blank_model_is_not_passed_to_anyone() {
        for h in Harness::ALL {
            let s = InlineSpec {
                model: "   ".into(),
                ..spec()
            };
            let a = inline_argv(h, &s);
            assert!(!a.contains(&"--model".to_string()), "{h:?}: {a:?}");
            assert!(!a.contains(&"-m".to_string()), "{h:?}: {a:?}");
        }
        // And it *is* passed when the user set one.
        for h in Harness::ALL {
            let a = inline_argv(h, &spec());
            assert_eq!(
                flag_value(&a, "--model").as_deref(),
                Some("some-model"),
                "{h:?}: {a:?}"
            );
        }
    }
}

/// Antigravity's bidirectional mode: one process for the conversation, turns on
/// stdin.
///
/// Every fact pinned here was measured against the installed `agy` before any of
/// it was driven — the protocol shape, the flag that must not be passed, and the
/// interrupt that does not exist.
#[cfg(test)]
mod bidirectional_tests {
    use super::*;
    use crate::CliSeal;

    fn spec() -> TurnSpec {
        TurnSpec {
            prompt: "count rows".into(),
            system: "tables: users(id)".into(),
            model: "gemini-3".into(),
            effort: "high".into(),
            ..Default::default()
        }
    }

    /// Two harnesses hold the conversation now, and two are still a process per
    /// turn.
    #[test]
    fn antigravity_holds_the_conversation_like_claude_does() {
        assert!(Harness::Antigravity.is_persistent());
        assert!(Harness::Claude.is_persistent());
        assert!(!Harness::Codex.is_persistent());
        assert!(!Harness::OpenCode.is_persistent());
    }

    /// **The pair that used to be one question.** Antigravity is the harness
    /// that needs both answers: it holds the conversation, and it still has to
    /// be resumable because Stop can only end it.
    #[test]
    fn persistence_and_resume_are_no_longer_complements() {
        assert!(Harness::Antigravity.is_persistent() && Harness::Antigravity.supports_resume());
        assert!(Harness::Claude.is_persistent() && !Harness::Claude.supports_resume());
        for h in [Harness::Codex, Harness::OpenCode] {
            assert!(!h.is_persistent() && h.supports_resume(), "{h:?}");
        }
    }

    /// The measured envelope. Claude's `{"type":"user"}` is refused by `agy`
    /// with *"stream input message is missing the \"event\" field"*, so the two
    /// dialects cannot share one encoder.
    #[test]
    fn each_persistent_harness_encodes_a_turn_its_own_way() {
        let agy: serde_json::Value =
            serde_json::from_str(&Harness::Antigravity.session_turn_line("count rows"))
                .expect("one JSON object per line");
        assert_eq!(agy["event"], "user");
        assert_eq!(agy["message"]["content"], "count rows");

        let claude: serde_json::Value =
            serde_json::from_str(&Harness::Claude.session_turn_line("count rows"))
                .expect("one JSON object per line");
        assert_eq!(claude["type"], "user");

        // Newline-terminated, or the reader never sees the line.
        for h in [Harness::Claude, Harness::Antigravity] {
            assert!(h.session_turn_line("x").ends_with('\n'), "{h:?}");
        }
        // The two per-turn harnesses take their prompt in argv.
        for h in [Harness::Codex, Harness::OpenCode] {
            assert!(h.session_turn_line("x").is_empty(), "{h:?}");
        }
    }

    /// **Measured, not assumed.** An unrecognised event on `agy`'s stdin is
    /// ignored in silence, so a guessed interrupt would not fail loudly — it
    /// would hang with the turn still running. `None` sends the caller to the
    /// only mechanism that works.
    #[test]
    fn only_claude_can_interrupt_a_turn_in_flight() {
        assert!(Harness::Claude.session_interrupt().is_some());
        for h in [Harness::Antigravity, Harness::Codex, Harness::OpenCode] {
            assert_eq!(h.session_interrupt(), None, "{h:?}");
        }
    }

    /// The prompt flag must be gone, not empty: `-p` takes the prompt as its
    /// *value*, so left in with nothing to give it the CLI reads the next flag
    /// as the prompt and says so.
    #[test]
    fn a_persistent_antigravity_is_spawned_with_no_prompt_flag() {
        let a = session_args(Harness::Antigravity, &spec(), CliSeal::ALL, &[]);
        assert!(!a.contains(&"-p".to_string()), "{a:?}");
        assert!(!a.contains(&"--print".to_string()), "{a:?}");
        assert!(!a.iter().any(|s| s.contains("count rows")), "{a:?}");
        // Both halves of the protocol, which its help says go together.
        let pos = |f: &str| a.iter().position(|x| x == f).map(|i| a[i + 1].clone());
        assert_eq!(pos("--input-format").as_deref(), Some("stream-json"));
        assert_eq!(pos("--output-format").as_deref(), Some("stream-json"));
        // The constraint and the settings it must not expand.
        assert!(a.contains(&"--sandbox".to_string()), "{a:?}");
        assert!(a.contains(&"--disable-slash-commands".to_string()), "{a:?}");
        assert_eq!(pos("--model").as_deref(), Some("gemini-3"));
        assert_eq!(pos("--effort").as_deref(), Some("high"));
    }

    /// A fresh session carries no `--conversation`: that flag is how a Stop is
    /// recovered from, and passing it unasked would reopen an old thread.
    #[test]
    fn a_conversation_is_resumed_only_when_one_was_kept() {
        let a = session_args(Harness::Antigravity, &spec(), CliSeal::ALL, &[]);
        assert!(!a.contains(&"--conversation".to_string()), "{a:?}");
        let resumed = TurnSpec {
            resume: Some("conv-7".into()),
            ..spec()
        };
        let b = session_args(Harness::Antigravity, &resumed, CliSeal::ALL, &[]);
        let i = b.iter().position(|x| x == "--conversation").expect("flag");
        assert_eq!(b[i + 1], "conv-7");
        // An empty id is not an id.
        let blank = TurnSpec {
            resume: Some(String::new()),
            ..spec()
        };
        assert!(
            !session_args(Harness::Antigravity, &blank, CliSeal::ALL, &[])
                .contains(&"--conversation".to_string())
        );
    }

    /// `session_args` and `turn_args` are the two halves of one question, and
    /// each declines the harnesses the other owns.
    #[test]
    fn the_two_spawn_shapes_do_not_overlap() {
        for h in Harness::ALL {
            let session = session_args(h, &spec(), CliSeal::ALL, &[]);
            let turn = turn_args(h, &spec());
            assert_eq!(
                session.is_empty(),
                !h.is_persistent(),
                "{h:?} session_args disagrees with is_persistent"
            );
            assert_eq!(
                turn.is_empty(),
                h.is_persistent(),
                "{h:?} turn_args disagrees with is_persistent"
            );
        }
    }

    /// Antigravity has no `--append-system-prompt`, so the schema outline has to
    /// travel in the text — once, not on every turn, which is most of what
    /// holding the process was for.
    #[test]
    fn only_the_harness_without_a_system_flag_folds_it_into_the_first_turn() {
        assert!(Harness::Antigravity.session_system_in_first_turn());
        assert!(!Harness::Claude.session_system_in_first_turn());
        let a = session_args(Harness::Antigravity, &spec(), CliSeal::ALL, &[]);
        assert!(
            !a.iter().any(|s| s.contains("tables: users(id)")),
            "the system context went in the argv: {a:?}"
        );
    }
}
