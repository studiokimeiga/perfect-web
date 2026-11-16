//! **An upload states where a form posts a file, and its limits** (track
//! `uploads`, ADR-0260; the integrator's ruling on its Q1).
//!
//! `upload PostImage  route "/uploads/post-image"  serves "/images"
//! max_bytes 5_000_000  types png, jpeg, webp, gif  max_width 4096
//! max_height 4096`: each clause written once and as a literal (PW5601, and
//! PW0335 for a value its domain does not have), its paths its own
//! (PW5602), and a form that sends a file posts it to one (PW5603). `pw
//! build` writes each to `uploads.json`. Each test states one case, with its
//! control.

use pw_core::check::check_sources;

fn program(src: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = Vec::new();
    for dir in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(dir))
            .expect("dir")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            sources.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    sources.push(("m.pw".to_string(), src.to_string()));
    sources
}

/// Every diagnostic of `m.pw`, as `CODE message`.
fn reported(src: &str) -> Vec<String> {
    check_sources(&program(src))
        .into_iter()
        .filter(|(path, _)| path == "m.pw")
        .flat_map(|(_, ds)| ds)
        .map(|d| format!("{} {}", d.code, d.message))
        .collect()
}

/// The uploads track's codes, and a clause's value refused by its domain.
fn uploads(src: &str) -> Vec<String> {
    reported(src)
        .into_iter()
        .filter(|d| d.starts_with("PW56") || d.starts_with("PW0335") || d.starts_with("PW5105"))
        .collect()
}

const PROGRAM: &str = r#"module m

upload PostImage
    route      "/uploads/post-image"
    serves     "/images"
    max_bytes  5_000_000
    types      png, jpeg, webp, gif
    max_width  4096
    max_height 4096

page Home() {
    route     "/"
    placement origin

    view {
        <title>Home</title>
        <main>
            <h1>Home</h1>
            <form method="post" action="/uploads/post-image" enctype="multipart/form-data">
                <label>Image <input type="file" name="image" accept="image/png,image/jpeg,image/webp,image/gif"></label>
                <button type="submit">Attach</button>
            </form>
        </main>
    }
}

page About() {
    route     "/about"
    placement origin

    view {
        <title>About</title>
        <main><h1>About</h1></main>
    }
}
"#;

#[test]
fn an_upload_that_states_its_limits_checks() {
    assert_eq!(uploads(PROGRAM), Vec::<String>::new());
    assert_eq!(reported(PROGRAM), Vec::<String>::new(), "and nothing else");
}

#[test]
fn every_clause_is_stated() {
    for head in [
        "route",
        "serves",
        "max_bytes",
        "types",
        "max_width",
        "max_height",
    ] {
        let without: String = PROGRAM
            .lines()
            .filter(|l| !l.trim_start().starts_with(&format!("{head} ")))
            .map(|l| format!("{l}\n"))
            .collect();
        let found = uploads(&without);
        assert!(
            found.contains(&format!("PW5601 `PostImage` states no `{head}`")),
            "{head}: {found:?}"
        );
    }
}

#[test]
fn a_limit_is_a_literal_count() {
    for (written, fault) in [
        ("max_bytes  0", "PW0335"),
        ("max_bytes  -1", "PW0335"),
        ("max_bytes  5.megabytes", "PW0335"),
        ("max_bytes  size()", "PW0335"),
        ("max_bytes  5__000", "PW0335"),
        ("max_bytes  100_000_001", "PW5601"),
    ] {
        let changed = PROGRAM.replace("max_bytes  5_000_000", written);
        let found = uploads(&changed);
        assert!(
            found.iter().any(|d| d.starts_with(fault)),
            "{written}: {found:?}"
        );
    }
    let changed = PROGRAM.replace("max_width  4096", "max_width  4294967296");
    assert!(uploads(&changed).iter().any(|d| d.starts_with("PW5601")));
    // The control: at the most, and with separators.
    let most = PROGRAM.replace("max_bytes  5_000_000", "max_bytes  100_000_000");
    assert_eq!(uploads(&most), Vec::<String>::new());
}

#[test]
fn its_types_are_a_closed_set() {
    let svg = PROGRAM.replace("types      png, jpeg, webp, gif", "types      png, svg");
    let found = uploads(&svg);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("PW0335 `types png, svg`"), "{found:?}");
    let one = PROGRAM.replace("types      png, jpeg, webp, gif", "types      jpeg");
    assert_eq!(uploads(&one), Vec::<String>::new());
}

#[test]
fn its_paths_are_literal_paths() {
    for (from, to) in [
        (
            "route      \"/uploads/post-image\"",
            "route      \"/uploads/{kind}\"",
        ),
        (
            "route      \"/uploads/post-image\"",
            "route      \"uploads\"",
        ),
        ("serves     \"/images\"", "serves     \"/images/\""),
        ("serves     \"/images\"", "serves     \"/Images?x=1\""),
    ] {
        let changed = PROGRAM.replace(from, to);
        let found = uploads(&changed);
        assert!(
            found
                .iter()
                .any(|d| d.starts_with("PW5601") && d.contains("is not a literal path")),
            "{to}: {found:?}"
        );
    }
}

#[test]
fn its_clauses_are_an_uploads_alone() {
    // An upload's head on a page, and a page's on an upload.
    let on_page = PROGRAM.replace(
        "    route     \"/about\"\n",
        "    route     \"/about\"\n    max_bytes 10\n",
    );
    assert!(
        uploads(&on_page).iter().any(|d| d.starts_with("PW5105")),
        "{:?}",
        uploads(&on_page)
    );
    let on_upload = PROGRAM.replace(
        "    max_height 4096\n",
        "    max_height 4096\n    cache      shared\n",
    );
    assert!(uploads(&on_upload).iter().any(|d| d.starts_with("PW5105")));
}

#[test]
fn its_paths_are_no_pages() {
    let page_at_route =
        PROGRAM.replace("route     \"/about\"", "route     \"/uploads/post-image\"");
    assert_eq!(
        uploads(&page_at_route),
        [
            "PW5602 `PostImage` answers `/uploads/post-image`, its route, and so does the page \
          `About`, at `/uploads/post-image`"
        ]
    );
    // A page whose route has a query part is served at its path (ADR-0309).
    let page_with_query = PROGRAM
        .replace("page About() {", "page About(q: Option<String>) {")
        .replace(
            "route     \"/about\"",
            "route     \"/uploads/post-image{?q}\"",
        );
    assert_eq!(
        uploads(&page_with_query),
        [
            "PW5602 `PostImage` answers `/uploads/post-image`, its route, and so does the page \
          `About`, at `/uploads/post-image{?q}`"
        ]
    );
    let page_under_serves = PROGRAM.replace("route     \"/about\"", "route     \"/images/{id}\"");
    let found = uploads(&page_under_serves);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("what it serves"), "{found:?}");
    // A parameter in the page's route at the lease's segment.
    let lease = PROGRAM.replace(
        "route     \"/about\"",
        "route     \"/uploads/post-image/7\"",
    );
    assert!(uploads(&lease)[0].contains("its lease"));
    // The control: a page beside it, not under it.
    let beside = PROGRAM.replace("route     \"/about\"", "route     \"/images\"");
    assert_eq!(uploads(&beside), Vec::<String>::new());
}

#[test]
fn two_uploads_are_at_two_paths() {
    let second = format!(
        "{PROGRAM}\nupload Avatar\n    route      \"/uploads/post-image\"\n    serves     \
         \"/avatars\"\n    max_bytes  1000\n    types      png\n    max_width  64\n    \
         max_height 64\n"
    );
    let found = uploads(&second);
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW5602 `Avatar` answers `/uploads/post-image`")),
        "{found:?}"
    );
    let apart = second.replace(
        "route      \"/uploads/post-image\"\n    serves     \"/avatars\"",
        "route      \"/uploads/avatar\"\n    serves     \"/avatars\"",
    );
    assert_eq!(uploads(&apart), Vec::<String>::new());
}

#[test]
fn a_form_that_sends_a_file_posts_it_to_an_upload() {
    let form =
        "<form method=\"post\" action=\"/uploads/post-image\" enctype=\"multipart/form-data\">";
    for (to, fault) in [
        (
            "<form method=\"post\" action=\"/about\" enctype=\"multipart/form-data\">",
            "posts to `/about`, which no upload declares",
        ),
        (
            "<form method=\"get\" action=\"/uploads/post-image\" enctype=\"multipart/form-data\">",
            "is not `method=\"post\"`",
        ),
        (
            "<form method=\"post\" action=\"/uploads/post-image\">",
            "is not `enctype=\"multipart/form-data\"`",
        ),
        (
            "<form method=\"post\" enctype=\"multipart/form-data\">",
            "states no `action`",
        ),
    ] {
        let changed = PROGRAM.replace(form, to);
        let found = uploads(&changed);
        assert_eq!(found.len(), 1, "{to}: {found:?}");
        assert!(
            found[0].starts_with("PW5603 a form in `Home` that sends a file")
                && found[0].contains(fault),
            "{to}: {found:?}"
        );
    }
    // Two files, and none, to an upload's route.
    let two = PROGRAM.replace(
        "<button type=\"submit\">Attach</button>",
        "<input type=\"file\" name=\"other\"><button type=\"submit\">Attach</button>",
    );
    assert!(uploads(&two)[0].contains("sends 2 files"));
    let none = PROGRAM.replace(
        "<label>Image <input type=\"file\" name=\"image\" accept=\"image/png,image/jpeg,image/webp,image/gif\"></label>",
        "<label>Name <input type=\"text\" name=\"n\"></label>",
    );
    assert!(uploads(&none)[0].contains("sends 0 files"));
    // A form that sends no file, anywhere, is not an upload's: the control.
    let plain = format!(
        "{PROGRAM}\npage Search() {{\n    route     \"/search\"\n    placement origin\n\n    view {{\n        \
         <title>Search</title>\n        <main><h1>Search</h1><form method=\"get\" action=\"/search\"><label>Q \
         <input type=\"text\" name=\"q\"></label></form></main>\n    }}\n}}\n"
    );
    assert_eq!(uploads(&plain), Vec::<String>::new());
}

#[test]
fn a_build_writes_each_upload_for_its_host() {
    let units: Vec<pw_core::check::Unit> = program(PROGRAM)
        .into_iter()
        .map(|(path, src)| pw_core::check::Unit {
            hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
            path,
            src,
        })
        .collect();
    let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|u| &u.hir).collect();
    assert_eq!(
        pw_core::uploads::upload_clauses(&hirs),
        [pw_core::uploads::UploadClauses {
            module: None,
            source: None,
            name: "PostImage".into(),
            route: "/uploads/post-image".into(),
            serves: "/images".into(),
            max_bytes: 5_000_000,
            types: vec!["png".into(), "jpeg".into(), "webp".into(), "gif".into()],
            max_width: 4096,
            max_height: 4096,
        }]
    );
    // A malformed upload is not written for its host: past what a host
    // reads whole, or a kind no host sniffs.
    for malformed in [
        PROGRAM.replace("max_bytes  5_000_000", "max_bytes  100_000_001"),
        PROGRAM.replace("types      png, jpeg, webp, gif", "types      png, svg"),
    ] {
        let units: Vec<pw_core::check::Unit> = program(&malformed)
            .into_iter()
            .map(|(path, src)| pw_core::check::Unit {
                hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                path,
                src,
            })
            .collect();
        let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|u| &u.hir).collect();
        assert_eq!(pw_core::uploads::upload_clauses(&hirs), [], "{malformed}");
    }
    // A link to an upload's route is no link to a page (PW5009).
    let link = PROGRAM.replace(
        "<h1>About</h1>",
        "<h1>About</h1><a href=\"/uploads/post-image\">x</a>",
    );
    assert!(reported(&link).iter().any(|d| d.starts_with("PW5009")));
}
