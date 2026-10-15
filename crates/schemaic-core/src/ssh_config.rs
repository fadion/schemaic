//! Reading **`~/.ssh/config`** for an SSH tunnel's host — so a tunnel whose
//! host is one of the user's `Host` aliases reaches what `ssh <alias>` would:
//! its `HostName`, `Port`, `User`, `IdentityFile` and `ProxyJump`.
//!
//! Pure over the file's text: the tunnel opener reads the file
//! (`db::ssh::open_tunnel`) and this decides what it says. OpenSSH's rules, as
//! far as they go here: keywords are case-insensitive and take their argument
//! after whitespace or `=`; options before the first `Host` apply to every
//! host; a `Host` line's patterns use `*` and `?`, a `!` pattern refuses the
//! block outright, and the **first** value found for an option wins, so the
//! specific blocks a config lists first beat the `Host *` it ends with. A `#`
//! that starts a word ends the line, as OpenSSH's argument splitter has it.
//!
//! **Not read**, and said so rather than half-done: `Match` blocks (whose
//! conditions need the local user, the network and a shell), `ProxyCommand`
//! (a program to run), `Include`, and every token but `%h`, `%d`, `~` and
//! `%%`. A `Match` block's options are skipped as a block that did not match.
//! A **route** either would set is not skipped silently — that would dial the
//! `HostName` directly, by a route `ssh` never takes — but marked
//! ([`SshConfigHost::unread_route`]): a `ProxyCommand` that takes the route
//! slot, or a `ProxyJump`/`ProxyCommand` in a `Match` block whose `host` and
//! `originalhost` conditions do not rule this host out. [`resolve_route`] then
//! refuses, saying so, unless the connection names its own jump hosts.
//!
//! **A password goes only to hosts the connection names**
//! ([`may_offer_password`]): a jump host only the config's `ProxyJump` supplies
//! is refused under password sign-in, before anything is dialled.

/// What `~/.ssh/config` says about one host alias. Each field is `None` where
/// no block that matched set it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SshConfigHost {
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    /// The first `IdentityFile`, its `~`/`%d` expanded against the home
    /// directory the caller gave.
    pub identity_file: Option<String>,
    /// `ProxyJump` as written — `Some("")` for an explicit `none`, which
    /// stops a later block's jump from applying.
    pub proxy_jump: Option<String>,
    /// A route to the host this reading cannot follow, where one may apply —
    /// so the tunnel refuses rather than dialling the `HostName` directly.
    pub unread_route: Option<UnreadRoute>,
}

/// A way `~/.ssh/config` may route a host that [`resolve`] does not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnreadRoute {
    /// The alias's route is a `ProxyCommand` — a program to run, which
    /// Schemaic does not. It takes the slot a later `ProxyJump` would.
    ProxyCommand,
    /// A `Match` block that may apply sets `ProxyJump` or `ProxyCommand`, and
    /// its conditions are not evaluated.
    Match,
}

/// The largest config file worth reading. A real one is a few kilobytes.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// `~/.ssh/config`'s text and the home directory it sits in, or `None` when
/// there is no such file (or it is too large to be one). The one read of it:
/// the tunnel opener reads it per connect (`db::ssh::open_tunnel`), as `ssh`
/// reads it per connection, so an edit applies at the next connect; the
/// connection form reads it once when the tunnel's fields are shown.
pub fn read_user_config() -> Option<(String, Option<String>)> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .filter(|h| !h.is_empty())?;
    let path = std::path::Path::new(&home).join(".ssh").join("config");
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_CONFIG_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    Some((
        crate::text::decode_text_file(&bytes),
        Some(home.to_string_lossy().into_owned()),
    ))
}

/// What `text` (an `ssh_config`) says about `alias`, with `home` (when known)
/// for `~` and `%d` in an `IdentityFile`.
pub fn resolve(text: &str, alias: &str, home: Option<&str>) -> SshConfigHost {
    let mut out = SshConfigHost::default();
    // Options before the first `Host` apply to every host.
    let mut applies = true;
    // Whether a `Port` has been found, readable or not — first value wins.
    let mut port_seen = false;
    // Whether the route is settled: `ProxyJump` and `ProxyCommand` share one
    // slot, and whichever comes first keeps the other out (ssh_config(5)).
    let mut route_seen = false;
    // Inside a `Match` block: whether it may apply to this host.
    let mut match_block: Option<bool> = None;
    for line in crate::text::strip_bom(text).lines() {
        let Some((key, arg)) = split_line(line) else {
            continue;
        };
        let key = key.to_ascii_lowercase();
        match key.as_str() {
            "host" => {
                applies = host_matches(&arg, alias);
                match_block = None;
                continue;
            }
            // Not evaluated: its block counts as one that did not match —
            // unless it may set the route, which is then marked unread.
            "match" => {
                applies = false;
                let host = out.hostname.as_deref().unwrap_or(alias);
                match_block = Some(match_may_apply(&arg, alias, host));
                continue;
            }
            "proxyjump" | "proxycommand"
                if match_block == Some(true)
                    && !route_seen
                    && !arg.eq_ignore_ascii_case("none") =>
            {
                out.unread_route.get_or_insert(UnreadRoute::Match);
                continue;
            }
            _ if !applies => continue,
            "proxyjump" | "proxycommand" if route_seen => {}
            "proxycommand" => {
                route_seen = true;
                if arg.eq_ignore_ascii_case("none") {
                    out.proxy_jump = Some(String::new());
                } else {
                    out.unread_route.get_or_insert(UnreadRoute::ProxyCommand);
                }
            }
            "hostname" if out.hostname.is_none() => {
                out.hostname = Some(expand(&arg, alias, home));
            }
            "port" if !port_seen => {
                port_seen = true;
                out.port = arg.parse::<u16>().ok().filter(|p| *p > 0);
            }
            "user" if out.user.is_none() => out.user = Some(arg),
            "identityfile" if out.identity_file.is_none() => {
                out.identity_file = Some(expand(&arg, alias, home));
            }
            "proxyjump" => {
                route_seen = true;
                out.proxy_jump = Some(if arg.eq_ignore_ascii_case("none") {
                    String::new()
                } else {
                    arg
                });
            }
            _ => {}
        }
    }
    out
}

/// `tunnel` as `~/.ssh/config` completes it, its host having been looked up as
/// an alias: what the connection states wins, as on `ssh`'s command line, and
/// the config fills what it leaves open.
///
/// - the host becomes the alias's `HostName`;
/// - the port is the config's where the connection's is 22 — the form's
///   default, which cannot be told from a port typed as 22;
/// - the user is the config's where the connection names none;
/// - the key is the config's `IdentityFile` where key-pair sign-in names no
///   key file;
/// - the jump hosts are the config's `ProxyJump` where the connection names
///   none (and an explicit `none` there names none).
pub fn apply(
    tunnel: &crate::connection::SshTunnel,
    cfg: &SshConfigHost,
) -> crate::connection::SshTunnel {
    let mut t = tunnel.clone();
    if let Some(h) = &cfg.hostname {
        t.host = h.clone();
    }
    if let (22, Some(p)) = (t.port, cfg.port) {
        t.port = p;
    }
    if let (true, Some(u)) = (t.user.trim().is_empty(), &cfg.user) {
        t.user = u.clone();
    }
    if t.auth == crate::connection::SshAuth::KeyPair
        && t.key_path.trim().is_empty()
        && let Some(k) = &cfg.identity_file
    {
        t.key_path = k.clone();
    }
    if let (true, Some(j)) = (t.jump.trim().is_empty(), &cfg.proxy_jump) {
        t.jump = j.clone();
    }
    t
}

/// What `~/.ssh/config` makes of `tunnel`, as the connection form shows it
/// under the SSH host — `~/.ssh/config: user@hostname:port`, then the key and
/// the jump hosts where the config supplied them — or `None` when the config
/// changes nothing ([`apply`] answers the tunnel as it was). A route the
/// config hides where the connection names no jump hosts is said instead, in
/// the sentence the connect refuses with ([`resolve_route`]).
pub fn describe(tunnel: &crate::connection::SshTunnel, cfg: &SshConfigHost) -> Option<String> {
    if let (true, Some(unread)) = (tunnel.jump.trim().is_empty(), cfg.unread_route) {
        return Some(unread_route_sentence(&tunnel.host, unread));
    }
    let t = apply(tunnel, cfg);
    if t == *tunnel {
        return None;
    }
    if tunnel.jump.trim().is_empty()
        && !t.jump.trim().is_empty()
        && t.auth == crate::connection::SshAuth::Password
        && !may_offer_password(HopOrigin::Config)
    {
        return Some(config_jump_password_sentence(&tunnel.host, &t.jump));
    }
    let user = t.user.trim();
    let mut out = if user.is_empty() {
        format!("~/.ssh/config: {}:{}", t.host, t.port)
    } else {
        format!("~/.ssh/config: {user}@{}:{}", t.host, t.port)
    };
    if t.key_path != tunnel.key_path {
        out.push_str(&format!(", key {}", t.key_path));
    }
    if t.jump != tunnel.jump && !t.jump.is_empty() {
        out.push_str(&format!(", through {}", t.jump));
    }
    Some(out)
}

/// A jump host as `~/.ssh/config` completes it: its `HostName`, and the
/// config's port where the hop's is 22. Its user stays the one the route gave
/// it — the route has already filled an unnamed one with the tunnel's, so a
/// config `User` cannot be told from an override.
pub fn apply_to_hop(
    hop: crate::connection::SshHop,
    cfg: &SshConfigHost,
) -> crate::connection::SshHop {
    let mut hop = hop;
    if let Some(h) = &cfg.hostname {
        hop.host = h.clone();
    }
    if let (22, Some(p)) = (hop.port, cfg.port) {
        hop.port = p;
    }
    hop
}

/// A tunnel's route as `~/.ssh/config` completes it ([`resolve_route`]): the
/// tunnel as signed in with — its host resolved, the config's key and user
/// filled in — and every SSH server on the way, jump hosts first.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub tunnel: crate::connection::SshTunnel,
    pub hops: Vec<crate::connection::SshHop>,
}

/// `tunnel`'s route, with `config` (`~/.ssh/config`'s text and home, as
/// [`read_user_config`] answers) applied: the SSH host resolves as an alias
/// *before* the route is built, so a config-supplied `ProxyJump` is read by the
/// same reader as typed jump hosts; then each jump host — never the last hop,
/// whose `HostName` is already a resolved name and not an alias to look up
/// again — takes its own `HostName` and port. A hop's user is the resolved
/// tunnel's where the entry names none.
///
/// `Err` with the sentence the connect shows when the route cannot be built.
pub fn resolve_route(
    tunnel: &crate::connection::SshTunnel,
    config: Option<(&str, Option<&str>)>,
) -> Result<Route, String> {
    let Some((text, home)) = config else {
        return Ok(Route {
            hops: tunnel.route()?,
            tunnel: tunnel.clone(),
        });
    };
    let found = resolve(text, &tunnel.host, home);
    let typed_jumps = !tunnel.jump.trim().is_empty();
    if let (false, Some(unread)) = (typed_jumps, found.unread_route) {
        return Err(unread_route_sentence(&tunnel.host, unread));
    }
    let resolved = apply(tunnel, &found);
    let mut hops = resolved.route()?;
    let jumps = hops.len() - 1;
    let jump_origin = if typed_jumps {
        HopOrigin::Connection
    } else {
        HopOrigin::Config
    };
    if jumps > 0
        && resolved.auth == crate::connection::SshAuth::Password
        && !may_offer_password(jump_origin)
    {
        return Err(config_jump_password_sentence(&tunnel.host, &resolved.jump));
    }
    for hop in hops.iter_mut().take(jumps) {
        let found = resolve(text, &hop.host, home);
        *hop = apply_to_hop(hop.clone(), &found);
    }
    Ok(Route {
        tunnel: resolved,
        hops,
    })
}

/// Where a hop of a tunnel's route came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HopOrigin {
    /// The connection names it — its SSH host, or a jump host typed into it.
    Connection,
    /// Only `~/.ssh/config`'s `ProxyJump` names it.
    Config,
}

/// May password sign-in send the connection's password to a hop from
/// `origin`? Only to a host the connection names: every hop signing in the
/// tunnel's way is a decision about the jump hosts the user typed, and a jump
/// host the config alone supplies — trusted on first sight — would otherwise
/// receive the target's password. Key and agent sign-in send no secret.
pub fn may_offer_password(origin: HopOrigin) -> bool {
    origin == HopOrigin::Connection
}

/// Why password sign-in refuses the jump hosts `~/.ssh/config` alone gives
/// `alias` — the connect's refusal and the form's note alike.
fn config_jump_password_sentence(alias: &str, jump: &str) -> String {
    format!(
        "~/.ssh/config reaches {alias} through {jump}, and a password is sent only to the \
         hosts this connection names. Add the jump host to Jump hosts if it takes the same \
         password, or sign in with a key or the SSH agent."
    )
}

/// Why a host whose route `~/.ssh/config` hides cannot be dialled — the
/// connect's refusal and the form's note alike.
fn unread_route_sentence(alias: &str, unread: UnreadRoute) -> String {
    match unread {
        UnreadRoute::ProxyCommand => format!(
            "~/.ssh/config reaches {alias} through a ProxyCommand, which Schemaic cannot run. \
             Give the route as Jump hosts instead."
        ),
        UnreadRoute::Match => format!(
            "~/.ssh/config may route {alias} through a Match block, which Schemaic does not \
             read. Give the route as Jump hosts instead."
        ),
    }
}

/// One config line as `(keyword, argument)`: blank lines and `#` comments are
/// nothing, the keyword ends at whitespace or `=`, and an argument in double
/// quotes loses them. A keyword with no argument is nothing either.
fn split_line(line: &str) -> Option<(&str, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let key = &line[..end];
    let rest = line[end..].trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
    let rest = without_comment(rest);
    let arg = match rest.strip_prefix('"') {
        Some(inner) => inner.split('"').next().unwrap_or("").to_string(),
        None => rest.to_string(),
    };
    (!key.is_empty() && !arg.is_empty()).then_some((key, arg))
}

/// `arg` up to a trailing comment — a `#` that starts a word outside double
/// quotes, as OpenSSH's argument splitter ends a line there — trimmed.
fn without_comment(arg: &str) -> &str {
    let mut quoted = false;
    let mut word_start = true;
    for (i, c) in arg.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if word_start && !quoted => return arg[..i].trim_end(),
            _ => {}
        }
        word_start = c.is_whitespace();
    }
    arg
}

/// The words of a `Match` line's criteria, a double-quoted one whole and
/// without its quotes.
fn match_words(arg: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in arg.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    words.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        words.push(cur);
    }
    words
}

/// Could a `Match` line's block apply to `alias`, whose host name so far is
/// `host` (its `HostName`, where one was found before the line)? Only a `host`
/// or `originalhost` condition that cannot take it says no — every other
/// condition (`exec`, `user`, `localnetwork`, …) needs the machine to answer,
/// so it may.
fn match_may_apply(criteria: &str, alias: &str, host: &str) -> bool {
    let words = match_words(criteria);
    let mut i = 0;
    while i < words.len() {
        let (negated, name) = match words[i].strip_prefix('!') {
            Some(n) => (true, n.to_ascii_lowercase()),
            None => (false, words[i].to_ascii_lowercase()),
        };
        i += 1;
        if matches!(name.as_str(), "all" | "canonical" | "final") {
            continue;
        }
        let Some(value) = words.get(i) else { break };
        i += 1;
        let subject = match name.as_str() {
            "host" => host,
            "originalhost" => alias,
            _ => continue,
        };
        if host_matches(&value.replace(',', " "), subject) == negated {
            return false;
        }
    }
    true
}

/// Does a `Host` line's pattern list take `alias`? Some positive pattern must
/// match, and no `!` pattern may.
fn host_matches(patterns: &str, alias: &str) -> bool {
    let mut any = false;
    for p in patterns.split_whitespace() {
        match p.strip_prefix('!') {
            Some(neg) if glob(neg, alias) => return false,
            Some(_) => {}
            None => any |= glob(p, alias),
        }
    }
    any
}

/// `*` and `?` wildcard matching, ASCII-case-insensitively as OpenSSH
/// compares host names.
fn glob(pattern: &str, s: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (
        pattern.to_ascii_lowercase().chars().collect(),
        s.to_ascii_lowercase().chars().collect(),
    );
    // The classic backtracking walk: remember the last `*` and retry from it.
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// `%h` (the alias), `%d` and a leading `~` (the home directory, left as
/// written when unknown) and `%%`.
fn expand(arg: &str, alias: &str, home: Option<&str>) -> String {
    let mut out = String::new();
    let mut chars = arg.chars().peekable();
    if let (Some(h), Some('~')) = (home, chars.peek()) {
        out.push_str(h.trim_end_matches(['/', '\\']));
        chars.next();
    }
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(alias),
            Some('d') if home.is_some() => {
                out.push_str(home.unwrap_or("").trim_end_matches(['/', '\\']))
            }
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
# A leading comment, then a global option.
ServerAliveInterval 30

Host prod-db prod-*
    HostName db.prod.internal
    User deploy
    Port 2222
    IdentityFile ~/.ssh/prod_ed25519
    IdentityFile ~/.ssh/fallback
    ProxyJump ops@bastion.example:22

Host *.lab !secret.lab
    User labuser
    HostName %h.corp

Host = quoted
    HostName "with space.example"
    Port=2200

Match host matched
    User nobody-should-see-this

Host *
    User fallback-user
    Port 22
    ProxyJump none
"#;

    #[test]
    fn an_alias_takes_its_blocks_first_values() {
        let h = resolve(CONFIG, "prod-db", Some("/home/me"));
        assert_eq!(h.hostname.as_deref(), Some("db.prod.internal"));
        assert_eq!(h.user.as_deref(), Some("deploy"));
        assert_eq!(h.port, Some(2222));
        assert_eq!(
            h.identity_file.as_deref(),
            Some("/home/me/.ssh/prod_ed25519")
        );
        assert_eq!(h.proxy_jump.as_deref(), Some("ops@bastion.example:22"));
    }

    #[test]
    fn a_wildcard_block_applies_and_a_later_star_fills_only_what_is_left() {
        let h = resolve(CONFIG, "prod-replica", None);
        assert_eq!(h.hostname.as_deref(), Some("db.prod.internal"));
        // `~` is left as written when no home is known.
        assert_eq!(h.identity_file.as_deref(), Some("~/.ssh/prod_ed25519"));
        let other = resolve(CONFIG, "anything", None);
        assert_eq!(other.user.as_deref(), Some("fallback-user"));
        assert_eq!(other.port, Some(22));
        assert_eq!(other.hostname, None);
        // An explicit `none` is kept, so the caller can tell it from unset.
        assert_eq!(other.proxy_jump.as_deref(), Some(""));
    }

    #[test]
    fn a_negated_pattern_refuses_the_block_and_percent_h_is_the_alias() {
        let h = resolve(CONFIG, "web.lab", None);
        assert_eq!(h.user.as_deref(), Some("labuser"));
        assert_eq!(h.hostname.as_deref(), Some("web.lab.corp"));
        let s = resolve(CONFIG, "secret.lab", None);
        assert_eq!(s.user.as_deref(), Some("fallback-user"));
        assert_eq!(s.hostname, None);
    }

    #[test]
    fn equals_signs_quotes_and_case_are_read_as_ssh_reads_them() {
        let h = resolve(CONFIG, "QUOTED", None);
        assert_eq!(h.hostname.as_deref(), Some("with space.example"));
        assert_eq!(h.port, Some(2200));
        let lower = resolve("host x\n  hostname X.EXAMPLE\n  PORT 7\n", "x", None);
        assert_eq!(lower.hostname.as_deref(), Some("X.EXAMPLE"));
        assert_eq!(lower.port, Some(7));
    }

    #[test]
    fn a_match_block_is_skipped_and_its_options_never_leak() {
        let h = resolve(CONFIG, "matched", None);
        assert_eq!(h.user.as_deref(), Some("fallback-user"));
    }

    #[test]
    fn an_empty_or_unparseable_config_says_nothing() {
        assert_eq!(resolve("", "h", None), SshConfigHost::default());
        let bad = resolve("Host h\n  Port notaport\n  User\n", "h", None);
        assert_eq!(bad, SshConfigHost::default());
        // The first `Port` is the one that counts even when it is unreadable:
        // a later block's does not slip in behind it.
        let shadowed = resolve("Host h\n  Port notaport\nHost *\n  Port 7\n", "h", None);
        assert_eq!(shadowed.port, None);
    }

    fn cfg() -> SshConfigHost {
        SshConfigHost {
            hostname: Some("db.prod.internal".into()),
            port: Some(2222),
            user: Some("deploy".into()),
            identity_file: Some("/home/me/.ssh/prod".into()),
            proxy_jump: Some("bastion".into()),
            unread_route: None,
        }
    }

    #[test]
    fn the_config_fills_what_the_connection_leaves_open() {
        use crate::connection::{SshAuth, SshTunnel};
        let open = SshTunnel {
            enabled: true,
            host: "prod-db".into(),
            auth: SshAuth::KeyPair,
            ..SshTunnel::default()
        };
        let t = apply(&open, &cfg());
        assert_eq!(t.host, "db.prod.internal");
        assert_eq!(t.port, 2222);
        assert_eq!(t.user, "deploy");
        assert_eq!(t.key_path, "/home/me/.ssh/prod");
        assert_eq!(t.jump, "bastion");
    }

    #[test]
    fn what_the_connection_states_wins_over_the_config() {
        use crate::connection::{SshAuth, SshTunnel};
        let stated = SshTunnel {
            enabled: true,
            host: "prod-db".into(),
            port: 2200,
            user: "me".into(),
            auth: SshAuth::KeyPair,
            key_path: "/k".into(),
            jump: "edge".into(),
            ..SshTunnel::default()
        };
        let t = apply(&stated, &cfg());
        assert_eq!(t.host, "db.prod.internal", "an alias always resolves");
        assert_eq!((t.port, t.user.as_str()), (2200, "me"));
        assert_eq!((t.key_path.as_str(), t.jump.as_str()), ("/k", "edge"));
        // A key file means nothing to password or agent sign-in.
        let agent = SshTunnel {
            auth: SshAuth::Agent,
            ..stated.clone()
        };
        assert_eq!(apply(&agent, &cfg()).key_path, "/k");
        let pw = SshTunnel {
            auth: SshAuth::Password,
            key_path: String::new(),
            ..stated
        };
        assert_eq!(apply(&pw, &cfg()).key_path, "");
        // Nothing in the config changes nothing.
        let none = apply(&open_tunnel(), &SshConfigHost::default());
        assert_eq!(none, open_tunnel());
    }

    fn open_tunnel() -> crate::connection::SshTunnel {
        crate::connection::SshTunnel {
            enabled: true,
            host: "h".into(),
            ..Default::default()
        }
    }

    #[test]
    fn an_explicit_proxyjump_none_names_no_jump() {
        let c = SshConfigHost {
            proxy_jump: Some(String::new()),
            ..SshConfigHost::default()
        };
        assert_eq!(apply(&open_tunnel(), &c).jump, "");
    }

    #[test]
    fn a_jump_hop_takes_its_hostname_and_a_port_left_at_22() {
        use crate::connection::SshHop;
        let hop = |port| SshHop {
            user: "ops".into(),
            host: "bastion".into(),
            port,
        };
        let h = apply_to_hop(hop(22), &cfg());
        assert_eq!(
            (h.host.as_str(), h.port, h.user.as_str()),
            ("db.prod.internal", 2222, "ops")
        );
        assert_eq!(apply_to_hop(hop(2200), &cfg()).port, 2200);
        assert_eq!(apply_to_hop(hop(22), &SshConfigHost::default()), hop(22));
    }

    #[test]
    fn options_before_the_first_host_apply_to_every_host() {
        let h = resolve("User early\nHost h\n  User late\n", "h", None);
        assert_eq!(h.user.as_deref(), Some("early"));
    }

    #[test]
    fn the_form_is_told_what_an_alias_resolved_to() {
        use crate::connection::{SshAuth, SshTunnel};
        let open = SshTunnel {
            enabled: true,
            host: "prod-db".into(),
            port: 22,
            auth: SshAuth::KeyPair,
            ..SshTunnel::default()
        };
        assert_eq!(
            describe(&open, &cfg()).as_deref(),
            Some(
                "~/.ssh/config: deploy@db.prod.internal:2222, key /home/me/.ssh/prod, through bastion"
            )
        );
        // What the connection states is not reported as the config's.
        let stated = SshTunnel {
            user: "me".into(),
            key_path: "/k".into(),
            jump: "edge".into(),
            ..open.clone()
        };
        assert_eq!(
            describe(&stated, &cfg()).as_deref(),
            Some("~/.ssh/config: me@db.prod.internal:2222")
        );
    }

    #[test]
    fn a_host_the_config_does_not_change_is_described_as_nothing() {
        assert_eq!(describe(&open_tunnel(), &SshConfigHost::default()), None);
        // A HostName equal to the host typed changes nothing either.
        let same = SshConfigHost {
            hostname: Some("h".into()),
            ..SshConfigHost::default()
        };
        assert_eq!(describe(&open_tunnel(), &same), None);
    }

    #[test]
    fn a_user_left_open_everywhere_reads_as_the_host_alone() {
        let c = SshConfigHost {
            hostname: Some("real.example".into()),
            ..SshConfigHost::default()
        };
        assert_eq!(
            describe(&open_tunnel(), &c).as_deref(),
            Some("~/.ssh/config: real.example:22")
        );
    }

    #[test]
    fn percent_d_and_double_percent_expand_in_an_identity_file() {
        let h = resolve(
            "Host h\n  IdentityFile %d/.ssh/id_%%x\n",
            "h",
            Some("C:/Users/me"),
        );
        assert_eq!(h.identity_file.as_deref(), Some("C:/Users/me/.ssh/id_%x"));
    }

    #[test]
    fn a_trailing_comment_is_no_part_of_a_value() {
        let h = resolve(
            "Host h # the box\n  HostName x.example # primary\n  Port 2222 # c\n  User deploy\t# ops\n  ProxyJump bastion # vpn\n",
            "h",
            None,
        );
        assert_eq!(h.hostname.as_deref(), Some("x.example"));
        assert_eq!(h.port, Some(2222));
        assert_eq!(h.user.as_deref(), Some("deploy"));
        assert_eq!(h.proxy_jump.as_deref(), Some("bastion"));
        // A `#` inside a word, or inside quotes, is no comment.
        let inner = resolve("Host h\n  User de#ploy\n  HostName \"a # b\"\n", "h", None);
        assert_eq!(inner.user.as_deref(), Some("de#ploy"));
        assert_eq!(inner.hostname.as_deref(), Some("a # b"));
        // The comment's words are no `Host` patterns either.
        assert_eq!(resolve("Host h # box\n  User u\n", "box", None).user, None);
    }

    #[test]
    fn a_proxycommand_route_is_marked_unread_and_takes_the_jump_slot() {
        let h = resolve(
            "Host db\n  HostName 10.1.2.3\n  ProxyCommand ssh -W %h:%p bastion\nHost *\n  ProxyJump edge\n",
            "db",
            None,
        );
        assert_eq!(h.unread_route, Some(UnreadRoute::ProxyCommand));
        assert_eq!(h.proxy_jump, None, "the later ProxyJump must not apply");
        // A ProxyJump found first wins the slot, and the ProxyCommand after it
        // is nothing.
        let j = resolve(
            "Host db\n  ProxyJump edge\nHost *\n  ProxyCommand nc %h %p\n",
            "db",
            None,
        );
        assert_eq!(
            (j.proxy_jump.as_deref(), j.unread_route),
            (Some("edge"), None)
        );
        // `ProxyCommand none` is a direct connection, as `ProxyJump none` is.
        let none = resolve(
            "Host db\n  ProxyCommand none\nHost *\n  ProxyJump edge\n",
            "db",
            None,
        );
        assert_eq!(
            (none.proxy_jump.as_deref(), none.unread_route),
            (Some(""), None)
        );
    }

    #[test]
    fn a_match_block_that_may_route_the_host_is_marked_unread() {
        let routed = resolve("Match host db\n  ProxyJump edge\n", "db", None);
        assert_eq!(routed.unread_route, Some(UnreadRoute::Match));
        assert_eq!(routed.proxy_jump, None);
        let exec = resolve("Match exec \"true\"\n  ProxyCommand nc %h %p\n", "db", None);
        assert_eq!(exec.unread_route, Some(UnreadRoute::Match));
        // A block whose host condition cannot take this host stays skipped.
        let other = resolve("Match host other,*.corp\n  ProxyJump edge\n", "db", None);
        assert_eq!(other, SshConfigHost::default());
        let orig = resolve("Match originalhost other\n  ProxyJump edge\n", "db", None);
        assert_eq!(orig.unread_route, None);
        // `host` is the HostName where one was found first.
        let by_name = resolve(
            "Host db\n  HostName db.corp\nMatch host db.corp\n  ProxyJump edge\n",
            "db",
            None,
        );
        assert_eq!(by_name.unread_route, Some(UnreadRoute::Match));
        // Once the route is settled, a Match block cannot change it.
        let settled = resolve(
            "Host db\n  ProxyJump edge\nMatch all\n  ProxyCommand nc %h %p\n",
            "db",
            None,
        );
        assert_eq!(settled.unread_route, None);
        // A Match block without a route is skipped silently, as before.
        let quiet = resolve("Match all\n  User x\n", "db", None);
        assert_eq!(quiet, SshConfigHost::default());
    }

    fn tunnel(host: &str, auth: crate::connection::SshAuth) -> crate::connection::SshTunnel {
        crate::connection::SshTunnel {
            enabled: true,
            host: host.into(),
            user: "me".into(),
            auth,
            ..Default::default()
        }
    }

    fn hop(user: &str, host: &str, port: u16) -> crate::connection::SshHop {
        crate::connection::SshHop {
            user: user.into(),
            host: host.into(),
            port,
        }
    }

    #[test]
    fn a_route_without_a_config_is_the_route_as_typed() {
        use crate::connection::SshAuth;
        let mut t = tunnel("db", SshAuth::Password);
        t.jump = "ops@edge:2200".into();
        let r = resolve_route(&t, None).unwrap();
        assert_eq!(r.tunnel, t);
        assert_eq!(r.hops, vec![hop("ops", "edge", 2200), hop("me", "db", 22)]);
    }

    #[test]
    fn an_alias_and_its_config_jump_resolve_hop_by_hop() {
        use crate::connection::SshAuth;
        // The hand-verified zzalias/zzjump shape: the alias's ProxyJump names a
        // second alias, which resolves to its own HostName and Port.
        let text = "Host zzalias\n  HostName 127.0.0.1\n  Port 2222\n  User root\n  ProxyJump zzjump\n\
                    Host zzjump\n  HostName 127.0.0.2\n  Port 2223\n  User nobody\n";
        let mut t = tunnel("zzalias", SshAuth::KeyPair);
        t.user = String::new();
        let r = resolve_route(&t, Some((text, None))).unwrap();
        // The jump host takes the resolved tunnel's user, never its own config's.
        assert_eq!(
            r.hops,
            vec![
                hop("root", "127.0.0.2", 2223),
                hop("root", "127.0.0.1", 2222)
            ]
        );
        assert_eq!(r.tunnel.host, "127.0.0.1");
    }

    #[test]
    fn the_last_hop_is_not_looked_up_a_second_time() {
        use crate::connection::SshAuth;
        // `a`'s HostName is `b`, which is itself an alias — `ssh a` dials `b`
        // as a name, and so must the route.
        let text = "Host a\n  HostName b\nHost b\n  HostName c.example\n  Port 2200\n";
        let r = resolve_route(&tunnel("a", SshAuth::Agent), Some((text, None))).unwrap();
        assert_eq!(r.hops, vec![hop("me", "b", 22)]);
    }

    #[test]
    fn a_route_the_config_hides_in_a_proxycommand_is_refused() {
        use crate::connection::SshAuth;
        let text = "Host db\n  HostName 10.1.2.3\n  ProxyCommand ssh -W %h:%p bastion\n";
        let t = tunnel("db", SshAuth::KeyPair);
        let why = resolve_route(&t, Some((text, None))).unwrap_err();
        assert!(why.contains("ProxyCommand"), "{why}");
        assert!(why.contains("Jump hosts"), "{why}");
        let m = resolve_route(&t, Some(("Match all\n  ProxyJump edge\n", None))).unwrap_err();
        assert!(m.contains("Match"), "{m}");
        // Jump hosts the connection names are its route, whatever the config
        // would have done.
        let mut typed = t.clone();
        typed.jump = "edge".into();
        let r = resolve_route(&typed, Some((text, None))).unwrap();
        assert_eq!(
            r.hops,
            vec![hop("me", "edge", 22), hop("me", "10.1.2.3", 22)]
        );
    }

    #[test]
    fn a_password_is_offered_only_to_hosts_the_connection_names() {
        use crate::connection::SshAuth;
        assert!(may_offer_password(HopOrigin::Connection));
        assert!(!may_offer_password(HopOrigin::Config));
        let text = "Host prod\n  ProxyJump jump.corp\n";
        // Password sign-in through a jump host only the config names: refused
        // before anything is dialled, rather than sending prod's password there.
        let why =
            resolve_route(&tunnel("prod", SshAuth::Password), Some((text, None))).unwrap_err();
        assert!(why.contains("jump.corp"), "{why}");
        assert!(why.contains("Jump hosts"), "{why}");
        // A key or the agent sends no secret, so the config's jump stands.
        for auth in [SshAuth::KeyPair, SshAuth::Agent] {
            let r = resolve_route(&tunnel("prod", auth), Some((text, None))).unwrap();
            assert_eq!(
                r.hops,
                vec![hop("me", "jump.corp", 22), hop("me", "prod", 22)]
            );
        }
        // A jump host the connection names is its stated decision.
        let mut typed = tunnel("prod", SshAuth::Password);
        typed.jump = "jump.corp".into();
        assert!(resolve_route(&typed, Some((text, None))).is_ok());
        // A config with no jump for the host changes nothing about passwords.
        assert!(
            resolve_route(
                &tunnel("prod", SshAuth::Password),
                Some(("Host prod\n  Port 2\n", None))
            )
            .is_ok()
        );
        // The form says so before the connect does.
        let found = resolve(text, "prod", None);
        let note = describe(&tunnel("prod", SshAuth::Password), &found).unwrap();
        assert!(note.contains("password is sent only"), "{note}");
        let keyed = describe(&tunnel("prod", SshAuth::KeyPair), &found).unwrap();
        assert!(keyed.ends_with("through jump.corp"), "{keyed}");
    }

    #[test]
    fn the_form_says_when_the_config_routes_the_host_unreadably() {
        let c = SshConfigHost {
            hostname: Some("10.1.2.3".into()),
            unread_route: Some(UnreadRoute::ProxyCommand),
            ..SshConfigHost::default()
        };
        let d = describe(&open_tunnel(), &c).unwrap();
        assert!(d.contains("ProxyCommand"), "{d}");
        // Said even when the config changes nothing else.
        let only = SshConfigHost {
            unread_route: Some(UnreadRoute::Match),
            ..SshConfigHost::default()
        };
        assert!(describe(&open_tunnel(), &only).unwrap().contains("Match"));
        // Not when the connection names its own jump hosts.
        let mut typed = open_tunnel();
        typed.jump = "edge".into();
        assert_eq!(describe(&typed, &only), None);
    }
}
