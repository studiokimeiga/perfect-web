//! Compiled count WIT, ownership and sharer contracts, on the reference adapter.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/counts.rs");
#[test]
fn counts_use_the_same_compiled_owner_and_sharer_boundary() {
    let program = count_program();
    let tables = seeded(&program);
    assert_compiled_counts(&program, &tables);
}

#[test]
fn a_forged_count_set_with_no_field_after_its_reader_is_refused() {
    let program = count_program();
    let mut raw = serde_json::to_value(&program.tables).unwrap();
    let functions = raw["tables"][0]["functions"].as_array_mut().unwrap();
    let mut forged = functions
        .iter()
        .find(|f| f["name"] == "count_by_author")
        .unwrap()
        .clone();
    forged["name"] = json!("count_by_author_many");
    forged["import"] = json!("pw:tables/notes#count-by-author-many");
    forged["primitive"] = json!("count-many");
    forged["sorted_by"] = json!(["author"]);
    forged["returns"] = json!("List<NotesAuthorCount>");
    functions.push(forged);
    let denied = pw_host::tables::Manifest::from_json(&raw.to_string());
    assert!(
        denied
            .as_ref()
            .is_err_and(|why| why.contains("no selector after its reader")),
        "non-generated owned count form must be refused: {denied:?}"
    );
}
