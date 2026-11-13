//! **A stream region, pending and settled** (ADR-0148).
//!
//! Pending, a streamed region shows its placeholder inside the range the
//! platform's out-of-order patch replaces: `<?start name="pw-N">` ..
//! `<?end>`. Settled, it shows the arm its query settled to, with the value
//! bound. The patch that fills a pending region holds what a full render of
//! the settled region holds between its anchors, byte for byte, so a page
//! patched and a page rendered settled are one document.

use pw_render::*;

fn template(chunks: Vec<Chunk>) -> Template {
    Template {
        path: "t.T".into(),
        name: "T".into(),
        params: vec![],
        schema: "s".into(),
        chunks,
    }
}

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

/// `<stream query={Recs(id)}>`: a placeholder, the names of what it
/// answered, and why it did not, `{#match why}{:Some(e)}..{:None}..`.
fn page(streamed: bool) -> Template {
    template(vec![
        st("<section>"),
        Chunk::Dynamic(Part::Stream {
            id: PartId(0),
            query: "t.Recs".into(),
            args: vec!["id".into()],
            streamed,
            placeholder: vec![st("<p>Finding</p>")],
            ready: StreamArm {
                binding: Some("items".into()),
                body: vec![
                    st("<ul>"),
                    Chunk::Dynamic(Part::Each {
                        id: PartId(1),
                        collection: "items".into(),
                        binding: "item".into(),
                        key: None,
                        row_anchor: Default::default(),
                        body: vec![st("<li>"), text(2, "item.name"), st("</li>")],
                    }),
                    st("</ul>"),
                ],
            },
            failed: StreamArm {
                binding: Some("why".into()),
                body: vec![Chunk::Dynamic(Part::Match {
                    id: PartId(3),
                    value: "why".into(),
                    arms: vec![
                        Arm {
                            case: "some".into(),
                            binding: Some("e".into()),
                            fields: vec![],
                            body: vec![st("<p>declared</p>")],
                        },
                        Arm {
                            case: "none".into(),
                            binding: None,
                            fields: vec![],
                            body: vec![st("<p>no answer</p>")],
                        },
                    ],
                })],
            },
        }),
        st("</section>"),
    ])
}

fn items(names: &[&str]) -> Value {
    Value::List(
        names
            .iter()
            .map(|n| {
                Value::Record(
                    [("name".to_string(), Value::Text(n.to_string()))]
                        .into_iter()
                        .collect(),
                )
            })
            .collect(),
    )
}

fn down() -> Value {
    Value::Variant {
        case: "down".into(),
        payload: None,
    }
}

#[test]
fn a_pending_streamed_region_shows_its_placeholder_in_a_patchable_range() {
    let html = render(&page(true), &Env::new(), &[]).expect("renders");
    assert_eq!(
        html,
        "<section><!--pw:s0--><?start name=\"pw-0\"><p>Finding</p><?end><!--pw:e0--></section>"
    );
}

#[test]
fn a_region_the_page_waits_for_is_never_pending() {
    // Nothing gave its answer: a region with no placeholder has nothing to
    // show, and an empty one would look like an answer.
    assert_eq!(
        render(&page(false), &Env::new(), &[]),
        Err(Blocked::MissingValue {
            path: "the answer of stream 0".into()
        })
    );
}

#[test]
fn a_settled_region_shows_its_arm_with_the_value_bound() {
    let ready = Env::new().settle(PartId(0), Settled::Ready(items(&["Cortado", "Mocha"])));
    let html = render(&page(false), &ready, &[]).expect("renders");
    assert_eq!(
        html,
        "<section><!--pw:s0--><ul><!--pw:s1--><li><!--pw:s2-->Cortado<!--pw:e2--></li>\
         <li><!--pw:s2-->Mocha<!--pw:e2--></li><!--pw:e1--></ul><!--pw:e0--></section>"
    );
    // Its failure: `Some` of the declared error, `None` for the host's.
    let declared = Env::new().settle(PartId(0), Settled::Failed(Some(down())));
    assert!(
        render(&page(true), &declared, &[])
            .expect("renders")
            .contains("<!--pw:s3--><p>declared</p><!--pw:e3-->")
    );
    let host = Env::new().settle(PartId(0), Settled::Failed(None));
    assert!(
        render(&page(true), &host, &[])
            .expect("renders")
            .contains("<!--pw:s3--><p>no answer</p><!--pw:e3-->")
    );
}

#[test]
fn the_patch_holds_what_the_settled_region_holds() {
    for outcome in [
        Settled::Ready(items(&["Cortado"])),
        Settled::Failed(Some(down())),
        Settled::Failed(None),
    ] {
        let env = Env::new().settle(PartId(0), outcome);
        let whole = render(&page(true), &env, &[]).expect("renders");
        let inner = whole
            .strip_prefix("<section><!--pw:s0-->")
            .and_then(|r| r.strip_suffix("<!--pw:e0--></section>"))
            .expect("the region's range");
        // And a comment after it, which says the template has all arrived
        // (ADR-0223).
        assert_eq!(
            settled_patch(&page(true), PartId(0), &env, &[]).expect("a patch"),
            format!("<template for=\"pw-0\">{inner}</template><!--/pw-0-->")
        );
    }
    // And a stream not settled has no patch to send.
    assert!(settled_patch(&page(true), PartId(0), &Env::new(), &[]).is_err());
}
