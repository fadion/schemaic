# SSH tunnels

When a database server can't be reached directly — it only listens on a private network, or behind
a bastion host — Schemaic can reach it through an SSH server that can. Every connection Schemaic
opens then travels through the tunnel; you don't need to start one yourself.

## Setting one up

In the connection form's **Network** section, turn on **SSH tunnel** and fill in:

- **SSH host** and **SSH port** — the machine you can SSH into.
- **SSH user**, and under **Authentication** how to sign in to it:
  - **Password**, typed in **SSH password**;
  - **Key pair**, with the **Private key** file and its **Passphrase** if it has one;
  - **SSH agent**, which leaves the signing to your running agent — the OpenSSH agent, or Pageant
    on Windows. Schemaic stores no key. If the agent has no keys loaded, run `ssh-add`.

**Host** and **Port** in the **Database** section stay as the *SSH host* sees them. A database on
the SSH host itself is usually `127.0.0.1`.

SQLite has no tunnel, since there's no server to reach. A SQL Server named instance
(`server\instance`) can't be reached through a tunnel either, because SQL Server Browser answers
over UDP; give the server's host and the instance's own port instead.

## Jump hosts

If the SSH host is itself only reachable through other hosts, list them in **Jump hosts**,
comma-separated and nearest first — as OpenSSH's `ProxyJump` does. Each one signs in the same way as
the SSH host.

## Using `~/.ssh/config`

You can give a `Host` alias from your `~/.ssh/config` as the **SSH host**. Schemaic reads the file
each time it opens the tunnel and fills in what the form leaves empty — the real address, port,
user, key and `ProxyJump` — as `ssh <alias>` would. A note under the field shows the route it
found.

Some things in the file Schemaic doesn't follow: a `ProxyCommand`, a `Match` block, or a jump host's
own `ProxyJump`. When the route depends on one of those, the note says so; give the route as
**Jump hosts** instead.

## Host keys

The first time Schemaic connects to an SSH host it hasn't seen, it checks the host's key against
your `~/.ssh/known_hosts`. If the host isn't listed there, Schemaic trusts the key and remembers
it.

If the key later changes, Schemaic refuses to connect and shows both fingerprints. That can mean
the server was rebuilt — or that something is intercepting the connection. Check the new
fingerprint with whoever runs the server before you trust it; the message says what to remove so
that Schemaic records the new key.
