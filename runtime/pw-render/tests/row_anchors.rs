//! ADR-0438: a planned row that is one element is its own anchor.
//!
//! A keyed list's row writes its identity between two comments,
//! `<!--pw:s3@T-->` the row `<!--pw:e3@T-->`, unless its list says its row is
//! its own anchor (`row_anchor: element`, which a build's packaging writes
//! where it read the row as one element a parser keeps whole): then the row's
//! element carries it, `<li data-pw-row="3@T">`, two nodes fewer a row. The
//! browser's renderer writes the same, held to this one by
//! `renderer-agreement.test.mjs`.

use pw_render::*;
use std::collections::BTreeMap;

fn record(fields: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in fields {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Record(m)
}

fn t(chunks: Vec<Chunk>) -> Template {
    Template {
        path: "t.T".into(),
        name: "T".into(),
        params: vec![],
        schema: "test".into(),
        chunks,
    }
}

fn st(s: &str) -> Chunk {
    Chunk::Static(s.to_string())
}

/// `{#each items as item (item.id)}BODY{/each}`, its rows anchored as `anchor` says.
fn list(id: u32, key: Option<&str>, anchor: RowAnchor, body: Vec<Chunk>) -> Part {
    Part::Each {
        id: PartId(id),
        collection: "items".into(),
        binding: "item".into(),
        key: key.map(str::to_string),
        row_anchor: anchor,
        body,
    }
}

/// `<li class={item.name}>{item.name}</li>`, as the compiler writes it.
fn row() -> Vec<Chunk> {
    vec![
        st("<li data-pw=\"0\" "),
        Chunk::Dynamic(Part::Attribute {
            id: PartId(1),
            owner: ElementId(0),
            name: "class".into(),
            value: "item.name".into(),
            context: Context::Attribute,
        }),
        st(">"),
        Chunk::Dynamic(Part::Text {
            id: PartId(2),
            value: "item.name".into(),
            context: Context::Text,
        }),
        st("</li>"),
    ]
}

fn items() -> Value {
    Value::List(vec![
        record(&[("id", Value::Int(1)), ("name", Value::Text("a".into()))]),
        record(&[("id", Value::Int(2)), ("name", Value::Text("b".into()))]),
    ])
}

/// Each `@token` numbered by when it is first seen: a host's token is 16
/// characters of base64url.
fn numbered(html: &str) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut out = String::new();
    let mut rest = html;
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..=at]);
        let after = &rest[at + 1..];
        let end = 16.min(after.len());
        let token = &after[..end];
        let n = seen.iter().position(|s| s == token).unwrap_or_else(|| {
            seen.push(token.to_string());
            seen.len() - 1
        });
        out.push_str(&n.to_string());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[test]
fn a_row_that_is_its_own_anchor_carries_its_identity_on_its_element() {
    let env = Env::new().set("items", items());
    let comments = render(
        &t(vec![Chunk::Dynamic(list(
            3,
            Some("id"),
            RowAnchor::Comments,
            row(),
        ))]),
        &env,
        &[],
    )
    .unwrap();
    let element = render(
        &t(vec![Chunk::Dynamic(list(
            3,
            Some("id"),
            RowAnchor::Element,
            row(),
        ))]),
        &env,
        &[],
    )
    .unwrap();
    assert_eq!(
        numbered(&comments),
        "<!--pw:s3-->\
         <!--pw:s3@0--><li data-pw=\"0\" class=\"a\"><!--pw:s2-->a<!--pw:e2--></li><!--pw:e3@0-->\
         <!--pw:s3@1--><li data-pw=\"0\" class=\"b\"><!--pw:s2-->b<!--pw:e2--></li><!--pw:e3@1-->\
         <!--pw:e3-->"
    );
    assert_eq!(
        numbered(&element),
        "<!--pw:s3-->\
         <li data-pw-row=\"3@0\" data-pw=\"0\" class=\"a\"><!--pw:s2-->a<!--pw:e2--></li>\
         <li data-pw-row=\"3@1\" data-pw=\"0\" class=\"b\"><!--pw:s2-->b<!--pw:e2--></li>\
         <!--pw:e3-->"
    );
    // The same token each way: the identity is the row's, wherever written.
    let token = |html: &str, at: &str| {
        let from = html.find(at).expect("a row") + at.len();
        html[from..from + 16].to_string()
    };
    assert_eq!(
        token(&comments, "<!--pw:s3@"),
        token(&element, "data-pw-row=\"3@")
    );
}

#[test]
fn an_instance_of_a_row_that_is_its_own_anchor_is_its_element() {
    // What a patch inserts is the render's row, byte for byte (ADR-0168).
    let item = record(&[("id", Value::Int(7)), ("name", Value::Text("x".into()))]);
    let env = Env::new().set("items", Value::List(vec![item.clone()]));
    let tpl = t(vec![Chunk::Dynamic(list(
        3,
        Some("id"),
        RowAnchor::Element,
        row(),
    ))]);
    let one = render_instance(&tpl, PartId(3), &item, &env, &[]).unwrap();
    assert!(one.starts_with("<li data-pw-row=\"3@"), "{one}");
    assert!(!one.contains("<!--pw:s3@"), "{one}");
    let whole = render(&tpl, &env, &[]).unwrap();
    assert_eq!(whole, format!("<!--pw:s3-->{one}<!--pw:e3-->"));
}

#[test]
fn an_elements_name_ends_where_html_reads_it_ending() {
    // After the name, wherever the tokenizer ends it: a space, a `/`, its
    // `>`, or the end of the markup before a hole.
    let env = Env::new().set("items", Value::List(vec![record(&[("id", Value::Int(1))])]));
    for (first, written) in [
        ("<li>x</li>", "<li data-pw-row=\"3@0\">x</li>"),
        (
            "<tr\tclass=\"r\"><td>x</td></tr>",
            "<tr data-pw-row=\"3@0\"\tclass=\"r\"><td>x</td></tr>",
        ),
        ("<img/>", "<img data-pw-row=\"3@0\"/>"),
        (
            "<my-row>x</my-row>",
            "<my-row data-pw-row=\"3@0\">x</my-row>",
        ),
        ("<LI>x</LI>", "<LI data-pw-row=\"3@0\">x</LI>"),
    ] {
        let tpl = t(vec![Chunk::Dynamic(list(
            3,
            Some("id"),
            RowAnchor::Element,
            vec![st(first)],
        ))]);
        let html = render(&tpl, &env, &[]).unwrap();
        assert_eq!(
            numbered(&html),
            format!("<!--pw:s3-->{written}<!--pw:e3-->"),
            "{first}"
        );
    }
    // A name the markup before a hole ends.
    let body = vec![
        st("<li"),
        Chunk::Dynamic(Part::BooleanAttribute {
            id: PartId(1),
            owner: ElementId(0),
            name: "hidden".into(),
            value: "on".into(),
        }),
        st(">x</li>"),
    ];
    let env = Env::new()
        .set("items", Value::List(vec![record(&[("id", Value::Int(1))])]))
        .set("on", Value::Bool(false));
    let tpl = t(vec![Chunk::Dynamic(list(
        3,
        Some("id"),
        RowAnchor::Element,
        body,
    ))]);
    assert_eq!(
        numbered(&render(&tpl, &env, &[]).unwrap()),
        "<!--pw:s3--><li data-pw-row=\"3@0\">x</li><!--pw:e3-->"
    );
}

#[test]
fn a_row_that_is_its_own_anchor_and_begins_with_no_element_is_refused() {
    // What its build read as one element begins with one: a row that does
    // not is refused where it is written, never written without its
    // identity.
    let env = Env::new().set("items", Value::List(vec![record(&[("id", Value::Int(1))])]));
    for body in [
        vec![st(" <li>x</li>")],
        vec![st("x")],
        vec![st("<!-- a --><li>x</li>")],
        vec![st("</li>")],
        vec![st("<1>")],
        vec![],
        vec![Chunk::Dynamic(Part::Text {
            id: PartId(2),
            value: "item.id".into(),
            context: Context::Text,
        })],
    ] {
        let tpl = t(vec![Chunk::Dynamic(list(
            3,
            Some("id"),
            RowAnchor::Element,
            body.clone(),
        ))]);
        assert!(
            matches!(
                render(&tpl, &env, &[]),
                Err(Blocked::UnrepresentedConstruct { .. })
            ),
            "{body:?}"
        );
        let item = record(&[("id", Value::Int(1))]);
        assert!(matches!(
            render_instance(&tpl, PartId(3), &item, &env, &[]),
            Err(Blocked::UnrepresentedConstruct { .. })
        ));
    }
}

#[test]
fn an_unkeyed_list_writes_no_identity_however_its_rows_are_anchored() {
    let env = Env::new().set("items", Value::List(vec![Value::Int(1), Value::Int(2)]));
    let tpl = t(vec![Chunk::Dynamic(list(
        3,
        None,
        RowAnchor::Element,
        vec![st("<li>x</li>")],
    ))]);
    assert_eq!(
        render(&tpl, &env, &[]).unwrap(),
        "<!--pw:s3--><li>x</li><li>x</li><!--pw:e3-->"
    );
}

#[test]
fn a_list_says_its_rows_anchor_only_where_it_is_its_element() {
    // The compiler writes none, and the IR it writes reads as it did: the
    // comments are what a list's rows have unless its build said otherwise.
    let comments = list(3, Some("id"), RowAnchor::Comments, vec![st("<li>x</li>")]);
    let json = serde_json::to_value(&comments).unwrap();
    assert!(json.get("row_anchor").is_none(), "{json}");
    let element = list(3, Some("id"), RowAnchor::Element, vec![st("<li>x</li>")]);
    let json = serde_json::to_value(&element).unwrap();
    assert_eq!(json["row_anchor"], "element");
    let read: Part = serde_json::from_value(json).unwrap();
    assert_eq!(read, element);
    let unsaid: Part = serde_json::from_str(
        r#"{"part":"each","id":3,"collection":"items","binding":"item","key":"id","body":[]}"#,
    )
    .unwrap();
    assert!(matches!(
        unsaid,
        Part::Each {
            row_anchor: RowAnchor::Comments,
            ..
        }
    ));
}
