# Command line

The `schemaic` command runs SQL against your saved connections without opening the window. It
never takes a credential: the connection comes from your saved list and its password from the OS
keyring, so a script, a cron job or an AI agent can be handed a connection *name* instead of a
password.

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
| `exec`       | Runs one statement that writes                                               |
| `mcp`        | Serves a connection to an AI agent over MCP                                  |
| `version`    | Prints the version                                                           |

`schemaic <command> --help` gives every flag. Each command that waits on a server takes
`--timeout`, in seconds: 30 by default, two minutes for `compare`, an hour for `export` and
`dump`.

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

## In this section

- [Reading and exporting](reading.md) — `query`, `explain`, `export`, `dump` and `compare`.
- [Running writes](exec.md) — `exec`, and the questions it asks first.
- [MCP server](mcp.md) — hand a connection to an AI agent in your editor or desktop app.
