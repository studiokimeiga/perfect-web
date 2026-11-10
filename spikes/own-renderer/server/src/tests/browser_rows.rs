//! **A row of a list the browser renders computes what it shows** (the row's
//! computed holes' ADR), the host's part: the first render computes each
//! row's value from its item and the signals' first values, as the browser
//! computes it after, and the page's signal is its own value. The plan is
//! the compiler's (`compiler/pw-core/tests/computed_rows.rs`).

use super::device_store::served;
use super::*;

/// A signal's rows, each its class from its item and the selection.
const ROWS: &str = "module t\n\n\
type Row = Row { id: Int, label: String }\n\n\
page Home() {\n    route \"/\"\n    placement origin\n\n    \
signal selected: Int = 2\n    \
signal rows: List<Row> = [Row { id: 1, label: \"a\" }, Row { id: 2, label: \"b\" }]\n\n    \
view {\n        <title>Home</title>\n        \
<button type=\"button\" on:press={() => selected = 1}>Select</button>\n        \
<ul>{#each rows as row (row.id)}<li class={if row.id == selected { \"on\" } else { \"off\" }}>\
{row.label}</li>{/each}</ul>\n    }\n}\n";

/// The same rows on a page a reader decides, which a request renders: it
/// reads a query.
const READ: &str = "module t\n\n\
type Row = Row { id: Int, label: String }\n\n\
public query Greeting() -> String !{} {\n    \"hello\"\n}\n\n\
page Home() {\n    route \"/\"\n    cache private\n\n    \
signal selected: Int = 2\n    \
signal rows: List<Row> = [Row { id: 1, label: \"a\" }, Row { id: 2, label: \"b\" }]\n    \
let greeting = query Greeting()\n\n    \
view {\n        <title>Home</title>\n        <h1>{greeting}</h1>\n        \
<button type=\"button\" on:press={() => selected = 1}>Select</button>\n        \
<ul>{#each rows as row (row.id)}<li class={if row.id == selected { \"on\" } else { \"off\" }}>\
{row.label}</li>{/each}</ul>\n    }\n}\n";

/// What `path` answers, whole.
fn answer(s: &Server, path: &str) -> String {
    fetched_as(s, path, Some("a"))
        .into_iter()
        .map(|(_, c)| c)
        .collect()
}

/// The document's manifest, `pw-parts`.
fn manifest(page: &str) -> serde_json::Value {
    let open = "<script type=\"application/json\" id=\"pw-parts\">";
    let start = page.find(open).expect("a manifest") + open.len();
    let end = start + page[start..].find("</script>").expect("its end");
    serde_json::from_str(&page[start..end]).expect("JSON")
}

#[test]
fn a_signals_rows_are_served_with_what_each_computes() {
    // Rendered before the host listens (ADR-0409), and as a request renders
    // a page a reader decides: each the same.
    let s = served(ROWS);
    let done = s.prerender();
    assert!(done.is_ok(), "{done:?}");
    the_rows_compute(&s, &answer(&s, "/"));
    let s = served(READ);
    let page = answer(&s, "/");
    assert!(page.contains(">hello<"), "{page}");
    the_rows_compute(&s, &page);
}

/// What `page`, `s`'s home, says of each row and of what each computes.
fn the_rows_compute(s: &Server, page: &str) {
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    // Each row's class, from its item and the selection's first value: the
    // class written before the row's text, its anchors around it.
    let class_of = |text: &str| -> String {
        let at = page
            .find(&format!("-->{text}<!--"))
            .unwrap_or_else(|| panic!("the row {text}: {page}"));
        let open = page[..at].rfind("class=\"").expect("its class") + "class=\"".len();
        let close = open + page[open..].find('"').expect("its end");
        page[open..close].to_string()
    };
    assert_eq!(class_of("a"), "off", "{page}");
    assert_eq!(class_of("b"), "on", "{page}");
    // The page holds its rows as they are: nothing computed is in them.
    let parts = manifest(page);
    assert_eq!(
        parts["signals"]["rows"],
        serde_json::json!([{ "id": 1, "label": "a" }, { "id": 2, "label": "b" }]),
        "{parts:#}"
    );
    // And it names what each row computes, what it reads, and the module.
    let rows = parts["rowsComputed"].as_array().expect("the rows' values");
    assert_eq!(rows.len(), 1, "{parts:#}");
    assert_eq!(rows[0]["inputs"], serde_json::json!(["row", "selected"]));
    let name = rows[0]["name"].as_str().expect("its name");
    assert!(name.starts_with("#t.Home~ui~"), "{name}");
    let module = parts["computed"].as_str().expect("the module is named");
    let served = answer(s, module);
    assert!(served.starts_with("HTTP/1.1 200"), "{served}");
    assert!(
        served.contains(&format!("\"{name}\": (j0, j1) =>")),
        "{served}"
    );
}
