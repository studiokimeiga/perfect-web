//! **A program's routes are its own** (track `deploy`, a production host;
//! the review's finding 5b): `pw check` refuses a page's route, or an
//! upload's route or the path it serves at, under a path the platform's
//! host answers itself (PW7001). Each case states one path, with its
//! control: a path of the program's own beside it.

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

/// The PW7001 diagnostics of `m.pw`, as `CODE message`, and whether it
/// reported anything else.
fn reserved(src: &str) -> (Vec<String>, Vec<String>) {
    let all: Vec<String> = check_sources(&program(src))
        .into_iter()
        .filter(|(path, _)| path == "m.pw")
        .flat_map(|(_, ds)| ds)
        .filter(|d| d.is_error())
        .map(|d| format!("{} {}", d.code, d.message))
        .collect();
    all.into_iter().partition(|d| d.starts_with("PW7001"))
}

/// A page at `route`, and one at the program's own `/about`.
fn page_at(route: &str) -> String {
    format!(
        r#"module m

page Home() {{
    route     "{route}"
    placement origin

    view {{
        <title>Home</title>
        <main><h1>Home</h1></main>
    }}
}}

page About() {{
    route     "/about"
    placement origin

    view {{
        <title>About</title>
        <main><h1>About</h1></main>
    }}
}}
"#
    )
}

/// An upload posted to `route` and served at `serves`, beside a page.
fn upload_at(route: &str, serves: &str) -> String {
    format!(
        r#"module m

upload PostImage
    route      "{route}"
    serves     "{serves}"
    max_bytes  5_000_000
    types      png, jpeg, webp, gif
    max_width  4096
    max_height 4096

page Home() {{
    route     "/"
    placement origin

    view {{
        <title>Home</title>
        <main>
            <h1>Home</h1>
            <form method="post" action="{route}" enctype="multipart/form-data">
                <label>Image <input type="file" name="image" accept="image/png,image/jpeg,image/webp,image/gif"></label>
                <button type="submit">Attach</button>
            </form>
        </main>
    }}
}}
"#
    )
}

/// **A page under a path the platform's host answers itself is refused**:
/// its health and readiness, a development server's test controls, its
/// runtime's subscription and modules, the relying party's sign-in, a
/// development server's measurement page; each named, with the path it is
/// under.
#[test]
fn a_page_under_a_reserved_path_is_refused() {
    for (route, under) in [
        ("/_pw/health", "/_pw"),
        ("/_pw/status", "/_pw"),
        ("/bench/clock", "/bench"),
        ("/bench", "/bench"),
        ("/stream", "/stream"),
        ("/command/x", "/command"),
        ("/metrics", "/metrics"),
        ("/menu", "/menu"),
        ("/sign-in", "/sign-in"),
        ("/sign-in/callback", "/sign-in"),
        ("/pw-runtime.mjs", "/pw-runtime.mjs"),
        ("/handler/{id}", "/handler"),
        ("/StorePage.html", "/StorePage.html"),
        // Its query part aside: a page at `/stream{?doc}` is at `/stream`.
        ("/stream{?doc}", "/stream"),
    ] {
        let src = page_at(route);
        let src = if route.contains("{id}") {
            src.replace("page Home()", "page Home(id: String)")
        } else if route.contains("{?doc}") {
            src.replace("page Home()", "page Home(doc: Option<String>)")
        } else {
            src
        };
        let (reserved, other) = reserved(&src);
        assert_eq!(reserved.len(), 1, "{route}: {reserved:?} {other:?}");
        assert!(
            reserved[0].contains(&format!(
                "`Home` is declared at `route \"{route}\"`, under `{under}`"
            )),
            "{route}: {reserved:?}"
        );
        assert!(other.is_empty(), "{route}: {other:?}");
    }
}

/// **A page at a path of the program's own is not**, though it begins with
/// a reserved path's letters.
#[test]
fn a_page_at_a_path_of_its_own_is_not_reserved() {
    for route in [
        "/",
        "/benchmark",
        "/streams",
        "/commands",
        "/menus",
        "/metrics-report",
        "/sign-inside",
        "/_pwa",
        "/stores/bench",
    ] {
        let (reserved, other) = reserved(&page_at(route));
        assert!(reserved.is_empty(), "{route}: {reserved:?}");
        assert!(other.is_empty(), "{route}: {other:?}");
    }
}

/// **An upload's route, or the path it serves at, under a reserved path is
/// refused**: posted at `/command/attach` or at `/stream`, it would take a
/// route every page's runtime needs; served at `/_pw/images`, a path the
/// host keeps for itself. Its control: `/uploads/post-image` and `/images`.
#[test]
fn an_upload_under_a_reserved_path_is_refused() {
    for (route, serves, refused) in [
        (
            "/command/attach",
            "/images",
            vec!["`route \"/command/attach\"`, under `/command`"],
        ),
        (
            "/stream",
            "/images",
            vec!["`route \"/stream\"`, under `/stream`"],
        ),
        (
            "/uploads/post-image",
            "/_pw/images",
            vec!["`serves \"/_pw/images\"`, under `/_pw`"],
        ),
        (
            "/bench/upload",
            "/handler/images",
            vec![
                "`route \"/bench/upload\"`, under `/bench`",
                "`serves \"/handler/images\"`, under `/handler`",
            ],
        ),
        ("/uploads/post-image", "/images", vec![]),
    ] {
        let (reserved, _) = reserved(&upload_at(route, serves));
        assert_eq!(
            reserved.len(),
            refused.len(),
            "{route} {serves}: {reserved:?}"
        );
        for (said, expected) in reserved.iter().zip(&refused) {
            assert!(
                said.contains(&format!("`PostImage` is declared at {expected}")),
                "{route} {serves}: {said}"
            );
        }
    }
}
