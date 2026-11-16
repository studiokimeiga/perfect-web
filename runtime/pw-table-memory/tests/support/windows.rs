
fn window_program() -> Program { window_program_kind(false) }

fn window_program_kind(latest_predicate: bool) -> Program {
    let notes = std::fs::read_to_string(root().join("examples/owned/Notes.pw")).unwrap()
        .replace("index   by_to on to", "index   by_to on to\n    index   by_to_added on to, added\n    index   by_author_to on author, to");
    let mut app = std::fs::read_to_string(root().join("examples/owned/app.pw")).unwrap();
    if latest_predicate {
        let old = "    match Notes.get(current_user(), id) {\n        Some(_) => true,\n        None => false,\n    }";
        assert!(app.contains(old));
        app = app.replace("import Notes\n", "import Notes\nimport List\n")
            .replace(old,"    List.any(Notes.last_by_author(current_user(), 1), note => note.id == id)");
    }
    for (name, params, returns, keys, invalidates, body) in [
        ("FirstMine", "reader: User<UserId>, n: Int", "List<Note>", "reader, n", "NotesChanged(reader)", "Notes.first_by_author(reader, n)"),
        ("LastMine", "reader: User<UserId>, n: Int", "List<Note>", "reader, n", "NotesChanged(reader)", "Notes.last_by_author(reader, n)"),
        ("FirstRecipients", "reader: User<UserId>, recipients: List<UserId>, n: Int", "List<Note>", "reader, recipients, n", "NotesChanged(reader)", "Notes.first_by_author_to_many(reader, recipients, n)"),
        ("LastTimes", "reader: User<UserId>, times: List<Int>, n: Int", "List<NoteSeen>", "reader, times, n", "NoteSent(reader), NotesForgotten(_)", "Notes.last_by_to_added_many(reader, times, n)"),
    ] {
        app.push_str(&format!("\nuser query {name}({params}) -> {returns}\n    freshness 0.seconds\n    consistency read_your_writes\n    cache private\n    key {keys}\n    invalidates_on {invalidates}\n    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{{\n    {body}\n}}\n"));
    }
    built(&[("Notes.pw", &notes), ("app.pw", &app)])
}

fn window_seed(tables: &MemoryTables) {
    tables.transaction(|tx| {
        for (author,id,to,stamp) in [(ALICE,"n1",BOB,1),(ALICE,"n2",BOB,1),(ALICE,"n3",CAROL,5),(CAROL,"n4",BOB,1)] {
            tx.write("pw:tables/notes#insert", &[note_json(author,id,to,stamp)])?;
        }
        Ok(())
    }).assert_ok("window seed");
}
