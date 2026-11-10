//! **A value computed in a row is the row's** (ADR-0228, ruling 0073-a).
//!
//! The feed's timeline says each post's likes in words, `{counted(p.likes,
//! "like", "likes")}`, in the row of `{#each feed as p (p.id)}`. The compiler
//! names the value's path from the row's item, `p.#feed.app.Home~10`, and a
//! host computes it for each row as it computes a member read of the item
//! (ADR-0169). A like is shown before the server answers: the page's
//! speculation module computes each row's value from the speculated
//! timeline, as it computes a member read for a row (ADR-0172). Until
//! ADR-0228 a value computed in a row was refused at build.
//!
//! Each test states one part, with controls.

use pw_core::check::{Unit, check_sources};
use pw_core::lower::lower_file;
use pw_core::page_values::Step;
use pw_syntax::parse_tree;

/// The platform's packages, and `files` after them.
fn program(files: &[(&str, String)]) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            out.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out.extend(files.iter().map(|(n, s)| (n.to_string(), s.clone())));
    out
}

fn reported(files: &[(String, String)]) -> Vec<String> {
    check_sources(files)
        .into_iter()
        .filter(|(n, _)| !n.contains("packages/"))
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

/// The build, or what refused it.
fn build(files: &[(String, String)]) -> Result<pw_core::build::Build, String> {
    let units: Vec<Unit> = files
        .iter()
        .map(|(path, src)| Unit {
            path: path.clone(),
            hir: lower_file(src, &parse_tree(src).green),
            src: src.clone(),
        })
        .collect();
    let b = pw_core::build::build(&units)?;
    match b.refusals() {
        refused if refused.is_empty() => Ok(b),
        refused => Err(refused.join("\n")),
    }
}

fn feed(change: &dyn Fn(&str) -> String) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = std::fs::read_to_string(root.join("examples/feed/app.pw")).expect("the feed");
    program(&[("app.pw", change(&src))])
}

/// A page of a list of posts and a signal's list, its markup `markup`.
fn page(markup: &str) -> Vec<(String, String)> {
    program(&[(
        "t.pw",
        format!(
            "module t\n\nimport List\n\n\
             type Author = Author {{ name: String, open: Bool }}\n\n\
             type Post = Post {{ id: String, likes: Int, author: Author, tags: List<String> }}\n\n\
             fn shown(open: Bool) -> String !{{}} {{\n    if open {{\n        \"open\"\n    }} else {{\n        \"shut\"\n    }}\n}}\n\n\
             view Opened(open: Bool) !{{}} {{\n    <p>{{shown(open)}}</p>\n}}\n\n\
             public query Posts() -> List<Post> !{{}} {{\n    \
             [Post {{ id: \"p1\", likes: 2, author: Author {{ name: \"Ada\", open: true }}, tags: [\"a\"] }}]\n}}\n\n\
             page P() {{\n    route \"/p\"\n    cache private\n\n    \
             signal flags: List<Bool> = [true, false]\n    let posts = query Posts()\n\n    \
             view {{\n        <title>P</title>\n        <main>\n            {markup}\n            \
             <button type=\"button\" on:press={{() => flags = []}}>Clear</button>\n        \
             </main>\n    }}\n}}\n"
        ),
    )])
}

/// A page whose rows a signal holds, beside a signal `selected`, its markup
/// `markup`: what the large list writes.
fn signal_page(markup: &str) -> Vec<(String, String)> {
    program(&[(
        "t.pw",
        format!(
            "module t\n\nimport List\n\n\
             type Row = Row {{ id: Int, label: String, open: Bool, tags: List<String> }}\n\n\
             fn shown(open: Bool) -> String !{{}} {{\n    if open {{\n        \"open\"\n    }} else {{\n        \"shut\"\n    }}\n}}\n\n\
             public query Posts() -> List<Row> !{{}} {{\n    \
             [Row {{ id: 7, label: \"q\", open: true, tags: [] }}]\n}}\n\n\
             page P() {{\n    route \"/p\"\n    cache private\n\n    \
             signal rows: List<Row> = [Row {{ id: 1, label: \"a\", open: true, tags: [\"x\"] }}]\n    \
             signal selected: Int = 0\n    signal names: List<String> = [\"x\"]\n    let posts = query Posts()\n\n    \
             view {{\n        <title>P</title>\n        <main>\n            {markup}\n            \
             <button type=\"button\" on:press={{() => selected = 1}}>Select</button>\n        \
             </main>\n    }}\n}}\n"
        ),
    )])
}

fn plan_of<'b>(b: &'b pw_core::build::Build, page: &str) -> &'b pw_core::page_values::PageValues {
    b.pages
        .iter()
        .find(|p| p.page == page)
        .unwrap_or_else(|| panic!("no page {page}"))
        .plan
        .as_ref()
        .unwrap_or_else(|e| panic!("{page}: {e}"))
}

#[test]
fn a_value_computed_in_a_row_is_each_rows() {
    let files = feed(&|s| s.to_string());
    assert_eq!(reported(&files), Vec::<String>::new());
    let b = build(&files).unwrap_or_else(|e| panic!("{e}"));
    let plan = plan_of(&b, "feed.app.Home~ui");
    // The likes in words: a read of each row's item, by the steps to its
    // input, then its function, set in the row by the path the compiler
    // named from the item. Of the values the row computes from its item, it
    // is the one the template reads in the row's `.likes`; the others are
    // whether the row waits (ADR-0275).
    let computed: Vec<&pw_core::page_values::RowRead> =
        plan.rows.iter().filter(|r| r.path.contains(".#")).collect();
    let template = b
        .templates
        .iter()
        .find(|t| t.path == "feed.app.Home~ui")
        .expect("the home page's template");
    let chunks = format!("{:?}", template.chunks);
    let (_, likes) = chunks
        .split_once(r#"class=\"likes\""#)
        .unwrap_or_else(|| panic!("the row's likes: {chunks}"));
    let row = computed
        .iter()
        .filter_map(|r| likes.find(&format!("\"{}\"", r.path)).map(|at| (at, *r)))
        .min_by_key(|(at, _)| *at)
        .map(|(_, r)| r)
        .unwrap_or_else(|| panic!("no row value read in the likes: {:?}", plan.rows));
    assert!(
        computed.len() > 1,
        "and whether the row waits: {:?}",
        plan.rows
    );
    assert_eq!(
        (row.collection.as_str(), row.binding.as_str()),
        ("feed", "p")
    );
    let part: u32 = row
        .path
        .strip_prefix("p.#feed.app.Home~ui~")
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{}", row.path));
    assert_eq!(
        row.steps,
        [Step::Derived(format!("feed.app.Home~ui.derived_{part}"))]
    );
    // And the speculation computes it for each row it renders, from the
    // item: the like's transition counts the like in the timeline.
    let home = b
        .speculations
        .iter()
        .find(|s| s.page == "feed.app.Home~ui")
        .expect("the home page speculates");
    let pw_core::backend::wasm::Encoding::Encoded(module) = &home.module else {
        panic!("{:?}", home.module);
    };
    let (_, rows) = module
        .source
        .split_once("rows: {")
        .unwrap_or_else(|| panic!("{}", module.source));
    assert!(
        rows.contains(&format!("\"#feed.app.Home~ui~{part}\": f")),
        "{}",
        module.source
    );
    assert_eq!(module.commands, ["feed.app.post", "feed.app.like"]);
}

#[test]
fn from_a_field_of_the_item_in_a_block_and_in_a_loop_in_the_row() {
    let files = page(
        "<ul>{#each posts as p (p.id)}<li><Opened open={p.author.open} />\
         {#if p.author.open}<p>{shown(p.likes == 2)}</p>{/if}\
         <ul>{#each p.tags as t (t)}<li>{shown(t == \"a\")}</li>{/each}</ul></li>{/each}</ul>",
    );
    assert_eq!(reported(&files), Vec::<String>::new());
    let b = build(&files).unwrap_or_else(|e| panic!("{e}"));
    let rows: Vec<(&str, &str, &[Step])> = plan_of(&b, "t.P~ui")
        .rows
        .iter()
        .map(|r| (r.collection.as_str(), r.path.as_str(), r.steps.as_slice()))
        .collect();
    let [
        (_, field, from_field),
        (_, block, whole),
        (inner, tag, from_tag),
    ] = rows.as_slice()
    else {
        panic!("three row values: {rows:?}");
    };
    // A view given a field of the item: the steps reach the field, then run
    // the view's function.
    assert!(field.starts_with("p.#t.P~ui~"), "{field}");
    assert!(
        matches!(from_field, [Step::Field(a), Step::Field(o), Step::Derived(_)]
            if a == "author" && o == "open"),
        "{from_field:?}"
    );
    // In a block in the row, from the item whole.
    assert!(block.starts_with("p.#t.P~ui~"), "{block}");
    assert!(matches!(whole, [Step::Derived(_)]), "{whole:?}");
    // In a loop in the row: the inner row's, through each outer item.
    assert_eq!(*inner, "posts.*.tags");
    assert!(tag.starts_with("t.#t.P~ui~"), "{tag}");
    assert!(matches!(from_tag, [Step::Derived(_)]), "{from_tag:?}");
}

/// **A row of a list a signal holds computes from its item and the signals**
/// (the row's computed holes' ADR): the large list's class, `if row.id ==
/// selected`, and a text from the item alone. Each is the item's, by the
/// path the compiler names from it, computed by the browser for each row it
/// renders, from the values the plan names in the function's order; and by
/// the page's computed module, keyed by that name.
#[test]
fn a_row_of_a_signals_list_computes_from_its_item_and_the_signals() {
    let files = signal_page(
        "<table><tbody>{#each rows as row (row.id)}<tr class={if row.id == selected { \"danger\" } else { \"\" }}>\
         <td>{shown(row.open)}</td></tr>{/each}</tbody></table>",
    );
    assert_eq!(reported(&files), Vec::<String>::new());
    let b = build(&files).unwrap_or_else(|e| panic!("{e}"));
    let plan = plan_of(&b, "t.P~ui");
    let rows: Vec<(&str, &str, &str, Vec<&str>)> = plan
        .browser_rows
        .iter()
        .map(|r| {
            (
                r.collection.as_str(),
                r.binding.as_str(),
                r.path.as_str(),
                r.inputs.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(rows.len(), 2, "two values each row computes: {rows:?}");
    let class = rows
        .iter()
        .find(|r| r.3.len() == 2)
        .unwrap_or_else(|| panic!("the class: {rows:?}"));
    let text = rows
        .iter()
        .find(|r| r.3.len() == 1)
        .unwrap_or_else(|| panic!("the text: {rows:?}"));
    assert_eq!((class.0, class.1), ("rows", "row"), "{class:?}");
    assert!(class.2.starts_with("row.#t.P~ui~"), "{class:?}");
    assert_eq!(class.3, ["row", "selected"], "{class:?}");
    assert!(text.2.starts_with("row.#t.P~ui~"), "{text:?}");
    assert_eq!(text.3, ["row"], "{text:?}");
    // No host computes them from a query's value, and none is the page's.
    assert!(
        plan.rows.iter().all(|r| !r.path.starts_with("row.")),
        "{:?}",
        plan.rows
    );
    // The list is its own block, read again when what its rows read changes:
    // the signal the class reads among it (the block of a list's ADR).
    let each: Vec<_> = plan.live.iter().filter(|l| l.kind == "each").collect();
    assert_eq!(each.len(), 1, "{:#?}", plan.live);
    assert!(
        each[0].reads.iter().any(|r| r == "selected"),
        "{:?}",
        each[0].reads
    );
    // The page's module computes each by its name.
    let module = b
        .computed
        .iter()
        .find(|c| c.page == "t.P~ui")
        .and_then(|c| c.module.as_ref().ok())
        .unwrap_or_else(|| panic!("no computed module: {:?}", b.computed));
    let (_, exported) = module
        .split_once("export const rows = {")
        .unwrap_or_else(|| panic!("{module}"));
    for (_, _, path, _) in &rows {
        let name = path.strip_prefix("row.").expect("the item's");
        assert!(exported.contains(&format!("\"{name}\": (")), "{module}");
    }
}

#[test]
fn what_a_row_does_not_compute_yet_is_refused_by_name() {
    for (files, why) in [
        // A row of a list inside a row a signal holds: the browser computes
        // none yet (the row's computed holes' ADR).
        (
            signal_page(
                "<ul>{#each rows as row (row.id)}<li><ul>{#each row.tags as t (t)}\
                 <li>{shown(t == \"x\")}</li>{/each}</ul></li>{/each}</ul>",
            ),
            "computes a value in a row of a list inside another row, which the browser computes \
             none of yet",
        ),
        // A row that is no record: what a row computes is held in its item.
        (
            signal_page("<ul>{#each names as n (n)}<li>{shown(n == \"x\")}</li>{/each}</ul>"),
            "computes a value in a row of `n`, which is no record",
        ),
        // A row's value from the item and another value.
        (
            page(
                "<ul>{#each posts as p (p.id)}<li>{shown(p.likes == List.length(posts))}</li>{/each}</ul>",
            ),
            "computes a value from `p` and `posts`, and a host computes one from one value \
             (ruling 0073-a)",
        ),
        // A speculated row's value from a field of its item: the module
        // computes one from the row's item whole.
        (
            feed(&|s| {
                s.replacen(
                    "<span class=\"likes\">{counted(p.likes, \"like\", \"likes\")}</span>",
                    "<Named author={p.author} />",
                    1,
                ) + "\nview Named(author: User) !{} {\n    <span>{String.length(author.name)}</span>\n}\n"
            }),
            "computes a value from `p.author`, a field of a row the page speculates on, and the \
             browser computes one from the row's item whole (ruling 0073-a)",
        ),
        // An attribute computed from a speculated value.
        (
            feed(&|s| {
                s.replace(
                    "            <h1>Home</h1>\n",
                    "            <h1 title={counted(List.length(feed), \"post\", \"posts\")}>Home</h1>\n",
                )
            }),
            "computes an attribute's value from `feed`, which the page speculates on, and the \
             browser computes a text part or a block's subject from the value whole (ruling \
             0073-a)",
        ),
    ] {
        assert_eq!(reported(&files), Vec::<String>::new());
        match build(&files) {
            Ok(_) => panic!("built: {why}"),
            Err(e) => assert!(e.contains(why), "{e}"),
        }
    }
}
