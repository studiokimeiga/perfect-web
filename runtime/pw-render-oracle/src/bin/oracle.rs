//! **The server's renderer, asked what the browser's is asked** (the browser
//! renderer's ADR).
//!
//! The browser renders with a page of JavaScript, and is held to this: one
//! request a line, `name<TAB>request`, and one answer a line, `[code, text]`,
//! the code 0 for rendered, 1 for refused by the renderer, 2 for a request
//! that is not one, and 3 for a renderer that panicked, which no request
//! should be able to make it do.

use std::io::{BufRead, Write};
use std::panic::catch_unwind;

fn main() {
    // A panic is an answer here, and says so in its own line.
    std::panic::set_hook(Box::new(|_| {}));
    let stdin = std::io::stdin();
    let mut out = std::io::BufWriter::new(std::io::stdout());
    for line in stdin.lock().lines() {
        let line = line.expect("a line of the requests");
        let (name, request) = line.split_once('\t').expect("name<TAB>request");
        let (code, text) = catch_unwind(|| answer(name, request.as_bytes()))
            .unwrap_or_else(|_| (3, "the renderer panicked".to_string()));
        writeln!(out, "{}", serde_json::json!([code, text])).expect("the answers are read");
    }
}

fn answer(name: &str, request: &[u8]) -> (u32, String) {
    match name {
        "render_part" => pw_render_oracle::render(request),
        "render_instance" => pw_render_oracle::instance(request),
        "instance_changes" => pw_render_oracle::changes(request),
        "attribute_value" => pw_render_oracle::attribute(request),
        "captures_value" => pw_render_oracle::captures(request),
        other => panic!("no request is a `{other}`"),
    }
}
