//! **`pw_render::decode` reads the template IR as serde's derives do**
//! (the first press's ADR, "Next" 1). The browser's renderer reads its
//! requests with it, the server with the derives; the two must give one
//! answer, or the browser renders what the server would not.
//!
//! Held three ways: over the compiler's own IR (the store's, kiokun's);
//! over every part kind, each optional field present and absent; and over
//! every one-step mutation of those (each field removed, each value replaced
//! by one of another type, each list shortened and lengthened, an unknown
//! field added), where both must read the same value or both refuse.

use pw_render::decode;
use pw_render::ir::{
    Arm, Chunk, Context, ElementId, Part, PartId, Segment, StreamArm, Template, TitlePiece,
};
use serde_json::{Value, json};

/// The one answer both readers must give for `v`, as a part.
fn agree_on_part(v: &Value) -> Option<Part> {
    let ours = decode::part(v);
    let serdes = serde_json::from_value::<Part>(v.clone());
    match (ours, serdes) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a, b, "the two read {v} differently");
            Some(a)
        }
        (Err(_), Err(_)) => None,
        (ours, serdes) => panic!("the two disagree on {v}: ours {ours:?}, serde's {serdes:?}"),
    }
}

/// The same, as a template.
fn agree_on_template(v: &Value) -> Option<Template> {
    let ours = decode::template(v);
    let serdes = serde_json::from_value::<Template>(v.clone());
    match (ours, serdes) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a, b, "the two read {v} differently");
            Some(a)
        }
        (Err(_), Err(_)) => None,
        (ours, serdes) => panic!("the two disagree on {v}: ours {ours:?}, serde's {serdes:?}"),
    }
}

/// Every one-step mutation of `v`: each value replaced by one of another
/// kind; in an object each key removed, and an unknown one added; in a list
/// each item removed, and one more added; and each of those inside.
fn mutants(v: &Value) -> Vec<Value> {
    let others = [
        Value::Null,
        json!(true),
        json!(-1),
        json!(1.5),
        json!(4_294_967_296_u64),
        json!(7),
        json!("x"),
        json!([]),
        json!({}),
    ];
    let mut out: Vec<Value> = others.iter().filter(|o| *o != v).cloned().collect();
    match v {
        Value::Object(map) => {
            for (key, child) in map {
                let mut less = map.clone();
                less.remove(key);
                out.push(Value::Object(less));
                for c in mutants(child) {
                    let mut changed = map.clone();
                    changed.insert(key.clone(), c);
                    out.push(Value::Object(changed));
                }
            }
            let mut more = map.clone();
            more.insert("not_a_field".into(), json!(1));
            out.push(Value::Object(more));
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                let mut less = items.clone();
                less.remove(i);
                out.push(Value::Array(less));
                for c in mutants(child) {
                    let mut changed = items.clone();
                    changed[i] = c;
                    out.push(Value::Array(changed));
                }
            }
            let mut more = items.clone();
            more.push(json!("more"));
            out.push(Value::Array(more));
        }
        _ => {}
    }
    out
}

/// **serde's two leniencies, refused here** (the first press's review): its
/// derives also read a struct, or a tagged variant, written as a list of its
/// fields in order, and an enum's unit variant written as a one-key map
/// (`{"text": null}`) as well as by its name. The compiler writes neither, and
/// this reads neither: the safe direction, reading nothing serde refuses.
#[test]
fn serdes_two_leniencies_are_refused_here() {
    let written = json!({"part": "text", "id": 1, "value": "v", "context": "text"});
    assert_eq!(agree_on_part(&written), Some(text(1, "v")));
    for lenient in [
        json!({"part": "text", "id": 1, "value": "v", "context": {"text": null}}),
        json!(["text", 1, "v", "text"]),
    ] {
        assert_eq!(
            serde_json::from_value::<Part>(lenient.clone()).ok(),
            Some(text(1, "v")),
            "serde reads {lenient}"
        );
        assert!(decode::part(&lenient).is_err(), "this refuses {lenient}");
    }
}

fn st(s: &str) -> Chunk {
    Chunk::Static(s.to_string())
}

fn text(id: u32, value: &str) -> Part {
    Part::Text {
        id: PartId(id),
        value: value.into(),
        context: Context::Text,
    }
}

/// Every kind of part, and each optional field both set and at its default.
fn every_kind() -> Vec<Part> {
    let pieces = vec![
        TitlePiece::Text("Store: ".into()),
        TitlePiece::Value("store.name".into()),
    ];
    vec![
        text(1, "greeting"),
        Part::Text {
            id: PartId(2),
            value: "note".into(),
            context: Context::Content,
        },
        Part::Attribute {
            id: PartId(3),
            owner: ElementId(1),
            name: "href".into(),
            value: "link".into(),
            context: Context::Url,
        },
        Part::BooleanAttribute {
            id: PartId(4),
            owner: ElementId(2),
            name: "disabled".into(),
            value: "busy".into(),
        },
        Part::Event {
            id: PartId(5),
            owner: ElementId(3),
            event: "press".into(),
            handler: "0123456789abcdef".into(),
            name: String::new(),
            captures: vec![],
            renames: Default::default(),
            modifiers: vec![],
        },
        Part::Event {
            id: PartId(6),
            owner: ElementId(3),
            event: "keydown".into(),
            handler: "fedcba9876543210".into(),
            name: "add".into(),
            captures: vec!["item.id".into(), "note".into()],
            renames: [("item".to_string(), "row".to_string())]
                .into_iter()
                .collect(),
            modifiers: vec!["prevent".into(), "stop".into()],
        },
        Part::Conditional {
            id: PartId(7),
            value: "open".into(),
            then: vec![st("<p>"), Chunk::Dynamic(text(8, "a")), st("</p>")],
            otherwise: vec![],
        },
        Part::Each {
            id: PartId(9),
            collection: "cart.lines".into(),
            binding: "line".into(),
            key: Some("id".into()),
            body: vec![
                st("<li>"),
                Chunk::Dynamic(text(10, "line.name")),
                st("</li>"),
            ],
        },
        Part::Each {
            id: PartId(11),
            collection: "tags".into(),
            binding: "t".into(),
            key: None,
            body: vec![],
        },
        Part::Match {
            id: PartId(12),
            value: "panel".into(),
            arms: vec![
                Arm {
                    case: "Some".into(),
                    binding: Some("x".into()),
                    fields: vec![],
                    body: vec![Chunk::Dynamic(text(13, "x"))],
                },
                Arm {
                    case: "circle".into(),
                    binding: None,
                    fields: vec!["r".into(), "at".into()],
                    body: vec![],
                },
                Arm {
                    case: "None".into(),
                    binding: None,
                    fields: vec![],
                    body: vec![st("none")],
                },
            ],
        },
        Part::Stream {
            id: PartId(14),
            query: "recs".into(),
            args: vec!["store.id".into()],
            streamed: true,
            placeholder: vec![st("…")],
            ready: StreamArm {
                binding: Some("picks".into()),
                body: vec![Chunk::Dynamic(text(15, "picks.first"))],
            },
            failed: StreamArm::default(),
        },
        Part::InterpolatedAttribute {
            id: PartId(16),
            owner: ElementId(4),
            name: "aria-label".into(),
            segments: vec![
                Segment::Static("Remove ".into()),
                Segment::Value("line.name".into()),
            ],
            context: Context::Attribute,
        },
        Part::Instance {
            id: PartId(17),
            path: "t.Card".into(),
            args: vec![("label".into(), "title".into())],
            elements: 2,
            deepest: 3,
        },
        Part::Instance {
            id: PartId(18),
            path: "t.Tree".into(),
            args: vec![],
            elements: 0,
            deepest: 0,
        },
        Part::Title {
            id: PartId(19),
            pieces: pieces.clone(),
        },
        Part::Meta {
            id: PartId(20),
            attribute: "name".into(),
            key: "description".into(),
            content: pieces.clone(),
        },
        Part::Canonical {
            id: PartId(21),
            route: "/stores/{id}".into(),
            args: vec![("id".into(), TitlePiece::Value("store.id".into()))],
        },
        Part::RawHtml {
            id: PartId(22),
            value: "markup".into(),
            capability: "TrustedHtml".into(),
        },
        Part::Blocked {
            reason: "not represented".into(),
            at: "app.pw:3:7".into(),
        },
        Part::Attribute {
            id: PartId(u32::MAX),
            owner: ElementId(0),
            name: "style".into(),
            value: "s".into(),
            context: Context::Style,
        },
        Part::Text {
            id: PartId(23),
            value: "raw".into(),
            context: Context::RawHtml,
        },
    ]
}

#[test]
fn every_part_kind_is_read_as_serde_reads_it() {
    for part in every_kind() {
        let v = serde_json::to_value(&part).expect("serialized");
        assert_eq!(agree_on_part(&v).as_ref(), Some(&part), "{v}");
    }
}

/// A field `#[serde(default)]`s, absent, and an `Option`, absent or `null`:
/// read as the derives read them, not merely as round trips write them.
#[test]
fn absent_and_null_fields_are_read_as_serde_reads_them() {
    for v in [
        json!({"part": "event", "id": 1, "owner": 2, "event": "press", "handler": "h"}),
        json!({"part": "instance", "id": 1, "path": "t.V", "args": []}),
        json!({"part": "each", "id": 1, "collection": "xs", "binding": "x", "body": []}),
        json!({"part": "each", "id": 1, "collection": "xs", "binding": "x", "key": null, "body": []}),
        json!({"part": "match", "id": 1, "value": "v", "arms": [
            {"case": "None", "body": []},
            {"case": "Some", "binding": null, "body": []}
        ]}),
        json!({"part": "stream", "id": 1, "query": "q", "args": [], "streamed": false,
            "placeholder": [], "ready": {"body": []}, "failed": {"binding": null, "body": []}}),
    ] {
        assert!(agree_on_part(&v).is_some(), "both read {v}");
    }
}

#[test]
fn every_one_step_mutation_of_every_kind_gets_one_answer() {
    let mut read = 0;
    let mut refused = 0;
    for part in every_kind() {
        let v = serde_json::to_value(&part).expect("serialized");
        for m in mutants(&v) {
            match agree_on_part(&m) {
                Some(_) => read += 1,
                None => refused += 1,
            }
        }
    }
    // Both outcomes are reached, so the sweep exercises acceptance and
    // refusal alike, not one of them a thousand times.
    assert!(
        read > 100 && refused > 1_000,
        "read {read}, refused {refused}"
    );
}

/// The IR the compiler wrote, kept in the repository: the store's pages and
/// kiokun's. Each template whole, and each of its parts alone and mutated.
#[test]
fn the_compilers_ir_is_read_as_serde_reads_it() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut parts = 0;
    for file in [
        "spikes/own-renderer/store-ir.json",
        "docs/evidence/E10/kiokun/templates.json",
    ] {
        let text = std::fs::read_to_string(root.join(file)).expect(file);
        let v: Value = serde_json::from_str(&text).expect(file);
        let ours = decode::templates(&v).expect(file);
        let serdes: Vec<Template> = serde_json::from_value(v.clone()).expect(file);
        assert_eq!(ours, serdes, "{file}");
        for template in v.as_array().expect("templates") {
            assert!(agree_on_template(template).is_some());
            let mut stack: Vec<&Value> = vec![template];
            while let Some(node) = stack.pop() {
                match node {
                    Value::Object(map) => {
                        if map.contains_key("part") {
                            parts += 1;
                            assert!(agree_on_part(node).is_some(), "{node}");
                            for m in mutants(node).iter().take(2_000) {
                                agree_on_part(m);
                            }
                        }
                        stack.extend(map.values());
                    }
                    Value::Array(items) => stack.extend(items),
                    _ => {}
                }
            }
        }
    }
    assert!(parts > 50, "the corpus holds {parts} parts");
}

#[test]
fn a_template_and_its_mutations_get_one_answer() {
    let template = Template {
        path: "t.Page".into(),
        name: "Page".into(),
        params: vec!["store".into()],
        schema: "s1".into(),
        chunks: vec![
            st("<h1>"),
            Chunk::Dynamic(text(0, "store.name")),
            st("</h1>"),
        ],
    };
    let v = serde_json::to_value(&template).expect("serialized");
    assert_eq!(agree_on_template(&v), Some(template));
    for m in mutants(&v) {
        agree_on_template(&m);
    }
    let list = json!([v.clone(), v]);
    assert_eq!(
        decode::templates(&list).expect("two"),
        serde_json::from_value::<Vec<Template>>(list).expect("two")
    );
}
