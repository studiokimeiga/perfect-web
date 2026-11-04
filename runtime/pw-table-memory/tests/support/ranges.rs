
fn range_program(predicate: bool) -> Program { range_program_kind(predicate,None) }

fn range_program_order(kind: &str) -> Program { range_program_kind(false,Some(kind)) }

fn range_program_owner(owner: &str) -> Program { range_program_config(false,None,Some(owner)) }

fn range_program_kind(predicate: bool, kind: Option<&str>) -> Program { range_program_config(predicate,kind,None) }

fn range_program_config(predicate: bool, kind: Option<&str>, owner: Option<&str>) -> Program {
    let mut notes = std::fs::read_to_string(root().join("examples/owned/Notes.pw")).unwrap()
        .replace("index   by_to on to", "index   by_to on to\n    index   by_author_to on author, to\n    index   by_to_added on to, added");
    let mut app = std::fs::read_to_string(root().join("examples/owned/app.pw")).unwrap();
    app=app.replace("import Notes\n","import Notes\nimport List\nimport Notes.{ NotesToAfter }\n");
    let request_mark=if kind.is_some() {"Mark"} else {"Int"};
    app.push_str(&format!("\ntype RangeRequest = RangeRequest {{ to: UserId, mark: {request_mark} }}\n"));
    if let Some(kind) = kind {
        let (ty,initial) = match kind {
            "refined" => ("Int where value >= 0","0"),
            "string" => ("String where String.length(value) >= 1","\"a\""),
            "bool" => ("Bool","false"),
            _ => panic!("unknown order fixture {kind}"),
        };
        notes = notes.replace("opaque type NoteId = String",&format!("opaque type NoteId = String\nopaque type Mark = {ty}"))
            .replace("added: Int","added: Mark");
        app = app.replace("import Notes\n","import Notes\nimport Notes.{ Mark }\n")
            .replace("added: 0",&format!("added: Mark({initial})"));
    }
    if predicate {
        let old = "    match Notes.get(current_user(), id) {\n        Some(_) => true,\n        None => false,\n    }";
        assert!(app.contains(old));
        app = app.replace(old, "    match Notes.get(current_user(), id) {\n        Some(note) => Notes.count_after_by_author(current_user(), note.added) > 0,\n        None => false,\n    }");
    }
    for (name, params, returns, keys, invalidates, body) in [
        ("MaybeSince", "reader: User<UserId>, mark: Int, run: Bool", "Int", "reader, mark, run", "NotesChanged(reader)", "if run { Notes.count_after_by_author(reader, mark) } else { 0 }"),
        ("RangeFromRequests", "reader: User<UserId>, requests: List<RangeRequest>", "List<NotesAuthorToAfterCount>", "reader, requests", "NotesChanged(reader)", "let ranges = List.map(requests, request => NotesToAfter { to: request.to, __range_mark: request.mark })\n    Notes.count_after_by_author_to_many(reader, ranges)"),
        ("SinceMine", "reader: User<UserId>, mark: Int", "Int", "reader, mark", "NotesChanged(reader)", "Notes.count_after_by_author(reader, mark)"),
        ("SinceShared", "reader: User<UserId>, mark: Int", "Int", "reader, mark", "NoteSent(reader), NotesForgotten(_)", "Notes.count_after_by_to(reader, mark)"),
        ("RangeMarks", "reader: User<UserId>, ranges: List<NotesAfter>", "List<NotesAuthorAfterCount>", "reader, ranges", "NotesChanged(reader)", "Notes.count_after_by_author_many(reader, ranges)"),
        ("RangeRecipients", "reader: User<UserId>, ranges: List<NotesToAfter>", "List<NotesAuthorToAfterCount>", "reader, ranges", "NotesChanged(reader)", "Notes.count_after_by_author_to_many(reader, ranges)"),
        ("RangeTimes", "reader: User<UserId>, ranges: List<NotesAddedAfter>", "List<NotesToAddedAfterCount>", "reader, ranges", "NoteSent(reader), NotesForgotten(_)", "Notes.count_after_by_to_added_many(reader, ranges)"),
    ] {
        let body=body.replace("__range_mark",if owner==Some("after") {"after_mark"} else {"after"});
        let params = if kind.is_some() { params.replace("mark: Int","mark: Mark") } else { params.to_string() };
        app.push_str(&format!("\nuser query {name}({params}) -> {returns}\n    freshness 0.seconds\n    consistency read_your_writes\n    cache private\n    key {keys}\n    invalidates_on {invalidates}\n    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{{\n    {body}\n}}\n"));
    }
    if let Some(owner)=owner {
        notes=notes.replace("author",owner);
        let pascal=owner.split('_').map(|part|format!("{}{}",part[..1].to_uppercase(),&part[1..])).collect::<String>();
        app=app.replace("author",owner).replace("Author",&pascal);
    }
    built(&[("Notes.pw", &notes), ("app.pw", &app)])
}

fn range_rows() -> Vec<serde_json::Value> {
    [(ALICE,"n1",BOB,1),(ALICE,"n2",BOB,1),(ALICE,"n3",CAROL,5),(CAROL,"n4",BOB,1)]
        .into_iter().map(|(author,id,to,stamp)| note_json(author,id,to,stamp)).collect()
}

fn requested(fields: Vec<(&str, Val)>, mark: i64) -> Val {
    let mut fields = fields.into_iter().map(|(name,value)| (name.to_string(),value)).collect::<Vec<_>>();
    fields.push(("after".into(),Val::S64(mark)));
    Val::Record(fields)
}

fn counted_range(fields: Vec<(&str, Val)>, mark: i64, count: i64) -> Val {
    Val::Record(vec![("key".into(),requested(fields,mark)),("count".into(),Val::S64(count))])
}


fn requested_order(fields: Vec<(&str,Val)>, mark: Val) -> Val {
    let mut fields = fields.into_iter().map(|(name,value)|(name.to_string(),value)).collect::<Vec<_>>();
    fields.push(("after".into(),mark));
    Val::Record(fields)
}

fn counted_order(fields: Vec<(&str,Val)>, mark: Val, count: i64) -> Val {
    Val::Record(vec![("key".into(),requested_order(fields,mark)),("count".into(),Val::S64(count))])
}

fn order_value(value: &serde_json::Value) -> Val {
    match value {
        serde_json::Value::Bool(value)=>Val::Bool(*value),
        serde_json::Value::String(value)=>Val::String(value.clone()),
        serde_json::Value::Number(value)=>Val::S64(value.as_i64().unwrap()),
        _=>panic!("non-scalar order fixture {value:?}"),
    }
}

type OrderCase = (&'static str,Vec<serde_json::Value>,Vec<(serde_json::Value,i64)>,Val);

fn order_cases() -> Vec<OrderCase> {
    vec![
        ("refined",vec![json!(0),json!(1),json!(1),json!(5),json!(9_007_199_254_740_993i64)],vec![(json!(0),4),(json!(1),2),(json!(5),1),(json!(9_007_199_254_740_992i64),1),(json!(9_007_199_254_740_993i64),0)],Val::S64(-1)),
        ("string",vec![json!("\0"),json!("a"),json!("a\0"),json!("z"),json!("é")],vec![(json!("\0"),4),(json!("a"),3),(json!("z"),1),(json!("é"),0)],Val::String("".into())),
        ("bool",vec![json!(false),json!(true),json!(false)],vec![(json!(false),1),(json!(true),0)],Val::S64(0)),
    ]
}
