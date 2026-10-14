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
//! specific blocks a config lists first beat the `Host *` it ends with.
//!
//! **Not read**, and said so rather than half-done: `Match` blocks (whose
//! conditions need the local user, the network and a shell), `Include`, and
//! every token but `%h`, `%d`, `~` and `%%`. A `Match` block's options are
//! skipped as a block that did not match.

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
    for line in crate::text::strip_bom(text).lines() {
        let Some((key, arg)) = split_line(line) else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                applies = host_matches(&arg, alias);
                continue;
            }
            // Not evaluated: its block counts as one that did not match.
            "match" => {
                applies = false;
                continue;
            }
            _ if !applies => continue,
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
            "proxyjump" if out.proxy_jump.is_none() => {
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
/// changes nothing ([`apply`] answers the tunnel as it was).
pub fn describe(tunnel: &crate::connection::SshTunnel, cfg: &SshConfigHost) -> Option<String> {
    let t = apply(tunnel, cfg);
    if t == *tunnel {
        return None;
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
    let arg = match rest.strip_prefix('"') {
        Some(inner) => inner.split('"').next().unwrap_or("").to_string(),
        None => rest.to_string(),
    };
    (!key.is_empty() && !arg.is_empty()).then_some((key, arg))
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
}
