//! **What changed in a block, part by part** (ADR-0320).
//!
//! A host renders a block that reads a query's value, and renders it again
//! when what it renders changed (ADR-0146). While it shows the arm it
//! showed, nothing inside it is a new element: each part is set where it is,
//! as a row's are (ADR-0168, ADR-0178), so a field keeps its focus and a
//! live region inside the block is the same region, told its new text. One
//! that shows another arm is rendered again, whole.
//!
//! Found by W9 (2026-10-12): the search page's status sentence, inside a
//! `{#match q}` on the page's parameter, was not told when the search's
//! result changed.

use pw_render::*;

fn st(s: &str) -> Chunk {
    Chunk::Static(s.to_string())
}

fn text(id: u32, value: &str) -> Chunk {
    Chunk::Dynamic(Part::Text {
        id: PartId(id),
        value: value.into(),
        context: Context::Text,
    })
}

/// `{#match q}{:Some(asked)}<p role="status">{search.said}</p><h2>{asked}
/// </h2><ul>{#each search.stores as store (store.id)}<li>{store.name}</li>
/// {/each}</ul>{:None}<p>Search the stores.</p>{/match}`: a block a page's
/// parameter decides, which renders a query's value.
fn search() -> Template {
    Template {
        path: "t.Search".into(),
        name: "Search".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![Chunk::Dynamic(Part::Match {
            id: PartId(0),
            value: "q".into(),
            arms: vec![
                Arm {
                    case: "Some".into(),
                    binding: Some("asked".into()),
                    fields: vec![],
                    body: vec![
                        st("<p role=\"status\">"),
                        text(1, "search.said"),
                        st("</p><h2>"),
                        text(2, "asked"),
                        st("</h2><ul>"),
                        Chunk::Dynamic(Part::Each {
                            id: PartId(3),
                            collection: "search.stores".into(),
                            binding: "store".into(),
                            key: Some("id".into()),
                            row_anchor: Default::default(),
                            body: vec![st("<li>"), text(4, "store.name"), st("</li>")],
                        }),
                        st("</ul>"),
                    ],
                },
                Arm {
                    case: "None".into(),
                    binding: None,
                    fields: vec![],
                    body: vec![st("<p>Search the stores.</p>")],
                },
            ],
        })],
    }
}

fn some(s: &str) -> Value {
    Value::Variant {
        case: "Some".into(),
        payload: Some(Box::new(Value::Text(s.into()))),
    }
}

fn store(id: &str, name: &str) -> Value {
    Value::Record(
        [
            ("id".to_string(), Value::Text(id.into())),
            ("name".to_string(), Value::Text(name.into())),
        ]
        .into(),
    )
}

/// The page's values: its parameter `q`, and the search's sentence and
/// stores.
fn env(q: Value, said: &str, stores: Vec<Value>) -> Env {
    Env::new()
        .set("q", q)
        .set("search.said", Value::Text(said.into()))
        .set("search.stores", Value::List(stores))
}

fn changes(before: &Env, after: &Env) -> Option<Vec<(PartId, InstanceChange)>> {
    block_changes(&search(), PartId(0), before, after, &[]).expect("derived")
}

#[test]
fn nothing_changed_is_no_change() {
    let a = env(some("coffee"), "No stores match.", vec![]);
    assert_eq!(changes(&a, &a), Some(vec![]));
}

/// **A part inside an arm that stays is set where it is**: the search's
/// sentence, inside the `Some` arm of a match on the page's parameter. Until
/// 2026-10-12 nothing told it.
#[test]
fn a_part_inside_an_arm_that_stays_is_set_where_it_is() {
    assert_eq!(
        changes(
            &env(some("flat white"), "No stores or dishes match.", vec![]),
            &env(some("flat white"), "1 store matches.", vec![]),
        ),
        Some(vec![(
            PartId(1),
            InstanceChange::Text("1 store matches.".into())
        )])
    );
}

/// **A keyed list inside it is changed where it is** (ADR-0181): its own
/// operations, each row that stayed keeping its nodes.
#[test]
fn a_keyed_list_inside_it_is_changed_where_it_is() {
    let got = changes(
        &env(some("tea"), "1 store matches.", vec![store("a", "Harbor")]),
        &env(
            some("tea"),
            "2 stores match.",
            vec![store("a", "Harbor"), store("b", "Pier")],
        ),
    )
    .expect("set where it is");
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(
        got[0],
        (PartId(1), InstanceChange::Text("2 stores match.".into()))
    );
    let (PartId(3), InstanceChange::List(list)) = &got[1] else {
        panic!("the list's changes: {got:?}");
    };
    assert!(
        matches!(list.as_slice(), [ListChange::InsertAfter { html, .. }] if html.contains("Pier")),
        "{list:?}"
    );
}

/// **What the arm binds is read on each side**: the arm's payload, given
/// anew, sets the part that reads it.
#[test]
fn what_the_arm_binds_is_read_on_each_side() {
    assert_eq!(
        changes(
            &env(some("tea"), "No stores match.", vec![]),
            &env(some("green tea"), "No stores match.", vec![]),
        ),
        Some(vec![(PartId(2), InstanceChange::Text("green tea".into()))])
    );
}

/// **A block that shows another arm is rendered again**, whole: nothing
/// inside it is the element it was.
#[test]
fn a_block_that_shows_another_arm_is_rendered_again() {
    assert_eq!(
        changes(
            &env(some("tea"), "No stores match.", vec![]),
            &env(
                Value::Variant {
                    case: "None".into(),
                    payload: None,
                },
                "",
                vec![]
            ),
        ),
        None
    );
}

/// `<main>{#if open}<p>{status}</p>{#if late}<b>{why}</b>{/if}{:else}<p>
/// Closed.</p>{/if}</main>`: a conditional, and a block inside it.
fn opened() -> Template {
    Template {
        path: "t.Open".into(),
        name: "Open".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![
            st("<main>"),
            Chunk::Dynamic(Part::Conditional {
                id: PartId(0),
                value: "open".into(),
                then: vec![
                    st("<p>"),
                    text(1, "status"),
                    st("</p>"),
                    Chunk::Dynamic(Part::Conditional {
                        id: PartId(2),
                        value: "late".into(),
                        then: vec![st("<b>"), text(3, "why"), st("</b>")],
                        otherwise: vec![],
                    }),
                ],
                otherwise: vec![st("<p>Closed.</p>")],
            }),
            st("</main>"),
        ],
    }
}

fn opened_env(open: bool, status: &str, late: bool, why: &str) -> Env {
    Env::new()
        .set("open", Value::Bool(open))
        .set("status", Value::Text(status.into()))
        .set("late", Value::Bool(late))
        .set("why", Value::Text(why.into()))
}

/// **A block inside one is looked into while it decides as it did**, and
/// renders the outer block again when it decides otherwise.
#[test]
fn a_block_inside_is_looked_into_while_it_decides_as_it_did() {
    let changes = |before: &Env, after: &Env| {
        block_changes(&opened(), PartId(0), before, after, &[]).expect("derived")
    };
    assert_eq!(
        changes(
            &opened_env(true, "Open", true, "busy"),
            &opened_env(true, "Open late", true, "very busy"),
        ),
        Some(vec![
            (PartId(1), InstanceChange::Text("Open late".into())),
            (PartId(3), InstanceChange::Text("very busy".into())),
        ])
    );
    assert_eq!(
        changes(
            &opened_env(true, "Open", false, ""),
            &opened_env(true, "Open", true, "busy"),
        ),
        None
    );
    // The outer block shows its other branch: rendered again.
    assert_eq!(
        changes(
            &opened_env(true, "Open", false, ""),
            &opened_env(false, "Open", false, ""),
        ),
        None
    );
}

/// A part the template does not have is the caller's mistake.
#[test]
fn a_block_that_is_not_there_is_refused() {
    let a = opened_env(true, "Open", false, "");
    assert!(block_changes(&opened(), PartId(9), &a, &a, &[]).is_err());
}
