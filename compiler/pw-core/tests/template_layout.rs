//! **A template's layout is not the page** ("a press changes what it changes",
//! item 7): a run of white space with a line break in it is one space, and
//! nothing at all where text can never be shown; what a `<pre>` or a
//! `<textarea>` holds is left as it is written.
//!
//! Until 2026-11-09 every line break and indentation of a template's source
//! went to the page: a row of a table was three times the size of its content.

use pw_core::template_ir::Chunk;

/// What the page writes inside its `<main>`, its static text joined.
fn inside_main(markup: &str) -> String {
    let src = format!(
        "module t\n\npage P() {{\n    cache private\n\n    view {{\n        <main>{markup}</main>\n    }}\n}}\n"
    );
    let parsed = pw_syntax::parse_tree(&src);
    assert!(parsed.ok(), "{:?}", parsed.errors);
    let hir = pw_core::lower::lower_file(&src, &parsed.green);
    let hirs = vec![&hir];
    let ws = pw_core::resolve::Workspace::build(&hirs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &hirs);
    let templates = pw_core::build::templates(&hirs, &[src.as_str()], &sigs);
    let page = templates
        .iter()
        .find(|t| t.path == "t.P~ui")
        .expect("the page's template");
    let all: String = page
        .chunks
        .iter()
        .map(|c| match c {
            Chunk::Static(s) => s.clone(),
            Chunk::Dynamic(_) => "{..}".to_string(),
        })
        .collect();
    all.strip_prefix("<main>")
        .and_then(|s| s.strip_suffix("</main>"))
        .unwrap_or_else(|| panic!("{all}"))
        .to_string()
}

#[test]
fn a_line_break_and_its_indentation_is_one_space() {
    assert_eq!(
        inside_main("<b>a</b>\n        <i>b</i>"),
        "<b>a</b> <i>b</i>"
    );
    assert_eq!(inside_main("<p>one\n        two</p>"), "<p>one two</p>");
    assert_eq!(inside_main("<p>one\r\n\t  two</p>"), "<p>one two</p>");
}

#[test]
fn white_space_with_no_line_break_is_left_as_it_is_written() {
    assert_eq!(inside_main("<p>a  b</p>"), "<p>a  b</p>");
    assert_eq!(inside_main("<b>a</b> <i>b</i>"), "<b>a</b> <i>b</i>");
}

#[test]
fn a_space_that_is_no_white_space_stays() {
    assert_eq!(inside_main("<p>a\u{a0}\n  b</p>"), "<p>a\u{a0} b</p>");
}

#[test]
fn a_run_with_a_line_break_beside_one_without_is_only_itself_made_one() {
    assert_eq!(inside_main("<p>a  b\n   c</p>"), "<p>a  b c</p>");
}

#[test]
fn what_preformatted_elements_hold_is_content() {
    assert_eq!(inside_main("<pre>a\n   b</pre>"), "<pre>a\n   b</pre>");
    assert_eq!(
        inside_main("<pre><code>a\n   b</code></pre>"),
        "<pre><code>a\n   b</code></pre>"
    );
    assert_eq!(
        inside_main("<textarea>a\n   b</textarea>"),
        "<textarea>a\n   b</textarea>"
    );
}

#[test]
fn where_text_is_never_shown_the_layout_is_nothing() {
    assert_eq!(
        inside_main(
            "<table>\n  <thead>\n    <tr>\n      <th>h</th>\n    </tr>\n  </thead>\n  <tbody>\n    <tr>\n      <td>d</td>\n    </tr>\n  </tbody>\n</table>"
        ),
        "<table><thead><tr><th>h</th></tr></thead><tbody><tr><td>d</td></tr></tbody></table>"
    );
    assert_eq!(
        inside_main("<select>\n  <option>a</option>\n  <option>b</option>\n</select>"),
        "<select><option>a</option><option>b</option></select>"
    );
}

#[test]
fn where_a_cell_can_show_text_the_layout_is_one_space() {
    assert_eq!(
        inside_main("<table><tr><td>\n  <b>x</b>\n</td></tr></table>"),
        "<table><tr><td> <b>x</b> </td></tr></table>"
    );
}

#[test]
fn a_list_keeps_a_space_a_style_could_show() {
    assert_eq!(
        inside_main("<ul>\n  <li>a</li>\n  <li>b</li>\n</ul>"),
        "<ul> <li>a</li> <li>b</li> </ul>"
    );
}

#[test]
fn where_text_is_never_shown_words_are_still_words() {
    assert_eq!(
        inside_main("<table>a\n  b<tr><td>x</td></tr></table>"),
        "<table>a b<tr><td>x</td></tr></table>"
    );
}

#[test]
fn a_value_is_content_however_it_is_written() {
    // A line that is to stay a line under `white-space: pre-line` is written
    // as a value: only the template's layout is folded.
    assert_eq!(inside_main("<p>{\"a\\n   b\"}</p>"), "<p>{..}</p>");
}

#[test]
fn a_blank_text_outside_any_element_of_those_is_one_space() {
    assert_eq!(inside_main("\n    <p>a</p>\n"), " <p>a</p> ");
}
