//! **A loop's key is a path from its element** (ADR-0073).
//!
//! The compiler keys `{#each ks as k (k.r.id)}` on the path `r.id`, and the
//! renderer follows it: when it renders the loop, when it renders one inserted
//! instance, and when it derives the token a patch addresses. Until 2026-09-26
//! the compiler kept the key's last segment, `id`, and the renderer read one
//! field, so a nested key keyed on the element's own `id`, silently.

use pw_render::*;
use std::collections::BTreeMap;

fn record(fields: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in fields {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Record(m)
}

fn keyed(key: &str) -> Template {
    Template {
        path: "t.T".into(),
        name: "T".into(),
        params: vec![],
        schema: "test".into(),
        chunks: vec![
            Chunk::Static("<ul>".into()),
            Chunk::Dynamic(Part::Each {
                id: PartId(0),
                collection: "items".into(),
                binding: "item".into(),
                key: Some(key.into()),
                row_anchor: Default::default(),
                body: vec![Chunk::Static("<li>x</li>".into())],
            }),
            Chunk::Static("</ul>".into()),
        ],
    }
}

#[test]
fn a_nested_key_is_followed() {
    // Two elements whose own `id` is one, and whose `r.id` differs.
    let items = vec![
        record(&[
            ("id", Value::Int(1)),
            ("r", record(&[("id", Value::Text("a".into()))])),
        ]),
        record(&[
            ("id", Value::Int(1)),
            ("r", record(&[("id", Value::Text("b".into()))])),
        ]),
    ];
    let env = Env::new().set("items", Value::List(items.clone()));
    let html = render(&keyed("r.id"), &env, &[]).expect("keyed on `r.id`");
    for item in &items {
        // The token a patch derives is the one the render wrote, and an
        // inserted instance is rendered under it.
        let token = instance_token_of(PartId(0), item, "r.id", &env);
        assert!(html.contains(&format!("@{token}-->")), "{token}: {html}");
        let one = render_instance(&keyed("r.id"), PartId(0), item, &env, &[])
            .expect("one instance, keyed on `r.id`");
        assert!(one.contains(&format!("@{token}-->")), "{token}: {one}");
    }
    // Keyed on the element's own `id`, as the last segment was, the two are
    // one key.
    assert!(
        matches!(
            render(&keyed("id"), &env, &[]),
            Err(Blocked::DuplicateLoopKey { .. })
        ),
        "the element's own `id` is not its key"
    );
}

#[test]
fn an_empty_key_is_the_element() {
    let items = vec![Value::Text("a".into()), Value::Text("b".into())];
    let env = Env::new().set("items", Value::List(items.clone()));
    let html = render(&keyed(""), &env, &[]).expect("keyed on the element");
    for item in &items {
        let token = instance_token_of(PartId(0), item, "", &env);
        assert!(html.contains(&format!("@{token}-->")), "{token}: {html}");
    }
    // A key the element does not have is refused, not read as the element.
    assert!(matches!(
        render(&keyed("id"), &env, &[]),
        Err(Blocked::MissingLoopKey { .. })
    ));
}
