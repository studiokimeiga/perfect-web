//! Generation-isolated storage; the database remains the liveness authority.
use super::*;
use pw_host::assets::{Descriptor, Namespace, Storage};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Default)]
pub(super) struct MemoryState {
    spaces: BTreeMap<Namespace, BTreeMap<String, MemoryGeneration>>,
}
struct MemoryGeneration {
    descriptor: Descriptor,
    payload: Option<Vec<u8>>,
}
fn invalid(why: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}
fn check_descriptor(descriptor: &Descriptor) -> io::Result<()> {
    descriptor.reference.validate().map_err(invalid)?;
    if descriptor.bytes == 0 {
        return Err(invalid("generation has no canonical bytes"));
    }
    Ok(())
}
fn check_bytes(descriptor: &Descriptor, bytes: &[u8]) -> io::Result<()> {
    check_descriptor(descriptor)?;
    if bytes.len() as u64 != descriptor.bytes
        || BlobKey::of(bytes).hex() != descriptor.reference.key
    {
        return Err(invalid(
            "generation payload differs from its canonical byte count or hash",
        ));
    }
    Ok(())
}
fn missing_namespace() -> io::Error {
    invalid("storage namespace is not enrolled")
}
impl Storage for MemoryBlobs {
    fn enroll(&self, namespace: &Namespace, fresh: bool) -> io::Result<()> {
        let mut state = self
            .assets
            .lock()
            .map_err(|_| io::Error::other("generation storage poisoned"))?;
        if fresh {
            if state.spaces.contains_key(namespace) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "namespace already reserved",
                ));
            }
            state.spaces.insert(namespace.clone(), BTreeMap::new());
        } else if !state.spaces.contains_key(namespace) {
            return Err(missing_namespace());
        }
        Ok(())
    }
    fn keep_new(
        &self,
        namespace: &Namespace,
        descriptor: &Descriptor,
        bytes: &[u8],
    ) -> io::Result<()> {
        check_bytes(descriptor, bytes)?;
        let mut state = self
            .assets
            .lock()
            .map_err(|_| io::Error::other("generation storage poisoned"))?;
        let space = state
            .spaces
            .get_mut(namespace)
            .ok_or_else(missing_namespace)?;
        if space.contains_key(&descriptor.reference.generation) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "generation already reserved",
            ));
        }
        space.insert(
            descriptor.reference.generation.clone(),
            MemoryGeneration {
                descriptor: descriptor.clone(),
                payload: Some(bytes.to_vec()),
            },
        );
        Ok(())
    }
    fn get_generation(
        &self,
        namespace: &Namespace,
        descriptor: &Descriptor,
    ) -> io::Result<Option<Vec<u8>>> {
        check_descriptor(descriptor)?;
        let state = self
            .assets
            .lock()
            .map_err(|_| io::Error::other("generation storage poisoned"))?;
        let generation = state
            .spaces
            .get(namespace)
            .ok_or_else(missing_namespace)?
            .get(&descriptor.reference.generation)
            .ok_or_else(|| invalid("generation has no reservation marker"))?;
        if generation.descriptor != *descriptor {
            return Err(invalid("generation descriptor marker changed"));
        }
        if let Some(bytes) = &generation.payload {
            check_bytes(descriptor, bytes)?;
        }
        Ok(generation.payload.clone())
    }
    fn collect_generation(&self, namespace: &Namespace, descriptor: &Descriptor) -> io::Result<()> {
        check_descriptor(descriptor)?;
        let mut state = self
            .assets
            .lock()
            .map_err(|_| io::Error::other("generation storage poisoned"))?;
        let generation = state
            .spaces
            .get_mut(namespace)
            .ok_or_else(missing_namespace)?
            .get_mut(&descriptor.reference.generation)
            .ok_or_else(|| invalid("generation has no reservation marker"))?;
        if generation.descriptor != *descriptor {
            return Err(invalid("generation descriptor marker changed"));
        }
        generation.payload = None;
        Ok(())
    }
}

fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create_new(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
/// Read at most the expected length plus one, checking file metadata before
/// allocation and actual length after the read. An unexpectedly huge/corrupt
/// file must not cause an unbounded read allocation.
fn read_sized(path: &Path, length: u64) -> io::Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() != length {
        return Err(invalid("stored generation length changed"));
    }
    let cap = length
        .checked_add(1)
        .ok_or_else(|| invalid("generation length overflow"))?;
    let mut bytes = Vec::new();
    file.take(cap).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != length {
        return Err(invalid("generation length changed during read"));
    }
    Ok(Some(bytes))
}
fn exact_marker(path: &Path, expected: &[u8]) -> io::Result<()> {
    if read_sized(path, expected.len() as u64)?.as_deref() != Some(expected) {
        return Err(invalid("missing or changed storage marker"));
    }
    Ok(())
}
impl LocalBlobs {
    fn namespace_dir(&self, namespace: &Namespace) -> PathBuf {
        self.root.join(".generic").join(namespace.hex())
    }
    fn namespace_marker(namespace: &Namespace) -> Vec<u8> {
        format!("pw-upload-namespace-v1\n{}\n", namespace.hex()).into_bytes()
    }
    fn checked_namespace(&self, namespace: &Namespace) -> io::Result<PathBuf> {
        let dir = self.namespace_dir(namespace);
        exact_marker(&dir.join(".namespace"), &Self::namespace_marker(namespace))?;
        if !dir.join("generations").is_dir() {
            return Err(missing_namespace());
        }
        Ok(dir)
    }
    fn checked_generation(
        &self,
        namespace: &Namespace,
        descriptor: &Descriptor,
    ) -> io::Result<PathBuf> {
        check_descriptor(descriptor)?;
        let dir = self
            .checked_namespace(namespace)?
            .join("generations")
            .join(&descriptor.reference.generation);
        let marker = serde_json::to_vec(descriptor).map_err(invalid)?;
        exact_marker(&dir.join(".descriptor"), &marker)?;
        Ok(dir)
    }
}
impl Storage for LocalBlobs {
    fn enroll(&self, namespace: &Namespace, fresh: bool) -> io::Result<()> {
        if !fresh {
            self.checked_namespace(namespace)?;
            return Ok(());
        }
        let base = self.root.join(".generic");
        std::fs::create_dir_all(&base)?;
        let dir = self.namespace_dir(namespace);
        std::fs::create_dir(&dir)?;
        write_new(&dir.join(".namespace"), &Self::namespace_marker(namespace))?;
        std::fs::create_dir(dir.join("generations"))?;
        sync_dir(&dir)?;
        sync_dir(&base)?;
        sync_dir(&self.root)
    }
    fn keep_new(
        &self,
        namespace: &Namespace,
        descriptor: &Descriptor,
        bytes: &[u8],
    ) -> io::Result<()> {
        check_bytes(descriptor, bytes)?;
        let parent = self.checked_namespace(namespace)?.join("generations");
        let dir = parent.join(&descriptor.reference.generation);
        // Exclusive reservation survives failed keep, rollback and collection.
        // Never remove this directory/marker as part of captured payload GC.
        std::fs::create_dir(&dir)?;
        let marker = serde_json::to_vec(descriptor).map_err(invalid)?;
        write_new(&dir.join(".descriptor"), &marker)?;
        write_new(&dir.join(&descriptor.reference.key), bytes)?;
        sync_dir(&dir)?;
        sync_dir(&parent)
    }
    fn get_generation(
        &self,
        namespace: &Namespace,
        descriptor: &Descriptor,
    ) -> io::Result<Option<Vec<u8>>> {
        let dir = self.checked_generation(namespace, descriptor)?;
        let bytes = read_sized(&dir.join(&descriptor.reference.key), descriptor.bytes)?;
        if let Some(bytes) = &bytes {
            check_bytes(descriptor, bytes)?;
        }
        Ok(bytes)
    }
    fn collect_generation(&self, namespace: &Namespace, descriptor: &Descriptor) -> io::Result<()> {
        let dir = self.checked_generation(namespace, descriptor)?;
        match std::fs::remove_file(dir.join(&descriptor.reference.key)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        sync_dir(&dir)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    struct Temporary(PathBuf);
    impl Temporary {
        fn new() -> Self {
            let mut id = [0u8; 16];
            getrandom::fill(&mut id).expect("OS random");
            let path = std::env::temp_dir().join(format!(
                "pw-generation-storage-{}",
                Namespace::from_bytes(id).hex()
            ));
            std::fs::create_dir(&path).expect("isolated temp root");
            Self(path)
        }
    }
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn ns() -> Namespace {
        Namespace::from_bytes([1; 16])
    }
    fn descriptor(bytes: &[u8], generation: char) -> Descriptor {
        Descriptor {
            reference: pw_host::assets::Reference {
                key: BlobKey::of(bytes).hex().into(),
                generation: generation.to_string().repeat(32),
                kind: "png".into(),
                width: 6,
                height: 4,
            },
            bytes: bytes.len() as u64,
        }
    }
    fn good<T>(result: io::Result<T>) -> T {
        assert!(
            result.is_ok(),
            "storage operation should succeed: {:?}",
            result.as_ref().err()
        );
        match result {
            Ok(v) => v,
            Err(_) => unreachable!(),
        }
    }
    fn contract(storage: &dyn Storage) {
        let ns = ns();
        let d = descriptor(b"canonical bytes", 'b');
        good(storage.enroll(&ns, true));
        good(storage.enroll(&ns, false));
        assert!(
            storage.enroll(&ns, true).is_err(),
            "fresh enrollment is exclusive"
        );
        assert!(
            storage
                .enroll(&Namespace::from_bytes([2; 16]), false)
                .is_err(),
            "existing enrollment never creates a missing namespace"
        );
        good(storage.keep_new(&ns, &d, b"canonical bytes"));
        let other = Namespace::from_bytes([2; 16]);
        good(storage.enroll(&other, true));
        good(storage.keep_new(&other, &d, b"canonical bytes"));

        assert_eq!(
            good(storage.get_generation(&ns, &d)),
            Some(b"canonical bytes".to_vec())
        );
        assert!(
            storage.keep_new(&ns, &d, b"canonical bytes").is_err(),
            "an orphan reservation cannot be reused"
        );
        let mut changed = d.clone();
        changed.reference.width += 1;
        assert!(storage.get_generation(&ns, &changed).is_err());
        assert!(storage.collect_generation(&ns, &changed).is_err());
        good(storage.collect_generation(&ns, &d));
        good(storage.collect_generation(&ns, &d));
        assert_eq!(good(storage.get_generation(&ns, &d)), None);
        assert_eq!(
            good(storage.get_generation(&other, &d)),
            Some(b"canonical bytes".to_vec()),
            "collection is namespace-specific"
        );

        assert!(
            storage.keep_new(&ns, &d, b"canonical bytes").is_err(),
            "collection retains the generation reservation"
        );
    }
    #[test]
    fn memory_provider_reserves_and_validates_generations() {
        contract(&MemoryBlobs::default());
    }
    #[test]
    fn local_provider_reserves_and_validates_generations() {
        let tmp = Temporary::new();
        contract(&LocalBlobs::new(&tmp.0).expect("local root"));
    }
    #[test]
    fn independent_handles_share_enrollment_and_different_roots_refuse_existing_namespace() {
        let root = Temporary::new();
        let other = Temporary::new();
        let a = LocalBlobs::new(&root.0).unwrap();
        let b = LocalBlobs::new(&root.0).unwrap();
        good(a.enroll(&ns(), true));
        good(b.enroll(&ns(), false));
        let d = descriptor(b"actual bytes", 'b');
        good(a.keep_new(&ns(), &d, b"actual bytes"));
        assert_eq!(
            good(b.get_generation(&ns(), &d)),
            Some(b"actual bytes".to_vec())
        );
        let wrong = LocalBlobs::new(&other.0).unwrap();
        assert!(wrong.enroll(&ns(), false).is_err());
        let namespace_dir = root.0.join(".generic").join(ns().hex());
        std::fs::remove_file(namespace_dir.join(".namespace")).expect("remove marker");
        assert!(b.enroll(&ns(), false).is_err());
        assert!(b.get_generation(&ns(), &d).is_err());
    }
    #[test]
    fn namespaces_isolate_identical_generation_names_and_legacy_blob_counts() {
        let root = Temporary::new();
        let store = LocalBlobs::new(&root.0).unwrap();
        let a = ns();
        let b = Namespace::from_bytes([2; 16]);
        good(store.enroll(&a, true));
        good(store.enroll(&b, true));
        let d = descriptor(b"actual bytes", 'b');
        good(store.keep_new(&a, &d, b"actual bytes"));
        good(store.keep_new(&b, &d, b"actual bytes"));
        good(store.collect_generation(&a, &d));
        assert_eq!(
            good(store.get_generation(&b, &d)),
            Some(b"actual bytes".to_vec())
        );
        assert_eq!(
            store.count().unwrap(),
            0,
            "generic metadata is not a legacy flat blob"
        );
        store.put(b"legacy bytes").unwrap();
        assert_eq!(store.count().unwrap(), 1);
    }
    #[test]
    fn old_captured_cleanup_cannot_delete_a_new_generation_of_identical_bytes() {
        let root = Temporary::new();
        let a = LocalBlobs::new(&root.0).unwrap();
        let b = LocalBlobs::new(&root.0).unwrap();
        good(a.enroll(&ns(), true));
        let old = descriptor(b"same canonical bytes", 'b');
        let new = descriptor(b"same canonical bytes", 'c');
        good(a.keep_new(&ns(), &old, b"same canonical bytes"));
        good(b.keep_new(&ns(), &new, b"same canonical bytes"));
        good(a.collect_generation(&ns(), &old));
        assert_eq!(
            good(b.get_generation(&ns(), &new)),
            Some(b"same canonical bytes".to_vec())
        );
        assert!(a.keep_new(&ns(), &old, b"same canonical bytes").is_err());
    }
    #[test]
    fn reads_distinguish_missing_corrupt_payload_and_changed_markers() {
        let root = Temporary::new();
        let store = LocalBlobs::new(&root.0).unwrap();
        good(store.enroll(&ns(), true));
        let d = descriptor(b"actual bytes", 'b');
        good(store.keep_new(&ns(), &d, b"actual bytes"));
        let dir = root
            .0
            .join(".generic")
            .join(ns().hex())
            .join("generations")
            .join(&d.reference.generation);
        let path = dir.join(&d.reference.key);
        std::fs::write(&path, b"broken bytes").expect("corrupt payload");
        assert!(store.get_generation(&ns(), &d).is_err());
        std::fs::remove_file(&path).expect("missing payload");
        assert_eq!(good(store.get_generation(&ns(), &d)), None);
        std::fs::write(dir.join(".descriptor"), b"{}").expect("corrupt descriptor marker");
        assert!(store.get_generation(&ns(), &d).is_err());
        assert!(store.collect_generation(&ns(), &d).is_err());
    }
    #[test]
    fn two_independent_writers_cannot_reserve_the_same_generation_for_different_bytes() {
        let root = Temporary::new();
        let store = LocalBlobs::new(&root.0).unwrap();
        good(store.enroll(&ns(), true));
        let barrier = Arc::new(Barrier::new(3));
        let mut children = Vec::new();
        for bytes in [
            b"first canonical bytes".as_slice(),
            b"second canonical bytes".as_slice(),
        ] {
            let path = root.0.clone();
            let ready = Arc::clone(&barrier);
            let bytes = bytes.to_vec();
            children.push(std::thread::spawn(move || {
                let s = LocalBlobs::new(path).unwrap();
                let d = descriptor(&bytes, 'b');
                ready.wait();
                let result = s.keep_new(&ns(), &d, &bytes);
                (d, bytes, result)
            }));
        }
        barrier.wait();
        let results = children
            .into_iter()
            .map(|c| c.join().expect("writer finished"))
            .collect::<Vec<_>>();
        assert_eq!(
            results.iter().filter(|(_, _, r)| r.is_ok()).count(),
            1,
            "one exclusive generation reservation"
        );
        let (d, bytes, _) = results
            .iter()
            .find(|(_, _, r)| r.is_ok())
            .expect("one successful writer");
        assert_eq!(good(store.get_generation(&ns(), d)), Some(bytes.clone()));
    }
    #[test]
    fn bad_bytes_and_path_components_refuse_before_physical_publication() {
        let root = Temporary::new();
        let store = LocalBlobs::new(&root.0).unwrap();
        good(store.enroll(&ns(), true));
        let d = descriptor(b"actual bytes", 'b');
        assert!(store.keep_new(&ns(), &d, b"different bytes").is_err());
        assert!(
            store.keep_new(&ns(), &d, b"broken bytes").is_err(),
            "same-length wrong hash refuses before reservation"
        );
        let mut bad = d.clone();
        bad.reference.generation = "../outside".into();
        assert!(store.keep_new(&ns(), &bad, b"actual bytes").is_err());
        bad = d.clone();
        bad.bytes += 1;
        assert!(store.keep_new(&ns(), &bad, b"actual bytes").is_err());
        good(store.keep_new(&ns(), &d, b"actual bytes"));
        assert_eq!(
            good(store.get_generation(&ns(), &d)),
            Some(b"actual bytes".to_vec())
        );
        assert!(Namespace::parse("../outside").is_none());
        assert!(Namespace::parse(&"A".repeat(32)).is_none());
        assert_eq!(Namespace::parse(ns().hex()), Some(ns()));
    }
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    #[test]
    fn an_interrupted_empty_generation_directory_still_reserves_its_identity() {
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).expect("OS random");
        let root = std::env::temp_dir().join(format!(
            "pw-partial-generation-{}",
            Namespace::from_bytes(id).hex()
        ));
        let store = LocalBlobs::new(&root).expect("local root");
        let ns = Namespace::from_bytes([1; 16]);
        assert!(store.enroll(&ns, true).is_ok());
        let bytes = b"canonical bytes";
        let descriptor = Descriptor {
            reference: pw_host::assets::Reference {
                key: BlobKey::of(bytes).hex().into(),
                generation: "b".repeat(32),
                kind: "png".into(),
                width: 6,
                height: 4,
            },
            bytes: bytes.len() as u64,
        };
        let dir = store
            .namespace_dir(&ns)
            .join("generations")
            .join(&descriptor.reference.generation);
        std::fs::create_dir(&dir)
            .expect("simulate interrupted keep after exclusive directory reservation");
        assert!(
            store.keep_new(&ns, &descriptor, bytes).is_err(),
            "even an incomplete reservation cannot be reused"
        );
        assert!(
            store.get_generation(&ns, &descriptor).is_err(),
            "an incomplete reservation has no immutable marker"
        );
        assert!(dir.is_dir());
        std::fs::remove_dir_all(&root).expect("remove isolated test root");
    }
}
