# `app/conn_sources.rs`

`conn_sources.rs` — the I/O half of connection import: which paths on *this* machine are worth
opening, and reading them. Deliberately only that half — nothing here interprets the bytes,
which is what keeps `core::conn_import` unit-tested and leaves the part that cannot be (a walk
of the user's home directory) small enough to read. `discover` returns the sources in the order
`conn_import::scan` relies on: the two GUI clients first, because they are the only sources
carrying a name a human chose, then the plain-text files, whose value is the passwords they can
lend to the rows above them.
**Every root is probed on every platform** — no `cfg` branches, even though the locations are
per-OS: an absent `~/Library` on Linux costs one failed `read_dir`, and a `cfg` costs a path
only a third of the developers can exercise. DBeaver is two levels of wildcard
(`DBeaverData/workspace<N>/<project>/.dbeaver/data-sources.json`) and both are real — taking
only `General` misses every connection of anyone who made a second project. JetBrains is
`JetBrains/<Product><Version>/options/dataSources.xml` and is **not** filtered by product,
since the file's shape is identical under IntelliJ or PhpStorm — **and that global file is not
where a DataGrip project keeps its connections.** A user with DataGrip installed through
JetBrains Toolbox on Linux got nothing from Import: every connection was in a project,
`~/DataGripProjects/<project>/.idea/dataSources.xml`, and only the global file was read. (The
Toolbox install was never the cause — wherever the binary lives, its configuration is under the
config root.) Projects sit wherever the user keeps code, and walking the home directory for them
is what this module refuses to do; the IDE's own record of them is not a walk. So each
product's `options/recentProjects.xml` is read — by `read_small`, under the same 4 MiB cap, and
never itself a source — through `conn_import::recent_project_dirs`, and
`<project>/.idea/dataSources.xml` is checked for every project it names, plus
`~/DataGripProjects/*` one level deep, DataGrip's default project location on every platform,
for a project that has fallen off the recent list. The only paths that come from outside a known
layout are ones the IDE itself recorded, and only that one fixed file inside each is opened.
**A project's file is still not the user's configuration**: it arrives with a checkout, so
`conn_import::scan` lets `~/.pgpass` complete its rows only as it would a hand-picked file's
(`is_project_file`, `PgpassScope::PickedFile`, under that entry). Globals are listed before projects, so where both describe one server `scan` keeps the global
row; a project reached twice (the recent list and `DataGripProjects`, or two products' lists) is
one file to `discover`'s resolved-path check. **A JetBrains file is read as a pair**, whichever
half is named: `jetbrains_pair` keeps the named half's own spelling and spells only the *other*
half the JetBrains way, beside it — respelling a picked `DATASOURCES.XML` would, on a
case-sensitive filesystem, name a different file or none (`a_named_main_file_keeps_its_own_spelling`)
— and `open_source` reads the main half for the servers
(required — naming only a local file with nothing beside it is the main file's error) and the
local half, if it is there, for the user names. *Choose a file…* goes through the same
`open_source`, so a project file picked by hand arrives with its users too, and picking the
local half reads the whole pair — **and a local half that was picked and fails to read is
reported**, where `.ok()` dropped it and the import went on with every user blank
(`a_named_local_half_that_cannot_be_read_says_so`); an unnamed sibling stays optional. Both
compositions are tested over a seam, since the parsers alone could not pin them:
`open_source_with` takes the file reader (`opening_either_jetbrains_half_reads_the_pair`), and
`jetbrains_files_in` a `Probe` for the directory listing, the file check and the small read
(`the_jetbrains_walk_finds_globals_then_listed_then_default_projects`, which pins the
globals-first order too). `PGPASSFILE`/`PGSERVICEFILE` are honoured,
because libpq reads them first. Reads are capped at 4 MiB and a file that fails is silently
skipped — this runs over paths the user never named — *except* one they picked by hand, which
goes through `open_source` — `read_source` is that call with the error dropped — and is
reported by the modal. `source_for_path` chooses that file's parser by the name its tool gives
it, and Schemaic's own export by a case-insensitive `conn_import::EXPORT_FILE_SUFFIX` on the
end of the name, since `.json` alone is DBeaver's too
(`a_tools_own_file_name_picks_its_parser`).
`password_sources` is the narrow half of that walk — today just `~/.pgpass` — for the one path
that reads a single file: a hand-picked DataGrip export would otherwise arrive with twelve
blank passwords that libpq's file, on the same machine, holds every one of, and whether a row
can be completed must not depend on how its file was found. Four `is_file` checks and one small
read — it used to run inside the file-picker callback on the strength of being that cheap, and
now rides the `spawn_blocking` the picked file's read and parse were moved onto anyway, since the
parse beside it is the part with no bound on how long it takes.
**`docker_source` is the one source that is a process, not a file**, which is why it is its own
button rather than part of `discover`: a walk of the home directory costs a few `read_dir`s, and
this costs two spawns and whatever the daemon takes. Two calls, because `docker ps` does not
carry the environment the images take their credentials from — `launch::DOCKER_PS_ARGS` lists
the running ids and `launch::docker_inspect_args` turns them into the second call or refuses it;
nothing running is `Ok(None)`, with no second spawn. `docker` is looked for on `PATH` and must
pass `launch::direct_spawn_verdict`; on Windows, failing that, it is the default WSL
distribution's through `wsl.exe -e docker` — the engine installed inside WSL without Docker
Desktop, whose published ports WSL forwards to this machine's loopback — and the rows' origin
says which (`docker`, `docker (WSL)`). So on Windows a `PATH` docker the verdict refuses falls
through to WSL, and the refusal is the message only where there is no `wsl.exe`. `run_bounded`
spawns with no shell and, on Windows, `CREATE_NO_WINDOW`; drains both pipes on their own
threads, each capped at the files' 4 MiB, so a large `inspect` cannot fill a pipe and stall the
child while it is waited on; and gives each call `DOCKER_TIMEOUT` (15 s) before killing it,
since a wedged daemon would otherwise hold the button on *Asking Docker…* for as long as it
stays wedged. A failure reads as docker's own first line of standard error, *Docker answered:
…*, which is where it says the daemon is not running. None of this has a test — it is nothing
without a process — and was verified live, when it landed, against the WSL Docker containers
(two SQL Server, one MySQL 8 published on `127.0.0.1:3307`).
