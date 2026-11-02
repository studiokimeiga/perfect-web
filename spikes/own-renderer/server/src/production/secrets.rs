//! **A deployment's secrets** (track `deploy`): the values the host holds and
//! nobody reads back, from its environment or from its secrets file.
//!
//! - **The names are the host's.** [`NAMES`] lists every secret it reads; a
//!   secrets file naming anything else is refused, so a misspelt name is not
//!   a secret silently missing.
//! - **One source each.** A secret given both in the environment and in the
//!   file, or twice in the file, is refused: which one the host used would
//!   otherwise be a guess.
//! - **The file is its owner's alone.** `PW_SECRETS_FILE` names a file of
//!   `NAME=value` lines; one that its group or anyone else may read is
//!   refused, as OpenSSH refuses a private key others may read. systemd's
//!   `LoadCredential=` (the single-VM recipe) hands the service such a file,
//!   mode 0400 and its own; a container mounts one.
//! - **A value is never said.** [`Secret`] has no `Display`, its `Debug` says
//!   only that it is one, and no refusal quotes a line of the file, which
//!   may hold a value.

use std::collections::BTreeMap;
use std::fmt;

use super::Env;

/// **The secrets this host reads**, by name: the deployment's identity key,
/// the databases' addresses (which may carry a password), and the token
/// `/metrics` is read with.
pub(crate) const NAMES: &[&str] = &[
    "PW_IDENTITY_KEY",
    "PW_STORE_DATABASE_URL",
    "PW_FEED_DATABASE_URL",
    "PW_METRICS_TOKEN",
];

/// **A value the deployment gave the host and nobody reads back.** No
/// `Display`, a `Debug` that hides it, and one way to read it,
/// [`Secret::expose`], named so that every place it is used is found by
/// searching for that name.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Secret(String);

impl Secret {
    #[cfg(test)]
    pub(crate) fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

    /// The value, where the host uses it: to connect, or to derive a key.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// **The deployment's secrets**, each by its name in [`NAMES`].
#[derive(Debug, Default)]
pub(crate) struct Secrets(BTreeMap<&'static str, Secret>);

impl Secrets {
    /// **Every secret the deployment states**: from the environment, and
    /// from the file `PW_SECRETS_FILE` names where it names one. A name set
    /// empty in the environment is no value.
    pub(crate) fn read(env: Env) -> Result<Secrets, String> {
        let mut found = BTreeMap::new();
        for name in NAMES {
            if let Some(value) = env(name).filter(|v| !v.is_empty()) {
                found.insert(*name, Secret(value));
            }
        }
        if let Some(path) = env("PW_SECRETS_FILE").filter(|p| !p.is_empty()) {
            let text = read_private(&path)?;
            for (at, line) in text.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let at = at + 1;
                // The line is never quoted: it may be a value with its `=`
                // lost.
                let Some((name, value)) = line.split_once('=') else {
                    return Err(format!(
                        "{path}, line {at}: not `NAME=value` (the line is not shown: it may hold a \
                         secret)"
                    ));
                };
                let name = name.trim();
                // Not quoted either: a line whose `=` came late makes part
                // of a value its name.
                let Some(known) = NAMES.iter().find(|n| **n == name) else {
                    return Err(format!(
                        "{path}, line {at}: names no secret this host reads (it reads {}; the \
                         line is not shown: it may hold a secret)",
                        NAMES.join(", ")
                    ));
                };
                let value = value.trim();
                if value.is_empty() {
                    return Err(format!("{path}, line {at}: {known} is empty"));
                }
                if found.contains_key(known) {
                    return Err(format!(
                        "{known} is given twice (in the environment and in {path}, or twice in \
                         {path}): which one holds would be a guess"
                    ));
                }
                found.insert(*known, Secret(value.to_string()));
            }
        }
        Ok(Secrets(found))
    }

    /// The secret named `name`, where the deployment states it.
    pub(crate) fn get(&self, name: &str) -> Option<&Secret> {
        self.0.get(name)
    }

    /// The names the deployment states, for what the host says at start.
    pub(crate) fn names(&self) -> Vec<&'static str> {
        self.0.keys().copied().collect()
    }

    #[cfg(test)]
    pub(crate) fn of(pairs: &[(&'static str, &str)]) -> Secrets {
        Secrets(
            pairs
                .iter()
                .map(|(n, v)| (*n, Secret(v.to_string())))
                .collect(),
        )
    }
}

/// **The file's text, where only its owner may read it.** Its mode is read
/// before its text, so a file others may read is refused unread.
#[cfg(unix)]
fn read_private(path: &str) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| format!("PW_SECRETS_FILE {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("PW_SECRETS_FILE {path} is not a file"));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(format!(
            "PW_SECRETS_FILE {path} may be read by others than its owner (mode {mode:03o}): a \
             secrets file is its owner's alone (chmod 600)"
        ));
    }
    std::fs::read_to_string(path).map_err(|e| format!("PW_SECRETS_FILE {path}: {e}"))
}

/// Elsewhere no mode says who may read a file, and no secrets file is read.
#[cfg(not(unix))]
fn read_private(path: &str) -> Result<String, String> {
    Err(format!(
        "PW_SECRETS_FILE {path}: a secrets file is read only where a file's mode says who may \
         read it"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string())
        }
    }

    /// A secrets file, `mode`, holding `text`, in a directory removed after.
    fn file(text: &str, mode: u32) -> (tempfile::TempDir, String) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::with_prefix("pw-secrets-").expect("a directory");
        let path = dir.path().join("secrets");
        std::fs::write(&path, text).expect("written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("mode");
        (dir, path.display().to_string())
    }

    fn read_with(path: &str, extra: &[(&'static str, &'static str)]) -> Result<Secrets, String> {
        let path = path.to_string();
        let extra: Vec<(&'static str, &'static str)> = extra.to_vec();
        Secrets::read(&move |name: &str| {
            if name == "PW_SECRETS_FILE" {
                return Some(path.clone());
            }
            extra
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string())
        })
    }

    /// **A secret is read from the environment**, and one set empty is
    /// none; a name the host does not read is not taken.
    #[test]
    fn a_secret_is_read_from_the_environment() {
        let env = env_of(&[
            ("PW_IDENTITY_KEY", KEY),
            ("PW_STORE_DATABASE_URL", ""),
            ("PW_OTHER", "x"),
        ]);
        let secrets = Secrets::read(&env).expect("read");
        assert_eq!(
            secrets.get("PW_IDENTITY_KEY").map(Secret::expose),
            Some(KEY)
        );
        assert!(secrets.get("PW_STORE_DATABASE_URL").is_none());
        assert_eq!(secrets.names(), vec!["PW_IDENTITY_KEY"]);
    }

    /// **A secret is read from the deployment's file**: `NAME=value` lines,
    /// a value holding `=` whole, comments and blank lines passed over.
    #[test]
    fn a_secret_is_read_from_the_deployments_file() {
        let (_dir, path) = file(
            &format!(
                "# the deployment's\n\nPW_IDENTITY_KEY = {KEY}\n\
                 PW_STORE_DATABASE_URL=postgresql://pw:pa=ss@127.0.0.1/pw?sslmode=disable\n"
            ),
            0o600,
        );
        let secrets = read_with(&path, &[]).expect("read");
        assert_eq!(
            secrets.get("PW_IDENTITY_KEY").map(Secret::expose),
            Some(KEY)
        );
        assert_eq!(
            secrets.get("PW_STORE_DATABASE_URL").map(Secret::expose),
            Some("postgresql://pw:pa=ss@127.0.0.1/pw?sslmode=disable")
        );
        // Its owner's to read and nobody else's, as systemd's credentials are.
        let (_dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), 0o400);
        assert!(read_with(&path, &[]).is_ok());
    }

    /// **A file others may read is refused, unread**: by its group, or by
    /// anyone.
    #[test]
    fn a_secrets_file_others_may_read_is_refused() {
        for mode in [0o640, 0o604, 0o644, 0o660] {
            let (_dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), mode);
            let refused = read_with(&path, &[]).expect_err("refused");
            assert!(refused.contains("chmod 600"), "{mode:o}: {refused}");
            assert!(!refused.contains(KEY), "{refused}");
        }
    }

    /// **A secret given twice is refused**: in the environment and in the
    /// file, or twice in the file.
    #[test]
    fn a_secret_given_twice_is_refused() {
        let (_dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), 0o600);
        let refused = read_with(&path, &[("PW_IDENTITY_KEY", KEY)]).expect_err("refused");
        assert!(refused.contains("given twice"), "{refused}");
        let (_dir, path) = file(
            &format!("PW_IDENTITY_KEY={KEY}\nPW_IDENTITY_KEY={KEY}\n"),
            0o600,
        );
        let refused = read_with(&path, &[]).expect_err("refused");
        assert!(refused.contains("given twice"), "{refused}");
        assert!(!refused.contains(KEY), "{refused}");
    }

    /// **A line that is no secret's is refused, and never quoted**: a name
    /// the host does not read, a line with no `=`, an empty value.
    #[test]
    fn a_line_that_is_no_secret_is_refused_and_never_quoted() {
        let (_dir, path) = file(&format!("PW_IDENTITY_KEY {KEY}\n"), 0o600);
        let refused = read_with(&path, &[]).expect_err("refused");
        assert!(refused.contains("line 1: not `NAME=value`"), "{refused}");
        assert!(!refused.contains(KEY), "{refused}");

        let (_dir, path) = file("PW_IDENTITY_KEYS=s3cr3t-value\n", 0o600);
        let refused = read_with(&path, &[]).expect_err("refused");
        assert!(refused.contains("line 1: names no secret"), "{refused}");
        assert!(!refused.contains("s3cr3t"), "{refused}");

        // A value whose `=` came first is no name, and is not quoted as one.
        let (_dir, path) = file("S3CR3T=PW_IDENTITY_KEY\n", 0o600);
        let refused = read_with(&path, &[]).expect_err("refused");
        assert!(!refused.contains("S3CR3T"), "{refused}");

        let (_dir, path) = file("PW_METRICS_TOKEN=\n", 0o600);
        let refused = read_with(&path, &[]).expect_err("refused");
        assert!(refused.contains("PW_METRICS_TOKEN is empty"), "{refused}");
    }

    /// **A secret is never shown by `Debug`**, alone or among the others.
    #[test]
    fn a_secret_is_never_shown() {
        let secrets = Secrets::of(&[("PW_IDENTITY_KEY", KEY), ("PW_METRICS_TOKEN", "t0k3n")]);
        let shown = format!("{secrets:?} {:?}", Secret::new(KEY));
        assert!(!shown.contains(KEY) && !shown.contains("t0k3n"), "{shown}");
        assert!(shown.contains("Secret(..)"), "{shown}");
    }
}
