//! **`pw build --dist DIR` builds a whole app** (ADR-0429): what an app's
//! build script did by hand, the command does. The standard packages are
//! added where the app names none of them, a directory is read for its
//! `.pw` files, the runtime a host serves it with is written beside its
//! artifacts, each `--asset` too, and last the record of what it was built
//! from (ADR-0166). Nothing but an app's build is ever replaced.
//!
//! The binary is run as a user runs it, as `related_files.rs` runs it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::Digest as _;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository")
}

/// A directory of this test's own, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pw-whole-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn pw(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pw"))
        .args(args)
        .current_dir(repository())
        .output()
        .expect("pw runs")
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// What `sources.json` says each source is: its path and its digest.
fn recorded(dist: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(dist.join("sources.json")).unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    json["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .map(|f| {
                    (
                        f["path"].as_str().unwrap_or_default().to_string(),
                        f["sha256"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn sha256(path: &Path) -> String {
    sha2::Sha256::digest(std::fs::read(path).unwrap_or_default())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The `.pw` files of a standard package, as the command adds them.
fn package(name: &str) -> Vec<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(repository().join("packages").join(name))
        .map(|dir| dir.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|x| x == "pw"));
    files.sort();
    files.into_iter().map(|p| p.display().to_string()).collect()
}

#[test]
fn an_app_is_built_with_the_standard_packages_its_runtime_and_its_record() {
    let dist = scratch("rows");
    let out = pw(&[
        "build",
        "--dist",
        dist.to_str().unwrap_or_default(),
        "examples/rows/app.pw",
    ]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(dist.join("build/templates.json").is_file(), "its artifacts");
    assert_eq!(
        std::fs::read(dist.join("pw-runtime.mjs")).unwrap_or_default(),
        std::fs::read(repository().join("spikes/own-renderer/public/pw-runtime.mjs"))
            .unwrap_or_default(),
        "the runtime its host serves it with"
    );
    let mut expected = package("pw-std");
    expected.extend(package("pw-platform-web"));
    expected.push(
        repository()
            .join("examples/rows/app.pw")
            .display()
            .to_string(),
    );
    let record = recorded(&dist);
    assert_eq!(
        record.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
        expected
    );
    for (path, digest) in &record {
        assert_eq!(digest, &sha256(Path::new(path)), "{path}");
    }
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn a_directory_given_is_each_pw_file_in_it() {
    let dist = scratch("folder");
    let out = pw(&[
        "build",
        "--dist",
        dist.to_str().unwrap_or_default(),
        "examples/rows",
    ]);
    assert!(out.status.success(), "{}", said(&out));
    let record = recorded(&dist);
    let apps: Vec<&str> = record
        .iter()
        .filter_map(|(p, _)| {
            p.strip_prefix(&repository().join("examples/rows/").display().to_string())
        })
        .collect();
    assert_eq!(apps, ["app.pw", "events.pw"]);
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn standard_packages_named_are_not_added_again() {
    let dist = scratch("named");
    let mut args = vec![
        "build".to_string(),
        "--dist".to_string(),
        dist.display().to_string(),
    ];
    args.extend(package("pw-std"));
    args.extend(package("pw-platform-web"));
    args.push("examples/rows/app.pw".to_string());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = pw(&args);
    assert!(out.status.success(), "{}", said(&out));
    let paths: Vec<String> = recorded(&dist).into_iter().map(|(p, _)| p).collect();
    let mut unique = paths.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), paths.len(), "each source once: {paths:?}");
    assert_eq!(
        paths.len(),
        package("pw-std").len() + package("pw-platform-web").len() + 1
    );
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn an_asset_is_written_beside_the_build() {
    let dist = scratch("asset");
    let asset = scratch("asset-file");
    let _ = std::fs::create_dir_all(&asset);
    let file = asset.join("style.css");
    let _ = std::fs::write(&file, "p{color:red}");
    let named = format!("{}=app.css", file.display());
    let out = pw(&[
        "build",
        "--dist",
        dist.to_str().unwrap_or_default(),
        "--asset",
        &named,
        "examples/rows/app.pw",
    ]);
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(
        std::fs::read_to_string(dist.join("app.css")).unwrap_or_default(),
        "p{color:red}"
    );
    for name in ["../app.css", "build", "sources.json", "pw-runtime.mjs", ""] {
        let named = format!("{}={name}", file.display());
        let out = pw(&[
            "build",
            "--dist",
            dist.to_str().unwrap_or_default(),
            "--asset",
            &named,
            "examples/rows/app.pw",
        ]);
        assert_eq!(out.status.code(), Some(2), "{name}: {}", said(&out));
    }
    let _ = std::fs::remove_dir_all(&dist);
    let _ = std::fs::remove_dir_all(&asset);
}

#[test]
fn a_directory_that_is_no_build_is_not_replaced() {
    let dist = scratch("kept");
    let _ = std::fs::create_dir_all(dist.join("build"));
    let _ = std::fs::write(dist.join("keep.txt"), "mine");
    let out = pw(&[
        "build",
        "--dist",
        dist.to_str().unwrap_or_default(),
        "examples/rows/app.pw",
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", said(&out));
    assert_eq!(
        std::fs::read_to_string(dist.join("keep.txt")).unwrap_or_default(),
        "mine"
    );
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn a_build_of_an_app_is_replaced_whole() {
    let dist = scratch("again");
    let first = pw(&[
        "build",
        "--dist",
        dist.to_str().unwrap_or_default(),
        "examples/rows/app.pw",
    ]);
    assert!(first.status.success(), "{}", said(&first));
    let _ = std::fs::write(dist.join("stale.txt"), "from before");
    let again = pw(&[
        "build",
        "--dist",
        dist.to_str().unwrap_or_default(),
        "examples/rows/app.pw",
    ]);
    assert!(again.status.success(), "{}", said(&again));
    assert!(
        !dist.join("stale.txt").exists(),
        "nothing of the build before is left"
    );
    assert!(dist.join("sources.json").is_file());
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn a_program_in_error_builds_nothing() {
    let dist = scratch("error");
    let app = scratch("error-app");
    let _ = std::fs::create_dir_all(&app);
    let _ = std::fs::write(
        app.join("app.pw"),
        "module bad\n\npage Home() {\n    route \"/\"\n\n    signal n: Int = \"text\"\n\n    view {\n        <title>Home</title>\n    }\n}\n",
    );
    let out = pw(&[
        "build",
        "--dist",
        dist.to_str().unwrap_or_default(),
        app.to_str().unwrap_or_default(),
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(said(&out).contains("nothing built"), "{}", said(&out));
    assert!(
        !dist.join("sources.json").exists(),
        "no record of a build that is not"
    );
    let _ = std::fs::remove_dir_all(&dist);
    let _ = std::fs::remove_dir_all(&app);
}

#[test]
fn what_builds_a_whole_app_is_asked_of_dist_alone() {
    for args in [
        vec![
            "build",
            "--out",
            "/tmp/pw-whole-app-out",
            "--production",
            "examples/rows/app.pw",
        ],
        vec![
            "build",
            "--out",
            "/tmp/pw-whole-app-out",
            "--dist",
            "/tmp/pw-whole-app-dist",
            "examples/rows/app.pw",
        ],
        vec!["build", "--dist", "/tmp/pw-whole-app-none"],
    ] {
        let out = pw(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", said(&out));
    }
}

/// `pw` run with a `node` of this test's own first on its path: one that
/// writes what it was asked, a line each, to `asked`, and packs nothing.
#[cfg(unix)]
fn pw_with_recording_node(args: &[&str], dir: &Path) -> Output {
    use std::os::unix::fs::PermissionsExt as _;
    let bin = dir.join("bin");
    let _ = std::fs::create_dir_all(&bin);
    let node = bin.join("node");
    let _ = std::fs::write(
        &node,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            dir.join("asked").display()
        ),
    );
    let _ = std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755));
    let path = std::env::var("PATH").unwrap_or_default();
    Command::new(env!("CARGO_BIN_EXE_pw"))
        .args(args)
        .current_dir(repository())
        .env("PATH", format!("{}:{path}", bin.display()))
        .output()
        .expect("pw runs")
}

/// **A production build drops what its pages report, unless asked**
/// (ADR-0430): the packaging is asked to keep them with `--report` alone.
#[cfg(unix)]
#[test]
fn a_production_build_keeps_its_reports_only_where_asked() {
    for (flags, keeps) in [
        (vec!["--production"], false),
        (vec!["--production", "--report"], true),
    ] {
        let dir = scratch(if keeps {
            "report-kept"
        } else {
            "report-dropped"
        });
        let dist = dir.join("dist");
        let mut args = vec!["build", "--dist", dist.to_str().unwrap_or_default()];
        args.extend(flags.iter().copied());
        args.push("examples/rows/app.pw");
        let out = pw_with_recording_node(&args, &dir);
        assert!(out.status.success(), "{flags:?}: {}", said(&out));
        let asked = std::fs::read_to_string(dir.join("asked")).unwrap_or_default();
        let asked: Vec<&str> = asked.lines().collect();
        let packaging = repository().join("spikes/own-renderer/optimize-assets.mjs");
        let first = asked.first().and_then(|p| Path::new(p).canonicalize().ok());
        assert_eq!(
            first,
            Some(packaging),
            "{flags:?}: the packaging is run: {asked:?}"
        );
        assert_eq!(
            asked.get(1..).unwrap_or_default(),
            if keeps {
                vec![dist.to_str().unwrap_or_default(), "--report"]
            } else {
                vec![dist.to_str().unwrap_or_default()]
            },
            "{flags:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A development build is its runtime as written, which reports: `--report`
/// is a production build's to ask for, and refused without one.
#[test]
fn reports_are_kept_by_a_production_build_alone() {
    let dist = scratch("report-alone");
    let out = pw(&[
        "build",
        "--dist",
        dist.to_str().unwrap_or_default(),
        "--report",
        "examples/rows/app.pw",
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", said(&out));
    assert!(!dist.exists(), "nothing is built");
}
