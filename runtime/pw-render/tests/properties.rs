//! E7 task 2 gates 2, 5 and 6 — dynamic values, determinism, and refusal.
//!
//! Each property is paired with a mutation that must break it, per the standing
//! discipline:
//!
//! > Every favorable observation needs a nearby implementation or mutation that
//! > makes the exact measurement go red.

use pw_render::*;
use std::collections::BTreeMap;

/// The document with its part anchors removed.
///
/// Used where the assertion is about CONTENT. The anchors have their own
/// tests: mixing the two would make every content expectation change whenever
/// an identity did, and then nobody would read them.
fn visible(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("<!--pw:") {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find("-->") else {
            break;
        };
        rest = &rest[i + j + 3..];
    }
    out.push_str(rest);
    out
}

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

// --- gate 2: dynamic values render correctly -----------------------------

#[test]
fn a_text_expression_renders_its_value() {
    let tpl = t(vec![
        Chunk::Static("<h1>".into()),
        Chunk::Dynamic(Part::Text {
            id: PartId(0),
            value: "name".into(),
            context: Context::Text,
        }),
        Chunk::Static("</h1>".into()),
    ]);
    let env = Env::new().set("name", Value::Text("Blue Bottle".into()));
    assert_eq!(
        visible(&render(&tpl, &env, &[]).unwrap()),
        "<h1>Blue Bottle</h1>"
    );
}

#[test]
fn an_attribute_renders_as_a_quoted_value() {
    let tpl = t(vec![
        Chunk::Static("<div ".into()),
        Chunk::Dynamic(Part::Attribute {
            id: PartId(0),
            owner: ElementId(0),
            name: "class".into(),
            value: "kind".into(),
            context: Context::Attribute,
        }),
        Chunk::Static("></div>".into()),
    ]);
    let env = Env::new().set("kind", Value::Text("card wide".into()));
    assert_eq!(
        visible(&render(&tpl, &env, &[]).unwrap()),
        "<div class=\"card wide\"></div>"
    );
}

#[test]
fn a_false_boolean_attribute_is_absent_not_empty() {
    // `disabled=""` is disabled. A renderer that emitted the attribute with an
    // empty value would produce a control nobody can use, and the markup would
    // look almost right.
    let tpl = t(vec![
        Chunk::Static("<input ".into()),
        Chunk::Dynamic(Part::BooleanAttribute {
            id: PartId(0),
            owner: ElementId(0),
            name: "disabled".into(),
            value: "locked".into(),
        }),
        Chunk::Static(">".into()),
    ]);
    let off = Env::new().set("locked", Value::Bool(false));
    assert_eq!(visible(&render(&tpl, &off, &[]).unwrap()), "<input>");

    let on = Env::new().set("locked", Value::Bool(true));
    assert_eq!(
        visible(&render(&tpl, &on, &[]).unwrap()),
        "<input disabled>"
    );
}

#[test]
fn a_conditional_region_renders_one_branch() {
    let tpl = t(vec![Chunk::Dynamic(Part::Conditional {
        id: PartId(0),
        value: "signed_in".into(),
        then: vec![Chunk::Static("<p>welcome</p>".into())],
        otherwise: vec![Chunk::Static("<p>sign in</p>".into())],
    })]);
    let yes = Env::new().set("signed_in", Value::Bool(true));
    let no = Env::new().set("signed_in", Value::Bool(false));
    assert_eq!(visible(&render(&tpl, &yes, &[]).unwrap()), "<p>welcome</p>");
    assert_eq!(visible(&render(&tpl, &no, &[]).unwrap()), "<p>sign in</p>");
}

#[test]
fn a_missing_else_renders_nothing_rather_than_failing() {
    let tpl = t(vec![Chunk::Dynamic(Part::Conditional {
        id: PartId(0),
        value: "show".into(),
        then: vec![Chunk::Static("<p>x</p>".into())],
        otherwise: vec![],
    })]);
    let no = Env::new().set("show", Value::Bool(false));
    assert_eq!(visible(&render(&tpl, &no, &[]).unwrap()), "");
}

#[test]
fn an_each_region_renders_once_per_element_with_the_binding_in_scope() {
    let tpl = t(vec![
        Chunk::Static("<ul>".into()),
        Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "items".into(),
            binding: "item".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![
                Chunk::Static("<li>".into()),
                Chunk::Dynamic(Part::Text {
                    id: PartId(0),
                    value: "item.name".into(),
                    context: Context::Text,
                }),
                Chunk::Static("</li>".into()),
            ],
        }),
        Chunk::Static("</ul>".into()),
    ]);
    let env = Env::new().set(
        "items",
        Value::List(vec![
            record(&[
                ("id", Value::Int(1)),
                ("name", Value::Text("Espresso".into())),
            ]),
            record(&[
                ("id", Value::Int(2)),
                ("name", Value::Text("Cortado".into())),
            ]),
        ]),
    );
    assert_eq!(
        visible(&render(&tpl, &env, &[]).unwrap()),
        "<ul><li>Espresso</li><li>Cortado</li></ul>"
    );
}

#[test]
fn an_empty_collection_renders_the_surrounding_markup_and_no_items() {
    let tpl = t(vec![
        Chunk::Static("<ul>".into()),
        Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "items".into(),
            binding: "item".into(),
            key: None,
            row_anchor: Default::default(),
            body: vec![Chunk::Static("<li>x</li>".into())],
        }),
        Chunk::Static("</ul>".into()),
    ]);
    let env = Env::new().set("items", Value::List(vec![]));
    assert_eq!(visible(&render(&tpl, &env, &[]).unwrap()), "<ul></ul>");
}

#[test]
fn a_nested_template_renders_in_place_with_its_own_arguments() {
    let child = Template {
        path: "t.Card".into(),
        name: "Card".into(),
        params: vec!["label".into()],
        schema: "test".into(),
        chunks: vec![
            Chunk::Static("<span>".into()),
            Chunk::Dynamic(Part::Text {
                id: PartId(0),
                value: "label".into(),
                context: Context::Text,
            }),
            Chunk::Static("</span>".into()),
        ],
    };
    let parent = t(vec![
        Chunk::Static("<div>".into()),
        Chunk::Dynamic(Part::Instance {
            id: PartId(0),
            path: "t.Card".into(),
            args: vec![("label".into(), "title".into())],
            elements: 1,
            deepest: 1,
        }),
        Chunk::Static("</div>".into()),
    ]);
    let env = Env::new().set("title", Value::Text("Menu".into()));
    assert_eq!(
        visible(&render(&parent, &env, std::slice::from_ref(&child)).unwrap()),
        "<div><span>Menu</span></div>"
    );

    // A child does NOT see the parent's other values. Scope is by argument, so
    // a template cannot depend on what happened to be in scope where it was
    // used — which is what makes it reusable and what makes its inputs
    // checkable.
    let leaky = Template {
        chunks: vec![Chunk::Dynamic(Part::Text {
            id: PartId(0),
            value: "title".into(),
            context: Context::Text,
        })],
        ..child
    };
    assert_eq!(
        render(&parent, &env, &[leaky]),
        Err(Blocked::MissingValue {
            path: "title".into()
        })
    );
}

// --- gate 5: deterministic output ----------------------------------------

#[test]
fn identical_ir_and_values_produce_identical_bytes() {
    let tpl = t(vec![
        Chunk::Static("<article ".into()),
        Chunk::Dynamic(Part::Attribute {
            id: PartId(0),
            owner: ElementId(0),
            name: "id".into(),
            value: "id".into(),
            context: Context::Attribute,
        }),
        Chunk::Static(" ".into()),
        Chunk::Dynamic(Part::Attribute {
            id: PartId(0),
            owner: ElementId(0),
            name: "class".into(),
            value: "kind".into(),
            context: Context::Attribute,
        }),
        Chunk::Static(">".into()),
        Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "items".into(),
            binding: "item".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![Chunk::Dynamic(Part::Text {
                id: PartId(0),
                value: "item.name".into(),
                context: Context::Text,
            })],
        }),
        Chunk::Static("</article>".into()),
    ]);
    let env = Env::new()
        .set("id", Value::Text("a".into()))
        .set("kind", Value::Text("card".into()))
        .set(
            "items",
            Value::List(vec![
                record(&[
                    ("id", Value::Text("a".into())),
                    ("name", Value::Text("x".into())),
                ]),
                record(&[
                    ("id", Value::Text("b".into())),
                    ("name", Value::Text("y".into())),
                ]),
            ]),
        );

    let first = render(&tpl, &env, &[]).unwrap();
    for _ in 0..64 {
        assert_eq!(render(&tpl, &env, &[]).unwrap(), first);
    }
    assert_eq!(
        visible(&first),
        "<article id=\"a\" class=\"card\">xy</article>"
    );
}

#[test]
fn the_determinism_check_can_fail() {
    // Architect ruling: "determinism test → deliberately randomize attribute
    // ordering → test must go red." Two IRs that differ only in attribute
    // ORDER must produce different bytes — which is what makes the assertion
    // above about the renderer rather than about the assertion.
    let attrs = |first: &str, second: &str| {
        t(vec![
            Chunk::Static("<div ".into()),
            Chunk::Dynamic(Part::Attribute {
                id: PartId(0),
                owner: ElementId(0),
                name: first.into(),
                value: "v".into(),
                context: Context::Attribute,
            }),
            Chunk::Static(" ".into()),
            Chunk::Dynamic(Part::Attribute {
                id: PartId(0),
                owner: ElementId(0),
                name: second.into(),
                value: "v".into(),
                context: Context::Attribute,
            }),
            Chunk::Static("></div>".into()),
        ])
    };
    let env = Env::new().set("v", Value::Text("x".into()));
    let a = render(&attrs("id", "class"), &env, &[]).unwrap();
    let b = render(&attrs("class", "id"), &env, &[]).unwrap();
    assert_ne!(
        a, b,
        "reordered attributes produced identical bytes, so the determinism \
         assertion is not observing order at all"
    );
}

// --- gate 6: no invalid state silently disappears ------------------------

#[test]
fn a_blocked_part_refuses_the_render() {
    let tpl = t(vec![
        Chunk::Static("<main>".into()),
        Chunk::Dynamic(Part::Blocked {
            reason: "the block directive `{#await x}` has no representation".into(),
            at: "{#await x}".into(),
        }),
        Chunk::Static("</main>".into()),
    ]);
    let r = render(&tpl, &Env::new(), &[]);
    assert!(
        matches!(r, Err(Blocked::UnrepresentedConstruct { .. })),
        "got {r:?}"
    );
}

#[test]
fn the_blocked_check_can_fail() {
    // Architect ruling: "blocked-state test → deliberately turn Blocked into
    // empty output → test must go red."
    //
    // The mutation is a renderer that skips what it does not understand, which
    // is the natural implementation and the one this design exists to reject.
    fn lenient(chunks: &[Chunk]) -> String {
        chunks
            .iter()
            .map(|c| match c {
                Chunk::Static(s) => s.clone(),
                Chunk::Dynamic(_) => String::new(),
            })
            .collect()
    }
    let tpl = t(vec![
        Chunk::Static("<main>".into()),
        Chunk::Dynamic(Part::Blocked {
            reason: "unrepresented".into(),
            at: "x".into(),
        }),
        Chunk::Static("</main>".into()),
    ]);
    assert_eq!(
        lenient(&tpl.chunks),
        "<main></main>",
        "the lenient renderer produces a page that LOOKS correct"
    );
    assert!(
        render(&tpl, &Env::new(), &[]).is_err(),
        "and the real one must refuse it"
    );
}

#[test]
fn a_missing_value_is_an_error_not_an_empty_string() {
    let tpl = t(vec![Chunk::Dynamic(Part::Text {
        id: PartId(0),
        value: "absent".into(),
        context: Context::Text,
    })]);
    assert_eq!(
        render(&tpl, &Env::new(), &[]),
        Err(Blocked::MissingValue {
            path: "absent".into()
        })
    );
}

#[test]
fn a_value_with_no_text_form_is_refused_rather_than_stringified() {
    // A list in text position has no rendering. `[object Object]` is what a
    // renderer produces when it decides to have an answer for everything.
    let tpl = t(vec![Chunk::Dynamic(Part::Text {
        id: PartId(0),
        value: "items".into(),
        context: Context::Text,
    })]);
    let env = Env::new().set("items", Value::List(vec![Value::Int(1)]));
    assert!(render(&tpl, &env, &[]).is_err());
}

#[test]
fn an_instance_of_an_unknown_template_is_refused_rather_than_skipped() {
    let tpl = t(vec![Chunk::Dynamic(Part::Instance {
        id: PartId(0),
        path: "t.Missing".into(),
        args: vec![],
        elements: 0,
        deepest: 0,
    })]);
    assert_eq!(
        render(&tpl, &Env::new(), &[]),
        Err(Blocked::UnknownInstance {
            path: "t.Missing".into()
        })
    );
}

#[test]
fn an_error_inside_an_instance_is_the_first_the_document_meets() {
    // An instance is written after the markup around it (ADR-0203), and its
    // error is still the first in the document: here, before the parent's
    // own missing value after it.
    let child = Template {
        path: "t.Card".into(),
        name: "Card".into(),
        params: vec!["label".into()],
        schema: "test".into(),
        chunks: vec![Chunk::Dynamic(Part::Text {
            id: PartId(0),
            value: "missing".into(),
            context: Context::Text,
        })],
    };
    let instance = Chunk::Dynamic(Part::Instance {
        id: PartId(0),
        path: "t.Card".into(),
        args: vec![("label".into(), "title".into())],
        elements: 0,
        deepest: 0,
    });
    let absent = Chunk::Dynamic(Part::Text {
        id: PartId(1),
        value: "absent".into(),
        context: Context::Text,
    });
    let env = Env::new().set("title", Value::Text("Menu".into()));
    let others = std::slice::from_ref(&child);
    assert_eq!(
        render(&t(vec![instance.clone(), absent.clone()]), &env, others),
        Err(Blocked::MissingValue {
            path: "missing".into()
        })
    );
    // And the parent's, where it comes first.
    assert_eq!(
        render(&t(vec![absent, instance]), &env, others),
        Err(Blocked::MissingValue {
            path: "absent".into()
        })
    );
}

#[test]
fn an_instance_rendered_alone_is_the_bytes_the_page_has_there() {
    // A host renders a view's instance again, whole, when what it is given
    // changed (ADR-0203): rendered alone, it is its frame and its markup,
    // byte for byte what the page's render wrote where it is.
    let child = Template {
        path: "t.Card".into(),
        name: "Card".into(),
        params: vec!["label".into()],
        schema: "test".into(),
        chunks: vec![
            Chunk::Static("<b>".into()),
            Chunk::Dynamic(Part::Text {
                id: PartId(0),
                value: "label".into(),
                context: Context::Text,
            }),
            Chunk::Static("</b>".into()),
        ],
    };
    let parent = t(vec![
        Chunk::Static("<div>".into()),
        Chunk::Dynamic(Part::Instance {
            id: PartId(3),
            path: "t.Card".into(),
            args: vec![("label".into(), "title".into())],
            elements: 1,
            deepest: 1,
        }),
        Chunk::Static("</div>".into()),
    ]);
    let env = Env::new().set("title", Value::Text("Menu".into()));
    let others = std::slice::from_ref(&child);
    let whole = render(&parent, &env, others).unwrap();
    let alone = render_part(&parent, PartId(3), &env, others).unwrap();
    assert!(alone.starts_with("<!--pw:s3@"), "{alone}");
    assert_eq!(format!("<div>{alone}</div>"), whole);
}

#[test]
fn the_templates_a_page_reaches_are_each_view_its_instances_reach() {
    // What a host sends the browser with a page (ADR-0203): each view an
    // instance renders, and each one those render, once.
    let view = |path: &str, inner: Option<&str>| Template {
        path: path.into(),
        name: path.into(),
        params: vec![],
        schema: "test".into(),
        chunks: inner
            .map(|p| {
                vec![Chunk::Dynamic(Part::Instance {
                    id: PartId(0),
                    path: p.into(),
                    args: vec![],
                    elements: 0,
                    deepest: 0,
                })]
            })
            .unwrap_or_default(),
    };
    // `A` renders `B`, which renders itself; `C` is rendered by nothing.
    let others = vec![
        view("t.A", Some("t.B")),
        view("t.B", Some("t.B")),
        view("t.C", None),
    ];
    let page = view("t.Page", Some("t.A"));
    let reached: Vec<&str> = instances_reached(&page, &others)
        .into_iter()
        .map(|t| t.path.as_str())
        .collect();
    assert_eq!(reached, ["t.A", "t.B"]);
    // One not among them is left out, and refused where it is rendered.
    assert!(instances_reached(&view("t.Page", Some("t.Missing")), &others).is_empty());
}

// --- E7-R: part identity in the document ---------------------------------

#[test]
fn a_range_part_is_delimited_by_its_own_anchors() {
    // The encoding a range needs and an attribute cannot give it: a part that
    // renders NOTHING still has a place to reappear.
    let tpl = t(vec![
        Chunk::Static("<ul>".into()),
        Chunk::Dynamic(Part::Each {
            id: PartId(4),
            collection: "items".into(),
            binding: "item".into(),
            key: None,
            row_anchor: Default::default(),
            body: vec![Chunk::Static("<li>x</li>".into())],
        }),
        Chunk::Static("</ul>".into()),
    ]);
    let empty = Env::new().set("items", Value::List(vec![]));
    assert_eq!(
        render(&tpl, &empty, &[]).unwrap(),
        "<ul><!--pw:s4--><!--pw:e4--></ul>",
        "an empty range keeps its boundaries, or the runtime has nowhere to \
         insert the first item"
    );
}

#[test]
fn a_static_region_carries_no_identity_markup() {
    // The invariant that makes "without making static HTML noisy" mean
    // something: only dynamic and resumable regions pay identity overhead.
    let tpl = t(vec![Chunk::Static(
        "<h1>Store</h1><p>Open until 10</p>".into(),
    )]);
    let out = render(&tpl, &Env::new(), &[]).unwrap();
    assert_eq!(out, "<h1>Store</h1><p>Open until 10</p>");
    assert!(!out.contains("pw:"), "{out}");
    assert!(!out.contains("data-pw"), "{out}");
}

#[test]
fn an_element_local_part_anchors_on_the_element_not_on_a_comment() {
    // Three dynamic things on one element cost ONE identity, not three comment
    // pairs. `data-pw` is written by the IR's static chunk, so the renderer
    // emits nothing extra for the parts themselves.
    let tpl = t(vec![
        Chunk::Static("<button data-pw=\"7\" ".into()),
        Chunk::Dynamic(Part::Attribute {
            id: PartId(1),
            owner: ElementId(7),
            name: "title".into(),
            value: "label".into(),
            context: Context::Attribute,
        }),
        Chunk::Static(" ".into()),
        Chunk::Dynamic(Part::BooleanAttribute {
            id: PartId(2),
            owner: ElementId(7),
            name: "disabled".into(),
            value: "locked".into(),
        }),
        Chunk::Static(">Add</button>".into()),
    ]);
    let env = Env::new()
        .set("label", Value::Text("Add to cart".into()))
        .set("locked", Value::Bool(true));
    let out = render(&tpl, &env, &[]).unwrap();
    assert_eq!(
        out,
        "<button data-pw=\"7\" title=\"Add to cart\" disabled>Add</button>"
    );
    assert!(
        !out.contains("pw:s"),
        "no comment anchors for element parts: {out}"
    );
}

#[test]
fn an_event_part_emits_nothing_because_behaviour_is_not_markup() {
    // The element already carries `data-pw`, which is what the runtime needs to
    // find it. Emitting anything more would be inventing a second encoding of
    // the same fact. A handler that reads nothing it captured carries nothing.
    let tpl = t(vec![
        Chunk::Static("<button data-pw=\"3\">".into()),
        Chunk::Dynamic(Part::Event {
            id: PartId(9),
            owner: ElementId(3),
            event: "press".into(),
            handler: "add_to_cart".into(),
            name: "add_to_cart".into(),
            captures: vec![],
            renames: BTreeMap::new(),
            modifiers: vec![],
        }),
        Chunk::Static("Add</button>".into()),
    ]);
    assert_eq!(
        render(&tpl, &Env::new(), &[]).unwrap(),
        "<button data-pw=\"3\">Add</button>"
    );
}

// --- E10: what a compiled handler reads, carried by its element -------------

fn button_capturing(paths: &[&str]) -> Template {
    button_renaming(paths, &[])
}

/// A button whose handler names `from` what the template holds at `to`: a
/// handler written in a view, composed into a page (ADR-0136).
fn button_renaming(paths: &[&str], renames: &[(&str, &str)]) -> Template {
    t(vec![
        Chunk::Static("<button data-pw=\"3\"".into()),
        Chunk::Dynamic(Part::Event {
            id: PartId(9),
            owner: ElementId(3),
            event: "press".into(),
            handler: "e1ab9fca1f6fc15b".into(),
            name: "add_to_cart".into(),
            captures: paths.iter().map(|p| p.to_string()).collect(),
            renames: renames
                .iter()
                .map(|(from, to)| (from.to_string(), to.to_string()))
                .collect(),
            modifiers: vec![],
        }),
        Chunk::Static(">Add</button>".into()),
    ])
}

fn item(id: &str, name: &str) -> Value {
    Value::Record(BTreeMap::from([
        ("id".to_string(), Value::Text(id.into())),
        ("name".to_string(), Value::Text(name.into())),
    ]))
}

/// The attribute's value, unescaped the way a browser's `dataset` reads it.
fn captures_of(html: &str) -> serde_json::Value {
    let start = html.find("data-pw-captures=\"").expect("the attribute") + 18;
    let end = start + html[start..].find('"').expect("its end");
    let raw = html[start..end]
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&#96;", "`")
        .replace("&amp;", "&");
    serde_json::from_str(&raw).expect("JSON")
}

#[test]
fn an_element_carries_exactly_the_paths_its_handler_reads() {
    // `item.id` and not the whole item: a field the handler does not read is
    // not in the document, where a patch could leave it stale.
    let env = Env::new().set("item", item("cortado", "Cortado"));
    let html = render(&button_capturing(&["item.id"]), &env, &[]).unwrap();
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "cortado" } }),
        "{html}"
    );

    // Two paths through one capture nest under one key.
    let html = render(&button_capturing(&["item.id", "item.name"]), &env, &[]).unwrap();
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "cortado", "name": "Cortado" } })
    );

    // A capture read whole is carried whole.
    let html = render(&button_capturing(&["item"]), &env, &[]).unwrap();
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "cortado", "name": "Cortado" } })
    );

    // What a host computed for a row is set in it by its path (ADR-0170),
    // and is the page's to show, not a field of the item: a handler given
    // the item is not given it (ADR-0172).
    let mut computed = item("cortado", "Cortado");
    let Value::Record(fields) = &mut computed else {
        unreachable!()
    };
    fields.insert("price.display".into(), Value::Text("$4.25".into()));
    let computed = Env::new().set("item", computed);
    let html = render(&button_capturing(&["item"]), &computed, &[]).unwrap();
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "cortado", "name": "Cortado" } }),
        "{html}"
    );
}

#[test]
fn a_capture_a_view_names_otherwise_is_read_where_it_is() {
    // ADR-0136: a view's handler captures its parameter `item`, and the view
    // was given the page's `entry`. The value is read at `entry.id` and
    // written under `item.id`, where the view's compiled handler reads it.
    let env = Env::new()
        .set("entry", item("cortado", "Cortado"))
        .set("item", item("decoy", "Decoy"));
    let html = render(
        &button_renaming(&["item.id"], &[("item", "entry")]),
        &env,
        &[],
    )
    .unwrap();
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "cortado" } }),
        "{html}"
    );
    // A parameter given a field: `item` is `order.line`.
    let env = Env::new().set(
        "order",
        Value::Record(BTreeMap::from([(
            "line".to_string(),
            item("flat-white", "Flat white"),
        )])),
    );
    let whole = render(
        &button_renaming(&["item"], &[("item", "order.line")]),
        &env,
        &[],
    )
    .unwrap();
    assert_eq!(
        captures_of(&whole),
        serde_json::json!({ "item": { "id": "flat-white", "name": "Flat white" } })
    );
    // Control: without the rename, the handler's own name is read.
    let env = Env::new()
        .set("entry", item("cortado", "Cortado"))
        .set("item", item("decoy", "Decoy"));
    let html = render(&button_capturing(&["item.id"]), &env, &[]).unwrap();
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "decoy" } })
    );
}

/// An element with two handlers: `on:input` capturing `paths_a` and
/// `on:keydown` capturing `paths_b`, adjacent in the IR, as the compiler
/// lowers an element's handlers (ADR-0138).
fn input_with_two_handlers(
    paths_a: &[&str],
    paths_b: &[&str],
    rename_b: &[(&str, &str)],
) -> Template {
    let event = |id: u32, event: &str, paths: &[&str], renames: &[(&str, &str)]| {
        Chunk::Dynamic(Part::Event {
            id: PartId(id),
            owner: ElementId(3),
            event: event.into(),
            handler: format!("h{id}"),
            name: String::new(),
            captures: paths.iter().map(|p| p.to_string()).collect(),
            renames: renames
                .iter()
                .map(|(from, to)| (from.to_string(), to.to_string()))
                .collect(),
            modifiers: vec![],
        })
    };
    t(vec![
        Chunk::Static("<input data-pw=\"3\"".into()),
        event(4, "input", paths_a, &[]),
        event(5, "keydown", paths_b, rename_b),
        Chunk::Static(">".into()),
    ])
}

#[test]
fn an_element_s_handlers_carry_one_attribute_between_them() {
    // Two attributes of one name are one attribute to a browser, which keeps
    // the first: the second handler would read the first's captures.
    let env = Env::new().set("item", item("cortado", "Cortado"));
    let html = render(
        &input_with_two_handlers(&["item.id"], &["item.name"], &[]),
        &env,
        &[],
    )
    .unwrap();
    assert_eq!(html.matches("data-pw-captures").count(), 1, "{html}");
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "cortado", "name": "Cortado" } })
    );
    // One path both read is written once.
    let html = render(
        &input_with_two_handlers(&["item.id"], &["item.id"], &[]),
        &env,
        &[],
    )
    .unwrap();
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "cortado" } })
    );
    // One name read from two places is refused, not guessed between.
    let env = env.set("other", item("decoy", "Decoy"));
    assert!(matches!(
        render(
            &input_with_two_handlers(&["item.id"], &["item.id"], &[("item", "other")]),
            &env,
            &[],
        ),
        Err(Blocked::UnrepresentedConstruct { .. })
    ));
}

#[test]
fn a_captured_value_survives_the_attribute_exactly() {
    // Quotes, angle brackets and ampersands inside the value: the attribute is
    // escaped once, and what the browser reads back is the value.
    let env = Env::new().set("item", item("a\"b<c>&d'e", "x"));
    let html = render(&button_capturing(&["item.id"]), &env, &[]).unwrap();
    assert!(
        !html[html.find("data-pw-captures").unwrap()..].starts_with("data-pw-captures=\"{\"item")
    );
    assert_eq!(
        captures_of(&html),
        serde_json::json!({ "item": { "id": "a\"b<c>&d'e" } }),
        "{html}"
    );
}

#[test]
fn a_capture_that_cannot_be_carried_blocks_the_render() {
    // A path with nothing behind it.
    let env = Env::new().set("item", item("cortado", "Cortado"));
    assert!(matches!(
        render(&button_capturing(&["item.price"]), &env, &[]),
        Err(Blocked::MissingValue { .. })
    ));

    // An integer a JavaScript number cannot hold exactly: the handler would
    // read a different number than the document was rendered with.
    let exact = Env::new().set("n", Value::Int(1 << 53));
    assert!(render(&button_capturing(&["n"]), &exact, &[]).is_ok());
    let inexact = Env::new().set("n", Value::Int((1 << 53) + 1));
    assert!(matches!(
        render(&button_capturing(&["n"]), &inexact, &[]),
        Err(Blocked::UnrepresentedConstruct { .. })
    ));

    // Raw HTML is bytes with an authority attached; neither is a value.
    let raw = Env::new().set(
        "h",
        Value::Raw {
            html: "<b>x</b>".into(),
            capability: "html.trusted".into(),
        },
    );
    assert!(matches!(
        render(&button_capturing(&["h"]), &raw, &[]),
        Err(Blocked::UnrepresentedConstruct { .. })
    ));
}

// --- E7-R: identity domains and instance tokens --------------------------

#[test]
fn a_keyed_loop_refuses_two_items_with_one_key() {
    // A key that does not identify is not a key. Refused rather than rendered,
    // because two instances with one address is exactly the ambiguity the
    // address model exists to remove.
    let tpl = t(vec![Chunk::Dynamic(Part::Each {
        id: PartId(0),
        collection: "items".into(),
        binding: "item".into(),
        key: Some("id".into()),
        row_anchor: Default::default(),
        body: vec![Chunk::Static("<li>x</li>".into())],
    })]);
    let env = Env::new().set(
        "items",
        Value::List(vec![
            record(&[("id", Value::Text("same".into()))]),
            record(&[("id", Value::Text("same".into()))]),
        ]),
    );
    assert_eq!(
        render(&tpl, &env, &[]),
        Err(Blocked::DuplicateLoopKey {
            each: PartId(0),
            key: "same".into()
        })
    );
}

#[test]
fn a_keyed_loop_refuses_an_item_with_no_key_value() {
    // Distinct from a duplicate: an item with no identity cannot be addressed,
    // and the repair is to the data rather than to the key field's name.
    let tpl = t(vec![Chunk::Dynamic(Part::Each {
        id: PartId(0),
        collection: "items".into(),
        binding: "item".into(),
        key: Some("id".into()),
        row_anchor: Default::default(),
        body: vec![Chunk::Static("<li>x</li>".into())],
    })]);
    let env = Env::new().set(
        "items",
        Value::List(vec![record(&[("name", Value::Text("no id here".into()))])]),
    );
    assert_eq!(
        render(&tpl, &env, &[]),
        Err(Blocked::MissingLoopKey {
            each: PartId(0),
            field: "id".into()
        })
    );
}

#[test]
fn an_unkeyed_loop_emits_no_instance_boundaries() {
    // An identity that is really a position looks addressable and moves when
    // the list does, which is worse than having none. So an unkeyed loop gets
    // its own range and nothing per item.
    let tpl = t(vec![Chunk::Dynamic(Part::Each {
        id: PartId(3),
        collection: "items".into(),
        binding: "item".into(),
        key: None,
        row_anchor: Default::default(),
        body: vec![Chunk::Static("<li>x</li>".into())],
    })]);
    let env = Env::new().set("items", Value::List(vec![Value::Int(1), Value::Int(2)]));
    let out = render(&tpl, &env, &[]).unwrap();
    assert_eq!(out, "<!--pw:s3--><li>x</li><li>x</li><!--pw:e3-->");
    assert!(!out.contains('@'), "no instance token: {out}");
}

#[test]
fn a_public_domain_and_a_session_domain_render_different_tokens() {
    // E6 decides who shares an identity domain; E7 reads that answer. The same
    // fragment in two session partitions must not correlate, and the same
    // fragment in one public partition must agree — a shared cache entry has to
    // be byte-identical for every reader.
    let tpl = t(vec![Chunk::Dynamic(Part::Each {
        id: PartId(1),
        collection: "items".into(),
        binding: "item".into(),
        key: Some("id".into()),
        row_anchor: Default::default(),
        body: vec![Chunk::Static("<li>x</li>".into())],
    })]);
    let items = Value::List(vec![record(&[("id", Value::Text("line-1".into()))])]);
    let render_in = |d: IdentityDomain| {
        render(
            &tpl,
            &Env::new().set("items", items.clone()).in_domain(d),
            &[],
        )
        .unwrap()
    };

    let public = |_: ()| IdentityDomain::materialization("F", Partition::Public, "B1");
    assert_eq!(
        render_in(public(())),
        render_in(public(())),
        "one public entry is one set of bytes, for every reader"
    );
    assert_ne!(
        render_in(IdentityDomain::materialization(
            "F",
            Partition::Session { id: "A".into() },
            "B1"
        )),
        render_in(IdentityDomain::materialization(
            "F",
            Partition::Session { id: "B".into() },
            "B1"
        )),
        "two sessions do not correlate"
    );
}

/// **Carried captures serialize deterministically** (charter §14 M10 task 10).
///
/// Two renders of one document are byte-identical, and so are two templates
/// listing the same capture paths in different orders. The object's keys are
/// sorted by `serde_json`'s map; if a dependency ever enabled its
/// `preserve_order` feature, the permuted case would fail here rather than as
/// a document whose bytes depended on how its template was built.
#[test]
fn carried_captures_serialize_deterministically() {
    let env = Env::new().set(
        "item",
        record(&[
            ("id", Value::Text("cortado".into())),
            ("name", Value::Text("Cortado".into())),
            ("price", Value::Int(450)),
        ]),
    );
    let one = render(
        &button_capturing(&["item.price", "item.id", "item.name"]),
        &env,
        &[],
    )
    .unwrap();
    let two = render(
        &button_capturing(&["item.name", "item.price", "item.id"]),
        &env,
        &[],
    )
    .unwrap();
    let again = render(
        &button_capturing(&["item.price", "item.id", "item.name"]),
        &env,
        &[],
    )
    .unwrap();
    assert_eq!(one, again, "the same document, twice");
    assert_eq!(
        one, two,
        "the order a template lists paths in does not reach the bytes"
    );
    assert!(
        one.contains("{&quot;item&quot;:{&quot;id&quot;:&quot;cortado&quot;,&quot;name&quot;:&quot;Cortado&quot;,&quot;price&quot;:450}}"),
        "keys sorted: {one}"
    );
}
