
// Real compiled variants: source order AB/A_Z differs from wire tag ordering.
fn case_program()->Program {
    let mut app=String::from("module owned.app\nimport Tags\n");
    for (name,params,ret,key,body) in [
        ("Every","","List<Tags.Item>","","Tags.all()"),
        ("First","lane: Bool, n: Int","List<Tags.Item>","lane, n","Tags.first_by_lane(lane,n)"),
        ("Last","lane: Bool, n: Int","List<Tags.Item>","lane, n","Tags.last_by_lane(lane,n)"),
        ("Many","keys: List<Tags.TagsKindId>","List<Tags.Item>","keys","Tags.get_many(keys)"),
        ("By","kind: Tags.Kind","List<Tags.Item>","kind","Tags.of_kind(kind)"),
        ("Counts","kinds: List<Tags.Kind>","List<Tags.TagsKindCount>","kinds","Tags.count_by_kind_many(kinds)"),
        ("Since","lane: Bool, mark: Tags.Kind","Int","lane, mark","Tags.count_after_by_lane(lane,mark)"),
        ("Ranges","requests: List<Tags.TagsLaneAfter>","List<Tags.TagsLaneAfterCount>","requests","Tags.count_after_by_lane_many(requests)"),
    ] {
        app.push_str(&format!("\npublic query {name}({params}) -> {ret}\n    freshness 0.seconds\n    consistency snapshot\n    cache shared\n{}    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{{\n    {body}\n}}\n",if key.is_empty(){String::new()}else{format!("    key {key}\n")}));
    }
    app.push_str("\ncommand Save(row: Tags.Item) -> ()\n    transaction serializable\n    invalidates Every(), First(_, _), Last(_, _), Many(_), By(_), Counts(_), Since(_, _), Ranges(_)\n{ Tags.put(row) }\n");
    case_built_from(&[("Tags.pw",r#"module Tags
type Kind = AB | A_Z
type Detail = Detail { kind: Kind, history: List<Kind>, maybe: Option<Kind> }
type Item = Item { kind: Kind, id: Int, lane: Bool, detail: Detail }
table Tags
    row Item
    key kind, id
    order kind
    index by_lane on lane
    bounded 100
"#),("TagData.pw",r#"module TagData
import Tags
source TagData
    holds Tags
    transactions serializable
    reads strong
    changes none
    sets one_call
"#),("app.pw",&app)])
}
fn case_rows()->Vec<serde_json::Value> {
    [("ab",1,false),("a-z",3,false),("a-z",2,false),("ab",4,true)].into_iter().map(|(kind,id,lane)|
        json!({"kind":kind,"id":id,"lane":lane,"detail":{"kind":kind,"history":["ab","a-z"],"maybe":"a-z"}})).collect()
}
fn tag(s:&str)->Val { Val::Variant(s.into(),None) }
fn case_row(kind:&str,id:i64,lane:bool)->Val {
    Val::Record(vec![("kind".into(),tag(kind)),("id".into(),Val::S64(id)),("lane".into(),Val::Bool(lane)),
        ("detail".into(),Val::Record(vec![("kind".into(),tag(kind)),("history".into(),Val::List(vec![tag("ab"),tag("a-z")])),("maybe".into(),Val::Option(Some(Box::new(tag("a-z")))))]))])
}
fn case_key(kind:&str,id:i64)->Val {Val::Record(vec![("kind".into(),tag(kind)),("id".into(),Val::S64(id))])}
fn case_range(lane:bool,mark:&str)->Val {Val::Record(vec![("lane".into(),Val::Bool(lane)),("after".into(),tag(mark))])}
fn case_queries(mut call:impl FnMut(&str,&[Val])->Result<Vec<Val>,String>) {
    let rows=vec![case_row("a-z",2,false),case_row("a-z",3,false),case_row("ab",1,false),case_row("ab",4,true)];
    assert_eq!(call("Every",&[]),Ok(vec![Val::List(rows.clone())]),"wire tags, not source spelling or ordinal");
    assert_eq!(call("First",&[Val::Bool(false),Val::S64(2)]),Ok(vec![Val::List(rows[..2].to_vec())]));
    assert_eq!(call("Last",&[Val::Bool(false),Val::S64(2)]),Ok(vec![Val::List(vec![rows[2].clone(),rows[1].clone()])]));
    assert_eq!(call("Many",&[Val::List(vec![case_key("ab",1),case_key("a-z",3),case_key("ab",1),case_key("ab",99)])]),Ok(vec![Val::List(vec![rows[1].clone(),rows[2].clone()])]));
    assert_eq!(call("By",&[tag("a-z")]),Ok(vec![Val::List(rows[..2].to_vec())]));
    assert_eq!(call("Counts",&[Val::List(vec![tag("ab"),tag("a-z"),tag("ab")])]),Ok(vec![Val::List(vec![
        Val::Record(vec![("key".into(),tag("a-z")),("count".into(),Val::S64(2))]),
        Val::Record(vec![("key".into(),tag("ab")),("count".into(),Val::S64(2))]),
    ])]));
    assert_eq!(call("Since",&[Val::Bool(false),tag("a-z")]),Ok(vec![Val::S64(1)]));
    assert_eq!(call("Since",&[Val::Bool(false),tag("ab")]),Ok(vec![Val::S64(0)]));
    assert_eq!(call("Ranges",&[Val::List(vec![case_range(true,"a-z"),case_range(false,"ab"),case_range(false,"a-z"),case_range(false,"a-z")])]),Ok(vec![Val::List(vec![
        Val::Record(vec![("key".into(),case_range(false,"a-z")),("count".into(),Val::S64(1))]),
        Val::Record(vec![("key".into(),case_range(false,"ab")),("count".into(),Val::S64(0))]),
        Val::Record(vec![("key".into(),case_range(true,"a-z")),("count".into(),Val::S64(1))]),
    ])]));
}
fn malformed_tags()->Vec<Val>{vec![tag("missing"),tag("A_Z"),Val::String("a-z".into()),Val::Enum("a-z".into()),Val::Variant("a-z".into(),Some(Box::new(Val::S64(1))))]}
fn malformed_case_schemas(program:&Program)->Vec<pw_core::tables::TablesJson>{
    use pw_core::tables::{Stored,StoredCase};
    [vec![],vec![StoredCase{name:"".into(),carries:vec![]}],vec![StoredCase{name:"%%".into(),carries:vec![]}],vec![StoredCase{name:"AB".into(),carries:vec![Stored::Bool]}],vec![StoredCase{name:"A_Z".into(),carries:vec![]},StoredCase{name:"A__Z".into(),carries:vec![]}]].into_iter().map(|cases|{
        let mut tables=program.tables.clone();tables.tables[0].schema.fields[0].stored=Stored::Cases(cases);rehash_cases(&mut tables);tables
    }).collect()
}

fn case_built_from(files: &[(&str,&str)])->Program { built_from(files) }

fn case_wrong_abi_program()->Program {
    let mut program=case_program();
    let pw_core::tables::Stored::Cases(cases)=&mut program.tables.tables[0].schema.fields[0].stored else{panic!("case fixture")};
    cases.reverse();
    rehash_cases(&mut program.tables);
    program.manifest=pw_host::tables::Manifest::from_json(&serde_json::to_string(&program.tables).unwrap()).expect("well formed reordered metadata");
    program
}

fn rehash_cases(tables:&mut pw_core::tables::TablesJson){
    let schema=pw_core::tables::Schema::of(tables.tables.iter().map(|t|t.schema.clone()).collect());
    tables.hash=schema.hash.clone();tables.migration.to=schema.hash;
}

fn case_migration_program(add:bool,released:Option<&pw_core::tables::Schema>)->Program {
    let table=format!("module Evolving\ntype Kind = AB | A_Z\ntype Row = Row {{ id: Int{} }}\ntable Evolving\n    row Row\n    key id\n    bounded 100\n",if add{", maybe: Option<Kind>"}else{""});
    let source="module EvolvingData\nimport Evolving\nsource EvolvingData\n    holds Evolving\n    transactions serializable\n    reads strong\n    changes none\n    sets one_call\n";
    let app="module owned.app\nimport Evolving\npublic query Every() -> List<Evolving.Row>\n    freshness 0.seconds\n    consistency snapshot\n    cache shared\n    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{ Evolving.all() }\ncommand Save(row:Evolving.Row)->()\n    transaction serializable\n    invalidates Every()\n{ Evolving.put(row) }\n";
    let units=units_of(vec![("Evolving.pw".into(),table),("EvolvingData.pw".into(),source.into()),("app.pw".into(),app.into())]);
    let tables=pw_core::build::build_with(&units,released).expect("released case successor builds").tables;
    let mut program=from_units(units);program.tables=tables;program
}
