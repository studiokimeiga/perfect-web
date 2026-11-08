//! **A template writes no code** (ADR-0094).
//!
//! The renderer escapes a value by where it sits: text, an attribute, a URL,
//! a style, or raw HTML behind a capability (the architect's ruling of
//! 2026-08-06). Four places were none of those, and each checked and built
//! until 2026-09-26 with ordinary escaping:
//! - `<script>{msg}</script>`, which runs `msg` as it loads;
//! - `<button onclick={msg}>`, which runs `msg` when pressed;
//! - `<iframe srcdoc={msg}>`, whose document runs its scripts on this origin;
//! - `<style>{msg}</style>`, a stylesheet a value writes into.
//!
//! A static `<script>` or `onclick` is code the language never checks, and
//! is refused with them. Each test states one case, with a control.

use pw_core::check::check_sources;

fn reported(markup: &str) -> Vec<String> {
    let src = format!(
        "module t\n\nfn go() -> () !{{}} {{ () }}\n\n\
         view Note(msg: String) !{{}} {{\n    <div>\n        {markup}\n    </div>\n}}\n"
    );
    check_sources(&[("t.pw".to_string(), src)])
        .into_iter()
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

fn one(markup: &str, says: &str) {
    let found = reported(markup);
    assert_eq!(found.len(), 1, "{markup}\n{found:#?}");
    assert!(found[0].contains(says), "{markup}\n{found:#?}");
}

fn clean(markup: &str) {
    let found = reported(markup);
    assert!(found.is_empty(), "{markup}\n{found:#?}");
}

#[test]
fn a_template_writes_no_script() {
    one("<script>{msg}</script>", "PW5023 `<script>` is code");
    one("<script>go()</script>", "PW5023 `<script>` is code");
    clean("<p>{msg}</p>");
}

#[test]
fn a_template_writes_no_inline_handler() {
    one(
        "<button onclick={msg}>go</button>",
        "PW5023 `onclick` is code",
    );
    one(
        "<button onclick=\"go()\">go</button>",
        "PW5023 `onclick` is code",
    );
    // `alt=""`: the image's text is PW5053's, and this case is the handler.
    one(
        "<img onerror={msg} src=\"/a.png\" alt=\"\" />",
        "PW5023 `onerror` is code",
    );
    // A value in an ordinary attribute is escaped as one, and a handler is
    // bound with `on:`.
    clean("<button title={msg}>go</button>");
    clean("<button on:press={go}>go</button>");
}

/// A URL the author wrote runs code when its scheme does, and a value after
/// the scheme runs with it: a browser percent-decodes the URL first.
#[test]
fn a_template_writes_no_script_url() {
    one(
        "<a href=\"javascript:go()\">x</a>",
        "PW5023 `href` begins with `javascript:`, a URL that runs code",
    );
    one(
        "<a href=\" JavaScript:go({msg})\">x</a>",
        "PW5023 `href` begins with `javascript:`",
    );
    one(
        "<a href=\"jav{msg}ascript:go()\">x</a>",
        "PW5023 `href` takes its scheme from a value",
    );
    clean("<a href=\"https://example.com/a?b={msg}\">x</a>");
    clean("<a href=\"/stores/{msg}:menu\">x</a>");
    clean("<a href=\"mailto:orders@example.com\">x</a>");
    // A `data:` image runs nothing: PW5023 says nothing of it. It is a load
    // from no origin of the page's, which its policy refuses, and PW5052 is
    // what refuses it (ADR-0195's ruling 13).
    one(
        "<img src=\"data:image/png;base64,AAAA\" alt=\"a\" />",
        "PW5052 `<img src=\"data:image/png;base64,AAAA\">` in `Note` loads from `data:`",
    );
}

#[test]
fn a_template_writes_no_document_inline() {
    one(
        "<iframe srcdoc={msg}></iframe>",
        "PW5023 `srcdoc` is a document",
    );
    clean("<iframe src=\"/a.html\"></iframe>");
}

#[test]
fn a_stylesheet_holds_no_value_a_program_writes() {
    one("<style>{msg}</style>", "PW5023 `<style>` holds a value");
    // A value in a `style` attribute is escaped as a style.
    clean("<p style={msg}>x</p>");
}
