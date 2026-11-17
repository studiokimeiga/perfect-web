//! **A deployment's blob storage** (track `uploads`): where an upload's
//! bytes are kept, by the SHA-256 of the bytes.
//!
//! Pleris builds no storage and no CDN. A deployment supplies a
//! [`BlobStore`]; this file has the one a development server and its tests
//! use, [`LocalBlobs`], a directory. What a deployment's must do is what the
//! trait says: keep bytes by their hash, give back exactly those bytes, and
//! forget them when told. Nothing a sender chose reaches a path: a key is
//! 64 lowercase hexadecimal digits or it is not a key.

use sha2::{Digest, Sha256};

mod assets;
use std::io;
use std::path::PathBuf;

/// **A blob's address**: the SHA-256 of its bytes (FIPS 180-4), as 64
/// lowercase hexadecimal digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobKey(String);

impl BlobKey {
    /// The key of `bytes`.
    pub fn of(bytes: &[u8]) -> BlobKey {
        let digest = Sha256::digest(bytes);
        BlobKey(digest.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// The key `text` is, where it is 64 lowercase hexadecimal digits and
    /// nothing else: no separator, no dot, no other case.
    pub fn parse(text: &str) -> Option<BlobKey> {
        (text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
            .then(|| BlobKey(text.to_string()))
    }

    pub fn hex(&self) -> &str {
        &self.0
    }
}

/// **What a deployment's blob storage does.**
pub trait BlobStore: Send + Sync {
    /// Keep `bytes`, by their key. Keeping bytes already kept keeps them
    /// once.
    fn put(&self, bytes: &[u8]) -> io::Result<BlobKey>;
    /// The bytes kept at `key`, exactly, or none.
    fn get(&self, key: &BlobKey) -> io::Result<Option<Vec<u8>>>;
    /// Forget the bytes at `key`; forgetting what is not kept is nothing.
    fn delete(&self, key: &BlobKey) -> io::Result<()>;
    /// How many blobs it keeps: what a deployment measures it by, and what a
    /// test counts to see one forgotten.
    fn count(&self) -> io::Result<usize>;
}

/// **Blobs in memory**: where an upload's bytes wait for their post. A lease
/// is the server's, and none outlives it, so neither do its bytes; nor can
/// two servers of one build, each with its own leases, forget each other's.
#[derive(Default)]
pub struct MemoryBlobs {
    assets: std::sync::Mutex<assets::MemoryState>,
    kept: std::sync::Mutex<std::collections::BTreeMap<BlobKey, Vec<u8>>>,
}

impl BlobStore for MemoryBlobs {
    fn put(&self, bytes: &[u8]) -> io::Result<BlobKey> {
        let key = BlobKey::of(bytes);
        self.kept
            .lock()
            .expect("blobs")
            .entry(key.clone())
            .or_insert_with(|| bytes.to_vec());
        Ok(key)
    }

    fn get(&self, key: &BlobKey) -> io::Result<Option<Vec<u8>>> {
        Ok(self.kept.lock().expect("blobs").get(key).cloned())
    }

    fn delete(&self, key: &BlobKey) -> io::Result<()> {
        self.kept.lock().expect("blobs").remove(key);
        Ok(())
    }

    fn count(&self) -> io::Result<usize> {
        Ok(self.kept.lock().expect("blobs").len())
    }
}

/// **A directory of blobs, for development and tests**: each at
/// `<root>/<first two digits>/<key>`, written whole to a temporary name and
/// renamed into place, so a reader never sees half of one.
pub struct LocalBlobs {
    root: PathBuf,
}

impl LocalBlobs {
    pub fn new(root: impl Into<PathBuf>) -> io::Result<LocalBlobs> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(LocalBlobs { root })
    }

    /// Where `key` is kept: built from the key's digits alone.
    fn path(&self, key: &BlobKey) -> PathBuf {
        self.root.join(&key.hex()[..2]).join(key.hex())
    }
}

impl BlobStore for LocalBlobs {
    fn put(&self, bytes: &[u8]) -> io::Result<BlobKey> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let key = BlobKey::of(bytes);
        let path = self.path(&key);
        if path.exists() {
            return Ok(key);
        }
        let dir = path.parent().expect("a key's directory");
        std::fs::create_dir_all(dir)?;
        let temporary = dir.join(format!(
            ".{}-{}-{}",
            key.hex(),
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &path)?;
        Ok(key)
    }

    fn get(&self, key: &BlobKey) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(key)) {
            // A blob whose bytes are not its key's is not given out.
            Ok(bytes) if BlobKey::of(&bytes) == *key => Ok(Some(bytes)),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("blob {} is not the bytes its key names", key.hex()),
            )),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn delete(&self, key: &BlobKey) -> io::Result<()> {
        match std::fs::remove_file(self.path(key)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Each file under a key's directory, a temporary one being written
    /// aside.
    fn count(&self) -> io::Result<usize> {
        let mut n = 0;
        for dir in std::fs::read_dir(&self.root)? {
            let dir = dir?.path();
            if dir.is_dir()
                && dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.len() == 2 && name.bytes().all(|b| matches!(b,b'0'..=b'9'|b'a'..=b'f'))
                    })
            {
                for f in std::fs::read_dir(dir)? {
                    let named = f?.file_name();
                    n += usize::from(!named.to_string_lossy().starts_with('.'));
                }
            }
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_the_sha256_of_the_bytes() {
        // FIPS 180-4's example, "abc".
        assert_eq!(
            BlobKey::of(b"abc").hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_key_is_64_lowercase_hex_digits_and_nothing_else() {
        let k = BlobKey::of(b"abc");
        assert_eq!(BlobKey::parse(k.hex()), Some(k.clone()));
        for bad in [
            "",
            "../etc/passwd",
            &k.hex()[..63],
            &format!("{}0", k.hex()),
            &k.hex().to_uppercase(),
            &format!("{}/", &k.hex()[..63]),
            &format!("..{}", &k.hex()[..62]),
        ] {
            assert_eq!(BlobKey::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_blob_is_kept_by_its_key_once_and_forgotten() {
        let dir = tempfile::TempDir::with_prefix("pw-blobs-").expect("dir");
        let blobs = LocalBlobs::new(dir.path()).expect("blobs");
        let k = blobs.put(b"one").expect("put");
        assert_eq!(blobs.put(b"one").expect("again"), k);
        assert_eq!(blobs.get(&k).expect("get"), Some(b"one".to_vec()));
        let path = dir.path().join(&k.hex()[..2]).join(k.hex());
        assert!(path.is_file(), "kept under its own digits");
        // Nothing but the blob in its directory: no temporary left.
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        assert_eq!(blobs.count().expect("count"), 1);
        blobs.delete(&k).expect("delete");
        assert_eq!(blobs.get(&k).expect("gone"), None);
        blobs.delete(&k).expect("forgetting twice is nothing");
        assert_eq!(blobs.count().expect("count"), 0);
    }

    #[test]
    fn a_blob_in_memory_is_kept_by_its_key_once_and_forgotten() {
        let blobs = MemoryBlobs::default();
        let k = blobs.put(b"one").expect("put");
        assert_eq!(blobs.put(b"one").expect("again"), k);
        assert_eq!(blobs.get(&k).expect("get"), Some(b"one".to_vec()));
        assert_eq!(blobs.count().expect("count"), 1);
        blobs.delete(&k).expect("delete");
        assert_eq!(
            (blobs.get(&k).expect("gone"), blobs.count().unwrap()),
            (None, 0)
        );
    }

    #[test]
    fn a_blob_changed_where_it_is_kept_is_not_given_out() {
        let dir = tempfile::TempDir::with_prefix("pw-blobs-").expect("dir");
        let blobs = LocalBlobs::new(dir.path()).expect("blobs");
        let k = blobs.put(b"one").expect("put");
        std::fs::write(dir.path().join(&k.hex()[..2]).join(k.hex()), b"two").expect("tamper");
        assert!(blobs.get(&k).is_err());
    }
}
