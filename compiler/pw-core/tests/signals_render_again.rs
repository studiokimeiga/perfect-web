//! **What the browser renders again, it can** (ADR-0137).
//!
//! The browser holds a page's signals and nothing else, and renders again a
//! text part outside any block, by its range, and a block a signal decides,
//! whole, from the signals' values (ADR-0133). ADR-0133 listed two other
//! places a signal could be read as refused by the plan, and neither was:
//! - a signal read inside an `{#each}` a query decides built, and would have
//!   shown its first value forever;
//! - a query's `{#each}` inside a block a signal decides built, and the
//!   browser could not have rendered the block again.
//!
//! Each is refused at build now, with the other places the browser does not
//! reach. Each test states one, with its control.

use pw_core::check::{Unit, check_sources};
use pw_core::lower::lower_file;
use pw_syntax::parse_tree;

fn library() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in [
        "packages/pw-std",
        "packages/pw-platform-web",
        "examples/lib",
        "examples/store",
    ] {
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
    out.push((
        "domain.pw".to_string(),
        std::fs::read_to_string(root.join("examples/domain.pw")).expect("domain.pw"),
    ));
    out
}

/// What `pw build` refuses of `src`, beside the store, once it checks.
fn refused(src: &str) -> Vec<String> {
    let mut sources = library();
    sources.push(("t.pw".to_string(), src.to_string()));
    let found: Vec<String> = check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect();
    assert_eq!(found, Vec::<String>::new(), "{src}");
    let units: Vec<Unit> = sources
        .into_iter()
        .map(|(path, src)| Unit {
            path,
            hir: lower_file(&src, &parse_tree(&src).green),
            src,
        })
        .collect();
    let built = pw_core::build::build(&units);
    assert!(
        built.is_ok(),
        "the checked store fixture builds: {:?}",
        built.as_ref().err()
    );
    built
        .unwrap()
        .refusals()
        .into_iter()
        .filter(|r| r.contains("`t.P~ui`"))
        .collect()
}

/// A store's items as one list, for a page over them: the store's own `Menu`
/// is grouped by category since ADR-0181.
const ITEMS: &str = "import Menus\nimport Events.{ MenuChanged }\n\
    import domain.{ StoreError, MenuItem }\n\n\
    public query Items(id: StoreId) -> Result<List<MenuItem>, StoreError>\n    \
    freshness      5.minutes\n    consistency    snapshot\n    cache          shared\n    \
    key            id\n    invalidates_on MenuChanged(id)\n    timeout        2.seconds\n\
    {\n    Menus.for_store(id)\n}\n\n";

/// A page over the store's menu with three signals, whose view holds
/// `markup` after a button that changes each.
fn page(markup: &str) -> String {
    format!(
        "module t\n\n\
         import domain.{{ StoreId, MenuItemId, CartError, InteractionId }}\n{items}\
         command pick(item: MenuItemId) -> Result<MenuItemId, CartError>\n    \
         requires SignedIn\n    idempotent_by InteractionId\n{{\n    Ok(item)\n}}\n\n\
         type Panel = Shut | Item(Int)\n\n\
         page P(id: StoreId, chosen: MenuItemId) {{\n    cache private\n\n    \
         signal count: Int = 0\n    signal open: Bool = false\n    signal panel: Panel = Panel.Shut\n    \
         signal tags: List<String> = [\"a\", \"b\"]\n    signal title: String = \"t\"\n\n    \
         let menu = query Items(id)\n\n    \
         view {{\n        <main>\n            \
         <button type=\"button\" on:press={{() => count = count + 1}}>Up</button>\n            \
         <button type=\"button\" on:press={{() => open = true}}>Open</button>\n            \
         <button type=\"button\" on:press={{() => panel = Panel.Item(count)}}>Item</button>\n            \
         <button type=\"button\" on:press={{() => tags = [\"c\"]}}>Tags</button>\n            \
         {markup}\n        </main>\n    }}\n}}\n",
        items = ITEMS
    )
}

fn refuses(markup: &str, why: &str) {
    let found = refused(&page(markup));
    assert!(
        found.iter().any(|r| r.contains(why)),
        "{markup}: {found:#?}"
    );
}

fn builds(markup: &str) {
    assert_eq!(refused(&page(markup)), Vec::<String>::new(), "{markup}");
}

#[test]
fn a_signal_read_in_a_block_a_query_decides_is_refused() {
    // ADR-0133 listed this as refused, and it built.
    refuses(
        "<ul>{#each menu as item (item.id)}<li>{item.name} {count}</li>{/each}</ul>",
        "reads the signal `count` inside a block a value other than a signal decides",
    );
    // A block a signal decides, inside a query's: an instance the browser
    // does not address.
    refuses(
        "<ul>{#each menu as item (item.id)}{#if open}<li>{item.name}</li>{/if}{/each}</ul>",
        "reads the signal `open` inside a block a value other than a signal decides",
    );
    // Control: outside any block, and inside a signal's block, it is live.
    builds("<p>{count}</p>{#if open}<p>{count}</p>{/if}");
}

#[test]
fn a_block_a_signal_decides_reads_the_signals_and_its_own_names() {
    // ADR-0133 listed this as refused, and it built: the browser renders the
    // block from the signals, and `menu` is not one.
    refuses(
        "{#if open}<ul>{#each menu as item (item.id)}<li>{item.name}</li>{/each}</ul>{/if}",
        "reads `menu` inside a block a signal decides",
    );
    // Control: an arm's name, a loop over a signal's list, and signals.
    builds(
        "{#match panel}{:Shut}<p>shut</p>{:Item(n)}<p>{n} of {count}</p>\
         <ul>{#each tags as t (t)}<li>{t}</li>{/each}</ul>{/match}",
    );
    // And an arm's name in a block inside the block.
    builds("{#if open}{#match panel}{:Shut}<p>shut</p>{:Item(n)}<p>{n}</p>{/match}{/if}");
}

#[test]
fn a_list_a_signal_holds_is_rendered_again_by_its_rows_at_the_top_and_inside_its_block() {
    // ADR-0137 refused one at the top of the page, which the browser
    // rendered once. It is its own block now, rendered again by its rows.
    builds("<ul>{#each tags as t (t)}<li>{t}</li>{/each}</ul>");
    builds("{#if open}<ul>{#each tags as t (t)}<li>{t}</li>{/each}</ul>{/if}");
    // Its rows read the signals and the name it binds, as any block a signal
    // decides: a query's list there is refused.
    refuses(
        "<ul>{#each tags as t (t)}<li>{t}{#each menu as item (item.id)}<b>{item.name}</b>{/each}</li>{/each}</ul>",
        "reads `menu` inside a block a signal decides",
    );
}

/// The plan of the page `page(markup)` makes, `List` imported.
fn plan_of(markup: &str) -> pw_core::page_values::PageValues {
    let mut sources = library();
    let src = page(markup).replacen("module t\n\n", "module t\n\nimport List\n", 1);
    assert!(src.contains("import List"), "{src}");
    sources.push(("t.pw".to_string(), src));
    let units: Vec<Unit> = sources
        .into_iter()
        .map(|(path, src)| Unit {
            path,
            hir: lower_file(&src, &parse_tree(&src).green),
            src,
        })
        .collect();
    // At its assert where it does not build, as the store fixture's beside it:
    // run 42825062947's control of `bind:value` failed here at an `expect`,
    // which the strict validator does not count.
    let built = pw_core::build::build(&units);
    assert!(built.is_ok(), "builds: {:?}", built.as_ref().err());
    let Ok(built) = built else {
        unreachable!("asserted above")
    };
    built
        .pages
        .iter()
        .find(|p| p.page == "t.P~ui")
        .expect("a plan")
        .plan
        .clone()
        .expect("planned")
}

#[test]
fn a_list_at_the_top_is_its_own_block_read_again_for_what_its_rows_read() {
    // A list a signal holds: rendered again when it changes, or when a signal
    // a row reads does.
    let plan = plan_of("<ul>{#each tags as t (t)}<li>{t} {count}</li>{/each}</ul>");
    let each: Vec<_> = plan.live.iter().filter(|l| l.kind == "each").collect();
    assert_eq!(each.len(), 1, "{:#?}", plan.live);
    assert_eq!(each[0].signal, "tags");
    assert_eq!(each[0].reads, ["count", "tags"]);
    // A list the browser computes from one: rendered again when it changes.
    let computed = plan_of("<ul>{#each List.reverse(tags) as t (t)}<li>{t}</li>{/each}</ul>");
    let each: Vec<_> = computed.live.iter().filter(|l| l.kind == "each").collect();
    assert_eq!(each.len(), 1, "{:#?}", computed.live);
    assert_eq!(each[0].reads, ["tags"]);
    // Control: a list inside a signal's block is rendered with the block.
    let inside = plan_of("{#if open}<ul>{#each tags as t (t)}<li>{t}</li>{/each}</ul>{/if}");
    assert!(
        inside.live.iter().all(|l| l.kind != "each"),
        "{:#?}",
        inside.live
    );
}

#[test]
fn an_attribute_a_signal_decides_is_set_in_place_or_refused() {
    // Set in place since ADR-0142, outside any loop.
    builds("<p title={title}>x</p>");
    // A URL is checked by the renderer, and would be checked a second way.
    refuses(
        "<a href={title}>x</a>",
        "is an attribute a signal decides whose value is a URL or a style",
    );
    // Inside a block a query decides, as any part a signal decides.
    refuses(
        "<ul>{#each menu as item (item.id)}<li title={title}>{item.name}</li>{/each}</ul>",
        "reads the signal `title` inside a block a value other than a signal decides",
    );
}

#[test]
fn what_a_handler_captures_in_a_signal_s_block_is_the_block_s() {
    // Rendered again from the signals alone, a capture of the page's
    // parameter has no value.
    refuses(
        "{#if open}<button type=\"button\" on:press|refusable={() => { let _picked = pick(chosen) }}>Pick</button>{/if}",
        "reads `chosen` inside a block a signal decides",
    );
    // Control: outside the block, the document carries it as rendered.
    builds(
        "<button type=\"button\" on:press|refusable={() => { let _picked = pick(chosen) }}>Pick</button>",
    );
}
