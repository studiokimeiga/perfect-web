//! **A keyed list inside a row is changed where it is** (ADR-0181).
//!
//! `{#each sections as section (section.id)}<h2>{section.name}</h2><ul>
//! {#each section.items as item (item.id)}<li>{item.name}</li>{/each}</ul>
//! {/each}`. A change to a section's items is that section's row's change: its
//! inner list's own operations, each inside the section's instance, every
//! item whose key stayed keeping its nodes. Until 2026-10-04 any change inside
//! a list in a row rendered the whole row again.

use pw_render::*;

fn sections() -> Template {
    let st = |s: &str| Chunk::Static(s.to_string());
    let text = |id: u32, value: &str| {
        Chunk::Dynamic(Part::Text {
            id: PartId(id),
            value: value.into(),
            context: Context::Text,
        })
    };
    Template {
        path: "t.M".into(),
        name: "M".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![Chunk::Dynamic(Part::Each {
            id: PartId(0),
            collection: "sections".into(),
            binding: "section".into(),
            key: Some("id".into()),
            row_anchor: Default::default(),
            body: vec![
                st("<h2>"),
                text(1, "section.name"),
                st("</h2><ul>"),
                Chunk::Dynamic(Part::Each {
                    id: PartId(2),
                    collection: "section.items".into(),
                    binding: "item".into(),
                    key: Some("id".into()),
                    row_anchor: Default::default(),
                    body: vec![st("<li>"), text(3, "item.name"), st("</li>")],
                }),
                st("</ul>"),
            ],
        })],
    }
}

fn item(id: &str, name: &str) -> Value {
    Value::Record(
        [
            ("id".to_string(), Value::Text(id.into())),
            ("name".to_string(), Value::Text(name.into())),
        ]
        .into(),
    )
}

fn section(id: &str, name: &str, items: Vec<Value>) -> Value {
    Value::Record(
        [
            ("id".to_string(), Value::Text(id.into())),
            ("name".to_string(), Value::Text(name.into())),
            ("items".to_string(), Value::List(items)),
        ]
        .into(),
    )
}

fn coffee(items: Vec<Value>) -> Value {
    section("coffee", "Coffee", items)
}

/// The changes from `old` sections to `new`.
fn changes(old: &[Value], new: &[Value]) -> Vec<ListChange> {
    list_changes(&sections(), PartId(0), old, new, &Env::new(), &[]).expect("derived")
}

/// The token an item has inside the coffee section, as a render gives it.
fn in_coffee(id: &str) -> InstanceToken {
    let t = sections();
    let scoped = instance_env(&t, PartId(0), &coffee(vec![]), &Env::new()).expect("inside");
    instance_token_of(PartId(2), &item(id, ""), "id", &scoped)
}

fn coffee_token() -> InstanceToken {
    instance_token_of(PartId(0), &coffee(vec![]), "id", &Env::new())
}

#[test]
fn an_item_renamed_inside_a_section_is_set_where_it_is() {
    let old = [coffee(vec![
        item("espresso", "Espresso"),
        item("cortado", "Cortado"),
    ])];
    let new = [coffee(vec![
        item("espresso", "Doppio"),
        item("cortado", "Cortado"),
    ])];
    assert_eq!(
        changes(&old, &new),
        [ListChange::Set {
            instance: coffee_token(),
            changes: vec![(
                PartId(2),
                InstanceChange::List(vec![ListChange::Set {
                    instance: in_coffee("espresso"),
                    changes: vec![(PartId(3), InstanceChange::Text("Doppio".into()))],
                }]),
            )],
        }]
    );
}

#[test]
fn an_item_inserted_moved_or_removed_inside_a_section_is_its_lists_own_operation() {
    let three = || {
        vec![
            item("espresso", "Espresso"),
            item("cortado", "Cortado"),
            item("cold-brew", "Cold Brew"),
        ]
    };
    let inner = |old: Vec<Value>, new: Vec<Value>| match changes(&[coffee(old)], &[coffee(new)])
        .as_slice()
    {
        [ListChange::Set { instance, changes }] => {
            assert_eq!(*instance, coffee_token(), "inside the coffee section");
            match changes.as_slice() {
                [(PartId(2), InstanceChange::List(list))] => list.clone(),
                other => panic!("the items' list alone: {other:?}"),
            }
        }
        other => panic!("one change to the section: {other:?}"),
    };
    // Inserted after the item before it ...
    let mut more = three();
    more.insert(2, item("mocha", "Mocha"));
    match inner(three(), more).as_slice() {
        [ListChange::InsertAfter { after, html }] => {
            assert_eq!(*after, in_coffee("cortado"));
            assert!(
                html.contains("Mocha") && html.contains(&format!("pw:s2@{}", in_coffee("mocha")))
            );
        }
        other => panic!("{other:?}"),
    }
    // ... and at the head, before the first, where there is one.
    let mut first = three();
    first.insert(0, item("mocha", "Mocha"));
    assert!(matches!(
        inner(three(), first).as_slice(),
        [ListChange::InsertBefore { before: Some(b), .. }] if *b == in_coffee("espresso")
    ));
    // Moved: the item's nodes, kept.
    let mut moved = three();
    let cold = moved.remove(2);
    moved.insert(0, cold);
    assert_eq!(
        inner(three(), moved),
        [ListChange::Move {
            instance: in_coffee("cold-brew"),
            after: None
        }]
    );
    // Removed.
    let mut fewer = three();
    fewer.remove(1);
    assert_eq!(
        inner(three(), fewer),
        [ListChange::Remove(in_coffee("cortado"))]
    );
}

/// **A key repeated in a list's new items is refused**, as a render refuses
/// it. Until 2026-11-09 the walk that puts each item in its place panicked
/// on it, where it is run on the thread of the command that made two keys
/// equal.
#[test]
fn a_new_list_that_repeats_a_key_is_refused() {
    let a = |id: &str| section(id, id, vec![]);
    for (old, new) in [
        (vec![a("a")], vec![a("a"), a("a")]),
        (vec![a("p"), a("q")], vec![a("z"), a("z")]),
        (vec![], vec![a("a"), a("a")]),
    ] {
        let refused = list_changes(&sections(), PartId(0), &old, &new, &Env::new(), &[]);
        assert!(
            matches!(refused, Err(pw_render::Blocked::DuplicateLoopKey { .. })),
            "{refused:?}"
        );
    }
}

#[test]
fn a_section_and_an_item_across_sections_are_the_outer_and_inner_lists_operations() {
    // An item that moves to another section leaves one list and enters the
    // other: two operations, one in each section, and the new one rendered.
    let tea = |items| section("tea", "Tea", items);
    let old = [
        coffee(vec![item("espresso", "Espresso"), item("matcha", "Matcha")]),
        tea(vec![item("sencha", "Sencha")]),
    ];
    let new = [
        coffee(vec![item("espresso", "Espresso")]),
        tea(vec![item("matcha", "Matcha"), item("sencha", "Sencha")]),
    ];
    let found = changes(&old, &new);
    assert_eq!(found.len(), 2, "{found:?}");
    let [
        ListChange::Set {
            changes: coffee_changes,
            ..
        },
        ListChange::Set {
            changes: tea_changes,
            ..
        },
    ] = found.as_slice()
    else {
        panic!("{found:?}")
    };
    assert!(matches!(
        coffee_changes.as_slice(),
        [(PartId(2), InstanceChange::List(l))] if matches!(l.as_slice(), [ListChange::Remove(_)])
    ));
    assert!(matches!(
        tea_changes.as_slice(),
        [(PartId(2), InstanceChange::List(l))]
            if matches!(l.as_slice(), [ListChange::InsertBefore { before: Some(_), .. }])
    ));
    // A new section is the outer list's insert, rendered with its items.
    let added = changes(
        &[coffee(vec![item("espresso", "Espresso")])],
        &[
            coffee(vec![item("espresso", "Espresso")]),
            tea(vec![item("sencha", "Sencha")]),
        ],
    );
    assert!(matches!(
        added.as_slice(),
        [ListChange::InsertAfter { html, .. }] if html.contains("Tea") && html.contains("Sencha")
    ));
    // Nothing changed is no change.
    assert!(changes(&old, &old).is_empty());
}
