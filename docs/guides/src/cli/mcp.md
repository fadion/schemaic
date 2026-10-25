# MCP server

`schemaic mcp` hands one saved connection to an AI agent that speaks
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

The server takes no password. It reads the connection's from the OS keyring, so nothing secret
goes into the client's configuration file.

## What the agent gets

Nothing the server offers writes to the database. What it can read depends on the connection's
**AI data access**:

| Tool             | What it does                                            | Offered                     |
|------------------|---------------------------------------------------------|-----------------------------|
| `list_databases` | The databases' names                                    | Always                      |
| `list_schema`    | An overview of the server, or one database in full      | Always                      |
| `describe_table` | One table's definition and foreign keys                 | Always                      |
| `search_objects` | Tables, views and columns by part of their name         | Always                      |
| `run_query`      | Runs one read-only statement, 200 rows at most          | **Let it read data** only   |
| `explain`        | A read's plan, and what in it looks slow                | **Let it read data** only   |
| `sample_rows`    | The first rows of a table                               | **Let it read data** only   |

**Only what I attach**, the default, gives an outside agent no rows at all: it has no way to attach
any. `run_query` and `explain` pass the same read-only check as
[`schemaic query`](reading.md#only-reads), and a query is cancelled after 30 seconds.

The level is read when the server starts. If you change it, restart the server from your client.

## When something is missing

The server writes a line to stderr as it starts — most clients show it in the server's log. It
names the connection being served, and says why the row-reading tools are withheld or that there
is no default database, with what to change. Look there first when the agent reports a tool
missing.

A connection without **CLI access** stops the server before it starts, and the client shows it as
failed. A database that's down doesn't: the agent sees the error on its first call, and can try
again.

On Windows, if the client can't find `schemaic`, give `command` the full path to `schemaic.com`
in Schemaic's install folder.
