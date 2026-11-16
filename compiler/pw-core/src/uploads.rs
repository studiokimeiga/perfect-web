//! **A file a form posts, and its limits** (track `uploads`, ADR-0260; the
//! integrator's ruling of 2026-10-07 on its Q1).
//!
//! ```text
//! upload PostImage
//!     route      "/uploads/post-image"
//!     serves     "/images"
//!     max_bytes  5_000_000
//!     types      png, jpeg, webp, gif
//!     max_width  4096
//!     max_height 4096
//! ```
//!
//! The program states its upload's invariants, as `PostText` states its 280
//! characters, and the host holds a browser to them: `pw build` writes each
//! declaration to `uploads.json`, which the host reads as it reads
//! `sources.json`. Each limit is a literal; `types` is a closed set, each
//! kind one the host sniffs from a file's bytes. A deployment may lower a
//! limit and never raise one.
//!
//! - **PW5601**: each clause written, and each as its domain reads it; a
//!   path a literal path, and at most what a development host reads whole.
//! - **PW5602**: an upload's paths are its own: its `route`, the lease it
//!   shows under it, and what it serves, no page's and no other upload's.
//! - **PW5603**: a form that sends a file posts it, as
//!   `multipart/form-data`, to an upload's route, with one file in it; and
//!   a form posting to an upload's route is such a form. A form is checked
//!   as a link is (PW5009): against what the program declares.

mod declared;
pub(crate) use declared::check_generated;
pub use declared::{Generated, GeneratedKind, Operation, generate};
use std::collections::BTreeMap;

use crate::codes;
use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};
use crate::hir::{AttrValue, DeclKind, Expr, Hir, Node};

/// The heads an upload states, each once.
const CLAUSES: [&str; 6] = [
    "route",
    "serves",
    "max_bytes",
    "types",
    "max_width",
    "max_height",
];

/// **At most what an upload may be**: a development host reads a form's body
/// whole, so a limit above this is one it would allocate for anyone.
pub const MOST_BYTES: u64 = 100_000_000;

/// **An upload, as `pw build` writes it** (`uploads.json`), the host's
/// `uploads::Declared`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UploadClauses {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub route: String,
    pub serves: String,
    pub max_bytes: u64,
    pub types: Vec<String>,
    pub max_width: u32,
    pub max_height: u32,
}

/// A path as an upload writes one: `/` and segments, each lowercase letters,
/// digits, `-` and `_`; no parameter, no query, no trailing `/`.
fn literal_path(p: &str) -> bool {
    p.len() > 1
        && p.starts_with('/')
        && !p.ends_with('/')
        && p[1..].split('/').all(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        })
}

fn unquoted(v: &str) -> &str {
    v.trim().trim_matches('"')
}

/// One declaration's clauses, where every one is written and well-formed.
fn read(decl: &crate::hir::Decl) -> Option<UploadClauses> {
    let text = |h: &str| decl.policy(h).map(|p| p.value.trim().to_string());
    let count = |h: &str| crate::policy::count(&text(h)?);
    let route = text("route")?;
    let serves = text("serves")?;
    let (route, serves) = (unquoted(&route), unquoted(&serves));
    let types: Vec<String> = text("types")?
        .split(',')
        .map(|t| t.trim().to_string())
        .collect();
    let bytes = count("max_bytes")?;
    let ok = literal_path(route)
        && literal_path(serves)
        && bytes <= MOST_BYTES
        && types
            .iter()
            .all(|t| crate::policy::UPLOAD_TYPES.contains(&t.as_str()));
    ok.then_some(UploadClauses {
        name: decl.name.clone(),
        module: None,
        source: None,
        route: route.to_string(),
        serves: serves.to_string(),
        max_bytes: bytes,
        types,
        max_width: u32::try_from(count("max_width")?).ok()?,
        max_height: u32::try_from(count("max_height")?).ok()?,
    })
}

/// **Every well-formed upload the program declares**, as `pw build` writes
/// them. A malformed one is PW5601's, and is not written.
pub fn upload_clauses(hirs: &[&Hir]) -> Vec<UploadClauses> {
    hirs.iter()
        .flat_map(|hir| {
            hir.all_decls()
                .filter(|(_, d)| d.kind == DeclKind::Upload)
                .filter_map(move |(_, d)| {
                    let mut upload = read(d)?;
                    if hir
                        .modules
                        .iter()
                        .next()
                        .is_some_and(|(_, m, _)| m.name == d.name)
                    {
                        upload.module = Some(d.name.clone());
                        upload.source = crate::tables::source_of(hirs, &d.name);
                    }
                    Some(upload)
                })
        })
        .collect()
}

fn diagnostic(
    code: codes::Code,
    reason: &'static str,
    message: String,
    span: crate::hir::Span,
    related: Vec<Related>,
    explanation: &str,
    repair: String,
) -> Diagnostic {
    Diagnostic {
        code: code.id,
        invariant: code.invariant,
        reason,
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message,
        primary_span: span,
        related,
        explanation: Some(explanation.to_string()),
        repairs: vec![Repair {
            description: repair,
            replacement: None,
        }],
    }
}

/// Does a page's route, `/post/{id}`, match the path `path`, a parameter
/// matching any one segment?
fn route_matches(route: &str, path: &str) -> bool {
    let r: Vec<&str> = route.trim_matches('/').split('/').collect();
    let p: Vec<&str> = path.trim_matches('/').split('/').collect();
    r.len() == p.len()
        && r.iter()
            .zip(&p)
            .all(|(r, p)| (r.starts_with('{') && r.ends_with('}')) || r == p || p.starts_with('{'))
}

/// **What `unit` writes wrong of uploads**: its declarations' clauses
/// (PW5601), their paths (PW5602), and its forms (PW5603).
pub fn check(hirs: &[&Hir], unit: usize) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let hir = hirs[unit];
    malformed(hir, &mut out);
    taken(hirs, unit, &mut out);
    // Every upload's route, its other clauses aside: a form posting to one
    // whose limits are malformed is PW5601's to report, not this one's.
    let routes: Vec<String> = hirs
        .iter()
        .flat_map(|hir| hir.all_decls())
        .filter(|(_, d)| d.kind == DeclKind::Upload)
        .filter_map(|(_, d)| d.policy("route").map(|p| unquoted(&p.value).to_string()))
        .filter(|r| literal_path(r))
        .collect();
    forms(hir, &routes, &mut out);
    out
}

const MALFORMED_WHY: &str = "An upload states where a form posts a file, where what is committed \
     is served, and the most it may be: its bytes, its kinds, its width and its height. Each is \
     a literal the host holds every browser to before it keeps a byte, so a limit left out is a \
     file nothing bounds.";

fn malformed(hir: &Hir, out: &mut Vec<Diagnostic>) {
    for (id, decl) in hir.all_decls() {
        if decl.kind != DeclKind::Upload {
            continue;
        }
        let at = hir.decl_span(id);
        for head in CLAUSES {
            if decl.policy(head).is_none() {
                out.push(diagnostic(
                    codes::UPLOAD_MALFORMED,
                    "upload_clause_missing",
                    format!("`{}` states no `{head}`", decl.name),
                    at.clone(),
                    Vec::new(),
                    MALFORMED_WHY,
                    format!("write `{head}` in `{}`", decl.name),
                ));
            }
        }
        for head in ["route", "serves"] {
            let Some(p) = decl.policy(head) else { continue };
            let path = unquoted(&p.value);
            if !literal_path(path) {
                out.push(diagnostic(
                    codes::UPLOAD_MALFORMED,
                    "upload_path_not_literal",
                    format!("`{head} {}` is not a literal path", p.value.trim()),
                    p.span.clone(),
                    vec![Related::new(
                        at.clone(),
                        format!("the upload `{}`", decl.name),
                    )],
                    "An upload's paths are where a host answers it: `/` and segments of \
                     lowercase letters, digits, `-` and `_`, with no parameter, since nothing \
                     gives one.",
                    format!("write `{head} \"/uploads/{}\"`", decl.name.to_lowercase()),
                ));
            }
        }
        if let Some(p) = decl.policy("max_bytes")
            && crate::policy::count(&p.value).is_some_and(|n| n > MOST_BYTES)
        {
            out.push(diagnostic(
                codes::UPLOAD_MALFORMED,
                "upload_too_large",
                format!(
                    "`max_bytes {}` is more than {MOST_BYTES}, the most a host reads whole",
                    p.value.trim()
                ),
                p.span.clone(),
                Vec::new(),
                "A development host reads an upload's form whole before it keeps it, so its \
                 limit is an allocation anyone may ask for.",
                format!("write `max_bytes` at most `{MOST_BYTES}`"),
            ));
        }
        for head in ["max_width", "max_height"] {
            if let Some(p) = decl.policy(head)
                && crate::policy::count(&p.value).is_some_and(|n| n > u32::MAX as u64)
            {
                out.push(diagnostic(
                    codes::UPLOAD_MALFORMED,
                    "upload_too_large",
                    format!("`{head} {}` is more than any image's", p.value.trim()),
                    p.span.clone(),
                    Vec::new(),
                    "An image's width and height are each at most 2³²−1 in every format \
                     an upload may be.",
                    format!("write `{head}` at most `{}`", u32::MAX),
                ));
            }
        }
    }
}

fn taken(hirs: &[&Hir], unit: usize, out: &mut Vec<Diagnostic>) {
    // Every page's route, and every upload's paths before this unit's.
    let mut pages: Vec<(String, String)> = Vec::new();
    for hir in hirs {
        for (_, decl) in hir.all_decls() {
            if decl.kind == DeclKind::Page
                && let Some(route) = crate::routes::declared_route(hir, decl)
            {
                pages.push((route, decl.name.clone()));
            }
        }
    }
    let mut claimed: BTreeMap<String, String> = BTreeMap::new();
    for (u, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if decl.kind != DeclKind::Upload {
                continue;
            }
            let Some(u_clauses) = read(decl) else {
                continue;
            };
            // What the upload answers: a form posted to its route, a lease
            // shown under it, and a committed file under what it serves.
            let paths = [
                (u_clauses.route.clone(), "its route"),
                (format!("{}/{{lease}}", u_clauses.route), "its lease"),
                (format!("{}/{{key}}", u_clauses.serves), "what it serves"),
            ];
            for (path, what) in paths {
                // A page's path, its query part aside (ADR-0309): `/a{?q}`
                // is served at `/a`.
                let page = pages
                    .iter()
                    .find(|(r, _)| route_matches(crate::routes::route_parts(r).0, &path));
                let upload = claimed.get(&path).filter(|other| **other != decl.name);
                let by = match (page, upload) {
                    (Some((route, name)), _) => format!("the page `{name}`, at `{route}`"),
                    (None, Some(other)) => format!("the upload `{other}`"),
                    (None, None) => continue,
                };
                if u == unit {
                    out.push(diagnostic(
                        codes::UPLOAD_ROUTE_TAKEN,
                        "upload_route_taken",
                        format!("`{}` answers `{path}`, {what}, and so does {by}", decl.name),
                        hir.decl_span(id),
                        Vec::new(),
                        "A host answers an upload's paths before any page's (ADR-0253's \
                         seam), so a page there would never be served, and two uploads \
                         at one path leave it to whichever is found first.",
                        "give the upload paths no page and no other upload has".to_string(),
                    ));
                }
            }
            for path in [
                u_clauses.route.clone(),
                format!("{}/{{lease}}", u_clauses.route),
                format!("{}/{{key}}", u_clauses.serves),
            ] {
                claimed.entry(path).or_insert_with(|| decl.name.clone());
            }
        }
    }
}

/// A static attribute's value, without its quotes.
fn static_attr<'a>(attrs: &'a [crate::hir::Attr], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| match &a.value {
            AttrValue::Static(raw) => Some(raw.trim_matches('"')),
            _ => None,
        })
}

fn forms(hir: &Hir, uploads: &[String], out: &mut Vec<Diagnostic>) {
    for (id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let mut roots = Vec::new();
        for e in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(e) {
                roots.extend(r.iter().copied());
            }
        }
        for n in body.walk_markup(&roots) {
            let Node::Element {
                tag,
                attrs,
                children,
                ..
            } = body.node(n)
            else {
                continue;
            };
            if tag != "form" {
                continue;
            }
            let files = body
                .walk_markup(children)
                .into_iter()
                .filter(|c| {
                    matches!(body.node(*c), Node::Element { tag, attrs, .. }
                        if tag == "input" && static_attr(attrs, "type") == Some("file"))
                })
                .count();
            let action = static_attr(attrs, "action");
            let enctype = static_attr(attrs, "enctype");
            let to_upload = action.and_then(|a| uploads.iter().find(|u| u.as_str() == a));
            if files == 0 && enctype.is_none() && to_upload.is_none() {
                continue;
            }
            let span = attrs
                .iter()
                .find(|a| a.name == "action")
                .map_or_else(|| hir.decl_span(id), |a| a.span.clone());
            let mut faults = Vec::new();
            if to_upload.is_none() {
                faults.push(match action {
                    Some(a) => format!("posts to `{a}`, which no upload declares"),
                    None => "states no `action`".to_string(),
                });
            }
            if !static_attr(attrs, "method").is_some_and(|m| m.eq_ignore_ascii_case("post")) {
                faults.push("is not `method=\"post\"`".to_string());
            }
            if !enctype.is_some_and(|e| e.eq_ignore_ascii_case("multipart/form-data")) {
                faults.push("is not `enctype=\"multipart/form-data\"`".to_string());
            }
            if files != 1 {
                faults.push(format!("sends {files} files, where an upload is one"));
            }
            if faults.is_empty() {
                continue;
            }
            let declared: Vec<String> = uploads.iter().map(|u| format!("`{u}`")).collect();
            out.push(diagnostic(
                codes::FILE_FORM_POSTS_TO_NO_UPLOAD,
                "file_form_posts_to_no_upload",
                format!(
                    "a form in `{}` that sends a file {}",
                    decl.name,
                    faults.join(", ")
                ),
                span,
                vec![Related::new(
                    hir.decl_span(id),
                    format!("`{}` renders the form", decl.name),
                )],
                "A file reaches a host as a form's part, posted as multipart/form-data to \
                 the route an upload declares, which reads it within that upload's limits. A \
                 form posted anywhere else sends its file to what never reads it, and a form \
                 that is not multipart sends its name and not its bytes.",
                match declared.as_slice() {
                    [] => "declare an `upload`, and post the form to its route".to_string(),
                    _ => format!(
                        "write `<form method=\"post\" enctype=\"multipart/form-data\" \
                         action=..>` with one `<input type=\"file\">`, to {}",
                        declared.join(" or ")
                    ),
                },
            ));
        }
    }
}
