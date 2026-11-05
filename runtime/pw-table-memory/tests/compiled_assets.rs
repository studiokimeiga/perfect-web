//! A reference's pure projection needs no claim or data callback.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/assets.rs");

#[test]
fn a_compiled_reference_projects_the_existing_public_url_without_host_work() {
    let program = asset_program();
    let query = program.component("Url");
    assert!(
        query.imports.is_empty(),
        "pure URL projection must import no host operation"
    );
    let answer = pw_host::engine::call_with_tables_as(
        &query.bytes,
        &query.contract,
        &query.granted(),
        &pw_host::Limits::unbounded(),
        &BTreeMap::new(),
        &program.manifest,
        &Caller::Nobody,
        &query.export(),
        &[image_reference()],
    );
    assert_eq!(
        answer,
        Ok(vec![Val::String(format!("/images/{}.png", "a".repeat(64)))])
    );
}

#[test]
fn asset_metadata_is_not_silently_served_by_an_adapter_with_no_upload_binding() {
    let program = asset_program();
    let result = MemoryTables::new(program.tables);
    assert!(
        matches!(result,Err(pw_table_memory::Error::Schema(ref why)) if why.contains("asset")),
        "asset declarations need their actual storage binding"
    );
}
