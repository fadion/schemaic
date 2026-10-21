# `core/ssh_config.rs`

`ssh_config.rs` — **reading `~/.ssh/config` for an SSH tunnel's host**, so a tunnel whose host
is one of the user's `Host` aliases reaches what `ssh <alias>` would. Pure over the file's text
but for **`read_user_config`, the one read of it** — `<USERPROFILE or HOME>/.ssh/config`, refused
over `MAX_CONFIG_BYTES`, decoded with `text::decode_text_file`, answering the text and the home
directory or `None`. It was `db::ssh`'s private `read_ssh_config`, moved here verbatim so the
connection form reads the same file the tunnel does; when each of them reads it is in the
`ssh.rs` passage and under `connection_form.rs`. `resolve(text, alias, home)` answers an `SshConfigHost` — `hostname`, `port`, `user`,
`identity_file`, `proxy_jump`, each `None` where no matching block set it, and `proxy_jump`
`Some("")` for an explicit `none` — `ProxyJump none` or `ProxyCommand none` — so a caller can
tell *no jump, said so* from *nothing said*; and `unread_route`, below.
OpenSSH's rules as far as they go here: keywords case-insensitive with the argument after
whitespace or `=`, double quotes stripped, `#` comments, options before the first `Host`
applying to every host, `Host` patterns with `*` and `?` matched ASCII-case-insensitively, a `!`
pattern refusing the block outright, and the **first** value found winning — so the specific
blocks a config lists first beat the `Host *` it ends with
(`an_alias_takes_its_blocks_first_values`,
`a_wildcard_block_applies_and_a_later_star_fills_only_what_is_left`,
`a_negated_pattern_refuses_the_block_and_percent_h_is_the_alias`). **A trailing comment ends
the line** — a `#` that starts a word outside double quotes, as OpenSSH's argument splitter
has it (`without_comment`). Before, it was part of the value: `HostName x # c` resolved to
`x # c`, `Port 2222 # c` claimed the first-wins `Port` slot with a port nobody could read, and
`Host h # box` made `box` a pattern (`a_trailing_comment_is_no_part_of_a_value`). **Not read,
and the module doc says so rather than half-doing them**: `Match` blocks, whose conditions need
the local user, the network and a shell — skipped as a block that did not match, so their
options never leak into the answer (`a_match_block_is_skipped_and_its_options_never_leak`);
`ProxyCommand`, a program to run; and every
token but `%h`, `%d`, a leading `~` and `%%`, with `~` and `%d` left as written when no home
directory is known. **A route either of the first two would set is marked, not skipped
silently** (`unread_route`), because skipping it dialled the alias straight at its `HostName`
— trust on first use, and under password sign-in the password, to whatever answered there, by
a route `ssh` itself never takes. `ProxyJump` and `ProxyCommand` share one first-wins slot, as
ssh_config(5) has them, so a `ProxyCommand` that takes it is `UnreadRoute::ProxyCommand` and
keeps a later `Host *`'s `ProxyJump` out
(`a_proxycommand_route_is_marked_unread_and_takes_the_jump_slot`). A `Match` block is still
skipped, but one setting either (not to `none`) while the slot is unclaimed marks
`UnreadRoute::Match` unless its `host` or `originalhost` conditions rule this host out
(`match_may_apply` — `host` is the `HostName` found so far, else the alias). Every other
criterion — `exec`, `user`, `localnetwork` — needs the machine to answer, so it counts as
may-apply (`a_match_block_that_may_route_the_host_is_marked_unread`); the cost is that a
`Match` which would never fire here refuses a tunnel `ssh` would open, and typing the route as
jump hosts is the way past it. **A may-apply `Match`'s `none` counts too**: it may settle the
slot, so a route a later block sets is `UnreadRoute::Match` — the "direct on the office LAN,
through the bastion elsewhere" config went silently through the bastion
(`a_match_none_before_a_later_route_marks_it_unread`). **And the destination**: a may-apply
`Match` setting `HostName` or `Port` before any block has is `UnreadRoute::MatchDestination`,
which typed jump hosts do not settle (`UnreadRoute::settled_by_typed_jumps`) — skipped, the
tunnel dialled the bare alias, trusting its key and, under password sign-in, sending the
password there (`a_match_block_that_may_set_the_destination_is_marked_unread`). **`Include` is
read** (`expand_includes`, the reader a seam): `read_user_config` replaces each `Include` line
with the files it names — relative to `~/.ssh`, a wildcard in the last component matched in name
order, OpenSSH's nesting limit of 16, each file under `MAX_CONFIG_BYTES` — in spans `resolve`
scopes as OpenSSH does: an include in a block that does not apply is skipped, and the block
state is put back after it. One that cannot be read is left as its line, which `resolve` marks
`UnreadRoute::Include` where it applies (`an_include_is_read_where_it_stands`); it was not read
at all, so an alias whose `HostName` lived in an included `config.d/work` dialled the bare name.
An unparseable `Port` or a keyword with no argument says nothing — and the
first `Port` still counts when it is unreadable, so a later block's cannot slip in behind it,
where OpenSSH would refuse the file (`an_empty_or_unparseable_config_says_nothing`). `MAX_CONFIG_BYTES` (1 MiB) is the largest file
the reader takes; a real one is a few kilobytes.
**`apply(tunnel, cfg)` is where the connection and the file meet, and what the connection
states wins**, as on `ssh`'s command line. The host always becomes the alias's `HostName` — the
connection's host *is* the name looked up, so an alias always resolves. The port takes the
config's only where the connection's is 22, which is the form's default and cannot be told from
a 22 typed on purpose, so a deliberate 22 loses to a config `Port`. The user is the config's
where the connection names none; the key path is the config's `IdentityFile` only for
`SshAuth::KeyPair` with no key file, since a key file means nothing to password or agent
sign-in; and the jump hosts are the config's `ProxyJump` where the connection names none
(`the_config_fills_what_the_connection_leaves_open`,
`what_the_connection_states_wins_over_the_config`, `an_explicit_proxyjump_none_names_no_jump`).
`apply_to_hop(hop, cfg)` is the jump hosts' narrower version: `HostName`, and the config's port
where the hop's is 22, but **never the config's `User`** — `SshTunnel::route` has already filled
an unnamed hop's user with the tunnel's, so by then a config `User` cannot be told from an
override (`a_jump_hop_takes_its_hostname_and_a_port_left_at_22`). Nor does it read a jump host's
own `ProxyJump` or `IdentityFile`, or its `unread_route`: a route is not followed recursively as
`ssh` follows it, and every hop signs in with the tunnel's credentials anyway — under password
sign-in, only a hop the connection names (`SshTunnel::jump`, under `connection.rs`). The
**first** jump host's own route is not ignored, though — `resolve_route` refuses it, below,
rather than following it or dialling around it.
**`resolve_route(tunnel, config)` is the composition `open_tunnel` connects by** — moved out of
it verbatim, so that it could be tested, answering a `Route { tunnel, hops }` or the
`RouteRefusal` below. With no config
it is `route()` as typed (`a_route_without_a_config_is_the_route_as_typed`); otherwise the SSH
host is `resolve`d and `apply`'d **before** `route()`, so a config `ProxyJump` is read by the
same reader as typed jump hosts, then every hop but the last goes through `apply_to_hop` — the
last is the SSH host, whose `HostName` is a resolved name and not an alias to look up again —
and a hop naming no user takes the resolved tunnel's
(`an_alias_and_its_config_jump_resolve_hop_by_hop`,
`the_last_hop_is_not_looked_up_a_second_time`). **It refuses three ways, before anything is
dialled.** An `unread_route` where the connection names no jump hosts is refused with
*~/.ssh/config reaches <alias> through a ProxyCommand, which Schemaic cannot run. Give the
route as Jump hosts instead.* (or *may route <alias> through a Match block, which Schemaic
does not read*); typed jump hosts still win over the config, so they are the way past it
(`a_route_the_config_hides_in_a_proxycommand_is_refused`). And password sign-in through jump
hosts only the config's `ProxyJump` supplies is refused with *~/.ssh/config reaches <alias>
through <jump>, and a password is sent only to the hosts this connection names. Add the jump
host to Jump hosts if it takes the same password, or sign in with a key or the SSH agent.* —
`may_offer_password(HopOrigin)` is true only for `HopOrigin::Connection`, because one sign-in
for the whole route was a decision about the jump hosts the user typed, and a host the config
alone supplies, trusted on first sight, would otherwise receive the target's password. Key and
agent sign-in send no secret and are unaffected
(`a_password_is_offered_only_to_hosts_the_connection_names`). And **the first jump host's own
route** — an `unread_route`, or a non-empty `ProxyJump` of its own — refuses the whole route
(`first_hop_refusal`), with the unread-route sentence about *jump host <alias>* or
*~/.ssh/config reaches jump host <alias> through <jump>, and Schemaic does not follow a jump
host's own ProxyJump. Give the route as Jump hosts instead, <jump> first.* It is the first hop
alone because of how `ssh` builds the chain: `ssh -J a,b host` reaches `b` as `ssh -J a -W … b`,
so the chain claims every later hop's route slot and `ssh` never takes a later hop's own route
either — the route stands as typed (`a_later_jump_hosts_own_route_is_the_chain_before_it`). But
`a` is dialled as `ssh a` would be, by `a`'s own route, and Schemaic used to skip that silently
and dial `a` directly — a route the user never configured. Its own `ProxyJump` is refused
rather than followed, so nothing is followed recursively, and `apply_to_hop` is unchanged. An
explicit `ProxyJump none` is still a direct dial. It applies whether the jump hosts were typed
or came from the SSH host's config `ProxyJump` — typing them is no way past it, unlike the
first refusal; putting that hop's route in front of it in Jump hosts is
(`a_route_the_config_hides_for_the_first_jump_host_is_refused`).
**`describe(tunnel, cfg)` is what the connection form says the file made of the tunnel** —
`~/.ssh/config: user@hostname:port`, the user left off when neither the connection nor the file
names one, then `, key <path>` and `, through <jump>` **only where the config supplied them**,
not where the connection already states them, so the line attributes to the file what came
from the file. It is phrased over `apply` and compares its answer with the tunnel it was given:
`None` when nothing changed, which is a host no block names as much as a field-for-field match,
and an explicit `ProxyJump none` adds no *through*
(`the_form_is_told_what_an_alias_resolved_to`,
`a_host_the_config_does_not_change_is_described_as_nothing`,
`a_user_left_open_everywhere_reads_as_the_host_alone`). Going through `apply` rather than
reading `cfg` is the point: the form cannot describe a resolution the connect does not perform.
Where the connect would refuse, `describe` answers the refusal's own sentence instead of that
line — the unread route where the connection names no jump hosts, the password one where
password sign-in would go through a config-supplied jump — so the form says so before anyone
connects (`the_form_says_when_the_config_routes_the_host_unreadably`). **The form asks
`describe_route(tunnel, config)`**, which answers `resolve_route`'s refusal where the connect
refuses and `describe`'s line otherwise: `describe` sees only the host's own block, so the
connect's refusal of the first jump host's own route read as *through inner* in the form
(`the_form_names_the_first_jump_hosts_refusal`). `describe` itself reaches its refusals
through the same `RouteRefusal`s and `may_offer_password`, not by calling `resolve_route`, and
speaks only of the SSH host — which is why the first jump host's refusal comes to the form
through `describe_route` alone. **Both answer a value, `RouteNote`, not a sentence** —
`Refused(RouteRefusal)`, or `Resolved { user, host, port, key, through }` with the optional
parts `None` where the line leaves them off — and `RouteRefusal` is one too: `JumpHost`
(a `JumpHostError`, with a `From` so `route()?` reads as it did), `Unread { host, unread }`,
`ConfigJumpPassword { host, jump }` and `JumpOwnProxyJump { host, jump }`, where `RouteHost`
tells the SSH host (`Ssh`) from the first jump host (`FirstJump`, which the English calls
*jump host <alias>*). The connection form words them in the interface's language, and so does
the connect's error, which carries one as `SshFailure::Route` (under `core::model`) and is
worded through the form's `route_note_text`; each `Display` is the English the CLI and the logs
still read, byte for byte the `String` it replaced, and `unread_route_sentence` and `config_jump_password_sentence` survive as that
English. The tests here read the `Display`.
