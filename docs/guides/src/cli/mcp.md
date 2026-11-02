# MCP server

`schemaic mcp` hands connections to an AI agent that speaks
[MCP](https://modelcontextprotocol.io) — an editor's assistant, a desktop app. It's the same
server Schemaic's own [AI assistant](../ai/README.md) uses; you only need this command for an
agent outside the app.

It isn't something you run in a terminal yourself. The agent starts it, talks to it over stdin and
stdout, and stops it when it's done.

## Setting it up

1. Turn on **CLI access** for the connection, as for any other command — see
   [Setting it up](README.md#setting-it-up).
2. Choose how much the agent may read, with **AI data access** in the connection's settings (see
   [below](#what-the-agent-gets)).
3. Add the server to your MCP client's configuration. Most clients take a block like this:

```json
{ "mcpServers": { "prod": { "command": "schemaic", "args": ["mcp", "--connection=prod"] } } }
```

Add `"--database=shop"` to the arguments to give the tools a default database. Without one, on a
server engine, the agent has to name the database in every call.

## Without a saved connection

In a container or a CI job, where Schemaic isn't set up, give the connection with `--url` in place
of `-c` — in the arguments themselves: unlike the other commands, `mcp` doesn't read
`SCHEMAIC_URL`, so an exported one can't quietly add a connection to the agent's server. It takes
the same forms as
[`--url` on the other commands](README.md#a-connection-without-saving-it):

```json
{ "mcpServers": { "app": { "command": "schemaic", "args": ["mcp", "--url", "sqlite:/data/app.db", "--ai-data", "full"] } } }
```

- *Its password comes only from `SCHEMAIC_PASSWORD`.* There's no `--password-stdin`, since stdin
  is the agent's, and a URL that carries a password or any other secret is refused. Most clients
  take an `env` block beside `args` to set it. A URL that signs in as you (Windows or Microsoft
  Entra) is refused too, and one that asks for a read-only session is read-only — the same rules
  as the other commands.
- *`--ai-data` sets what the agent may read*, in place of **AI data access**: `schema-only` (the
  default), `on-request` or `full`. Only `full` lets it run read-only queries and read sample rows;
  `on-request` gives it no rows, as there's nothing to attach them from. `--ai-data` is refused
  without `--url` — a saved connection's level is set in the app.
- *Its name comes from the URL*: the file name for SQLite (`app.db` above), otherwise
  `<database>@<host>`. That's the name the agent and the [agent log](#the-agent-log) see.
- *Nothing is saved*, and **CLI access** doesn't apply.

## Serving several connections

Repeat `-c` to serve more than one connection from the same server:

```json
{ "mcpServers": { "work": { "command": "schemaic", "args": ["mcp", "-c", "prod", "-c", "staging"] } } }
```

- Each connection needs **CLI access**, and each keeps its own **AI data access**. They are all
  checked before any is opened, so one without CLI access stops the server before it starts.
- Every tool takes an extra `connection` argument, the connection's name. A call that leaves
  it out goes to the first `-c`.
- The agent gets a `list_connections` tool: each connection's name, engine, default database, and
  whether it may read rows.
- `compare_schemas` can compare across them: its `source_connection` argument names the
  connection holding the database to match. It reads no rows, so it works whatever either
  connection's **AI data access**.
- `-d` sets the first connection's default database. The others use their own.
- A connection given twice is refused.
- A [`--url`](#without-a-saved-connection) can join them. It's served after the `-c`s, so it's
  never the default, and it can't share a name with a saved connection being served.
  The `-c` has to be typed: `--url` with `SCHEMAIC_CONNECTION` set is refused, so a connection
  exported in your shell can't become the agent's default unnoticed. Unset it, or name it with
  `-c` to serve both.

The server takes no password for a saved connection. It reads it from the OS keyring, so nothing
secret goes into the client's configuration file — however many connections it serves.

## What the agent gets

Nothing the server offers writes to the database. What it can read depends on the connection's
**AI data access** (for a `--url`, on `--ai-data`, where `full` is **Let it read data**):

| Tool             | What it does                                            | Offered                     |
|------------------|---------------------------------------------------------|-----------------------------|
| `list_databases` | The databases' names                                    | Always                      |
| `list_schema`    | An overview of the server, or one database in full      | Always                      |
| `describe_table` | One table's definition and foreign keys                 | Always                      |
| `search_objects` | Tables, views and columns by part of their name         | Always                      |
| `compare_schemas` | The SQL that would make one database's structure match another's | Always            |
| `run_query`      | Runs one read-only statement, 200 rows at most          | **Let it read data** only   |
| `explain`        | A read's plan, and what in it looks slow                | **Let it read data** only   |
| `sample_rows`    | The first rows of a table                               | **Let it read data** only   |
| `list_connections` | The connections the server serves                     | With several connections    |

Two tools the app's own assistant has are left out: `propose_table_change` and `propose_sql`.
Each hands its result to the AI panel — a change to review, a statement to insert or run — and
there's no panel on the other end.

`compare_schemas` is the same comparison as [`schemaic compare`](reading.md#compare): it reads
both databases' structure and hands back the script, with what in it is destructive and what it
leaves out. It runs nothing. Both databases must be the same engine, and a `tables` argument
narrows it to the tables you name.

**Only what I attach**, the default, gives an outside agent no rows at all: it has no way to attach
any. `run_query` and `explain` pass the same read-only check as
[`schemaic query`](reading.md#only-reads), and a query is cancelled after 30 seconds.

With several connections, a tool is listed when any of them allows it, and a call is refused on
a connection whose level withholds it.

The level is read when the server starts. If you change it, restart the server from your client.

## The agent log

Every call an agent makes to these tools is recorded in `agent_log.jsonl`, in the settings folder
beside the [write log](exec.md#the-write-log). That covers the app's own
[AI assistant](../ai/README.md) as well as `schemaic mcp`, and calls that were refused. It's on
for every connection, a `--url` one included, under its name.

Each line is one call: when it was made, the connection and database, the tool and the table or
search it named, whether it came from the panel (`panel`) or `schemaic mcp` (`mcp`), and whether
it succeeded — with the reason, when it was refused or failed. For `run_query` and `explain` the
line holds the SQL too, its values masked unless **Keep values in the write log** is on in
**Settings** › **Query**. SQL that carries a password is masked either way.

A successful call's answer — the rows or definitions the agent was shown — is never copied into
the log.

The file is capped at 8 MiB. Past that it's renamed `agent_log.jsonl.1`, replacing the previous
one, and a new file starts, so the log keeps the most recent calls and drops the oldest.

## When something is missing

The server writes a line to stderr for each connection as it starts — most clients show it in the
server's log. It names the connection being served, and says why the row-reading tools are withheld or that there
is no default database, with what to change. Look there first when the agent reports a tool
missing.

A connection without **CLI access** stops the server before it starts, and the client shows it as
failed. So does an SSH tunnel that won't open: every connection's tunnel opens at the start. A
database that's down doesn't: the agent sees the error on its first call, and can try
again.

On Windows, if the client can't find `schemaic`, give `command` the full path to `schemaic.com`
in Schemaic's install folder.
