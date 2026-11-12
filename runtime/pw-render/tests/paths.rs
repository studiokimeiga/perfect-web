//! **A path is read field by field, from the longest name bound**
//! (ADR-0169).
//!
//! A row's `{item.price.display}` is `item`'s `price`'s `display`: the value a
//! host computes for the row is set in it, and the renderer reads it there.
//! Until 2026-10-03 the renderer read one field after a name, so a path two
//! fields deep named nothing, and a row could show no member's value.
//!
//! A value computed for a row is set in it whole, by its path from the row,
//! and read whole at each record before a field is (ADR-0170): `quantity` is
//! a number, which has no field to hold `count`.

use pw_render::*;
use std::collections::BTreeMap;

fn record(fields: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in fields {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Record(m)
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

/// `{#each menu as item (item.id)}<li>{item.price.display}</li>{/each}`,
/// and `{total.display}`.
fn page() -> Template {
    Template {
        path: "t.T".into(),
        name: "T".into(),
        params: vec![],
        schema: "test".into(),
        chunks: vec![
            Chunk::Static("<ul>".into()),
            Chunk::Dynamic(Part::Each {
                id: PartId(0),
                collection: "menu".into(),
                binding: "item".into(),
                key: Some("id".into()),
                row_anchor: Default::default(),
                body: vec![
                    Chunk::Static("<li>".into()),
                    Chunk::Dynamic(Part::Text {
                        id: PartId(1),
                        value: "item.price.display".into(),
                        context: Context::Text,
                    }),
                    Chunk::Static("</li>".into()),
                ],
            }),
            Chunk::Static("</ul><p>".into()),
            Chunk::Dynamic(Part::Text {
                id: PartId(2),
                value: "total.display".into(),
                context: Context::Text,
            }),
            Chunk::Static("</p>".into()),
        ],
    }
}

fn item(id: &str, display: &str) -> Value {
    record(&[
        ("id", text(id)),
        (
            "price",
            record(&[("minor_units", Value::Int(350)), ("display", text(display))]),
        ),
    ])
}

fn shown(html: &str) -> String {
    // The text between tags, without the runtime's comment markers.
    let mut out = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        out.push(' ');
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn a_row_reads_a_value_two_fields_deep_in_its_item() {
    let env = Env::new()
        .set(
            "menu",
            Value::List(vec![item("espresso", "$3.50"), item("cortado", "$4.25")]),
        )
        .set("total", record(&[("display", text("$7.75"))]));
    let html = render(&page(), &env, &[]).expect("renders");
    assert_eq!(shown(&html), "$3.50 $4.25 $7.75");
}

#[test]
fn a_path_set_whole_is_read_before_the_name_it_begins_with() {
    // A host sets a top-level part's value by its path (ADR-0125).
    let env = Env::new()
        .set("menu", Value::List(vec![]))
        .set("total", record(&[("display", text("from the record"))]))
        .set("total.display", text("as computed"));
    let html = render(&page(), &env, &[]).expect("renders");
    assert_eq!(shown(&html), "as computed");
}

#[test]
fn a_field_the_value_does_not_have_is_missing_not_empty() {
    let env = Env::new()
        .set(
            "menu",
            Value::List(vec![record(&[
                ("id", text("espresso")),
                ("price", record(&[("minor_units", Value::Int(350))])),
            ])]),
        )
        .set("total", record(&[("display", text("$3.50"))]));
    assert_eq!(
        render(&page(), &env, &[]),
        Err(Blocked::MissingValue {
            path: "item.price.display".into()
        })
    );
}

#[test]
fn a_row_whose_computed_value_changed_sets_its_text_where_it_is() {
    assert_eq!(
        instance_changes(
            &page(),
            PartId(0),
            &item("espresso", "$3.50"),
            &item("espresso", "$3.75"),
            &Env::new(),
            &[]
        )
        .expect("derived"),
        Some(vec![(PartId(1), InstanceChange::Text("$3.75".into()))])
    );
}

/// `{#each cart.lines as line (line.id)}<li>{line.quantity}
/// {line.quantity.count}</li>{/each}`: a member of a number.
fn lines() -> Template {
    let text = |id: u32, value: &str| {
        Chunk::Dynamic(Part::Text {
            id: PartId(id),
            value: value.into(),
            context: Context::Text,
        })
    };
    Template {
        path: "t.L".into(),
        name: "L".into(),
        params: vec![],
        schema: "test".into(),
        chunks: vec![Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "cart.lines".into(),
            binding: "line".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![
                Chunk::Static("<li>".into()),
                text(1, "line.quantity"),
                Chunk::Static(" ".into()),
                text(2, "line.quantity.count"),
                Chunk::Static("</li>".into()),
            ],
        })],
    }
}

/// A line whose `quantity` is a number, with what a host computed of it set
/// whole, by its path from the line (ADR-0170).
fn line(id: &str, quantity: i64, count: &str) -> Value {
    record(&[
        ("id", text(id)),
        ("quantity", Value::Int(quantity)),
        ("quantity.count", text(count)),
    ])
}

#[test]
fn a_value_computed_for_a_row_is_read_whole_where_a_field_has_none() {
    // `quantity` is a number, with no field to hold `count`: the row holds
    // it at its whole path, and the number is still the number.
    let env = Env::new().set(
        "cart",
        record(&[("lines", Value::List(vec![line("a", 2, "two")]))]),
    );
    let html = render(&lines(), &env, &[]).expect("renders");
    assert_eq!(shown(&html), "2 two");
}

#[test]
fn a_value_computed_for_a_row_that_changed_is_set_where_it_is() {
    assert_eq!(
        instance_changes(
            &lines(),
            PartId(0),
            &line("a", 2, "two"),
            &line("a", 3, "three"),
            &Env::new(),
            &[]
        )
        .expect("derived"),
        Some(vec![
            (PartId(1), InstanceChange::Text("3".into())),
            (PartId(2), InstanceChange::Text("three".into())),
        ])
    );
}
