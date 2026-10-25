# Exporting

Writes the tables of a database to disk — either one `.sql` file that rebuilds them, or one file
per table in CSV, JSON, Markdown, HTML or Excel.

## Opening it

Right-click in the schema tree and choose **Export**, then a format:

- on a *database*, every table in it starts ticked;
- on a *table*, only that table is ticked, with the rest of the database a click away;
- on a *schema* (PostgreSQL and SQL Server), the list holds only that schema's tables.

Export works on a read-only connection: it only reads the server.

## Choosing tables

Tick the tables you want. **Select all** and **None** sit above the list.

## A SQL export

The SQL format writes one file that recreates the tables and their rows when you run it — with
**Import** on a database (see [Running a SQL script](run-script.md)), or any client.

| Option                      | What it does                                                         |
|-----------------------------|----------------------------------------------------------------------|
| **Structure**               | `CREATE TABLE`, the table's triggers, and its foreign keys           |
| **Data**                    | Every row, as `INSERT` statements                                    |
| **Other objects**           | Types, sequences, routines and events the tables rely on             |
| **Drop before create**      | Drops each table first, so the file loads onto a database that has them |
| **One transaction**         | Wraps the load, so a failure leaves nothing behind                   |
| **Disable foreign-key checks** | Lets rows arrive in any order (MySQL, MariaDB and SQLite)         |

All are on to start with. Each option's note in the window says exactly what it does on your
engine — read the one under **Drop before create** before replaying a file onto a database you
care about: on every engine but SQLite it also replaces the routines in the dumped schemas.

Press **Export** and choose where to save. Foreign keys are added after all the data, and
triggers after that, so restoring rows doesn't fire them.

## The other formats

CSV, JSON, Markdown, HTML and Excel write one file per table into a folder you choose; there are
no options. If any of the files already exists there, you're asked before it's replaced
(**Replace files**).

## While it runs

The footer shows which table is being written and how many rows so far. **Stop** cancels the
export; a file it was replacing is left as it was, and what had been written is kept beside it
under another name, which the footer gives. When it's done, the footer says how many
tables were written — and names anything that didn't make it, such as a table that disappeared
since you ticked it, or binary columns a text format couldn't hold.
