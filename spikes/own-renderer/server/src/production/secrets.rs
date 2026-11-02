//! **A deployment's secrets** (track `deploy`): the values the host holds and
//! nobody reads back, from its environment or from its secrets file.
//!
//! - **The names are the host's.** [`NAMES`] lists every secret it reads; a
//!   secrets file naming anything else is refused, so a misspelt name is not
//!   a secret silently missing.
//! - **One source each.** A secret given both in the environment and in the
//!   file, or twice in the file, is refused: which one the host used would
//!   otherwise be a guess.
//! - **The file is this process's, and no one else's** (the review's finding
//!   4). `PW_SECRETS_FILE` names a file of `NAME=value` lines, opened once,
//!   without following a link (`O_NOFOLLOW`); every check is made of what
//!   was opened (`fstat` of its descriptor), and the text is read from that
//!   same descriptor, so a file swapped between a check and a read is never
//!   the one read. It is a regular file; its owner is this process's user or
//!   root; no one else may read or write it; its group may not write it, and
//!   may read it only where the group is this process's own. An ACL is read
//!   as what it grants: a named user may read it only where it is this
//!   process's user, a named group only where it is this process's group,
//!   and none may write. OpenSSH refuses a private key others may read; this
//!   is the same rule, read where a container or a platform puts a secret:
//!   - systemd's `LoadCredential=` (the single-VM recipe) hands the service
//!     a copy of mode 0400 that its user reads through an ACL entry, which
//!     `stat` shows as the group's bits (0440), or, where its file system
//!     keeps no ACL, a copy it owns (`src/core/exec-credential.c`);
//!   - Kubernetes's secret volume with `fsGroup` gives its files mode
//!     `defaultMode | 0440` and that group (`roMask` in the kubelet's
//!     `pkg/volume/volume_linux.go`): the process's own group where
//!     `runAsGroup` and `fsGroup` are the image's (65532), and a link the
//!     host does not follow unless mounted by `subPath`;
//!   - Podman's `--secret …,uid=65532,gid=65532,mode=0400`, Compose's
//!     secret long syntax from an `environment` source (a `file` source is
//!     bind-mounted as it is, its `uid` and `mode` ignored), or a file
//!     bind-mounted that is owned by 65532 with mode 0400.
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
    /// from the deployment's secrets file ([`secrets_file`]). A name set
    /// empty in the environment is no value.
    pub(crate) fn read(env: Env) -> Result<Secrets, String> {
        let mut found = BTreeMap::new();
        for name in NAMES {
            if let Some(value) = env(name).filter(|v| !v.is_empty()) {
                found.insert(*name, Secret(value));
            }
        }
        if let Some(path) = secrets_file(env) {
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

/// **The deployment's secrets file**: the one `PW_SECRETS_FILE` names; or,
/// where it names none, systemd's credential named `secrets`
/// (`LoadCredential=secrets:…`), which systemd hands a service in the
/// directory `$CREDENTIALS_DIRECTORY` names (systemd.exec(5),
/// "Credentials"). Read from there, the unit needs no specifier
/// (`%d`), which systemd 249 does not resolve in `Environment=`.
pub(crate) fn secrets_file(env: Env) -> Option<String> {
    if let Some(path) = env("PW_SECRETS_FILE").filter(|p| !p.is_empty()) {
        return Some(path);
    }
    let directory = env("CREDENTIALS_DIRECTORY").filter(|d| !d.is_empty())?;
    let credential = std::path::Path::new(&directory).join("secrets");
    credential
        .exists()
        .then(|| credential.display().to_string())
}

/// **The file's text, where it is this process's and no one else's** (the
/// module's rule): opened once without following a link, held to the rule
/// by what the opened descriptor says of itself, and read from it. A file
/// that breaks the rule is refused unread.
#[cfg(unix)]
fn read_private(path: &str) -> Result<String, String> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        // Not a link's target, and no wait for a writer where it is a pipe.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ELOOP) => format!(
                "PW_SECRETS_FILE {path} is a symbolic link, which the host does not follow \
                 (O_NOFOLLOW): name the file itself (in Kubernetes, mount it by `subPath`)"
            ),
            Some(libc::EACCES) => format!(
                "PW_SECRETS_FILE {path} may not be read by this process (uid {}): a secrets \
                 file is its user's (chown it, chmod 400 or 600), or root's and readable by \
                 its group alone where that group is this process's",
                // SAFETY: geteuid has no preconditions and cannot fail.
                unsafe { libc::geteuid() }
            ),
            _ => format!("PW_SECRETS_FILE {path}: {e}"),
        })?;
    let meta = file
        .metadata()
        .map_err(|e| format!("PW_SECRETS_FILE {path}: {e}"))?;
    if !meta.file_type().is_file() {
        return Err(format!("PW_SECRETS_FILE {path} is not a regular file"));
    }
    let acl = acl_of(&file).map_err(|why| format!("PW_SECRETS_FILE {path}: {why}"))?;
    // SAFETY: geteuid and getegid have no preconditions and cannot fail.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    private(
        &Access {
            owner: meta.uid(),
            group: meta.gid(),
            mode: meta.mode(),
            acl,
        },
        uid,
        gid,
    )
    .map_err(|why| format!("PW_SECRETS_FILE {path} {why}"))?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| format!("PW_SECRETS_FILE {path}: {e}"))?;
    Ok(text)
}

/// **Who a file says may read and write it**: its owner and group, its mode,
/// and its access ACL's entries where it has one.
#[derive(Debug, Clone)]
struct Access {
    owner: u32,
    group: u32,
    mode: u32,
    acl: Option<Vec<AclEntry>>,
}

/// One entry of a POSIX access ACL, as Linux keeps it
/// (`system.posix_acl_access`, `include/uapi/linux/posix_acl_xattr.h`):
/// its tag, its permissions (read 4, write 2, execute 1) and the user or
/// group it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AclEntry {
    tag: u16,
    perm: u16,
    id: u32,
}

const ACL_USER_OBJ: u16 = 0x01;
const ACL_USER: u16 = 0x02;
const ACL_GROUP_OBJ: u16 = 0x04;
const ACL_GROUP: u16 = 0x08;
const ACL_MASK: u16 = 0x10;
const ACL_OTHER: u16 = 0x20;
const READ: u16 = 4;
const WRITE: u16 = 2;

/// **Whether a file is this process's alone** (user `uid`, group `gid`):
/// why not, where it is not, said of the file. Its owner is `uid` or root;
/// no one else may read or write it; its group, by mode or ACL, may read
/// only where it is `gid`, and never write; a named user may read only
/// where it is `uid`, a named group only where it is `gid`, and neither
/// write. An ACL's mask is a limit, which grants nothing.
fn private(access: &Access, uid: u32, gid: u32) -> Result<(), String> {
    let mode = access.mode & 0o777;
    if access.owner != uid && access.owner != 0 {
        return Err(format!(
            "is owned by uid {}, and this host runs as uid {uid}: a secrets file is this \
             process's user's, or root's",
            access.owner
        ));
    }
    if mode & 0o007 != 0 {
        return Err(format!(
            "may be read or written by anyone (mode {mode:03o}): a secrets file is its owner's \
             (chmod 600, or 400)"
        ));
    }
    let refused = |who: &str, perm: u16, ours: bool| -> Result<(), String> {
        if perm & WRITE != 0 {
            return Err(format!(
                "may be written by {who} (mode {mode:03o}): only its owner writes a secrets file \
                 (chmod 600, or 400)"
            ));
        }
        if perm & READ != 0 && !ours {
            return Err(format!(
                "may be read by {who}, which is not this process's (mode {mode:03o}): a secrets \
                 file is its owner's (chmod 600, or 400), and its group's only where that group \
                 is this host's own (gid {gid})"
            ));
        }
        Ok(())
    };
    match &access.acl {
        None => refused(
            &format!("its group, gid {}", access.group),
            ((mode >> 3) & 0o7) as u16,
            access.group == gid,
        )?,
        Some(entries) => {
            for entry in entries {
                match entry.tag {
                    ACL_USER => refused(
                        &format!("the user uid {} its ACL names", entry.id),
                        entry.perm,
                        entry.id == uid,
                    )?,
                    ACL_GROUP_OBJ => refused(
                        &format!("its group, gid {}", access.group),
                        entry.perm,
                        access.group == gid,
                    )?,
                    ACL_GROUP => refused(
                        &format!("the group gid {} its ACL names", entry.id),
                        entry.perm,
                        entry.id == gid,
                    )?,
                    ACL_OTHER if entry.perm & (READ | WRITE) != 0 => {
                        return Err(format!(
                            "may be read or written by anyone, as its ACL says (mode \
                             {mode:03o}): a secrets file is its owner's (chmod 600, or 400)"
                        ));
                    }
                    ACL_USER_OBJ | ACL_MASK | ACL_OTHER => {}
                    other => {
                        return Err(format!(
                            "has an ACL entry this host does not read ({other:#x})"
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// **An access ACL's entries, as Linux writes them**: a version-2 header
/// and eight bytes an entry, each little-endian; none for anything else.
fn parse_acl(bytes: &[u8]) -> Result<Vec<AclEntry>, String> {
    let malformed = || "its ACL is in no form this host reads".to_string();
    let (header, entries) = bytes.split_at_checked(4).ok_or_else(malformed)?;
    if u32::from_le_bytes(header.try_into().map_err(|_| malformed())?) != 2
        || !entries.len().is_multiple_of(8)
    {
        return Err(malformed());
    }
    Ok(entries
        .chunks_exact(8)
        .map(|e| AclEntry {
            tag: u16::from_le_bytes([e[0], e[1]]),
            perm: u16::from_le_bytes([e[2], e[3]]),
            id: u32::from_le_bytes([e[4], e[5], e[6], e[7]]),
        })
        .collect())
}

/// **The access ACL of an opened file**, read from its descriptor: none
/// where it has none, or its file system keeps none.
#[cfg(target_os = "linux")]
fn acl_of(file: &std::fs::File) -> Result<Option<Vec<AclEntry>>, String> {
    use std::os::fd::AsRawFd;
    let mut buffer = [0u8; 4096];
    // SAFETY: a valid descriptor, a NUL-terminated name, and a buffer of the
    // length given, which fgetxattr writes at most.
    let read = unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            c"system.posix_acl_access".as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if read < 0 {
        let e = std::io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ENODATA) | Some(libc::ENOTSUP) => Ok(None),
            _ => Err(format!("its ACL could not be read: {e}")),
        };
    }
    parse_acl(&buffer[..read as usize]).map(Some)
}

/// Elsewhere no ACL is read: the mode is the rule.
#[cfg(all(unix, not(target_os = "linux")))]
fn acl_of(_file: &std::fs::File) -> Result<Option<Vec<AclEntry>>, String> {
    Ok(None)
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

    /// **A file anyone may read, or its group may write, is refused,
    /// unread**; one its group may read is read where the group is this
    /// process's own, as a file the test makes is (its user's group).
    #[test]
    fn a_secrets_file_others_may_read_is_refused() {
        for mode in [0o604, 0o644, 0o660, 0o606, 0o601, 0o620, 0o602] {
            let (_dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), mode);
            let refused = read_with(&path, &[]).err().unwrap_or_default();
            assert!(refused.contains("chmod 600, or 400"), "{mode:o}: {refused}");
            assert!(!refused.contains(KEY), "{refused}");
        }
        for mode in [0o600, 0o400, 0o640, 0o440] {
            let (_dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), mode);
            let read = read_with(&path, &[]).unwrap_or_else(|e| panic!("{mode:o}: {e}"));
            assert_eq!(read.get("PW_IDENTITY_KEY").map(Secret::expose), Some(KEY));
        }
    }

    /// **A link is not followed, and a pipe is not waited on** (the
    /// review's finding 4): a secrets file named by a symbolic link is
    /// refused, as is one that is no regular file; the file the link names,
    /// named itself, is read.
    #[test]
    fn a_link_is_not_followed_and_a_pipe_not_waited_on() {
        let (dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), 0o600);
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).expect("a link");
        let refused = read_with(link.to_str().expect("a path"), &[])
            .err()
            .unwrap_or_default();
        assert!(refused.contains("is a symbolic link"), "{refused}");
        assert!(read_with(&path, &[]).is_ok());

        let pipe = dir.path().join("pipe");
        let name = std::ffi::CString::new(pipe.to_str().expect("a path")).expect("a name");
        // SAFETY: a NUL-terminated path in a directory of the test's own.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (done, ended) = std::sync::mpsc::channel();
        let pipe = pipe.display().to_string();
        std::thread::spawn(move || {
            let _ = done.send(read_with(&pipe, &[]));
        });
        let refused = ended
            .recv_timeout(std::time::Duration::from_secs(5))
            .map(|r| r.err().unwrap_or_default());
        assert!(
            refused
                .as_deref()
                .is_ok_and(|r| r.contains("is not a regular file")),
            "{refused:?}"
        );
    }

    /// The access a file says it gives, its owner and group this process's.
    fn access(mode: u32, acl: Option<Vec<AclEntry>>) -> Access {
        Access {
            owner: 1000,
            group: 1000,
            mode,
            acl,
        }
    }

    fn entry(tag: u16, perm: u16, id: u32) -> AclEntry {
        AclEntry { tag, perm, id }
    }

    /// **Whose a file is, read whole** (the review's finding 4): its owner
    /// this process's user or root; its group's read only where that group
    /// is this process's; and an ACL read as what it grants, as systemd's
    /// credential (0400 and an entry for the service's user) is read, and
    /// one granting another user, another group, a write or anyone is not.
    #[test]
    fn whose_a_file_is_read_whole() {
        let (uid, gid) = (1000, 1000);
        let ok = |a: &Access| private(a, uid, gid);
        assert!(ok(&access(0o600, None)).is_ok());
        assert!(ok(&access(0o400, None)).is_ok());
        assert!(ok(&access(0o440, None)).is_ok());
        let root = Access {
            owner: 0,
            ..access(0o440, None)
        };
        assert!(ok(&root).is_ok(), "root's, its group this process's");
        let someone = Access {
            owner: 1001,
            ..access(0o600, None)
        };
        assert!(ok(&someone).is_err_and(|e| e.contains("owned by uid 1001")));
        let their_group = Access {
            group: 1001,
            ..access(0o440, None)
        };
        assert!(ok(&their_group).is_err_and(|e| e.contains("its group, gid 1001")));
        assert!(
            ok(&Access {
                group: 1001,
                ..access(0o400, None)
            })
            .is_ok()
        );

        // systemd's credential: 0400, read by the service's user through
        // an entry `stat` shows as the group's bits.
        let systemd = vec![
            entry(ACL_USER_OBJ, READ, u32::MAX),
            entry(ACL_USER, READ, uid),
            entry(ACL_GROUP_OBJ, 0, u32::MAX),
            entry(ACL_MASK, READ, u32::MAX),
            entry(ACL_OTHER, 0, u32::MAX),
        ];
        let credential = Access {
            owner: 0,
            group: 0,
            ..access(0o440, Some(systemd.clone()))
        };
        assert!(ok(&credential).is_ok(), "{:?}", ok(&credential));
        for (changed, why) in [
            (entry(ACL_USER, READ, 1001), "the user uid 1001"),
            (
                entry(ACL_USER, READ | WRITE, uid),
                "may be written by the user",
            ),
            (entry(ACL_GROUP_OBJ, READ, u32::MAX), "its group, gid 0"),
            (entry(ACL_GROUP, READ, 1001), "the group gid 1001"),
            (entry(ACL_OTHER, READ, u32::MAX), "anyone"),
        ] {
            let mut acl = systemd.clone();
            acl.retain(|e| e.tag != changed.tag);
            acl.push(changed);
            let given = Access {
                acl: Some(acl),
                ..credential.clone()
            };
            assert!(
                ok(&given).is_err_and(|e| e.contains(why)),
                "{changed:?}: {:?}",
                ok(&given)
            );
        }
        let ours = Access {
            acl: Some(vec![
                entry(ACL_USER_OBJ, READ, u32::MAX),
                entry(ACL_GROUP, READ, gid),
                entry(ACL_GROUP_OBJ, 0, u32::MAX),
                entry(ACL_MASK, READ, u32::MAX),
                entry(ACL_OTHER, 0, u32::MAX),
            ]),
            ..credential.clone()
        };
        assert!(ok(&ours).is_ok(), "a named group that is this process's");
    }

    /// **An ACL is read as Linux writes it**: a version-2 header, eight
    /// bytes an entry, little-endian; anything else is refused.
    #[test]
    fn an_acl_is_read_as_linux_writes_it() {
        let mut bytes = 2u32.to_le_bytes().to_vec();
        for (tag, perm, id) in [(ACL_USER_OBJ, 6u16, u32::MAX), (ACL_USER, 4, 65532)] {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(perm.to_le_bytes());
            bytes.extend(id.to_le_bytes());
        }
        assert_eq!(
            parse_acl(&bytes),
            Ok(vec![
                entry(ACL_USER_OBJ, 6, u32::MAX),
                entry(ACL_USER, 4, 65532)
            ])
        );
        assert!(parse_acl(&bytes[..bytes.len() - 1]).is_err());
        let mut other_version = bytes.clone();
        other_version[0] = 1;
        assert!(parse_acl(&other_version).is_err());
        assert!(parse_acl(&[]).is_err());
    }

    /// **A file's own ACL is read from its descriptor**, where its file
    /// system keeps one: systemd's shape, for this process's user, is read;
    /// one naming another user is refused, unread.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_files_own_acl_is_read() {
        use std::os::fd::AsRawFd;
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        let with_acl = |named: u32| {
            let (dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), 0o400);
            let mut bytes = 2u32.to_le_bytes().to_vec();
            for (tag, perm, id) in [
                (ACL_USER_OBJ, READ, u32::MAX),
                (ACL_USER, READ, named),
                (ACL_GROUP_OBJ, 0, u32::MAX),
                (ACL_MASK, READ, u32::MAX),
                (ACL_OTHER, 0, u32::MAX),
            ] {
                bytes.extend(tag.to_le_bytes());
                bytes.extend(perm.to_le_bytes());
                bytes.extend(id.to_le_bytes());
            }
            let opened = std::fs::File::open(&path).expect("opened");
            // SAFETY: a valid descriptor, a NUL-terminated name, and the
            // buffer's own length.
            let set = unsafe {
                libc::fsetxattr(
                    opened.as_raw_fd(),
                    c"system.posix_acl_access".as_ptr(),
                    bytes.as_ptr().cast(),
                    bytes.len(),
                    0,
                )
            };
            (dir, path, set == 0)
        };
        let (_dir, path, kept) = with_acl(uid);
        if !kept {
            eprintln!("skipped: this file system keeps no ACL");
            return;
        }
        assert!(read_with(&path, &[]).is_ok(), "{:?}", read_with(&path, &[]));
        let (_dir, path, _) = with_acl(uid.wrapping_add(1));
        let refused = read_with(&path, &[]).err().unwrap_or_default();
        assert!(refused.contains("its ACL names"), "{refused}");
        assert!(!refused.contains(KEY), "{refused}");
    }

    /// **systemd's credential is the secrets file where none is named**:
    /// `secrets` in `$CREDENTIALS_DIRECTORY`, mode 0440 as systemd's ACL
    /// shows it; `PW_SECRETS_FILE` named wins; a directory with no
    /// `secrets` names none.
    #[test]
    fn systemds_credential_is_the_secrets_file_where_none_is_named() {
        let (dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), 0o440);
        let credentials = dir.path().display().to_string();
        let given = |pairs: Vec<(&'static str, String)>| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.clone())
            }
        };
        let systemd = given(vec![("CREDENTIALS_DIRECTORY", credentials.clone())]);
        assert_eq!(secrets_file(&systemd), Some(path.clone()));
        let read = Secrets::read(&systemd).expect("read");
        assert_eq!(read.get("PW_IDENTITY_KEY").map(Secret::expose), Some(KEY));

        let named = given(vec![
            ("CREDENTIALS_DIRECTORY", credentials),
            ("PW_SECRETS_FILE", "/elsewhere/secrets".into()),
        ]);
        assert_eq!(secrets_file(&named).as_deref(), Some("/elsewhere/secrets"));
        let empty = tempfile::TempDir::with_prefix("pw-credentials-").expect("a directory");
        let none = given(vec![(
            "CREDENTIALS_DIRECTORY",
            empty.path().display().to_string(),
        )]);
        assert_eq!(secrets_file(&none), None);
        assert_eq!(secrets_file(&given(vec![])), None);
    }

    /// **A secret given twice is refused**: in the environment and in the
    /// file, or twice in the file.
    #[test]
    fn a_secret_given_twice_is_refused() {
        let (_dir, path) = file(&format!("PW_IDENTITY_KEY={KEY}\n"), 0o600);
        let refused = read_with(&path, &[("PW_IDENTITY_KEY", KEY)])
            .err()
            .unwrap_or_default();
        assert!(refused.contains("given twice"), "{refused}");
        let (_dir, path) = file(
            &format!("PW_IDENTITY_KEY={KEY}\nPW_IDENTITY_KEY={KEY}\n"),
            0o600,
        );
        let refused = read_with(&path, &[]).err().unwrap_or_default();
        assert!(refused.contains("given twice"), "{refused}");
        assert!(!refused.contains(KEY), "{refused}");
    }

    /// **A line that is no secret's is refused, and never quoted**: a name
    /// the host does not read, a line with no `=`, an empty value.
    #[test]
    fn a_line_that_is_no_secret_is_refused_and_never_quoted() {
        let (_dir, path) = file(&format!("PW_IDENTITY_KEY {KEY}\n"), 0o600);
        let refused = read_with(&path, &[]).err().unwrap_or_default();
        assert!(refused.contains("line 1: not `NAME=value`"), "{refused}");
        assert!(!refused.contains(KEY), "{refused}");

        let (_dir, path) = file("PW_IDENTITY_KEYS=s3cr3t-value\n", 0o600);
        let refused = read_with(&path, &[]).err().unwrap_or_default();
        assert!(refused.contains("line 1: names no secret"), "{refused}");
        assert!(!refused.contains("s3cr3t"), "{refused}");

        // A value whose `=` came first is no name, and is not quoted as one.
        let (_dir, path) = file("S3CR3T=PW_IDENTITY_KEY\n", 0o600);
        let refused = read_with(&path, &[]).err().unwrap_or_default();
        assert!(!refused.contains("S3CR3T"), "{refused}");

        let (_dir, path) = file("PW_METRICS_TOKEN=\n", 0o600);
        let refused = read_with(&path, &[]).err().unwrap_or_default();
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
