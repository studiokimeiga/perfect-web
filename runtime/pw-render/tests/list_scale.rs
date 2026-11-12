//! **A list renders in its length** (ADR-0294).
//!
//! Each item's scope copied the page's whole values until ADR-0294, the list
//! itself with them, so a list rendered in its length times the page's size:
//! kiokun's さえこ, 128 Japanese names, took 218 ms to render (W6, 2026-10-09),
//! 32 names 16.7 ms and 64 names 57.7 ms, about four times for each doubling.

use pw_render::*;

fn names_page() -> Template {
    let st = |s: &str| Chunk::Static(s.to_string());
    Template {
        path: "t.Names".into(),
        name: "Names".into(),
        params: vec![],
        schema: "s".into(),
        chunks: vec![
            st("<ul>"),
            Chunk::Dynamic(Part::Each {
                id: PartId(0),
                collection: "page.names".into(),
                binding: "name".into(),
                key: Some("id".into()),
                row_anchor: Default::default(),
                body: vec![
                    st("<li>"),
                    Chunk::Dynamic(Part::Text {
                        id: PartId(1),
                        value: "name.text".into(),
                        context: Context::Text,
                    }),
                    st("</li>"),
                ],
            }),
            st("</ul>"),
        ],
    }
}

/// A page whose query answered `n` names, each a record with some weight.
fn page(n: usize) -> Env {
    let name = |i: usize| {
        Value::Record(
            [
                ("id".to_string(), Value::Text(format!("{i}"))),
                (
                    "text".to_string(),
                    Value::Text(format!("名前 {i} ").repeat(8)),
                ),
            ]
            .into(),
        )
    };
    Env::new().set(
        "page",
        Value::Record([("names".to_string(), Value::List((0..n).map(name).collect()))].into()),
    )
}

#[test]
fn every_name_is_rendered_once_in_order() {
    let html = render(&names_page(), &page(64), &[]).expect("renders");
    assert_eq!(html.matches("<li").count(), 64, "{html}");
    let first = html.find("名前 0 ").expect("the first");
    let last = html.find("名前 63 ").expect("the last");
    assert!(first < last);
}

/// The fastest of five renders of `n` names, in microseconds.
fn fastest(n: usize) -> u128 {
    let (t, env) = (names_page(), page(n));
    (0..5)
        .map(|_| {
            let at = std::time::Instant::now();
            render(&t, &env, &[]).expect("renders");
            at.elapsed().as_micros()
        })
        .min()
        .expect("five")
}

/// **Eight times the names take about eight times as long, not sixty-four**
/// (ADR-0294). Timed, so it runs where the recipe records it (`--ignored`),
/// with a bound wide enough for a busy machine: linear is about 8, the
/// quadratic it was about 64.
#[test]
#[ignore]
fn eight_times_the_names_take_about_eight_times_as_long() {
    let small = fastest(250);
    let large = fastest(2000);
    let ratio = large as f64 / small.max(1) as f64;
    println!("250 names: {small} us; 2000 names: {large} us; ratio {ratio:.1}");
    for n in [32, 64, 128, 256, 512, 1024] {
        println!("{n} names: {} us", fastest(n));
    }
    assert!(
        ratio < 24.0,
        "ratio {ratio:.1}: a list renders in more than its length"
    );
}
