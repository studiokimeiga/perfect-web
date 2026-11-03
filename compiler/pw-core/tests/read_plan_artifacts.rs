//! A real compiled Pleris table: inspectable read candidates and deterministic
//! build artifacts, not an assertion that arbitrary source loops were optimized.
use std::path::Path;

use pw_core::{program, read_plans};

fn build_chores() -> pw_core::build::Build {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = Vec::<(String, String)>::new();
    for directory in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<_> = std::fs::read_dir(root.join(directory))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "pw"))
            .collect();
        paths.sort();
        for path in paths {
            sources.push((
                path.display().to_string(),
                std::fs::read_to_string(path).unwrap(),
            ));
        }
    }
    let fixture =
        root.join("examples/accepted/A-050-a-table-read-and-written-through-its-functions.pw");
    sources.push((
        fixture.display().to_string(),
        std::fs::read_to_string(fixture).unwrap(),
    ));
    program::validate_sources(&sources)
        .into_build()
        .expect("accepted Pleris application builds")
}

#[test]
fn a_validated_program_exposes_the_same_read_families_in_build_and_inventory() {
    let build = build_chores();
    let inventory = read_plans::inventory(&build.tables);
    assert_eq!(inventory.schema_hash, build.tables.hash);
    assert_eq!(inventory.families.len(), 3);
    assert!(
        inventory
            .families
            .iter()
            .any(|f| f.keyed() == "get" && f.set() == "get_many")
    );
    assert!(
        inventory
            .families
            .iter()
            .any(|f| f.keyed() == "of_room" && f.set() == "of_room_many")
    );
    assert!(
        inventory
            .families
            .iter()
            .any(|f| f.keyed() == "by_done" && f.set() == "by_done_many")
    );

    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    build.write(first.path()).expect("first build");
    build.write(second.path()).expect("second build");
    let bytes = std::fs::read(first.path().join("read-plans.json")).unwrap();
    assert_eq!(
        bytes,
        std::fs::read(second.path().join("read-plans.json")).unwrap()
    );
    assert_eq!(
        std::fs::read(first.path().join("build-id")).unwrap(),
        std::fs::read(second.path().join("build-id")).unwrap()
    );
    let artifact: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(artifact["schema_hash"].as_str().unwrap(), build.tables.hash);
    assert_eq!(artifact["families"].as_array().unwrap().len(), 3);
    assert!(
        artifact["policy"]
            .as_str()
            .unwrap()
            .contains("no automatic loop rewrite")
    );
}

#[test]
fn a_rebuild_without_tables_removes_stale_read_plans() {
    let mut build = build_chores();
    let output = tempfile::tempdir().unwrap();
    build.write(output.path()).expect("with tables");
    assert!(output.path().join("read-plans.json").exists());

    build.tables.tables.clear();
    build.write(output.path()).expect("without tables");
    assert!(!output.path().join("read-plans.json").exists());
    assert!(!output.path().join("tables.json").exists());
    assert!(!output.path().join("schema.lock").exists());
}
