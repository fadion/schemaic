# Importing connections

If you're coming from another database client, you don't have to type your connections in again.
In **Manage connections**, choose **Import connections**.

## Where they can come from

| Source                     | How                                                                         |
|----------------------------|-----------------------------------------------------------------------------|
| A URL                      | Paste it into **Connection URL** and press **Add**                          |
| Clients on this machine    | **Scan installed clients**                                                  |
| Running containers         | **Docker containers**                                                       |
| A file                     | **Choose a file…**                                                          |

- *A URL* can be a `mysql://`, `mariadb://`, `postgresql://`, `sqlserver://` or `sqlite:` URL, a
  JDBC URL, a `DATABASE_URL=` line copied from a `.env` file, or a SQL Server connection string
  (`Server=…;Database=…`). Paste several, one per line.
- *Scan installed clients* looks where other tools keep their connections: DBeaver, DataGrip and
  the other JetBrains IDEs, and the command-line clients' `~/.my.cnf`, `~/.pgpass` and
  `~/.pg_service.conf`.
- *Docker containers* lists the database containers Docker is running here, with the port each
  publishes to this machine.
- *A file* is any of the above saved somewhere else, or a file made by **Export connections**.

## Choosing what to import

Nothing is saved until you press **Import**. Until then you get a list of what was found, each row
ticked, with a note where something needs your attention:

- *No password in the source* — DBeaver and DataGrip keep passwords where Schemaic can't read them.
  Type the password into the connection after importing. For PostgreSQL, Schemaic may find it in
  your own `~/.pgpass` instead; such a row says *Password taken from your own .pgpass*.
- *Already saved* — you already have a connection to the same server. The row is shown, unticked,
  in case the saved one is out of date.
- *Signs in a way Schemaic can't* — give it a SQL login after importing.

Untick what you don't want and press **Import**. Imported passwords go to the keyring, like any
other.

Read-only mode and the environment badge are never brought over from another client: set them on
anything that matters before you use it. See [Safeguards](safeguards.md).

## Sharing your connections

**Export connections** saves your connections to a file you can give to a colleague, or import on
another machine. The file leaves out every password, SSH password and key passphrase, and resets
**CLI access** and **AI data access** — permission to use a database from the command line, or to
show its data to an AI, is given on each machine, not carried with the file. Paths to keys and
certificates stay in it.
