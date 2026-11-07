//! **The renderer, compiled for the browser** (ADR-0130).
//!
//! A part a signal decides is rendered again in the browser when the signal
//! changes. This is `runtime/pw-render`, the server's renderer, built for
//! `wasm32-unknown-unknown`, so the browser and the server render a part with
//! one implementation: the same escaping in each of the five contexts, the
//! same anchors, the same refusals. `pw-resume-wasm` exists for the same
//! reason: a second implementation of a security-relevant function is two
//! things that can disagree, and the disagreement is silent.
//!
//! # The ABI
//!
//! ```text
//! alloc(len)                  -> ptr   the caller writes a request here
//! render_part(ptr, len)       -> code  0 done, 1 refused, 2 malformed
//! render_instance(ptr, len)   -> code  one instance of a loop (ADR-0172)
//! instance_changes(ptr, len)  -> code  what changed in one (ADR-0172)
//! attribute_value(ptr, len)   -> code  an attribute as written (ADR-0172)
//! warm()                      -> code  each of these run once on a sample
//! out_ptr() / out_len()                the answer, or why it was refused
//! ```
//!
//! A request is JSON: `{ "part": <a template part>, "values": { name: value } }`,
//! each value in the wire form a page's signals are held in, and
//! `"templates"`, each view's template an instance in the part is rendered
//! with (ADR-0203). An instance's
//! request names its row as `item`; an instance's changes, its row before and
//! after as `was` and `now`.
//!
//! A speculation's parts are rendered here too (ADR-0172): its rows, their
//! changes and its attributes, from the speculated value, with the server's
//! own escaping and anchors.

use std::sync::Mutex;

static OUT: Mutex<String> = Mutex::new(String::new());

/// Bytes the caller may write a request into. Leaked deliberately, as
/// `pw-resume-wasm` leaks its manifests: a page renders a handful of parts
/// per press, and a free() across the ABI is more ways to be wrong.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = vec![0u8; len];
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// **Render one part** with the values given.
///
/// # Safety
///
/// `ptr` must point at `len` bytes the caller obtained from [`alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn render_part(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let (code, out) = render(bytes);
    *OUT.lock().unwrap() = out;
    code
}

#[unsafe(no_mangle)]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.lock().unwrap().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn out_len() -> usize {
    OUT.lock().unwrap().len()
}

/// **Render one instance of a loop** (ADR-0172).
///
/// # Safety
///
/// As [`render_part`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn render_instance(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let (code, out) = instance(bytes);
    *OUT.lock().unwrap() = out;
    code
}

/// **What changed in one instance of a loop** (ADR-0172).
///
/// # Safety
///
/// As [`render_part`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instance_changes(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let (code, out) = changes(bytes);
    *OUT.lock().unwrap() = out;
    code
}

/// **An attribute's value, as the document writes it** (ADR-0172).
///
/// # Safety
///
/// As [`render_part`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn attribute_value(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let (code, out) = attribute(bytes);
    *OUT.lock().unwrap() = out;
    code
}

/// **What an element's handlers capture, as the document writes it**
/// (ADR-0217).
///
/// # Safety
///
/// As [`render_part`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn captures_value(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let (code, out) = captures(bytes);
    *OUT.lock().unwrap() = out;
    code
}

/// **Each request run once, on a sample, and thrown away** (the first
/// press's ADR). A browser compiles a WebAssembly function the first time
/// it is called (V8 compiles lazily: TodoMVC's first add compiled 521 of
/// this module's functions, 24 ms of it at 4x CPU), so a page's first change
/// paid for compiling the paths it renders by. The runtime calls this when
/// the page is idle. The sample holds every part a block can hold, in each
/// context, at values of each wire form (`warm.json`). `0`, or the first
/// code that was not; the answer is left as it was.
#[unsafe(no_mangle)]
pub extern "C" fn warm() -> u32 {
    warm_up()
}

/// The sample [`warm`] runs: one request each export reads its own fields
/// of, and an attribute's, whose part is its own. Each is read as any
/// request is, so the warm-up adds no reader of its own to the module.
const WARM: &str = include_str!("warm.json");
const WARM_ATTRIBUTE: &str = r#"{"part":{"part":"attribute","id":1,"owner":1,"name":"aria-label","value":"flag","context":"attribute"},"values":{"flag":"on"}}"#;

/// [`warm`], each request in turn.
pub fn warm_up() -> u32 {
    let sample = WARM.as_bytes();
    [
        render(sample).0,
        instance(sample).0,
        changes(sample).0,
        captures(sample).0,
        attribute(WARM_ATTRIBUTE.as_bytes()).0,
    ]
    .into_iter()
    .find(|&code| code != 0)
    .unwrap_or(0)
}

/// A request read: its JSON, its part, the values it gives, set into a
/// rendering environment, and the templates of the views it renders an
/// instance of (ADR-0203). `Err((2, why))` for a request that is not one.
///
/// The part and the templates are read by [`pw_render::decode`], which
/// reads the IR as serde's derives do without their generic visitors, which
/// were most of this module's bytes (ADR-0399).
fn read(request: &[u8]) -> Result<Read, (u32, String)> {
    let request = serde_json::from_slice::<serde_json::Value>(request)
        .map_err(|_| (2, "the request is not JSON".to_string()))?;
    let part =
        pw_render::decode::part(&request["part"]).map_err(|e| (2, format!("the part: {e}")))?;
    let mut env = pw_render::Env::new();
    if let Some(values) = request["values"].as_object() {
        for (name, value) in values {
            env = env.set(name, wire(value, name)?);
        }
    }
    let templates = match request.get("templates") {
        None => Vec::new(),
        Some(t) => {
            pw_render::decode::templates(t).map_err(|e| (2, format!("the templates: {e}")))?
        }
    };
    Ok((request, part, env, templates))
}

/// A value the request gives, or a malformed graph refused as a request
/// that is not one (ADR-0205).
fn wire(value: &serde_json::Value, name: &str) -> Result<pw_render::Value, (u32, String)> {
    pw_render::Value::from_wire(value).map_err(|e| (2, format!("`{name}`: {e}")))
}

/// What [`read`] reads.
type Read = (
    serde_json::Value,
    pw_render::ir::Part,
    pw_render::Env,
    Vec<pw_render::Template>,
);

/// The one-part template a request's part is rendered in.
fn template_of(part: pw_render::ir::Part) -> pw_render::Template {
    pw_render::Template {
        path: String::new(),
        name: String::new(),
        params: Vec::new(),
        schema: String::new(),
        chunks: vec![pw_render::ir::Chunk::Dynamic(part)],
    }
}

/// The request's part, rendered: `(0, html)`, `(1, why)` refused by the
/// renderer, or `(2, why)` for a request that is not one.
pub fn render(request: &[u8]) -> (u32, String) {
    let (_, part, env, templates) = match read(request) {
        Ok(read) => read,
        Err(refused) => return refused,
    };
    match pw_render::render(&template_of(part), &env, &templates) {
        Ok(html) => (0, html),
        Err(why) => (1, format!("{why:?}")),
    }
}

/// One instance of the request's loop, for its `item`: `(0, html)`.
pub fn instance(request: &[u8]) -> (u32, String) {
    let (request, part, env, templates) = match read(request) {
        Ok(read) => read,
        Err(refused) => return refused,
    };
    let Some(each) = part.id() else {
        return (2, "the part is no loop".to_string());
    };
    let item = match wire(&request["item"], "item") {
        Ok(item) => item,
        Err(refused) => return refused,
    };
    match pw_render::render_instance(&template_of(part), each, &item, &env, &templates) {
        Ok(html) => (0, html),
        Err(why) => (1, format!("{why:?}")),
    }
}

/// What changed in one instance, from `was` to `now`: `(0, json)`, the JSON
/// each change as `[part, {"text": ..}]` or `[part, {"attribute": name,
/// "value": ..}]`, or `null` where the instance must be rendered again.
pub fn changes(request: &[u8]) -> (u32, String) {
    let (request, part, env, templates) = match read(request) {
        Ok(read) => read,
        Err(refused) => return refused,
    };
    let Some(each) = part.id() else {
        return (2, "the part is no loop".to_string());
    };
    let (was, now) = match (wire(&request["was"], "was"), wire(&request["now"], "now")) {
        (Ok(was), Ok(now)) => (was, now),
        (Err(refused), _) | (_, Err(refused)) => return refused,
    };
    match pw_render::instance_changes(&template_of(part), each, &was, &now, &env, &templates) {
        Ok(None) => (0, "null".to_string()),
        // A list inside a speculated row is refused at build (ADR-0170), and
        // the browser sets no list's operations of its own: rendered again.
        Ok(Some(changes))
            if changes
                .iter()
                .any(|(_, c)| matches!(c, pw_render::InstanceChange::List(_))) =>
        {
            (0, "null".to_string())
        }
        Ok(Some(changes)) => {
            let out: Vec<serde_json::Value> = changes
                .into_iter()
                .map(|(id, change)| match change {
                    pw_render::InstanceChange::Text(text) => {
                        serde_json::json!([id.0, { "text": text }])
                    }
                    pw_render::InstanceChange::Attribute { name, value } => {
                        serde_json::json!([id.0, { "attribute": name, "value": value }])
                    }
                    pw_render::InstanceChange::List(_) => serde_json::Value::Null,
                })
                .collect();
            (0, serde_json::Value::Array(out).to_string())
        }
        Err(why) => (1, format!("{why:?}")),
    }
}

/// The request's attribute, as the document writes it: `(0, json)`, the JSON
/// `[name, value]`, `value` `null` for a boolean attribute that is absent.
pub fn attribute(request: &[u8]) -> (u32, String) {
    let (_, part, env, _) = match read(request) {
        Ok(read) => read,
        Err(refused) => return refused,
    };
    match pw_render::attribute_value(&part, &env) {
        Ok(Some((name, value))) => (0, serde_json::json!([name, value]).to_string()),
        Ok(None) => (2, "the part is no attribute".to_string()),
        Err(why) => (1, format!("{why:?}")),
    }
}

/// **What an element's handlers capture** (ADR-0217), given its handler
/// parts as `parts`: `(0, json)`, the attribute's value as the document writes
/// it, a JSON string, or `null` where nothing is captured.
pub fn captures(request: &[u8]) -> (u32, String) {
    let request = match serde_json::from_slice::<serde_json::Value>(request) {
        Ok(r) => r,
        Err(_) => return (2, "the request is not JSON".to_string()),
    };
    let parts = match pw_render::decode::parts(&request["parts"]) {
        Ok(p) => p,
        Err(e) => return (2, format!("the parts: {e}")),
    };
    let mut env = pw_render::Env::new();
    if let Some(values) = request["values"].as_object() {
        for (name, value) in values {
            match wire(value, name) {
                Ok(v) => env = env.set(name, v),
                Err(refused) => return refused,
            }
        }
    }
    match pw_render::element_captures(&parts, &env) {
        Ok(value) => (0, serde_json::json!(value).to_string()),
        Err(why) => (1, format!("{why:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::render;

    /// The browser's renderer escapes as the server's does, because it is
    /// the server's: a text part holding markup is text.
    #[test]
    fn a_text_part_is_escaped_as_the_server_escapes_it() {
        let request = br#"{
            "part": { "part": "text", "id": 3, "value": "greeting", "context": "text" },
            "values": { "greeting": "<b>hi</b> & bye" }
        }"#;
        assert_eq!(
            render(request),
            (
                0,
                "<!--pw:s3-->&lt;b&gt;hi&lt;/b&gt; &amp; bye<!--pw:e3-->".to_string()
            )
        );
    }

    /// **An element's captures, as the server writes them** (ADR-0217):
    /// every handler's, one attribute, escaped for one; none where nothing
    /// is captured.
    #[test]
    fn an_elements_captures_are_written_as_the_server_writes_them() {
        let request = br#"{
            "parts": [
                { "part": "event", "id": 4, "owner": 2, "event": "press", "handler": "h1",
                  "captures": ["cart.lines"] },
                { "part": "event", "id": 5, "owner": 2, "event": "keydown", "handler": "h2",
                  "captures": ["note"] }
            ],
            "values": { "cart": { "lines": [1, 2] }, "note": "a \"b\"" }
        }"#;
        let (code, out) = super::captures(request);
        assert_eq!(code, 0, "{out}");
        let written: Option<String> = serde_json::from_str(&out).expect("JSON");
        assert_eq!(
            written.as_deref(),
            Some(
                "{&quot;cart&quot;:{&quot;lines&quot;:[1,2]},&quot;note&quot;:&quot;a \\&quot;b\\&quot;&quot;}"
            )
        );
        let none = br#"{
            "parts": [{ "part": "event", "id": 4, "owner": 2, "event": "press", "handler": "h1" }],
            "values": {}
        }"#;
        assert_eq!(super::captures(none), (0, "null".to_string()));
    }

    /// A `{#match}` renders the arm its signal's case names.
    #[test]
    fn a_match_renders_the_case_it_is_given() {
        let request = br#"{
            "part": { "part": "match", "id": 5, "value": "panel", "arms": [
                { "case": "shut", "body": [{ "chunk": "static", "value": "<p>none</p>" }] },
                { "case": "cart", "body": [{ "chunk": "static", "value": "<aside>cart</aside>" }] }
            ] },
            "values": { "panel": { "$case": "cart" } }
        }"#;
        assert_eq!(
            render(request),
            (0, "<!--pw:s5--><aside>cart</aside><!--pw:e5-->".to_string())
        );
    }

    #[test]
    fn a_request_that_is_not_one_is_refused() {
        assert_eq!(render(b"not json").0, 2);
        assert_eq!(render(br#"{ "part": 1, "values": {} }"#).0, 2);
    }

    use super::{attribute, changes, instance};
    use pw_render::ir::{Chunk, Context, ElementId, Part, PartId, Segment};

    /// `{#each cart.lines as line (line.id)}<li data-pw="0" aria-label="Remove
    /// {line.name}"><span>{line.name}</span>{#if line.gift}..{/if}</li>{/each}`
    fn lines() -> Part {
        let st = |s: &str| Chunk::Static(s.to_string());
        Part::Each {
            id: PartId(0),
            collection: "cart.lines".into(),
            binding: "line".into(),
            key: Some("id".into()),
            body: vec![
                st("<li data-pw=\"0\" "),
                Chunk::Dynamic(Part::InterpolatedAttribute {
                    id: PartId(1),
                    owner: ElementId(0),
                    name: "aria-label".into(),
                    segments: vec![
                        Segment::Static("Remove ".into()),
                        Segment::Value("line.name".into()),
                    ],
                    context: Context::Attribute,
                }),
                st("><span>"),
                Chunk::Dynamic(Part::Text {
                    id: PartId(2),
                    value: "line.name".into(),
                    context: Context::Text,
                }),
                st("</span>"),
                Chunk::Dynamic(Part::Conditional {
                    id: PartId(3),
                    value: "line.gift".into(),
                    then: vec![st("<b>gift</b>")],
                    otherwise: vec![],
                }),
                st("</li>"),
            ],
        }
    }

    fn request(fields: serde_json::Value) -> Vec<u8> {
        let mut r = serde_json::json!({ "part": lines(), "values": {} });
        for (k, v) in fields.as_object().expect("fields") {
            r[k] = v.clone();
        }
        r.to_string().into_bytes()
    }

    /// The browser's instance is the server's, from one renderer: its
    /// anchors, its escaping, its parts (ADR-0172).
    #[test]
    fn an_instance_is_rendered_as_the_server_renders_it() {
        let item = serde_json::json!({ "id": "a", "name": "Tea & Cake", "gift": false });
        let (code, html) = instance(&request(serde_json::json!({ "item": item })));
        assert_eq!(code, 0, "{html}");
        let env = pw_render::Env::new();
        let template = super::template_of(lines());
        let server = pw_render::render_instance(
            &template,
            PartId(0),
            &pw_render::Value::from_wire(&item).expect("a value"),
            &env,
            &[],
        )
        .expect("the server's");
        assert_eq!(html, server);
        assert!(
            html.contains("aria-label=\"Remove Tea &amp; Cake\""),
            "{html}"
        );
        assert!(
            html.contains("<span><!--pw:s2-->Tea &amp; Cake<!--pw:e2--></span>"),
            "{html}"
        );
    }

    #[test]
    fn what_changed_in_an_instance_is_each_part_set_where_it_is() {
        let was = serde_json::json!({ "id": "a", "name": "Tea", "gift": false });
        let now = serde_json::json!({ "id": "a", "name": "Green Tea", "gift": false });
        let (code, out) = changes(&request(serde_json::json!({ "was": was, "now": now })));
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&out).expect("JSON"),
            serde_json::json!([
                [1, { "attribute": "aria-label", "value": "Remove Green Tea" }],
                [2, { "text": "Green Tea" }],
            ])
        );
        // A block that changed: the instance is rendered again.
        let gift = serde_json::json!({ "id": "a", "name": "Tea", "gift": true });
        let (code, out) = changes(&request(serde_json::json!({ "was": was, "now": gift })));
        assert_eq!((code, out.as_str()), (0, "null"));
    }

    #[test]
    fn an_attribute_is_written_as_the_document_writes_it() {
        let title = Part::Attribute {
            id: PartId(4),
            owner: ElementId(1),
            name: "title".into(),
            value: "cart.note".into(),
            context: Context::Attribute,
        };
        let hidden = Part::BooleanAttribute {
            id: PartId(5),
            owner: ElementId(1),
            name: "hidden".into(),
            value: "cart.lines".into(),
        };
        let ask = |part: &Part, values: serde_json::Value| {
            attribute(
                serde_json::json!({ "part": part, "values": values })
                    .to_string()
                    .as_bytes(),
            )
        };
        assert_eq!(
            ask(&title, serde_json::json!({ "cart": { "note": "\"hot\"" } })),
            (0, r#"["title","&quot;hot&quot;"]"#.to_string())
        );
        assert_eq!(
            ask(&hidden, serde_json::json!({ "cart": { "lines": [1] } })),
            (0, r#"["hidden",""]"#.to_string())
        );
        assert_eq!(
            ask(&hidden, serde_json::json!({ "cart": { "lines": [] } })),
            (0, r#"["hidden",null]"#.to_string())
        );
    }

    /// A block holding an instance of a view that contains itself is
    /// rendered with the view's template its request carries (ADR-0203),
    /// and refused without it.
    #[test]
    fn an_instance_is_rendered_with_the_template_the_request_carries() {
        let request = |templates: &str| {
            format!(
                r#"{{
                "part": {{ "part": "conditional", "id": 1, "value": "open", "otherwise": [],
                    "then": [{{ "chunk": "dynamic", "value": {{ "part": "instance", "id": 2,
                        "path": "t.Card", "args": [["label", "title"]],
                        "elements": 0, "deepest": 1 }} }}] }},
                "values": {{ "open": true, "title": "<i>Menu</i>" }}
                {templates}
            }}"#
            )
        };
        let card = r#", "templates": [{ "path": "t.Card", "name": "Card", "params": ["label"],
            "schema": "s", "chunks": [
                { "chunk": "static", "value": "<b>" },
                { "chunk": "dynamic", "value": { "part": "text", "id": 0, "value": "label",
                    "context": "text" } },
                { "chunk": "static", "value": "</b>" }] }]"#;
        let (code, html) = render(request(card).as_bytes());
        assert_eq!(code, 0, "{html}");
        let shown: String = html
            .split("<!--")
            .map(|piece| piece.split_once("-->").map_or(piece, |(_, after)| after))
            .collect();
        assert_eq!(shown, "<b>&lt;i&gt;Menu&lt;/i&gt;</b>");
        assert!(html.contains("<!--pw:s2@"), "{html}");
        assert_eq!(render(request("").as_bytes()).0, 1);
    }

    /// **The warm-up's sample is rendered, by every request** (the first
    /// press's ADR): one the renderer refused would warm only the way to the
    /// refusal. Each row renders, its view's instance and its streamed
    /// placeholder too, and an unsafe address is made safe.
    #[test]
    fn the_warm_up_renders_its_sample_in_every_request() {
        assert_eq!(super::warm_up(), 0);
        let (code, html) = render(super::WARM.as_bytes());
        assert_eq!(code, 0, "{html}");
        for shown in [
            "&amp; &lt;eggs&gt;",
            "<em>",
            "<s>done</s>",
            "-->z<!--",
            "-->p<!--",
            "<p>…</p>",
        ] {
            assert!(html.contains(shown), "{shown} in {html}");
        }
        assert!(html.contains("href=\"about:blank\""), "{html}");
        assert!(!html.contains("JavaScript:"), "{html}");
        let (code, out) = super::changes(super::WARM.as_bytes());
        assert_eq!(code, 0, "{out}");
        assert_ne!(out, "null", "a row's own changes, not the row again");
    }

    /// **The sample holds every part a block can hold, and each context**:
    /// a kind it lacks is a path a first change still compiles. Exhaustive,
    /// so a kind the IR gains fails here until the sample has it or says why
    /// not.
    #[test]
    fn the_warm_up_s_sample_holds_every_part_a_block_can() {
        use pw_render::ir::{Chunk, Context, Part};
        use std::collections::BTreeSet;
        fn walk(chunks: &[Chunk], seen: &mut BTreeSet<String>) {
            for chunk in chunks {
                if let Chunk::Dynamic(p) = chunk {
                    part(p, seen);
                }
            }
        }
        fn part(p: &Part, seen: &mut BTreeSet<String>) {
            let kind = match p {
                Part::Text { context, .. } | Part::Attribute { context, .. } => {
                    seen.insert(format!("{context:?}"));
                    if matches!(p, Part::Text { .. }) {
                        "text"
                    } else {
                        "attribute"
                    }
                }
                Part::BooleanAttribute { .. } => "boolean_attribute",
                Part::Event { .. } => "event",
                Part::Conditional {
                    then, otherwise, ..
                } => {
                    walk(then, seen);
                    walk(otherwise, seen);
                    "conditional"
                }
                Part::Each { body, .. } => {
                    walk(body, seen);
                    "each"
                }
                Part::Match { arms, .. } => {
                    arms.iter().for_each(|a| walk(&a.body, seen));
                    "match"
                }
                Part::Stream {
                    placeholder,
                    ready,
                    failed,
                    ..
                } => {
                    walk(placeholder, seen);
                    walk(&ready.body, seen);
                    walk(&failed.body, seen);
                    "stream"
                }
                Part::InterpolatedAttribute { context, .. } => {
                    seen.insert(format!("{context:?}"));
                    "interpolated_attribute"
                }
                Part::Instance { .. } => "instance",
                // Written into the head by its host, and no block's: a raw
                // value, which the browser's renderer is granted no
                // capability for (`read`), and a construct refused.
                Part::Title { .. }
                | Part::Meta { .. }
                | Part::Canonical { .. }
                | Part::RawHtml { .. }
                | Part::Blocked { .. } => "none a block renders",
            };
            seen.insert(kind.to_string());
        }
        let sample: serde_json::Value = serde_json::from_str(super::WARM).unwrap();
        let mut seen = BTreeSet::new();
        part(
            &pw_render::decode::part(&sample["part"]).unwrap(),
            &mut seen,
        );
        for t in pw_render::decode::templates(&sample["templates"]).unwrap() {
            walk(&t.chunks, &mut seen);
        }
        let contexts = [
            Context::Text,
            Context::Attribute,
            Context::Url,
            Context::Style,
            Context::Content,
        ];
        let kinds = [
            "text",
            "attribute",
            "boolean_attribute",
            "event",
            "conditional",
            "each",
            "match",
            "stream",
            "interpolated_attribute",
            "instance",
        ];
        for want in kinds
            .map(String::from)
            .into_iter()
            .chain(contexts.map(|c| format!("{c:?}")))
        {
            assert!(seen.contains(&want), "the sample holds no {want}: {seen:?}");
        }
        assert!(!seen.contains("none a block renders"), "{seen:?}");
    }
}
