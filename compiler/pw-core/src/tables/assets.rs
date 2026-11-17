//! An annotated public field stores the exact generated upload reference.
use super::*;
use crate::uploads::GeneratedKind;

pub(super) fn clause(value: &str) -> Result<AssetClause, String> {
    let words = value.split_whitespace().collect::<Vec<_>>();
    match words.as_slice() {
        [field, "from", upload] if is_name(field) && upload.split('.').all(is_name) => {
            Ok(AssetClause {
                field: (*field).into(),
                upload: (*upload).into(),
            })
        }
        _ => {
            Err("is written `asset <field> from <upload>` for one top-level reference field".into())
        }
    }
}
fn contains_kind(
    hirs: &[&Hir],
    sigs: &Signatures,
    ty: &ResolvedType,
    seen: &mut BTreeSet<DefId>,
    what: GeneratedKind,
) -> bool {
    if ty
        .args()
        .iter()
        .any(|a| contains_kind(hirs, sigs, a, seen, what))
    {
        return true;
    }
    let Some(def) = ty.def_id() else { return false };
    if hirs[def.unit]
        .generated_uploads
        .get(&DeclId(def.decl))
        .is_some_and(|g| g.what == what)
    {
        return true;
    }
    if !seen.insert(def) {
        return false;
    }
    sigs.type_decl(def).is_some_and(|d| {
        d.record
            .iter()
            .flatten()
            .filter_map(|(_, t)| t.resolved())
            .chain(d.representation.iter().filter_map(|t| t.resolved()))
            .chain(
                d.variants
                    .iter()
                    .flatten()
                    .flat_map(|(_, ts)| ts)
                    .filter_map(|t| t.resolved()),
            )
            .any(|t| contains_kind(hirs, sigs, t, seen, what))
    })
}
pub(super) fn check(
    hirs: &[&Hir],
    unit: usize,
    ws: &Workspace,
    sigs: &Signatures,
    t: &Table,
) -> Vec<Diagnostic> {
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    let row = t
        .row
        .as_ref()
        .and_then(|r| match ws.resolve_in(unit, Namespace::Type, &r.value) {
            Resolution::Local(def) => sigs.type_decl(def).and_then(|d| d.record.as_ref()),
            _ => None,
        });
    let bad = |message: String, span: Span| {
        asset_malformed(
            message,
            span,
            vec![],
            "use the generated reference of an upload held by this table's source".into(),
        )
    };
    for (field, ty) in row.into_iter().flatten() {
        if ty.resolved().is_some_and(|ty| {
            contains_kind(hirs, sigs, ty, &mut BTreeSet::new(), GeneratedKind::Claim)
        }) {
            out.push(bad(
                format!(
                    "field `{field}` cannot store an upload claim, a transaction-owned resource"
                ),
                t.span.clone(),
            ));
        }
    }
    for (field, ty) in row.into_iter().flatten() {
        if let Some(ty) = ty.resolved()
            && contains_kind(hirs, sigs, ty, &mut BTreeSet::new(), GeneratedKind::Ref)
        {
            if !t.assets.iter().any(|asset| asset.value.field == *field) {
                out.push(bad(
                    format!("reference field `{field}` requires its asset association"),
                    t.span.clone(),
                ));
            }
            let base = if ty.as_builtin() == Some(crate::resolved::Builtin::Option) {
                ty.args().first().unwrap_or(ty)
            } else {
                ty
            };
            let direct = base.def_id().is_some_and(|def| {
                hirs[def.unit]
                    .generated_uploads
                    .get(&DeclId(def.decl))
                    .is_some_and(|g| g.what == GeneratedKind::Ref)
            });
            if !direct {
                out.push(bad(
                    format!("reference field `{field}` must be a top-level Ref or Option<Ref>"),
                    t.span.clone(),
                ));
            }
        }
    }
    // **An asset's table is one every reader reads** (L7's G4, 2026-11-17):
    // a table no one owns, or an owned `public table` its owner writes and
    // every reader reads (ADR-0395's ruling 4, the feed's posts and their
    // images). Never one read by its reader alone: an image is served to
    // anyone by its hash, cached immutably and kept as one generation with
    // every row naming its bytes. A `public` or unowned table keeping fields
    // private or rows shared is refused by its own rules (PW0369, PW0370).
    // One defect, one diagnostic (ADR-0112): the table's, told once at its
    // first asset, its others related (G4's review, L2).
    if t.read_by_its_reader()
        && let Some((first, others)) = t.assets.split_first()
    {
        let kept_too = others
            .iter()
            .map(|other| {
                Related::new(
                    other.span.clone(),
                    format!("asset field `{}` is kept in it too", other.value.field),
                )
            })
            .collect();
        out.push(asset_malformed(
            format!(
                "asset field `{}` requires a table every reader reads, but `{}` is read by its \
                 reader alone",
                first.value.field, t.name
            ),
            first.span.clone(),
            kept_too,
            "declare it a `public table`, read by every reader and written by its owner alone, \
             or keep the image in a table no one owns"
                .into(),
        ));
    }
    for asset in &t.assets {
        let a = &asset.value;
        let err = |message| bad(message, asset.span.clone());
        if !seen.insert(&a.field) {
            out.push(err(format!(
                "asset field `{}` is associated twice",
                a.field
            )));
        }
        let Some(upload) = ws.visible_module(unit, &a.upload) else {
            out.push(err(format!(
                "asset field `{}` names no visible upload `{}`",
                a.field, a.upload
            )));
            continue;
        };
        let target = hirs[upload.unit];
        if target.all_decls().any(|(_, d)| {
            d.kind == DeclKind::Upload
                && d.name == upload.name
                && d.visibility.as_deref() == Some("private")
        }) {
            out.push(err("an asset requires a public upload declaration".into()));
        }
        let reference = target.generated_uploads.iter().find_map(|(id, g)| {
            (g.what == GeneratedKind::Ref).then_some(DefId {
                unit: upload.unit,
                decl: id.0,
            })
        });
        if reference.is_none() {
            out.push(err(format!(
                "asset field `{}` requires a generated upload in the module of its name",
                a.field
            )));
        }
        let source = source_of(hirs, &t.name);
        let target_source = source_of(hirs, &upload.name);
        if source.is_none() || source != target_source {
            out.push(err(
                "an asset and its upload require the same explicit source".into(),
            ));
        }
        let Some(field) = row
            .and_then(|r| r.iter().find(|(name, _)| *name == a.field))
            .and_then(|(_, ty)| ty.resolved())
        else {
            out.push(err(format!(
                "asset field `{}` is not declared in the row",
                a.field
            )));
            continue;
        };
        let field = if field.as_builtin() == Some(crate::resolved::Builtin::Option) {
            field.args().first().unwrap_or(field)
        } else {
            field
        };
        if field.as_builtin().is_some() {
            out.push(err(format!(
                "asset field `{}` must be one top-level Ref or Option<Ref>",
                a.field
            )));
        }
        if field.def_id() != reference || !field.args().is_empty() {
            out.push(err(format!(
                "asset field `{}` must have the exact generated upload reference type",
                a.field
            )));
        }
    }
    out
}
