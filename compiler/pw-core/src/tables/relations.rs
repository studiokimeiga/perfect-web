//! Fixed one-hop public-target windows, never a query language or optimizer.
use super::*;

pub(super) fn clause(value: &str) -> Result<WindowClause, String> {
    let words = value.split_whitespace().collect::<Vec<_>>();
    match words.as_slice() {
        [
            name,
            "from",
            target,
            "row",
            row_type,
            "on",
            source_field,
            "to",
            target_field,
        ] if [name, target, source_field, target_field]
            .iter()
            .all(|n| is_name(n))
            && row_type.split('.').all(is_name) =>
        {
            Ok(WindowClause {
                name: (*name).into(),
                target: (*target).into(),
                row_type: (*row_type).into(),
                source_field: (*source_field).into(),
                target_field: (*target_field).into(),
            })
        }
        _ => Err(
            "is written `window <name> from <table> row <row type> on <field> to <field>`".into(),
        ),
    }
}

fn selectors(t: &Table, window: &WindowClause) -> Vec<(Vec<String>, Span)> {
    let paths = t.key.iter().map(|key| (&key.value, &key.span)).chain(
        t.indexes
            .iter()
            .map(|index| (&index.value.fields, &index.span)),
    );
    let mut out = Vec::new();
    for (path, span) in paths {
        if let Some(at) = path.iter().position(|field| field == &window.source_field)
            && at > 0
        {
            let by = path[..at].to_vec();
            if !matches!(reading(t, &by), Reading::Refused)
                && !out.iter().any(|(old, _)| old == &by)
            {
                out.push((by, span.clone()));
            }
        }
    }
    out
}

fn name(window: &WindowClause, primitive: Primitive, by: &[String]) -> String {
    format!(
        "{}_{}_by_{}{}",
        if primitive.descending() {
            "last"
        } else {
            "first"
        },
        window.name,
        by.join("_"),
        if primitive.many() { "_many" } else { "" }
    )
}

pub(super) fn of_function<'a>(
    t: &'a Table,
    function: &str,
    primitive: Primitive,
    by: &[String],
) -> Option<&'a WindowClause> {
    primitive
        .related()
        .then(|| {
            t.windows
                .iter()
                .find(|w| name(&w.value, primitive, by) == function)
                .map(|w| &w.value)
        })
        .flatten()
}

pub(super) fn planned(
    t: &Table,
    fields: &[(String, DeclaredType)],
) -> (Vec<Planned>, Vec<(Decl, Wrote)>) {
    let mut functions = Vec::new();
    let mut records: Vec<(Decl, Wrote)> = Vec::new();
    let list = |ty| DeclaredType::new("List", vec![ty]);
    let ty = |field: &str| {
        fields
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, ty)| ty.clone())
    };
    for window in &t.windows {
        for (by, span) in selectors(t, &window.value) {
            if by.iter().any(|field| ty(field).is_none()) {
                continue;
            }
            let mut tuple = |held: &[String]| {
                if held.len() == 1 {
                    return ty(&held[0]).unwrap();
                }
                let name = tuple_name(&t.name, held);
                let what = Wrote::Tuple {
                    fields: held.to_vec(),
                };
                if !records.iter().any(|(d, w)| d.name == name && *w == what) {
                    records.push((
                        record(
                            &name,
                            &held
                                .iter()
                                .map(|field| (field.clone(), ty(field).unwrap()))
                                .collect::<Vec<_>>(),
                            &span,
                        ),
                        what,
                    ));
                }
                DeclaredType::new(name, Vec::new())
            };
            let mode = reading(t, &by);
            let mut given = by
                .iter()
                .map(|field| (field.clone(), ty(field).unwrap()))
                .collect::<Vec<_>>();
            if matches!(mode, Reading::Owner | Reading::Sharer) {
                given[0].1 = DeclaredType::new("capability.User", vec![given[0].1.clone()]);
            }
            let target = DeclaredType::new(&window.value.row_type, Vec::new());
            let mut limit = "limit".to_string();
            while given
                .iter()
                .any(|(field, _)| crate::wit::ident(field) == crate::wit::ident(&limit))
            {
                limit.push_str("_bound");
            }
            let many = match mode {
                Reading::Plain => Some(vec![("keys".into(), list(tuple(&by)))]),
                Reading::Owner | Reading::Sharer if by.len() > 1 => Some(vec![
                    given[0].clone(),
                    (
                        (if crate::wit::ident(&by[0]) == "keys" {
                            "keys_values"
                        } else {
                            "keys"
                        })
                        .into(),
                        list(tuple(&by[1..])),
                    ),
                ]),
                _ => None,
            };
            let group = if many.is_some() {
                let key = tuple(&by);
                let group = format!(
                    "{}{}Window",
                    tuple_name(&t.name, &by),
                    pascal(&window.value.name)
                );
                records.push((
                    record(
                        &group,
                        &[("key".into(), key), ("rows".into(), list(target.clone()))],
                        &span,
                    ),
                    Wrote::RelatedGroup {
                        fields: by.clone(),
                        window: window.value.clone(),
                    },
                ));
                Some(group)
            } else {
                None
            };
            for (single, batch) in [
                (Primitive::RelatedFirst, Primitive::RelatedFirstMany),
                (Primitive::RelatedLast, Primitive::RelatedLastMany),
            ] {
                let mut params = given.clone();
                params.push((limit.clone(), DeclaredType::new("Int", Vec::new())));
                functions.push(Planned {
                    name: name(&window.value, single, &by),
                    params,
                    ret: list(target.clone()),
                    primitive: single,
                    by: by.clone(),
                    span: span.clone(),
                });
                if let (Some(mut params), Some(group)) = (many.clone(), group.as_ref()) {
                    let mut bound = "limit".to_string();
                    while params
                        .iter()
                        .any(|(field, _)| crate::wit::ident(field) == crate::wit::ident(&bound))
                    {
                        bound.push_str("_bound");
                    }
                    params.push((bound, DeclaredType::new("Int", Vec::new())));
                    functions.push(Planned {
                        name: name(&window.value, batch, &by),
                        params,
                        ret: list(DeclaredType::new(group, Vec::new())),
                        primitive: batch,
                        by: by.clone(),
                        span: span.clone(),
                    });
                }
            }
        }
    }
    (functions, records)
}

pub(super) fn check(
    hirs: &[&Hir],
    unit: usize,
    ws: &Workspace,
    sigs: &Signatures,
    t: &Table,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut names = BTreeSet::new();
    let hir = hirs[unit];
    let module = ws.module_of(unit).map(|m| m.name.as_str());
    let source = source_of(hirs, &t.name);
    let source_row =
        t.row.as_ref().and_then(
            |row| match ws.resolve_in(unit, Namespace::Type, &row.value) {
                Resolution::Local(def) => sigs.type_decl(def).and_then(|d| d.record.as_ref()),
                _ => None,
            },
        );
    for window in &t.windows {
        let w = &window.value;
        let bad = |message: String| {
            malformed(
                message,
                window.span.clone(),
                Vec::new(),
                "declare one typed relation between tables of the same source".into(),
            )
        };
        if !names.insert(crate::wit::ident(&w.name)) {
            out.push(bad(format!("window `{}` is declared twice", w.name)));
        }
        let Some((target_unit, target)) = table_module(ws, hirs, unit, &w.target) else {
            out.push(bad(format!(
                "window `{}` names no target table `{}` visible here",
                w.name, w.target
            )));
            continue;
        };
        if target.owner.is_some() || !target.private_fields().is_empty() {
            out.push(refusal(
                codes::OWNED_READ_BY_ITS_READER,
                "owned_read_by_its_reader",
                Severity::Error,
                format!(
                    "window `{}` requires a public target table, but `{}` is protected",
                    w.name, w.target
                ),
                window.span.clone(),
                Vec::new(),
                READ_BY_ITS_READER,
                "use a public target table".into(),
            ));
        }
        if source.is_none() || source != source_of(hirs, &target.name) {
            out.push(bad(format!(
                "window `{}` requires both tables to have the same source, explicitly declared",
                w.name
            )));
        }
        if target.order.is_none() {
            out.push(bad(format!(
                "window `{}` requires a declared target order",
                w.name
            )));
        }
        let target_module = ws.module_of(target_unit).map(|m| m.name.as_str());
        let annotation = sigs.resolve_type(
            module,
            hir.decl(t.decl),
            &DeclaredType::new(&w.row_type, Vec::new()),
            window.span.clone(),
        );
        let target_type = target.row.as_ref().map(|row| {
            sigs.resolve_type(
                target_module,
                hirs[target_unit].decl(target.decl),
                &DeclaredType::new(&row.value, Vec::new()),
                row.span.clone(),
            )
        });
        // Resolving a written policy type does not by itself run the normal
        // import-visibility diagnostic. Its actual target row must be visible
        // across this module boundary as well as nominally identical.
        let private_row = target_unit != unit
            && target_type
                .as_ref()
                .and_then(|ty| ty.resolved())
                .and_then(|ty| ty.def_id())
                .is_some_and(|def| {
                    hirs[def.unit].decl(DeclId(def.decl)).visibility.as_deref() == Some("private")
                });
        if private_row {
            out.push(bad(format!(
                "window `{}` row type `{}` is private to its declaring module",
                w.name, w.row_type
            )));
        }
        if !annotation
            .resolved()
            .zip(target_type.as_ref().and_then(|r| r.resolved()))
            .is_some_and(|(a, b)| a.same_as(b))
        {
            out.push(bad(format!(
                "window `{}` row type `{}` is not the exact declared row type of `{}`",
                w.name, w.row_type, w.target
            )));
        }
        let target_row = target_type
            .as_ref()
            .and_then(|r| r.resolved())
            .and_then(|r| r.def_id())
            .and_then(|def| sigs.type_decl(def))
            .and_then(|d| d.record.as_ref());
        let source_field = source_row
            .and_then(|row| row.iter().find(|(name, _)| *name == w.source_field))
            .and_then(|(_, ty)| ty.resolved());
        let target_field = target_row
            .and_then(|row| row.iter().find(|(name, _)| *name == w.target_field))
            .and_then(|(_, ty)| ty.resolved());
        if !source_field
            .zip(target_field)
            .is_some_and(|(a, b)| a.same_as(b) && keyable_at(sigs, a, 0).is_ok())
        {
            out.push(bad(format!(
                "window `{}` match fields must have the same complete keyable type",
                w.name
            )));
        }
        let indexed = target
            .key
            .as_ref()
            .is_some_and(|key| key.value.first() == Some(&w.target_field))
            || target
                .indexes
                .iter()
                .any(|index| index.value.fields.first() == Some(&w.target_field));
        if !indexed || selectors(t, w).is_empty() {
            out.push(bad(format!("window `{}` requires a declared lookup path for its source selector/match and target match",w.name)));
        }
        if t.private_fields().contains(&w.source_field)
            && selectors(t, w)
                .iter()
                .any(|(by, _)| matches!(reading(t, by), Reading::Sharer))
        {
            out.push(refusal(
                codes::PRIVATE_FIELD_UNSEEN,
                "private_field_unseen",
                Severity::Error,
                format!(
                    "window `{}` cannot select through private match field `{}` for a sharer",
                    w.name, w.source_field
                ),
                window.span.clone(),
                Vec::new(),
                UNSEEN,
                "match only a field in the shared seen row".into(),
            ));
        }
    }
    out
}
