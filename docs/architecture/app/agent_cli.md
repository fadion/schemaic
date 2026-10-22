# `app/agent_cli.rs`

`agent_cli.rs` — finding the agent CLI, and asking it what it accepts. It was `claude_cli.rs`
until the settings gained a harness, and the rename is not cosmetic: the module now looks for
six binaries and interrogates whichever one is selected, so leaving it named for one of them
while it probes the other five is the drift this document exists to catch.
`detect_bin(h)` asks three sources in order — `$SCHEMAIC_<HARNESS>_BIN` (Claude keeps
`SCHEMAIC_CLAUDE_BIN`, which shipped before any other harness existed and someone's launcher may
still set it), then that harness's **known install locations**, then a `PATH` search honouring
Windows `PATHEXT`. The middle step is not belt-and-braces. On the machine this was written
against, `codex` and `agy` were both installed and working while absent from the `PATH` a child
process inherits, so a `PATH`-only search reported two working CLIs as not installed and the
panel would have said so. The locations are Claude `~/.local/bin`, Codex
`%LOCALAPPDATA%\Programs\OpenAI\Codex\bin` and `~/.codex/bin`, Antigravity
`%LOCALAPPDATA%\agy\bin` and `~/.local/bin`, OpenCode `%APPDATA%\npm\node_modules\opencode-ai\bin`,
`~/.opencode/bin`, `~/.local/bin` and `~/.npm-global/lib/node_modules/opencode-ai/bin`, Copilot
`%APPDATA%\npm\node_modules\@github\copilot\node_modules\@github\copilot-win32-<x64|arm64>` and
`~/.local/bin`, Cursor `%LOCALAPPDATA%\cursor-agent\cursor-agent.cmd` on Windows and
`~/.local/bin/cursor-agent` elsewhere — and each candidate ends in that harness's own executable
(`a_known_location_ends_in_the_harnesss_own_executable`, which also holds the Windows
extension on, `.exe` for every harness but Cursor, whose installer writes no executable of its
own there — only the launcher script, which `launch` below looks behind). Cursor's `bin` is
`cursor-agent` and not the shorter `agent` its installer also puts on `PATH`, because `agent` is
a word other tools claim and a `PATH` search for it can find somebody else's.
**For OpenCode the middle step is doing a second job**, and it is a Windows one: a
global `npm i -g opencode-ai` puts an `opencode.cmd` shim on `PATH` beside the real
`opencode.exe` it calls, `which_on_path` honours `PATHEXT`, and a `.cmd` needs a shell rather than
the `Command::new` this spawns with. Listing the real binary first is what makes it win before the
shim is ever found. **Copilot's is the same job one layer deeper**: `npm i -g @github/copilot`
puts `copilot.cmd` on `PATH`, which runs `npm-loader.js` under node, which spawns the real
binary out of a per-platform package beside it — `@github/copilot-win32-x64/copilot.exe`,
measured on 1.0.88 — so the known location is that nested path, its architecture component read
off `std::env::consts::ARCH` (`aarch64` → `arm64`, anything else → `x64`), and the shim, which
would refuse every multi-line argument, is never what is spawned when the real file is there.
`copilot_looks_behind_npm_s_shim_for_the_real_binary` pins it, and returns early off Windows or
without `APPDATA`, so it asserts nothing on any other platform. **A shim found anywhere else is
looked behind too**, by `launch`: under a non-default npm prefix — or named by a path override —
`copilot.cmd` would otherwise be spawned through `cmd.exe`, and ending `cmd.exe` leaves node and
`copilot.exe` running the turn, Cursor's hazard below. The package sits beside the shim, so
`copilot_native_behind` looks for it relative to the shim's own directory, through the same
`copilot_native_path` the default location is built from
(`copilot_s_binary_is_found_beside_whichever_npm_prefix_holds_the_shim`).
**Cursor's is not a shim to look past but a chain to start behind, and that is `launch(h, bin)
-> Launch { program, prefix }`.** Its Windows installer writes `cursor-agent.cmd`, which runs a
PowerShell script, which runs `versions\<newest>\node.exe index.js` — and ending a `.cmd` ends
`cmd.exe` and leaves node running, so Stop and a dropped session's `kill_on_drop` would have
stopped the panel waiting while the turn went on spending the user's quota. The launcher does
nothing but pick the version and pass its arguments through, so `cursor_node_launch` does the
same and Schemaic starts that `node.exe` with `index.js` as the prefix: one process, which dies
when told to. Measured: a turn, a resume and `mcp enable` all ran identically started that way.
The version is chosen by the launcher's own rule, `newest_cursor_version` — `YYYY.M.D-<hex>` or
`YYYY.M.D-HH-MM-SS-<hex>`, dates compared as numbers, the build time breaking a tie the script
leaves to directory order, anything else in `versions\` ignored
(`the_newest_cursor_version_is_picked_by_the_launcher_s_rule`). Anything that is not that layout
is started as it is — an override pointing straight at a `node.exe`, a Unix install whose link is
a real executable — and no harness but these two is rewritten at all
(`a_launcher_is_rewritten_only_to_something_that_exists`, which also holds a `.cmd` with nothing
behind it to `Launch::direct` rather than an invented path, for both). The layout itself — the `.cmd` resolving to
a real install's newest `node.exe index.js` — was measured rather than tested, because a test
reading the installed directory would break the suite's no-filesystem rule. `launch` is
an exhaustive `match`, so the next harness decides rather than defaults, and the per-turn loop,
the one-shot, `cursor::approve` and `help_command` all go through it — the last because probing
the `.cmd` would answer for the chain rather than for the node the session will start. **The
persistent branch does not**: it spawns `bin` directly, which is right only because `launch`
answers `direct` for Claude and Antigravity, the two harnesses that reach it.
**Every agent spawn hides its console, through `hide_console_window`.** The release build is a
`windows_subsystem = "windows"` binary with no console to share, so each console program it starts
gets a window of its own unless the spawn sets `CREATE_NO_WINDOW` — and none of the agent spawns
did, so the startup probe's `claude --help` flashed a console window on every launch, as every AI
turn, inline generation, Cursor `mcp enable` and `agy mcp` call would have. `Launch::std_command`
calls it, which covers `probe` and `cursor::approve`; the three tokio spawns in `ai.rs` (the
one-shot, the per-turn loop, the persistent branch) pass `cmd.as_std_mut()`; and
`antigravity::agy_mcp` calls it on its own `Command`. The child still gets a console, only an
invisible one, which is what its own children (`reg.exe`, `node`) inherit. The other crates' spawns
— `db::entra`, `db::cloud_iam`, `conn_sources`, `term::shell`, `main.rs`'s `open_url` — already set
the flag inline. `every_agent_spawn_hides_its_console_window` counts `Command::new(` against the
call in `ai.rs`, `antigravity.rs` and this file, so a new spawn without it shows up as an
imbalance; it is a count, so it cannot tell *which* spawn lacks the call, only that one does.
`harness_bin(h, override)` and `harness_reachable(h, path)` are the former
`claude_bin`/`claude_reachable` with the harness passed in rather than assumed — the first
resolves what to spawn, the second answers the settings modal's "is this reachable".
**`pick_executable` is `which_on_path`'s per-directory half, pure so it can be tested, and on
Windows the bare name must not win.** `npm i -g` writes *three* shims per binary: `<name>`, a
`#!/bin/sh` script for Git Bash, plus `<name>.cmd` and `<name>.ps1`. Taking the bare file first
resolved an npm-installed harness to the `sh` script, which `Command::spawn` answers with **os
error 193, "%1 is not a valid Win32 application"** — and the two surfaces then contradicted
each other: `harness_reachable` said connected, because it only asks whether the file exists,
while `probe`'s `--help` errored into `Constraint::Unknown` and `inline_gate` refused *every*
generation with "could not confirm … so the assistant is disabled". The whole AI feature dead,
for a harness that was installed. `cmd.exe` and `where.exe` consider only `PATHEXT` extensions,
so this does too: the bare name is taken only when its own extension is already one of them
(`foo.exe` asked for by full name), and `PATHEXT`'s order *is* the preference order, which is
what puts `.CMD` ahead of a `.ps1` nobody can spawn directly. **With one departure from
`where.exe`: a batch shim goes last**, which is two passes over `PATHEXT` in its own order —
everything that is not `.BAT`/`.CMD`, then those. Rust runs a batch file through `cmd.exe` and
refuses any argument it cannot escape for it, and every prompt this app builds is multi-line by
construction (`harness::prefixed_prompt` joins with `\n\n`), so a harness resolved to a `.cmd`
answered `InvalidInput: batch file arguments are invalid` for *every* generation — Ctrl+K,
Optimise, Fix with AI and each chat turn — reported as a batch-file problem rather than a prompt
one. A default `PATHEXT` lists `.EXE` before `.CMD` and hid it; the variable is editable and
installers edit it. The shim is still taken when it is the only candidate, which is npm's layout.
Off Windows `exts` is empty and
the bare name is the only candidate, which is correct there — the executable bit is the test,
not the name.
**One `--help`, three answers.** `probe(h, bin)` returns a `Probe { seal, constraint,
isolate_config }` rather than the old bare `CliSeal`, because all three come out of the same help
text and spawning it three times is three times the ~140 ms nobody has. **Which page that is comes
from `Harness::help_args`, not from a literal `--help` here**, and the difference was Codex's whole
config isolation: `--ignore-user-config` is documented on `codex exec --help` and not on the
top-level page this used to ask, so `codex_isolates_config` was `false` on every real machine and
every Codex session loaded the user's own `~/.codex/config.toml`. `exec` carries `--sandbox` too,
so one spawn still answers all three. `schemaic-ai` states the measurement and names the tests.
**The invocation is built by `help_command(h, bin)` rather than spawned inline, so that argv has
a test of its own.** The regression's own test lives in `schemaic-ai` and asserts
`help_args()`'s *return* — which is the answer, not the question — so reverting this line to
`.arg("--help")` left the entire workspace green, and this module's tests never reached `probe`
at all. `the_probe_asks_each_harness_for_the_page_that_answers_both_questions` reads
`Command::get_args()` back, and also checks the program is the binary it was handed rather than
a re-resolved one. Say plainly what it does not buy: it is still **one function away from
compile-forced**, since nothing stops a future edit building its own `Command` here. What the
extraction buys is that the revert now has to be made in two places, and the second is the one
carrying the comment.
It folds **stdout and
stderr** together: `agy --help` writes to *stderr* and exits 0, so reading stdout alone found an
empty help and refused a working binary. A non-zero exit yields `Constraint::Unknown` without
consulting the text at all — a `--help` that failed printed a diagnostic, not a help text. And a
spawn that fails outright yields `seal: CliSeal::ALL` **and** `constraint: Unknown`, which point
in opposite directions on purpose: for Claude the dangerous guess is that a sealing flag is
absent, so an unread probe passes all three and risks a loud death instead of a quiet unsealed
session; for the others there is no safe "pass everything", since `--sandbox` on a binary that
never heard of it *is* that death, so nothing established means do not spawn. `schemaic-ai`
states the reasoning at each end. The cache is keyed by **(harness, resolved path)** and not by
path alone, because the same binary answers different questions for different harnesses and
switching harness must not read the previous one's answer.
**A spawn that never ran is the one outcome not cached.** Every other answer here is a property
of a file that will not change while the app runs, which is what makes memoising it sound; a
failed spawn is not — an antivirus lock, an in-place upgrade of the CLI, or a moment of resource
exhaustion during the startup `warm_probe_cache` each produce one, and each lands on
`Constraint::Unknown`, which *refuses* the session. Remembered, one such moment disabled the
assistant for the rest of the run, saying the constraint could not be confirmed and offering no
retry path anywhere. `probe` now returns that answer early without inserting it, so the next
attempt is free to ask again.
**Inline generation follows the picker now, and for a while it deliberately did not.** Ctrl+K, AI
Fill and AI Seed build their argv with `harness::inline_argv` and resolve their binary through the
same `harness_bin(selected, override)` the chat panel uses, so the AI CLI override and the model
id belong to whichever CLI is selected, exactly as they do everywhere else
(`the_inline_paths_use_the_selected_harnesss_own_binary`, which holds the override for every
harness in `Harness::ALL`).
**What that replaced was `inline_claude_bin` and `inline_claude_model`, and the bug behind them is
why this paragraph is still here.** While all three features spawned `claude` whatever drove the
chat panel, the override and the model followed the selection anyway: `ai_cli_path` on Codex is a
path to `codex`, and handing that to a Claude-flagged spawn runs the wrong binary with flags it
has never heard of. `inline_claude_bin` withheld the override for that reason, and the second half
took longer to arrive — `ai_model` follows the selection too, so under Codex the suggestion chips
are `gpt-5.4`, `gpt-5.4-codex`, `o3`, and Ctrl+K after picking one spawned `claude` with
`--model gpt-5.4-codex`, dying on an unknown model under a "check your installation" message
naming the wrong cause. Both functions and their three tests are gone, because the rule they
enforced — withhold the override path *and* the model id from every harness but Claude — is not a
rule any more: the argv is the selected harness's own. The failure they guarded is not gone with
them, and it is now answered in two other places: each arm of `inline_argv` is built for the
harness it names, and `cli_failure_message` takes a `Harness` rather than reporting every failure
as Claude's.
