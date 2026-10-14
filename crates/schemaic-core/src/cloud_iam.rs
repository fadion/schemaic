//! **Signing in to a cloud database with the cloud's own CLI** — AWS RDS IAM
//! and Google Cloud SQL IAM, for PostgreSQL and the MySQL family. The pure
//! half: where the CLI is, the argv that asks it for a token, what its answer
//! and its errors mean. `schemaic_db::cloud_iam` runs it and caches the token.
//!
//! `entra`'s shape, and for its reasons: the CLI holds the long-lived sign-in
//! and its refresh, Schemaic asks for a short-lived token when a connection
//! needs one, and **nothing is saved** — the token stands in for the password
//! the connection does not have.
//!
//! - **AWS** — `aws rds generate-db-auth-token --hostname H --port P
//!   --username U [--region R]`: a SigV4-signed token valid fifteen minutes,
//!   for that one host, port and user. The region is read off an RDS endpoint
//!   name ([`rds_region`]); elsewhere the CLI's default applies.
//! - **Google Cloud** — `gcloud sql generate-login-token`: an OAuth access
//!   token scoped to Cloud SQL sign-in, valid an hour, for every instance the
//!   account may reach — which is why its TLS floor is higher
//!   (`connection::AuthMode::tls_floor`).
//!
//! Both arrive as the password; on MySQL in the clear inside TLS, through the
//! `mysql_clear_password` plugin the driver enables for these modes only.

use std::path::{Path, PathBuf};

/// The cloud whose CLI a sign-in asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cloud {
    Aws,
    Gcloud,
}

impl Cloud {
    /// The CLI's own name, as a message says it.
    pub fn cli(self) -> &'static str {
        match self {
            Cloud::Aws => "AWS CLI (aws)",
            Cloud::Gcloud => "Google Cloud CLI (gcloud)",
        }
    }

    /// How long a token is taken to be good for **after it was fetched** — the
    /// cache's horizon, short of the token's own life by a margin for the
    /// clock and the connect: AWS's live 15 minutes, gcloud's an hour.
    pub fn reuse_secs(self) -> u64 {
        match self {
            Cloud::Aws => 10 * 60,
            Cloud::Gcloud => 50 * 60,
        }
    }
}

/// Where the CLI may be, most likely first: every absolute `PATH` directory,
/// then on Windows the installers' own folders (`program_files` for AWS's,
/// `local_app_data` and `program_files` for the Cloud SDK's) — which catch a
/// CLI installed after the app started. A relative `PATH` entry is skipped,
/// as `entra::azure_cli_candidates` skips one.
pub fn cli_candidates(
    cloud: Cloud,
    path: Option<&std::ffi::OsStr>,
    program_files: &[&Path],
    local_app_data: Option<&Path>,
    windows: bool,
) -> Vec<PathBuf> {
    let names: &[&str] = match (cloud, windows) {
        (Cloud::Aws, true) => &["aws.exe", "aws.cmd"],
        (Cloud::Gcloud, true) => &["gcloud.cmd", "gcloud.exe"],
        (Cloud::Aws, false) => &["aws"],
        (Cloud::Gcloud, false) => &["gcloud"],
    };
    let mut out = Vec::new();
    if let Some(path) = path {
        for dir in std::env::split_paths(path) {
            if dir.as_os_str().is_empty() || dir.is_relative() {
                continue;
            }
            out.extend(names.iter().map(|n| dir.join(n)));
        }
    }
    if windows {
        match cloud {
            Cloud::Aws => {
                for pf in program_files {
                    out.push(pf.join("Amazon").join("AWSCLIV2").join("aws.exe"));
                }
            }
            Cloud::Gcloud => {
                let sdk = |base: &Path| {
                    base.join("Google")
                        .join("Cloud SDK")
                        .join("google-cloud-sdk")
                        .join("bin")
                        .join("gcloud.cmd")
                };
                out.extend(local_app_data.map(sdk));
                out.extend(program_files.iter().map(|pf| sdk(pf)));
            }
        }
    }
    out
}

/// The files whose metadata says **which sign-in the CLI holds** — what the
/// token cache keys on, as `entra`'s keys on the Azure CLI's profile: after
/// `aws configure`, `aws sso login`, `gcloud auth login` or `gcloud config set
/// account`, one of these is rewritten and a cached token, minted for the
/// previous identity, is not handed out again.
///
/// AWS's are `~/.aws/credentials` and `~/.aws/config`, or the files
/// `AWS_SHARED_CREDENTIALS_FILE` and `AWS_CONFIG_FILE` name. gcloud's live in
/// `CLOUDSDK_CONFIG`, else `%APPDATA%\gcloud` on Windows and
/// `~/.config/gcloud` elsewhere: the file naming the active configuration,
/// **that configuration's own file**, and the credential store.
///
/// The active configuration's, not `config_default`'s: `gcloud config set
/// account` on a named configuration rewrites `configurations/config_<name>`
/// and neither of the other two when the account already has credentials, so
/// a cache keyed on the default's would hand out the previous account's token.
/// Its name is `CLOUDSDK_ACTIVE_CONFIG_NAME`, else the content of
/// `active_config` (`read`), else `default` — and a name that is not one
/// gcloud would make (letters, digits, `-`, `_`) is not joined onto a path.
pub fn identity_files(
    cloud: Cloud,
    home: Option<&Path>,
    app_data: Option<&Path>,
    env: impl Fn(&str) -> Option<PathBuf>,
    read: impl Fn(&Path) -> Option<String>,
    windows: bool,
) -> Vec<PathBuf> {
    match cloud {
        Cloud::Aws => {
            let aws = home.map(|h| h.join(".aws"));
            [
                env("AWS_SHARED_CREDENTIALS_FILE")
                    .or_else(|| aws.as_ref().map(|a| a.join("credentials"))),
                env("AWS_CONFIG_FILE").or_else(|| aws.as_ref().map(|a| a.join("config"))),
            ]
            .into_iter()
            .flatten()
            .collect()
        }
        Cloud::Gcloud => {
            let dir = env("CLOUDSDK_CONFIG").or_else(|| {
                if windows {
                    app_data.map(|a| a.join("gcloud"))
                } else {
                    home.map(|h| h.join(".config").join("gcloud"))
                }
            });
            dir.map(|d| {
                let active = d.join("active_config");
                let name = env("CLOUDSDK_ACTIVE_CONFIG_NAME")
                    .map(|p| p.to_string_lossy().into_owned())
                    .or_else(|| read(&active))
                    .map(|n| n.trim().to_string())
                    .filter(|n| {
                        !n.is_empty()
                            && n.chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    })
                    .unwrap_or_else(|| "default".to_string());
                vec![
                    active,
                    d.join("configurations").join(format!("config_{name}")),
                    d.join("credentials.db"),
                ]
            })
            .unwrap_or_default()
        }
    }
}

/// The region of an RDS endpoint — `db.abc123.eu-west-1.rds.amazonaws.com`
/// is `eu-west-1` — or `None` for any other host (a custom DNS name, an IP),
/// where the CLI's configured default region applies.
pub fn rds_region(host: &str) -> Option<String> {
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    let rds = labels.iter().position(|l| l.eq_ignore_ascii_case("rds"))?;
    let after = labels.get(rds + 1)?;
    if rds == 0 || !after.eq_ignore_ascii_case("amazonaws") {
        return None;
    }
    let region = labels[rds - 1];
    let shaped = region.split('-').count() >= 3
        && region
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    shaped.then(|| region.to_string())
}

/// The arguments that ask the CLI for a token for `user` at `host:port` — each
/// connection-supplied value through `launch::cli_flag_value`, so none can be
/// read as an option.
pub fn token_args(cloud: Cloud, host: &str, port: u16, user: &str) -> Result<Vec<String>, String> {
    match cloud {
        Cloud::Aws => {
            let host = crate::launch::cli_flag_value("host", host)?;
            let user = crate::launch::cli_flag_value("user", user)?;
            let mut args: Vec<String> = [
                "rds",
                "generate-db-auth-token",
                "--hostname",
                host,
                "--port",
                &port.to_string(),
                "--username",
                user,
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            if let Some(region) = rds_region(host) {
                args.extend(["--region".to_string(), region]);
            }
            Ok(args)
        }
        // The token names the account, not the user or the instance: the
        // server reads the user from the connection, and gcloud is asked
        // nothing about either.
        Cloud::Gcloud => Ok(vec!["sql".into(), "generate-login-token".into()]),
    }
}

/// The program and argv to run, **with no shell in between**. `cli` is where
/// the CLI was found; `args` are [`token_args`].
///
/// A Windows `.cmd` shim is not run through `cmd.exe`. The Cloud SDK's
/// `gcloud.cmd` runs the Python the SDK bundles over `lib\gcloud.py`, so that
/// is what runs here — `<sdk>\platform\bundledpython\python.exe` beside the
/// shim's `bin` — and a shim without it is refused. The AWS CLI v2 is an
/// `.exe`; an `aws.cmd` is the old pip-installed v1, refused with the reason.
pub fn token_argv(
    cloud: Cloud,
    cli: &Path,
    args: Vec<String>,
    exists: impl Fn(&Path) -> bool,
) -> Result<(PathBuf, Vec<String>), &'static str> {
    let is_shim = cli
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if !is_shim {
        return Ok((cli.to_path_buf(), args));
    }
    match cloud {
        Cloud::Aws => Err(
            "The AWS CLI found is a .cmd shim (the pip-installed version 1), and Schemaic does not \
             run a shim through cmd.exe. Install the AWS CLI version 2, or sign in with a password.",
        ),
        Cloud::Gcloud => {
            const SHIM: &str = "The gcloud found is a .cmd shim with no bundled Python beside it, \
                and Schemaic does not run a shim through cmd.exe. Reinstall the Google Cloud CLI \
                with its installer, or sign in with a password.";
            let sdk = cli.parent().and_then(Path::parent).ok_or(SHIM)?;
            let python = sdk
                .join("platform")
                .join("bundledpython")
                .join("python.exe");
            let entry = sdk.join("lib").join("gcloud.py");
            if !exists(&python) || !exists(&entry) {
                return Err(SHIM);
            }
            let mut argv = vec![entry.to_string_lossy().into_owned()];
            argv.extend(args);
            Ok((python, argv))
        }
    }
}

/// The token in the CLI's output: its one non-empty line. More than one is
/// not a token — a prompt, a warning on stdout — and is refused rather than
/// sent to the server as a password.
pub fn parse_token(stdout: &str) -> Result<String, &'static str> {
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    match (lines.next(), lines.next()) {
        (Some(token), None) if !token.chars().any(char::is_whitespace) => Ok(token.to_string()),
        (None, _) => Err("The cloud CLI answered with no token."),
        _ => Err("The cloud CLI answered with more than a token."),
    }
}

/// The sentence for a CLI that exited unsuccessfully: a sign-in to do when it
/// says so, its own first line otherwise.
pub fn failure_text(cloud: Cloud, stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    let signed_out = match cloud {
        Cloud::Aws => {
            lower.contains("unable to locate credentials")
                || lower.contains("aws sso login")
                || lower.contains("token has expired")
        }
        Cloud::Gcloud => {
            lower.contains("gcloud auth login")
                || lower.contains("you do not currently have an active account")
                || lower.contains("reauthentication")
        }
    };
    if signed_out {
        return match cloud {
            Cloud::Aws => {
                "The AWS CLI has no credentials. Run `aws configure` or `aws sso login`, \
                           then connect again."
                    .to_string()
            }
            Cloud::Gcloud => {
                "The Google Cloud CLI is not signed in. Run `gcloud auth login`, then connect \
                 again."
                    .to_string()
            }
        };
    }
    let first = stderr.lines().map(str::trim).find(|l| !l.is_empty());
    match first {
        Some(line) => format!("The {} could not get a token: {line}", cloud.cli()),
        None => format!("The {} could not get a token.", cloud.cli()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_rds_endpoint_names_its_region() {
        assert_eq!(
            rds_region("db.abc123xyz.eu-west-1.rds.amazonaws.com").as_deref(),
            Some("eu-west-1")
        );
        assert_eq!(
            rds_region("c.cluster-ro-abc.us-gov-west-1.rds.amazonaws.com").as_deref(),
            Some("us-gov-west-1")
        );
        assert_eq!(
            rds_region("db.x.cn-north-1.rds.amazonaws.com.cn.").as_deref(),
            Some("cn-north-1")
        );
    }

    #[test]
    fn any_other_host_leaves_the_region_to_the_cli() {
        for host in [
            "db.example.com",
            "10.0.0.5",
            "rds.amazonaws.com",
            "x.rds.example.com",
            "",
        ] {
            assert_eq!(rds_region(host), None, "{host}");
        }
    }

    #[test]
    fn the_aws_ask_names_the_host_port_user_and_region() {
        assert_eq!(
            token_args(Cloud::Aws, "db.a.us-east-2.rds.amazonaws.com", 5432, "app").unwrap(),
            [
                "rds",
                "generate-db-auth-token",
                "--hostname",
                "db.a.us-east-2.rds.amazonaws.com",
                "--port",
                "5432",
                "--username",
                "app",
                "--region",
                "us-east-2"
            ]
        );
        let custom = token_args(Cloud::Aws, "db.internal", 3306, "app").unwrap();
        assert!(!custom.contains(&"--region".to_string()));
    }

    /// A value the CLI would read as an option is refused before anything
    /// runs: a user called `--profile=prod` is not a user.
    #[test]
    fn a_value_shaped_like_an_option_is_refused() {
        assert!(token_args(Cloud::Aws, "db", 5432, "--profile=prod").is_err());
        assert!(token_args(Cloud::Aws, "-h", 5432, "app").is_err());
        assert!(token_args(Cloud::Aws, "db", 5432, "").is_err());
        assert!(token_args(Cloud::Aws, "db\nx", 5432, "app").is_err());
    }

    #[test]
    fn the_gcloud_ask_is_constant() {
        assert_eq!(
            token_args(Cloud::Gcloud, "--anything", 1, "").unwrap(),
            ["sql", "generate-login-token"]
        );
    }

    #[test]
    fn a_gcloud_shim_runs_the_sdks_own_python_and_nothing_else() {
        let sdk = Path::new(r"C:\sdk");
        let shim = sdk.join("bin").join("gcloud.cmd");
        let python = sdk
            .join("platform")
            .join("bundledpython")
            .join("python.exe");
        let entry = sdk.join("lib").join("gcloud.py");
        let args = vec!["sql".to_string(), "generate-login-token".to_string()];
        let (prog, argv) = token_argv(Cloud::Gcloud, &shim, args.clone(), |p| {
            p == python || p == entry
        })
        .unwrap();
        assert_eq!(prog, python);
        assert_eq!(argv[0], entry.to_string_lossy());
        assert_eq!(&argv[1..], &args[..]);
        assert!(token_argv(Cloud::Gcloud, &shim, args, |_| false).is_err());
    }

    #[test]
    fn an_aws_v1_shim_is_refused_and_an_exe_runs_as_found() {
        let args = vec!["rds".to_string()];
        assert!(
            token_argv(Cloud::Aws, Path::new(r"C:\x\aws.cmd"), args.clone(), |_| {
                true
            })
            .is_err()
        );
        let exe = Path::new(r"C:\Program Files\Amazon\AWSCLIV2\aws.exe");
        assert_eq!(
            token_argv(Cloud::Aws, exe, args.clone(), |_| true).unwrap(),
            (exe.to_path_buf(), args)
        );
    }

    #[test]
    fn the_candidates_cover_path_and_the_installers() {
        let path = std::env::join_paths([r"C:\bin", ""]).unwrap();
        let pf = Path::new(r"C:\Program Files");
        let got = cli_candidates(Cloud::Aws, Some(&path), &[pf], None, true);
        assert!(got.contains(&Path::new(r"C:\bin").join("aws.exe")));
        assert!(got.contains(&pf.join("Amazon").join("AWSCLIV2").join("aws.exe")));
        let lad = Path::new(r"C:\Users\u\AppData\Local");
        let got = cli_candidates(Cloud::Gcloud, None, &[], Some(lad), true);
        assert_eq!(
            got,
            [lad.join("Google")
                .join("Cloud SDK")
                .join("google-cloud-sdk")
                .join("bin")
                .join("gcloud.cmd")]
        );
        // Absolute on whichever OS runs this — `/usr/bin` is relative on Windows.
        let abs = std::env::temp_dir();
        assert_eq!(
            cli_candidates(Cloud::Gcloud, Some(abs.as_os_str()), &[], None, false),
            [abs.join("gcloud")]
        );
    }

    #[test]
    fn the_identity_is_read_off_each_clis_own_files() {
        let home = Path::new("/home/u");
        let none = |_: &str| None;
        let unread = |_: &Path| None;
        assert_eq!(
            identity_files(Cloud::Aws, Some(home), None, none, unread, false),
            [
                home.join(".aws").join("credentials"),
                home.join(".aws").join("config")
            ]
        );
        let moved = |k: &str| (k == "AWS_CONFIG_FILE").then(|| PathBuf::from("/etc/aws.cfg"));
        assert_eq!(
            identity_files(Cloud::Aws, Some(home), None, moved, unread, false)[1],
            PathBuf::from("/etc/aws.cfg")
        );
        let g = home.join(".config").join("gcloud");
        assert_eq!(
            identity_files(Cloud::Gcloud, Some(home), None, none, unread, false),
            [
                g.join("active_config"),
                g.join("configurations").join("config_default"),
                g.join("credentials.db")
            ]
        );
        let app = Path::new(r"C:\Users\u\AppData\Roaming");
        assert_eq!(
            identity_files(Cloud::Gcloud, None, Some(app), none, unread, true)[0],
            app.join("gcloud").join("active_config")
        );
        assert!(identity_files(Cloud::Gcloud, None, None, none, unread, false).is_empty());
    }

    /// **The active configuration's own file**, named by `active_config` or
    /// the environment: `gcloud config set account` on a named configuration
    /// rewrites that one alone. A name gcloud would not make is not joined
    /// onto a path.
    #[test]
    fn gcloud_is_keyed_on_the_active_configuration() {
        let home = Path::new("/home/u");
        let g = home.join(".config").join("gcloud");
        let none = |_: &str| None;
        let work = |p: &Path| p.ends_with("active_config").then(|| "work\n".to_string());
        assert_eq!(
            identity_files(Cloud::Gcloud, Some(home), None, none, work, false)[1],
            g.join("configurations").join("config_work")
        );
        let env = |k: &str| (k == "CLOUDSDK_ACTIVE_CONFIG_NAME").then(|| PathBuf::from("prod"));
        assert_eq!(
            identity_files(Cloud::Gcloud, Some(home), None, env, work, false)[1],
            g.join("configurations").join("config_prod")
        );
        let hostile = |_: &Path| Some("../../x".to_string());
        assert_eq!(
            identity_files(Cloud::Gcloud, Some(home), None, none, hostile, false)[1],
            g.join("configurations").join("config_default")
        );
    }

    #[test]
    fn a_token_is_one_line_and_nothing_else() {
        assert_eq!(parse_token("abc.def?X-Amz=1\n").unwrap(), "abc.def?X-Amz=1");
        assert_eq!(parse_token("\n  ya29.tok  \n\n").unwrap(), "ya29.tok");
        assert!(parse_token("").is_err());
        assert!(parse_token("WARNING: x\ntoken").is_err());
        assert!(parse_token("two words").is_err());
    }

    #[test]
    fn a_signed_out_cli_says_how_to_sign_in() {
        assert!(
            failure_text(
                Cloud::Aws,
                "Unable to locate credentials. You can configure ..."
            )
            .contains("aws configure")
        );
        assert!(
            failure_text(
                Cloud::Gcloud,
                "ERROR: (gcloud.sql.generate-login-token) You do not currently have an active account selected."
            )
            .contains("gcloud auth login")
        );
        assert_eq!(
            failure_text(Cloud::Aws, "\nboom\nmore"),
            "The AWS CLI (aws) could not get a token: boom"
        );
        assert_eq!(
            failure_text(Cloud::Gcloud, ""),
            "The Google Cloud CLI (gcloud) could not get a token."
        );
    }
}
