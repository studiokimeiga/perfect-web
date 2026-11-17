//! Asset declarations require the upload manifest/storage binding explicitly.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
include!("../../pw-table-memory/tests/support/assets.rs");
mod common;
use pw_table_postgres::PgTables;

#[test]
fn asset_metadata_is_not_silently_served_by_an_adapter_with_no_upload_binding() {
    let program = asset_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "asset_binding_required");
    let result = PgTables::open(&url, scratch.schema(), program.tables);
    assert!(
        matches!(result,Err(pw_table_postgres::Error::Schema(ref why)) if why.contains("asset")),
        "asset declarations need their actual storage binding"
    );
}
