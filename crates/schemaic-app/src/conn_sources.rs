//! Finding, on this machine, the files [`schemaic_core::conn_import`] knows how
//! to read — and reading them.
//!
//! The whole of the I/O half of connection import, and deliberately *only* the
//! I/O half: this module decides which paths are worth opening and hands the
//! bytes over; nothing here interprets them. That is what keeps the parsing
//! unit-tested in `schemaic-core` while the part that cannot be — a walk of the
//! user's home directory — stays small enough to read.
//!
//! **Every root is probed on every platform.** There are no `cfg` branches here
//! even though the locations are per-OS, because the cost of probing an absent
//! `~/Library` on Linux is one failed `read_dir` and the cost of a `cfg` is a
//! path that only one third of the developers can ever exercise. A root that
//! isn't there simply yields nothing.
//!
//! Nothing is written, and nothing outside a known layout is opened: the walks
//! below are bounded to a fixed depth under a named directory, so a symlink farm
//! or a large workspace cannot turn "open the import modal" into a filesystem
//! scan.

use std::path::{Path, PathBuf};

use schemaic_core::conn_import::{ImportSource, SourceFile};

/// Largest source file worth reading.
///
/// A `data-sources.json` for a hundred connections is tens of kilobytes; a file
/// this size under one of these names is not a connection list, and reading it
/// into a string would be the only unbounded allocation in the feature.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Every connection source found on this machine, in the order the review list
/// should show them.
///
/// The order is not arbitrary and is relied upon by `conn_import::scan`, which
/// keeps the **first** description of any given server: the two GUI clients come
/// first because they are the only sources that carry a name a human chose, and
/// the plain-text files come last because their value is the passwords they can
/// lend to the rows above them.
pub fn discover() -> Vec<SourceFile> {
    let mut paths: Vec<(ImportSource, PathBuf)> = Vec::new();
    paths.extend(dbeaver_files());
    paths.extend(jetbrains_files());
    paths.extend(cli_files());

    let mut out = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for (source, path) in paths {
        // One file can be reached through two roots (`$XDG_CONFIG_HOME` set to
        // `~/.config` is the common one). Compare resolved paths, since that is
        // what makes the two spellings one file.
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if let Some(f) = read_source(&path, source) {
            out.push(f);
        }
    }
    out
}

/// Just the files that can supply a **password** for somebody else's row —
/// today, `~/.pgpass`.
///
/// A file the user picks by hand goes through `conn_import::scan` on its own, so
/// without this it would be the one path where a DataGrip export arrives with
/// twelve blank passwords that libpq's file, on the same machine, has every one
/// of. Whether a row's password can be completed must not depend on how its file
/// was found.
///
/// Deliberately not the whole of [`discover`]: this is four `is_file` checks and
/// one small read, cheap enough to run inside a file-picker callback, where the
/// two directory walks would not be.
pub fn password_sources() -> Vec<SourceFile> {
    cli_files()
        .into_iter()
        .filter(|(source, _)| *source == ImportSource::Pgpass)
        .filter_map(|(source, path)| read_source(&path, source))
        .collect()
}

/// Read one file as a source, or `None` if it isn't there, is too big, or isn't
/// text.
///
/// A file that fails to read is not reported: this runs over paths the user
/// never named, and "your `~/.pgpass` is unreadable" is not news about a button
/// they pressed. A file they *did* name goes through the same function, and the
/// modal says so when nothing comes back.
pub fn read_source(path: &Path, source: ImportSource) -> Option<SourceFile> {
    open_source(path, source).ok()
}

/// Why a source file could not be read — **one cause per message**, for the
/// path where the user named the file.
///
/// [`read_source`] answers `None` for all of these because it runs over files
/// nobody asked about. *Choose a file…* is the other caller, and it reported
/// one sentence — `"<path> could not be read."` — for three different problems,
/// two of which are actionable and one of which was not even true: a non-UTF-8
/// file *can* be read, and now is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceError {
    /// There is nothing at that path, or it is a directory.
    NotAFile,
    /// Larger than [`MAX_BYTES`].
    TooBig(u64),
    /// The read itself failed — a permission, a vanished network mount.
    Unreadable(String),
}

impl SourceError {
    /// The sentence for the modal, naming the file and what to do about it.
    pub fn message(&self, path: &Path) -> String {
        let p = path.display();
        match self {
            SourceError::NotAFile => format!("{p} is not a file."),
            SourceError::TooBig(bytes) => format!(
                "{p} is {}, past the {} this reads.",
                schemaic_core::stats::format_bytes(*bytes),
                schemaic_core::stats::format_bytes(MAX_BYTES),
            ),
            SourceError::Unreadable(why) => format!("{p} could not be read: {why}"),
        }
    }
}

/// [`read_source`] with the failure kept. See [`SourceError`].
///
/// **Bytes, then decoded** — never `read_to_string`. That answers
/// `Err(InvalidData)` for a cp1252 `~/.my.cnf` or a UTF-16LE `my.ini`, and the
/// scan turned that into `None`, so the file vanished from the list with no note
/// anywhere while the modal said the files are "in known places". At its worst
/// the vanished file is `~/.pgpass`. `text::decode_text_file` reads all three.
pub fn open_source(path: &Path, source: ImportSource) -> Result<SourceFile, SourceError> {
    let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => SourceError::NotAFile,
        _ => SourceError::Unreadable(e.to_string()),
    })?;
    if !meta.is_file() {
        return Err(SourceError::NotAFile);
    }
    if meta.len() > MAX_BYTES {
        return Err(SourceError::TooBig(meta.len()));
    }
    let bytes = std::fs::read(path).map_err(|e| SourceError::Unreadable(e.to_string()))?;
    Ok(SourceFile {
        source,
        path: path.to_string_lossy().into_owned(),
        text: schemaic_core::text::decode_text_file(&bytes),
    })
}

/// Which parser a file the user picked by hand should go through.
///
/// Keyed on the name the tool gives the file rather than on its extension,
/// because the extension is the ambiguous half: `.json` and `.xml` say nothing,
/// while `data-sources.json` and `dataSources.xml` are each written by exactly
/// one program. A name nothing recognises is read as a list of URLs, which is
/// also what a `.env` or a scratch file of connection strings is.
pub fn source_for_path(path: &Path) -> ImportSource {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "data-sources.json" => ImportSource::DBeaver,
        "datasources.xml" | "datasources.local.xml" => ImportSource::DataGrip,
        ".my.cnf" | "my.cnf" | "my.ini" => ImportSource::MyCnf,
        ".pgpass" | "pgpass.conf" => ImportSource::Pgpass,
        ".pg_service.conf" | "pg_service.conf" => ImportSource::PgService,
        _ => ImportSource::Url,
    }
}

// ---------------------------------------------------------------------------
// Per-tool layouts
// ---------------------------------------------------------------------------

/// `…/DBeaverData/workspace<N>/<project>/.dbeaver/data-sources.json`.
///
/// Two levels of wildcard, and both are real: the workspace directory carries a
/// version number DBeaver bumps between major releases, and a user may have any
/// number of projects inside it — each with its own connection list. Taking only
/// `General` would silently miss every connection of anyone who made a second
/// project.
fn dbeaver_files() -> Vec<(ImportSource, PathBuf)> {
    let mut out = Vec::new();
    for root in data_roots() {
        let base = root.join("DBeaverData");
        for workspace in children(&base) {
            if !file_name(&workspace).starts_with("workspace") {
                continue;
            }
            for project in children(&workspace) {
                let path = project.join(".dbeaver").join("data-sources.json");
                if path.is_file() {
                    out.push((ImportSource::DBeaver, path));
                }
            }
        }
    }
    out
}

/// `…/JetBrains/<Product><Version>/options/dataSources.xml`.
///
/// Every JetBrains IDE with the database plugin writes this file under its own
/// product directory, so the walk does not filter by product name: DataGrip is
/// the one this is named for, but a user's connections are just as likely to
/// live under IntelliJ or PhpStorm, and the file's shape is identical.
///
/// Per-project `.idea/dataSources.xml` files are **not** searched. They are
/// scattered wherever the user keeps code, finding them would mean walking the
/// home directory, and the modal's "Choose a file…" opens one directly.
fn jetbrains_files() -> Vec<(ImportSource, PathBuf)> {
    let mut out = Vec::new();
    for root in config_roots() {
        for product in children(&root.join("JetBrains")) {
            let path = product.join("options").join("dataSources.xml");
            if path.is_file() {
                out.push((ImportSource::DataGrip, path));
            }
        }
    }
    out
}

/// The three command-line clients' files, at the paths their own documentation
/// names — including the environment variables libpq lets a user move them to.
fn cli_files() -> Vec<(ImportSource, PathBuf)> {
    let mut out: Vec<(ImportSource, PathBuf)> = Vec::new();
    let mut add = |source, path: PathBuf| {
        if path.is_file() {
            out.push((source, path));
        }
    };
    if let Some(home) = home() {
        add(ImportSource::MyCnf, home.join(".my.cnf"));
        // The Windows spelling of the same file.
        add(ImportSource::MyCnf, home.join("my.ini"));
        add(ImportSource::Pgpass, home.join(".pgpass"));
        add(ImportSource::PgService, home.join(".pg_service.conf"));
    }
    if let Some(appdata) = env_path("APPDATA") {
        add(ImportSource::Pgpass, appdata.join("postgresql/pgpass.conf"));
        add(
            ImportSource::PgService,
            appdata.join("postgresql/.pg_service.conf"),
        );
    }
    // libpq reads these first, so a user who set them keeps their file
    // somewhere this walk would never have looked.
    if let Some(p) = env_path("PGPASSFILE") {
        add(ImportSource::Pgpass, p);
    }
    if let Some(p) = env_path("PGSERVICEFILE") {
        add(ImportSource::PgService, p);
    }
    out
}

// ---------------------------------------------------------------------------
// Roots
// ---------------------------------------------------------------------------

/// Where per-user *configuration* lives, on each platform.
fn config_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    out.extend(env_path("APPDATA"));
    out.extend(env_path("XDG_CONFIG_HOME"));
    if let Some(home) = home() {
        out.push(home.join(".config"));
        out.push(home.join("Library/Application Support"));
    }
    out
}

/// Where per-user *data* lives — DBeaver's workspace is data, not config.
fn data_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    out.extend(env_path("APPDATA"));
    out.extend(env_path("XDG_DATA_HOME"));
    if let Some(home) = home() {
        out.push(home.join(".local/share"));
        out.push(home.join("Library"));
        // DBeaver's installer has also put `DBeaverData` straight in the home
        // directory on Linux, depending on how it was packaged.
        out.push(home);
    }
    out
}

fn home() -> Option<PathBuf> {
    env_path("USERPROFILE").or_else(|| env_path("HOME"))
}

fn env_path(key: &str) -> Option<PathBuf> {
    let v = std::env::var_os(key)?;
    (!v.is_empty()).then(|| PathBuf::from(v))
}

/// The immediate sub-directories of `dir`, or nothing if it isn't one.
fn children(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tools_own_file_name_picks_its_parser() {
        let cases = [
            ("/x/.dbeaver/data-sources.json", ImportSource::DBeaver),
            ("/x/options/dataSources.xml", ImportSource::DataGrip),
            ("/x/.idea/dataSources.local.xml", ImportSource::DataGrip),
            ("/home/me/.my.cnf", ImportSource::MyCnf),
            ("C:/Users/me/my.ini", ImportSource::MyCnf),
            ("/home/me/.pgpass", ImportSource::Pgpass),
            (
                "C:/Users/me/AppData/postgresql/pgpass.conf",
                ImportSource::Pgpass,
            ),
            ("/home/me/.pg_service.conf", ImportSource::PgService),
        ];
        for (path, want) in cases {
            assert_eq!(source_for_path(Path::new(path)), want, "{path}");
        }
    }

    #[test]
    fn the_file_name_decides_regardless_of_how_it_is_cased() {
        // JetBrains writes `dataSources.xml`; a user's shell completion, an
        // archive, or a case-insensitive copy may hand back any casing.
        assert_eq!(
            source_for_path(Path::new("/x/DATASOURCES.XML")),
            ImportSource::DataGrip
        );
    }

    #[test]
    fn an_unrecognised_file_is_read_as_a_list_of_urls() {
        // A `.env`, or a scratch file of connection strings — the one shape a
        // file nobody's tool wrote is likely to have.
        assert_eq!(source_for_path(Path::new("/x/.env")), ImportSource::Url);
        assert_eq!(
            source_for_path(Path::new("/x/connections.txt")),
            ImportSource::Url
        );
        assert_eq!(source_for_path(Path::new("/x")), ImportSource::Url);
    }

    #[test]
    fn a_path_that_is_not_a_file_reads_as_nothing() {
        assert!(read_source(Path::new("/definitely/not/here"), ImportSource::Url).is_none());
        // A directory is not a source, even though it exists.
        let dir = std::env::temp_dir();
        assert!(read_source(&dir, ImportSource::Url).is_none());
    }

    #[test]
    fn discovery_never_fails_however_empty_the_machine_is() {
        // The one thing this walk must not do is panic on a home directory that
        // doesn't have any of these tools in it — which is most of them.
        let _ = discover();
    }
}
