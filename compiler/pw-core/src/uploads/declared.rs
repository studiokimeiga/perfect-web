//! Resource-only declarations generated for an upload's own module.
use super::*;
use crate::hir::{
    Arena, Body, BodyId, Decl, DeclId, DeclaredType, EffectRef, Expr, ExprId, Literal, MatchArm,
    Param, Pattern, PatternId, Policy, Span, VariantDef,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Attached,
    Claim,
    Publish,
    Discard,
    Src,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedKind {
    Claim,
    Ref,
    Kind,
    Attached,
    Error,
    Function(Operation),
}
#[derive(Debug, Clone)]
pub struct Generated {
    pub upload: DeclId,
    pub what: GeneratedKind,
}

fn ty(name: &str) -> DeclaredType {
    DeclaredType::new(name, Vec::new())
}
fn generic(name: &str, args: Vec<DeclaredType>) -> DeclaredType {
    DeclaredType::new(name, args)
}
fn param(name: &str, ty: DeclaredType, span: &Span) -> Param {
    Param {
        name: name.into(),
        ty: Some(ty),
        span: span.clone(),
    }
}
fn blank(name: &str, span: &Span) -> Decl {
    crate::tables::record(name, &[], span)
}
fn cases(name: &str, values: impl Iterator<Item = String>, span: &Span) -> Decl {
    let mut d = blank(name, span);
    d.fields = None;
    d.variants = Some(
        values
            .map(|name| VariantDef {
                name,
                fields: Vec::new(),
                span: span.clone(),
            })
            .collect(),
    );
    d
}
fn variant(kind: &str) -> String {
    format!("{}{}", kind[..1].to_ascii_uppercase(), &kind[1..])
}

pub fn generate(hir: &mut Hir) {
    let Some((module_id, module, _)) = hir.modules.iter().next() else {
        return;
    };
    let module = module.name.clone();
    let Some((upload, decl)) = hir
        .all_decls()
        .find(|(_, d)| d.kind == DeclKind::Upload && d.name == module)
    else {
        return;
    };
    let Some(meta) = read(decl) else { return };
    let span = decl.name_span.clone();
    let visibility = decl.visibility.clone();
    let mut claim = blank("Claim", &span);
    claim.kind = DeclKind::Opaque;
    claim.fields = None;
    claim.opaque_of = Some(ty("String"));
    let reference = crate::tables::record(
        "Ref",
        &[
            ("key".into(), ty("String")),
            ("generation".into(), ty("String")),
            ("kind".into(), ty("Kind")),
            ("width".into(), ty("Int")),
            ("height".into(), ty("Int")),
        ],
        &span,
    );
    let attached = crate::tables::record(
        "Attached",
        &[
            ("src".into(), ty("String")),
            ("width".into(), ty("Int")),
            ("height".into(), ty("Int")),
        ],
        &span,
    );
    let mut declarations = vec![
        (claim, GeneratedKind::Claim),
        (reference, GeneratedKind::Ref),
        (
            cases("Kind", meta.types.iter().map(|k| variant(k)), &span),
            GeneratedKind::Kind,
        ),
        (attached, GeneratedKind::Attached),
        (
            cases("Error", std::iter::once("NotFound".into()), &span),
            GeneratedKind::Error,
        ),
    ];
    if !hir
        .all_decls()
        .any(|(_, d)| d.kind == DeclKind::Import && d.name == "capability" && d.imports.is_empty())
    {
        let mut import = blank("capability", &span);
        import.kind = DeclKind::Import;
        import.fields = None;
        let id = DeclId(hir.decls.alloc(import, span.clone()));
        hir.modules.get_mut(module_id).unwrap().decls.push(id);
    }
    let session = generic("capability.Session", vec![ty("capability.SessionId")]);
    for (name, operation, result, resource, database) in [
        (
            "attached",
            Operation::Attached,
            generic("List", vec![ty("Attached")]),
            None,
            "read",
        ),
        (
            "claim",
            Operation::Claim,
            generic("Result", vec![ty("Claim"), ty("Error")]),
            Some("acquire"),
            "read",
        ),
        (
            "publish",
            Operation::Publish,
            generic("Result", vec![ty("Ref"), ty("Error")]),
            Some("release"),
            "write",
        ),
        (
            "discard",
            Operation::Discard,
            generic("Result", vec![ty("Attached"), ty("Error")]),
            Some("release"),
            "write",
        ),
    ] {
        let mut d = blank(name, &span);
        d.kind = DeclKind::Fn;
        d.fields = None;
        d.params = vec![param("session", session.clone(), &span)];
        if matches!(operation, Operation::Publish | Operation::Discard) {
            d.params.push(param("claim", ty("Claim"), &span));
        }
        d.ret = Some(result);
        let mut effects = vec![EffectRef {
            path: format!("database.{database}"),
            written: format!("database.{database}<{module}>"),
            args: vec![module.clone()],
            span: span.clone(),
        }];
        if let Some(resource) = resource {
            effects.push(EffectRef {
                path: format!("resource.{resource}"),
                written: format!("resource.{resource}<{module}.Claim>"),
                args: vec![format!("{module}.Claim")],
                span: span.clone(),
            });
        }
        d.declared_effects = Some(effects);
        d.policies.push(Policy {
            name: "host".into(),
            value: format!("\"pw:uploads/{}#{}\"", crate::wit::ident(&module), name),
            roots: vec![],
            keys: vec![],
            span: span.clone(),
        });
        declarations.push((d, GeneratedKind::Function(operation)));
    }
    let mut source = blank("src", &span);
    source.kind = DeclKind::Fn;
    source.fields = None;
    source.params = vec![param("image", ty("Ref"), &span)];
    source.ret = Some(ty("String"));
    source.declared_effects = Some(vec![]);
    declarations.push((source, GeneratedKind::Function(Operation::Src)));
    for (mut decl, what) in declarations {
        decl.visibility = visibility.clone();
        let id = DeclId(hir.decls.alloc(decl, span.clone()));
        if what == GeneratedKind::Function(Operation::Src) {
            let body = src_body(id, &meta, &span);
            let body = BodyId(hir.bodies.alloc(body, span.clone()));
            hir.decls.get_mut(id.index()).unwrap().body = Some(body);
        }
        hir.modules.get_mut(module_id).unwrap().decls.push(id);
        hir.generated_uploads.insert(id, Generated { upload, what });
    }
}

fn src_body(owner: DeclId, meta: &UploadClauses, span: &Span) -> Body {
    let mut exprs = Arena::default();
    let mut pats = Arena::default();
    let image = ExprId(exprs.alloc(Expr::Name("image".into()), span.clone()));
    let key = ExprId(exprs.alloc(
        Expr::Field {
            base: image,
            name: "key".into(),
        },
        span.clone(),
    ));
    let kind = ExprId(exprs.alloc(
        Expr::Field {
            base: image,
            name: "kind".into(),
        },
        span.clone(),
    ));
    let arms = meta
        .types
        .iter()
        .map(|name| {
            let pat = PatternId(pats.alloc(
                Pattern::Ctor {
                    path: variant(name),
                    args: vec![],
                },
                span.clone(),
            ));
            let body = ExprId(exprs.alloc(
                Expr::Literal(Literal::Str(serde_json::to_string(name).unwrap())),
                span.clone(),
            ));
            MatchArm { pat, body }
        })
        .collect();
    let suffix = ExprId(exprs.alloc(
        Expr::Match {
            scrutinee: kind,
            arms,
        },
        span.clone(),
    ));
    let root = ExprId(exprs.alloc(
        Expr::Interpolated {
            text: format!("\"{}/{{key}}.{{suffix}}\"", meta.serves),
            parts: vec![key, suffix],
        },
        span.clone(),
    ));
    Body {
        owner,
        exprs,
        pats,
        types: Arena::default(),
        nodes: Arena::default(),
        root,
        signals: Default::default(),
        param_types: Default::default(),
        bound: Default::default(),
        provides: Default::default(),
    }
}

pub(crate) fn check_generated(
    hirs: &[&Hir],
    unit: usize,
    inference: &crate::effects::Inference<'_>,
) -> Vec<Diagnostic> {
    let hir = hirs[unit];
    let mut out = vec![];
    for (id, d) in hir
        .all_decls()
        .filter(|(id, d)| d.kind == DeclKind::Upload && hir.module_of(*id) == Some(d.name.as_str()))
    {
        let identity = crate::wit::ident(&d.name);
        if let Some(other) = upload_clauses(hirs).iter().find(|other| {
            other.module.is_some()
                && other.name != d.name
                && crate::wit::ident(&other.name) == identity
        }) {
            // Charter §16.3's boundary: the other upload, which emits the
            // same interface (the L5 corpus, 2026-11-17: it named none).
            // Only an upload that emits it: in the module of its name, and
            // well formed, as `upload_clauses` reads one (its review, L1: an
            // upload of that name in another module emits nothing).
            let mut related = Vec::new();
            for (u, h) in hirs.iter().enumerate() {
                let own = h.modules.iter().next().map(|(_, m, _)| m.name.as_str());
                for (_, o) in h.all_decls() {
                    if o.kind == DeclKind::Upload
                        && o.name == other.name
                        && own == Some(o.name.as_str())
                        && read(o).is_some()
                    {
                        related.push(Related::in_unit(
                            u,
                            o.name_span.clone(),
                            format!("`{}` emits the interface `{identity}` too", o.name),
                        ));
                    }
                }
            }
            out.push(diagnostic(
                codes::UPLOAD_IDENTITY_TAKEN,
                "upload_interface_collision",
                format!(
                    "`{}` emits a generated upload interface already named by another declaration",
                    d.name
                ),
                hir.decl_span(id),
                related,
                "Each resource operation has one emitted identity and declared meaning.",
                "give each upload a distinct emitted module name".into(),
            ));
        }
    }
    for (id, g) in &hir.generated_uploads {
        let d = hir.decl(*id);
        let identity = crate::wit::ident(&d.name);
        if let Some((_, decl)) = hir.all_decls().find(|&(other, decl)| {
            !hir.generated_uploads.contains_key(&other) && crate::wit::ident(&decl.name) == identity
        }) {
            // Charter §16.3's boundary: the program's declaration, which
            // emits the generated one's name (the L5 corpus, 2026-11-17).
            let related = vec![Related::new(
                decl.name_span.clone(),
                format!("`{}` is declared here, and emits `{identity}`", decl.name),
            )];
            out.push(diagnostic(codes::UPLOAD_IDENTITY_TAKEN,"upload_generated_collision",format!("`{}` is a generated upload declaration and the program declares its emitted identity",d.name),hir.decl_span(g.upload),related,"Generated resource types and operations have one declared meaning.","rename the program's declaration".into()));
        }
    }
    // Each generated claim's resource, and the upload whose claim it is:
    // charter §16.3's boundary for a command that takes one (the L5 corpus,
    // 2026-11-17: it named none).
    let resources = hirs
        .iter()
        .enumerate()
        .flat_map(|(u, h)| {
            h.generated_uploads
                .iter()
                .filter(|&(_id, g)| g.what == GeneratedKind::Claim)
                .map(move |(id, g)| {
                    let upload = h.decl(g.upload);
                    (
                        format!(
                            "{}.{}",
                            h.module_of(*id).unwrap_or_default(),
                            h.decl(*id).name
                        ),
                        (u, upload.name_span.clone(), upload.name.clone()),
                    )
                })
        })
        .collect::<Vec<_>>();
    for (id, d) in hir
        .all_decls()
        .filter(|(_, d)| d.kind == DeclKind::Command && d.policy("idempotent_by").is_none())
    {
        let effects = inference.effective_effects(unit, hir, id);
        if let Some((_, (u, at, upload))) = resources.iter().find(|(r, _)| {
            effects.iter().any(|e| {
                e == &format!("resource.acquire<{r}>") || e == &format!("resource.release<{r}>")
            })
        }) {
            let related = vec![Related::in_unit(
                *u,
                at.clone(),
                format!("`{upload}` is the upload whose claim `{}` takes", d.name),
            )];
            out.push(diagnostic(
                codes::UPLOAD_CLAIM_NOT_IDEMPOTENT,
                "upload_without_idempotency",
                format!(
                    "`{}` acquires or releases a generic upload claim and requires idempotent_by",
                    d.name
                ),
                hir.decl_span(id),
                related,
                "An uncertain upload commit is reconciled using the command's durable record.",
                "declare idempotent_by for this command".into(),
            ));
        }
    }
    out
}
