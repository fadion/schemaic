# Command line

The `schemaic` command runs SQL against your saved connections without opening the window. It
never takes a credential: the connection comes from your saved list and its password from the OS
keyring, so a script, a cron job or an AI agent can be handed a connection *name* instead of a
password. Where nothing is saved, [`--url`](#a-connection-without-saving-it) names a
connection on the spot — still with no password on the command line.

```sh
schemaic list
schemaic tables -c prod -d shop
schemaic query "SELECT * FROM orders LIMIT 5" -c prod -d shop --format json
schemaic exec "UPDATE orders SET state = 'sent' WHERE id = 7" -c prod -d shop
```

## Setting it up

1. *Put it on your PATH.* Open **Settings** › **General** and press **Install** on the
   **Command line** row. On Windows this adds Schemaic's folder to your user PATH; on macOS and
   Linux it links the command into `~/.local/bin`. Open a new terminal afterwards. The Linux
   `.deb` and `.rpm` packages already install the command, so there the button is greyed out.
   **Remove** undoes it.
2. *Let it use a connection.* Edit the connection and turn on **CLI access**, under
   **Safeguards**. It is off for every connection until you do, and a connection without it is
   refused before its password is even read.

`schemaic list` shows the connections the command may use; `schemaic list --all` shows the rest
too, with a `cli access` column, so you can see why one you expected is missing.

## Tab completion

`schemaic completions <shell>` prints a completion script for `bash`, `zsh`, `fish`, `elvish` or
`powershell`. It completes the commands and their flags, not your connection names. Save it
where your shell looks for completions, then open a new terminal:

| Shell      | Where it goes                                                                       |
|------------|-------------------------------------------------------------------------------------|
| bash       | `schemaic completions bash > ~/.local/share/bash-completion/completions/schemaic` (needs the `bash-completion` package) |
| zsh        | A file named `_schemaic` in a folder on your `$fpath`: `schemaic completions zsh > <folder>/_schemaic` |
| fish       | `schemaic completions fish > ~/.config/fish/completions/schemaic.fish`              |
| PowerShell | Add `schemaic completions powershell \| Out-String \| Invoke-Expression` to your `$PROFILE` |

The script comes from the version that printed it, so print it again after an update that adds a
command or a flag.

## Picking a connection and database

Every command that reaches a server takes `-c` (`--connection`) and, where it runs in a database,
`-d` (`--database`):

- `-c` takes the connection's name, ignoring case, or its id from `schemaic list`. `#5` always
  means id 5, even if another connection is *named* `5`. A name that matches two connections is
  refused, not guessed — use the id. Scripts should prefer the id: it never changes when you
  rename the connection.
- `-d` defaults to the connection's own database. `schemaic databases -c prod` lists the names
  it takes.
- `SCHEMAIC_CONNECTION` and `SCHEMAIC_DATABASE` stand in for `-c` and `-d` when set.

Where there is no keyring to read — over SSH, in a container — `--password-stdin` reads the
password from a pipe instead: `cat pw.txt | schemaic ping -c prod --password-stdin`.

## A connection without saving it

In a container or a CI job, where Schemaic isn't set up, give the connection with `--url` (or
`SCHEMAIC_URL`) in place of `-c`. Every command that takes `-c` takes it, except `compare`,
which needs saved connections. `mcp` takes it too, with its own rules — see
[Without a saved connection](mcp.md#without-a-saved-connection).

```sh
schemaic query "SELECT count(*) FROM people" --url "sqlite:/data/app.db" --format csv
SCHEMAIC_PASSWORD="$DB_PASSWORD" schemaic query "SELECT 1" --url "mysql://app@db.example:3306/shop"
```

- *Forms.* `postgresql://user@host:port/db`, `mysql://` or `mariadb://`, `sqlserver://` and
  `sqlite:path`; a SQL Server `Server=…;` connection string, a JDBC URL and a `DATABASE_URL=…`
  line work too — the same forms as pasting a URL into the app.
- *No secret in the URL.* One that carries a password or any other secret is refused (exit 2) —
  `user:pass@`, `password=`, but also `sslpassword=`, `passwd=` or `trustStorePassword=` — since a
  command line ends up in the process list and your shell history. Pipe the password with
  `--password-stdin` or set `SCHEMAIC_PASSWORD`, which is read only for a `--url` connection.
- *A password sign-in only.* A URL that signs in as you — Windows sign-in
  (`Integrated Security=true`) or Microsoft Entra (`Authentication=ActiveDirectoryDefault`) — is
  refused, as it would hand your identity to whatever host the URL names. So is an Entra method
  Schemaic can't perform. Save the connection in Schemaic to sign in that way.
- *Nothing is saved*, and the keyring isn't touched. **CLI access** doesn't apply: you typed the
  connection yourself.
- *Read-only.* Add `--read-only` to refuse writes as a saved read-only connection does (exit 3).
  It only goes with `--url`; a saved connection is made read-only in the app. A URL that asks
  for a read-only session is read-only without it: SQLite's `?mode=ro` or `immutable=1`,
  PostgreSQL's `target_session_attrs=read-only` (or `standby`) or
  `options=-c default_transaction_read_only=on`, SQL Server's `ApplicationIntent=ReadOnly`.
  `--read-only` can add read-only, never take it away.
- *A database.* If the URL names none, pass `-d` or end the URL with `/<database>`.
- `-c` and `--url` together are refused, even when one comes from the environment — unset the
  variable you don't mean. `mcp` is the exception: it serves a `-c` you type beside a `--url`,
  but still refuses `--url` with `SCHEMAIC_CONNECTION` set — unset it, or name it with `-c` to
  serve both.

## The commands

| Command      | What it does                                                                 |
|--------------|------------------------------------------------------------------------------|
| `list`       | The saved connections the command may use                                    |
| `databases`  | The databases on a connection                                                |
| `ping`       | Logs in and runs `SELECT 1`, to check a connection answers                   |
| `tables`     | The tables and views in a database                                           |
| `describe`   | One table's or view's columns: type, nullability, default and key            |
| `query`      | Runs one read-only statement and prints its rows                             |
| `explain`    | Shows how the server would run a read, and what in the plan looks slow       |
| `export`     | Streams a read's whole result into a file                                    |
| `dump`       | Writes a database's tables to a `.sql` file that recreates them              |
| `compare`    | Prints the SQL that would make one database's structure match another's      |
| `exec`       | Runs one statement that writes, or a whole `.sql` script with `--script`     |
| `import`     | Loads a CSV, TSV, JSON or Excel file into an existing table                  |
| `mcp`        | Serves a connection to an AI agent over MCP                                  |
| `version`    | Prints the version                                                           |
| `completions` | Prints a tab-completion script for your shell                                |

`schemaic <command> --help` gives every flag. Each command that waits on a server takes
`--timeout`, in seconds: 30 by default, two minutes for `compare`, an hour for `export`, `dump`,
`import` and `exec --script`.

## Output

Rows go to stdout and everything else — warnings, progress, the "rows were cut short" note — goes
to stderr, so stdout is always safe to pipe or parse. `--format` picks the shape:

| Format     | What you get                                              |
|------------|-----------------------------------------------------------|
| `table`    | A Markdown table with a row-count footer (the default)    |
| `json`     | A JSON array of row objects                               |
| `jsonl`    | One JSON object per line                                  |
| `csv`      | RFC 4180 CSV with a header row                            |
| `vertical` | One `name: value` record per row, like `mysql`'s `\G`     |

`--no-header` leaves out the column names (and the table's footer), for `table` and `csv`.

## Exit codes

| Code | Meaning                                                                                |
|------|----------------------------------------------------------------------------------------|
| 0    | It ran                                                                                 |
| 2    | A usage error, or a connection, file or table that isn't there. Nothing was sent       |
| 3    | Refused: not a read, a read-only connection, no `--yes`, no CLI access, or a connection setting that makes it unsafe. Your statement never ran, and running it again unchanged refuses again |
| 4    | The server or the connection failed. Retrying may work                                 |
| 5    | A write timed out after it was sent and may have been applied. Check before retrying   |
| 6    | `query --fail-on-cap` only: `--limit` cut the rows short (they were still printed)     |
| 7    | `import` only: the file has problems, listed on stderr — or can't be read as its format, or is empty. Nothing was written |

## In this section

- [Reading and exporting](reading.md) — `query`, `explain`, `export`, `dump` and `compare`.
- [Running writes](exec.md) — `exec`, the questions it asks first, and running a whole script.
- [Importing a file](import.md) — `import`, loading a CSV, JSON or Excel file into a table.
- [MCP server](mcp.md) — hand a connection to an AI agent in your editor or desktop app.
