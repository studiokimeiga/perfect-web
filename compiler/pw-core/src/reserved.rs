//! **A program's routes are its own** (track `deploy`, a production host;
//! the review's finding 5b): no page's route and no upload's path is under
//! one the platform's host answers itself (PW7001).
//!
//! The host answers its own routes before any page's: its runtime's
//! subscription and reads, its build's modules, its commands, its health,
//! readiness and metrics, the relying party's sign-in. A page declared at
//! one of them would never be served, or worse, an upload at `/stream`
//! would take the subscription from every page; and a production host,
//! which serves only the program's routes and the platform's own, would
//! refuse a page under a development server's test controls (`/bench/…`)
//! that a development server had served. So `pw check` refuses each.

use crate::codes;
use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};
use crate::hir::{DeclKind, Hir};

/// **The paths the platform's host answers itself**, each with every path
/// under it, segment by segment (`/command` holds `/command/x`, not
/// `/commands`). The host keeps the same list (`production::RESERVED` in
/// the development server, which a test there holds to this one, and every
/// route it answers itself to that). A development server's own routes
/// elsewhere (`/menu`, `/menu-entry`, `/page/…`) yield to a page a program
/// declares there, and are not among them: `/menu` is a restaurant's.
pub const RESERVED: &[&str] = &[
    "/_pw",
    "/bench",
    "/command",
    "/computed",
    "/dev-idp",
    "/handler",
    "/metrics",
    "/payments",
    "/pw-handlers",
    "/pw-read",
    "/pw-render.wasm",
    "/pw-resume.wasm",
    "/pw-runtime.mjs",
    "/sign-in",
    "/sign-out",
    "/sign-up",
    "/speculation",
    "/StorePage.html",
    "/stream",
];

/// **The reserved path `path` is, or is under**, where it is one: its query
/// part aside (`/stream{?doc}` is `/stream`'s).
pub fn reserved_by(path: &str) -> Option<&'static str> {
    let path = crate::routes::route_parts(path).0;
    RESERVED.iter().copied().find(|r| {
        path.strip_prefix(r)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// **What `hir` declares under a reserved path** (PW7001): each page's route
/// and each upload's route and the path it serves at.
pub fn check(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        let at = hir.decl_span(id);
        let declared: Vec<(&str, String, crate::hir::Span)> = match decl.kind {
            DeclKind::Page => crate::routes::declared_route(hir, decl)
                .map(|route| ("route", route, at.clone()))
                .into_iter()
                .collect(),
            DeclKind::Upload => ["route", "serves"]
                .into_iter()
                .filter_map(|head| {
                    let p = decl.policy(head)?;
                    Some((
                        head,
                        p.value.trim().trim_matches('"').to_string(),
                        p.span.clone(),
                    ))
                })
                .collect(),
            _ => Vec::new(),
        };
        for (head, path, span) in declared {
            let Some(reserved) = reserved_by(&path) else {
                continue;
            };
            let code = codes::ROUTE_RESERVED;
            out.push(Diagnostic {
                code: code.id,
                invariant: code.invariant,
                reason: "route_reserved",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!(
                    "`{}` is declared at `{head} \"{path}\"`, under `{reserved}`, which the \
                     platform's host answers itself",
                    decl.name
                ),
                primary_span: span,
                related: if decl.kind == DeclKind::Upload {
                    vec![Related::new(
                        at.clone(),
                        format!("the upload `{}`", decl.name),
                    )]
                } else {
                    Vec::new()
                },
                explanation: Some(
                    "The host answers its own routes before any page's or upload's: its \
                     runtime's subscription and reads, its build's modules, its commands, its \
                     health, readiness and metrics, the relying party's sign-in, and a \
                     development server's test controls. A program declared there would \
                     never be served, or would take a route every page needs, and a \
                     production host serves only the program's routes and the platform's \
                     own."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: format!(
                        "declare `{}` at a path of the program's own, outside `{reserved}`",
                        decl.name
                    ),
                    replacement: None,
                }],
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A path is reserved where it is a reserved one or under one**,
    /// segment by segment, its query part aside; a path that only begins
    /// with the same letters is the program's.
    #[test]
    fn a_path_is_reserved_where_it_is_under_a_reserved_one() {
        for (path, by) in [
            ("/_pw", "/_pw"),
            ("/_pw/health", "/_pw"),
            ("/bench/clock", "/bench"),
            ("/command/attach", "/command"),
            ("/stream", "/stream"),
            ("/stream{?doc}", "/stream"),
            ("/sign-in/callback", "/sign-in"),
            ("/pw-runtime.mjs", "/pw-runtime.mjs"),
            ("/dev-idp/authorize", "/dev-idp"),
            ("/handler/{id}", "/handler"),
        ] {
            assert_eq!(reserved_by(path), Some(by), "{path}");
        }
        for path in [
            "/",
            "/benchmark",
            "/commands",
            "/menu",
            "/page/{slug}",
            "/menus/{name}",
            "/streams",
            "/stores/{id}",
            "/uploads/post-image",
            "/images",
            "/_pwx",
            "/word/{word}",
            "/search{?q}",
        ] {
            assert_eq!(reserved_by(path), None, "{path}");
        }
    }
}
