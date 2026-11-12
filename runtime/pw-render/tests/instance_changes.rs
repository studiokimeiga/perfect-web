//! **What changed in one instance of a keyed loop, part by part** (ADR-0168).
//!
//! A host sets what a change reaches where it is: a text part's text, and an
//! attribute's value, so the instance keeps its nodes. Until 2026-10-03 only
//! text parts were set: an attribute that read a changed field kept its old
//! value, and the store's Add button kept the name of the item it was before.

use pw_render::*;

/// `{#each items as item (item.id)}<li title="{item.name}" data-x={item.note}
/// hidden={item.gone}><span>{item.name}</span>{#if item.new}<b>new</b>{/if}</li>{/each}`
fn page() -> Template {
    let st = |s: &str| Chunk::Static(s.to_string());
    Template {
        path: "t.T".into(),
        name: "T".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "items".into(),
            binding: "item".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![
                st("<li data-pw=\"0\" "),
                Chunk::Dynamic(Part::InterpolatedAttribute {
                    id: PartId(1),
                    owner: ElementId(0),
                    name: "aria-label".into(),
                    segments: vec![
                        Segment::Static("Add ".into()),
                        Segment::Value("item.name".into()),
                    ],
                    context: Context::Attribute,
                }),
                st(" "),
                Chunk::Dynamic(Part::Attribute {
                    id: PartId(2),
                    owner: ElementId(0),
                    name: "title".into(),
                    value: "item.note".into(),
                    context: Context::Attribute,
                }),
                st(" "),
                Chunk::Dynamic(Part::BooleanAttribute {
                    id: PartId(3),
                    owner: ElementId(0),
                    name: "hidden".into(),
                    value: "item.gone".into(),
                }),
                st("><span>"),
                Chunk::Dynamic(Part::Text {
                    id: PartId(4),
                    value: "item.name".into(),
                    context: Context::Text,
                }),
                st("</span>"),
                Chunk::Dynamic(Part::Conditional {
                    id: PartId(5),
                    value: "item.new".into(),
                    then: vec![st("<b>new</b>")],
                    otherwise: vec![],
                }),
                st("</li>"),
            ],
        })],
    }
}

fn item(name: &str, note: &str, gone: bool, new: bool) -> Value {
    Value::Record(
        [
            ("id".to_string(), Value::Text("a".into())),
            ("name".to_string(), Value::Text(name.into())),
            ("note".to_string(), Value::Text(note.into())),
            ("gone".to_string(), Value::Bool(gone)),
            ("new".to_string(), Value::Bool(new)),
        ]
        .into(),
    )
}

fn changes(was: &Value, now: &Value) -> Option<Vec<(PartId, InstanceChange)>> {
    instance_changes(&page(), PartId(0), was, now, &Env::new(), &[]).expect("derived")
}

#[test]
fn nothing_changed_is_no_change() {
    let a = item("Tea", "hot", false, false);
    assert_eq!(changes(&a, &a), Some(vec![]));
}

#[test]
fn a_renamed_item_sets_its_text_and_every_attribute_that_reads_the_name() {
    assert_eq!(
        changes(
            &item("Tea", "hot", false, false),
            &item("Green Tea", "hot", false, false)
        ),
        Some(vec![
            (
                PartId(1),
                InstanceChange::Attribute {
                    name: "aria-label".into(),
                    value: Some("Add Green Tea".into()),
                }
            ),
            (PartId(4), InstanceChange::Text("Green Tea".into())),
        ])
    );
}

#[test]
fn an_attribute_is_set_as_the_document_writes_it_and_a_boolean_one_comes_and_goes() {
    assert_eq!(
        changes(
            &item("Tea", "hot", false, false),
            &item("Tea", "\"very\" hot", true, false)
        ),
        Some(vec![
            (
                PartId(2),
                InstanceChange::Attribute {
                    name: "title".into(),
                    value: Some("&quot;very&quot; hot".into()),
                }
            ),
            (
                PartId(3),
                InstanceChange::Attribute {
                    name: "hidden".into(),
                    value: Some(String::new()),
                }
            ),
        ])
    );
    assert_eq!(
        changes(
            &item("Tea", "hot", true, false),
            &item("Tea", "hot", false, false)
        ),
        Some(vec![(
            PartId(3),
            InstanceChange::Attribute {
                name: "hidden".into(),
                value: None,
            }
        )])
    );
}

#[test]
fn a_block_that_changed_renders_the_instance_again() {
    assert_eq!(
        changes(
            &item("Tea", "hot", false, false),
            &item("Tea", "hot", false, true)
        ),
        None
    );
}

/// `{#each items as item (item.id)}<li><button on:press={…captures item.name}
/// on:focus={…captures item.id}>{item.name}</button></li>{/each}`
fn buttons() -> Template {
    let st = |s: &str| Chunk::Static(s.to_string());
    let handler = |id: u32, event: &str, captures: &[&str]| {
        Chunk::Dynamic(Part::Event {
            id: PartId(id),
            owner: ElementId(1),
            event: event.into(),
            handler: format!("h{id}"),
            name: format!("on_{event}"),
            captures: captures.iter().map(|c| c.to_string()).collect(),
            renames: Default::default(),
            modifiers: vec![],
        })
    };
    Template {
        path: "t.B".into(),
        name: "B".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "items".into(),
            binding: "item".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![
                st("<li><button data-pw=\"1\""),
                handler(1, "press", &["item.name"]),
                handler(2, "focus", &["item.id"]),
                st(">"),
                Chunk::Dynamic(Part::Text {
                    id: PartId(3),
                    value: "item.name".into(),
                    context: Context::Text,
                }),
                st("</button></li>"),
            ],
        })],
    }
}

/// **What a handler captures is set where it is** (ADR-0172): the attribute
/// the runtime reads when the handler runs, once for its element's handlers.
/// Until 2026-10-03 a change to it rendered the instance again, and a
/// renamed item's button lost its nodes, and focus.
#[test]
fn what_a_handler_captures_is_set_where_it_is() {
    let changes = |was: &Value, now: &Value| {
        instance_changes(&buttons(), PartId(0), was, now, &Env::new(), &[]).expect("derived")
    };
    assert_eq!(
        changes(
            &item("Tea", "hot", false, false),
            &item("Green Tea", "hot", false, false)
        ),
        Some(vec![
            (
                PartId(1),
                InstanceChange::Attribute {
                    name: "data-pw-captures".into(),
                    value: Some(
                        "{&quot;item&quot;:{&quot;id&quot;:&quot;a&quot;,\
                         &quot;name&quot;:&quot;Green Tea&quot;}}"
                            .into()
                    ),
                }
            ),
            (PartId(3), InstanceChange::Text("Green Tea".into())),
        ])
    );
    // What no handler captures changes nothing of the element's.
    assert_eq!(
        changes(
            &item("Tea", "hot", false, false),
            &item("Tea", "cold", false, false)
        ),
        Some(vec![])
    );
}

/// `{#each items as item (item.id)}<li>{#if item.new}<b>{item.name}</b>
/// {:else}<i data-pw="2" title={item.note}>{item.name}</i>{/if}</li>{/each}`
fn branches() -> Template {
    let st = |s: &str| Chunk::Static(s.to_string());
    let name = |id: u32| {
        Chunk::Dynamic(Part::Text {
            id: PartId(id),
            value: "item.name".into(),
            context: Context::Text,
        })
    };
    Template {
        path: "t.C".into(),
        name: "C".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "items".into(),
            binding: "item".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![
                st("<li>"),
                Chunk::Dynamic(Part::Conditional {
                    id: PartId(1),
                    value: "item.new".into(),
                    then: vec![st("<b>"), name(2), st("</b>")],
                    otherwise: vec![
                        st("<i data-pw=\"2\" "),
                        Chunk::Dynamic(Part::Attribute {
                            id: PartId(3),
                            owner: ElementId(2),
                            name: "title".into(),
                            value: "item.note".into(),
                            context: Context::Attribute,
                        }),
                        st(">"),
                        name(4),
                        st("</i>"),
                    ],
                }),
                st("</li>"),
            ],
        })],
    }
}

/// **A block that decides as it did is set where it is** (ADR-0178): the
/// parts of the branch it shows, text and attributes alike. Until 2026-10-04
/// any change inside a block rendered the row again.
#[test]
fn a_block_that_decides_as_it_did_is_set_where_it_is() {
    let changes = |was: &Value, now: &Value| {
        instance_changes(&branches(), PartId(0), was, now, &Env::new(), &[]).expect("derived")
    };
    assert_eq!(
        changes(
            &item("Tea", "hot", false, false),
            &item("Green Tea", "cold", false, false)
        ),
        Some(vec![
            (
                PartId(3),
                InstanceChange::Attribute {
                    name: "title".into(),
                    value: Some("cold".into()),
                }
            ),
            (PartId(4), InstanceChange::Text("Green Tea".into())),
        ])
    );
    // The other branch, as it was.
    assert_eq!(
        changes(
            &item("Tea", "hot", false, true),
            &item("Green Tea", "hot", false, true)
        ),
        Some(vec![(PartId(2), InstanceChange::Text("Green Tea".into()))])
    );
    // A block that decides otherwise is rendered again.
    assert_eq!(
        changes(
            &item("Tea", "hot", false, false),
            &item("Tea", "hot", false, true)
        ),
        None
    );
}

/// `{#each items as item (item.id)}<li>{#each item.tags as tag}<i>{tag}</i>
/// {/each}</li>{/each}`
fn tagged() -> Template {
    let st = |s: &str| Chunk::Static(s.to_string());
    Template {
        path: "t.D".into(),
        name: "D".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "items".into(),
            binding: "item".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![
                st("<li>"),
                Chunk::Dynamic(Part::Each {
                    id: PartId(1),
                    collection: "item.tags".into(),
                    binding: "tag".into(),
                    key: None,
                    row_anchor: Default::default(),
                    body: vec![
                        st("<i>"),
                        Chunk::Dynamic(Part::Text {
                            id: PartId(2),
                            value: "tag".into(),
                            context: Context::Text,
                        }),
                        st("</i>"),
                    ],
                }),
                st("</li>"),
            ],
        })],
    }
}

/// **Any other block that changed renders the row again** (ADR-0168): a
/// block that decides is looked into (ADR-0178), and so is a keyed list
/// inside the row (ADR-0181, `tests/nested_lists.rs`); an unkeyed one has no
/// instance to address.
#[test]
fn an_unkeyed_list_inside_the_row_that_changed_renders_the_row_again() {
    let row = |tags: &[&str]| {
        Value::Record(
            [
                ("id".to_string(), Value::Text("a".into())),
                (
                    "tags".to_string(),
                    Value::List(tags.iter().map(|t| Value::Text((*t).into())).collect()),
                ),
            ]
            .into(),
        )
    };
    let changes = |was: &Value, now: &Value| {
        instance_changes(&tagged(), PartId(0), was, now, &Env::new(), &[]).expect("derived")
    };
    assert_eq!(changes(&row(&["hot"]), &row(&["hot"])), Some(vec![]));
    assert_eq!(changes(&row(&["hot"]), &row(&["hot", "new"])), None);
}

/// `{#each items as item (item.id)}<li>{#match item.deal}{:Some(deal)}<b>
/// {deal}</b>{:None}{/match}</li>{/each}`
fn dealt() -> Template {
    let st = |s: &str| Chunk::Static(s.to_string());
    Template {
        path: "t.E".into(),
        name: "E".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "items".into(),
            binding: "item".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![
                st("<li>"),
                Chunk::Dynamic(Part::Match {
                    id: PartId(1),
                    value: "item.deal".into(),
                    arms: vec![
                        Arm {
                            case: "Some".into(),
                            binding: Some("deal".into()),
                            fields: vec![],
                            body: vec![
                                st("<b>"),
                                Chunk::Dynamic(Part::Text {
                                    id: PartId(2),
                                    value: "deal".into(),
                                    context: Context::Text,
                                }),
                                st("</b>"),
                            ],
                        },
                        Arm {
                            case: "None".into(),
                            binding: None,
                            fields: vec![],
                            body: vec![],
                        },
                    ],
                }),
                st("</li>"),
            ],
        })],
    }
}

/// **A `{#match}` that shows the arm it showed is set where it is**
/// (ADR-0320), as a conditional that decides as it did is (ADR-0178): the
/// part that reads its payload. Until 2026-10-12 any change inside one
/// rendered the row again.
#[test]
fn a_match_that_shows_the_arm_it_showed_is_set_where_it_is() {
    let row = |deal: Option<&str>| {
        Value::Record(
            [
                ("id".to_string(), Value::Text("a".into())),
                (
                    "deal".to_string(),
                    match deal {
                        Some(d) => Value::Variant {
                            case: "Some".into(),
                            payload: Some(Box::new(Value::Text(d.into()))),
                        },
                        None => Value::Variant {
                            case: "None".into(),
                            payload: None,
                        },
                    },
                ),
            ]
            .into(),
        )
    };
    let changes = |was: &Value, now: &Value| {
        instance_changes(&dealt(), PartId(0), was, now, &Env::new(), &[]).expect("derived")
    };
    assert_eq!(
        changes(&row(Some("2 for 1")), &row(Some("3 for 2"))),
        Some(vec![(PartId(2), InstanceChange::Text("3 for 2".into()))])
    );
    // Another arm: the row is rendered again.
    assert_eq!(changes(&row(Some("2 for 1")), &row(None)), None);
}
