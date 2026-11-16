//! Exact generic resource ABI and whole values, independent of host callbacks.
use super::{Val, table_link, table_selectors};
use crate::assets::{Access, Operation, Reference};
use crate::tables::{Case, Field, Stored};
use wasmtime::component::types::{ComponentFunc, Type};

fn string() -> Stored {
    Stored::String {
        min_length: None,
        max_length: None,
    }
}
fn integer() -> Stored {
    Stored::Int {
        at_least: None,
        at_most: None,
    }
}
fn record(fields: Vec<(&str, Stored)>) -> Stored {
    Stored::Record(
        fields
            .into_iter()
            .map(|(name, stored)| Field {
                name: name.into(),
                stored,
            })
            .collect(),
    )
}
fn error() -> Stored {
    Stored::Cases(vec![Case {
        name: "NotFound".into(),
        carries: vec![],
    }])
}
fn attached(access: &Access) -> Stored {
    let dimension = |maximum| Stored::Int {
        at_least: Some(1),
        at_most: Some(i64::from(maximum)),
    };
    record(vec![
        ("src", string()),
        ("width", dimension(access.declaration().max_width())),
        ("height", dimension(access.declaration().max_height())),
    ])
}
fn reference(access: &Access) -> Stored {
    record(vec![
        ("key", string()),
        ("generation", string()),
        (
            "kind",
            Stored::Cases(
                access
                    .declaration()
                    .kinds()
                    .iter()
                    .map(|name| Case {
                        name: name.clone(),
                        carries: vec![],
                    })
                    .collect(),
            ),
        ),
        ("width", integer()),
        ("height", integer()),
    ])
}
fn answer(access: &Access) -> Stored {
    match access.operation() {
        Operation::Attached => Stored::List(Box::new(attached(access))),
        Operation::Claim => string(),
        Operation::Publish => reference(access),
        Operation::Discard => attached(access),
    }
}
pub(super) fn check_import(access: &Access, function: &ComponentFunc) -> Result<(), String> {
    let params = function.params().map(|(_, t)| t).collect::<Vec<_>>();
    let count = if matches!(access.operation(), Operation::Publish | Operation::Discard) {
        2
    } else {
        1
    };
    if params.len() != count || params.iter().any(|ty| !matches!(ty, Type::String)) {
        return Err("upload ABI: session/claim parameters differ from declaration".into());
    }
    let results = function.results().collect::<Vec<_>>();
    if access.operation() == Operation::Attached {
        let [ty] = results.as_slice() else {
            return Err("upload ABI: attached answers one list".into());
        };
        return table_link::stored(&answer(access), ty).map_err(|why| format!("upload ABI: {why}"));
    }
    let [Type::Result(result)] = results.as_slice() else {
        return Err("upload ABI: resource operation answers one Result".into());
    };
    let ok = result
        .ok()
        .ok_or("upload ABI: result has no success value")?;
    let err = result
        .err()
        .ok_or("upload ABI: result has no declared error")?;
    table_link::stored(&answer(access), &ok)
        .and_then(|()| table_link::stored(&error(), &err))
        .map_err(|why| format!("upload ABI: {why}"))
}
pub(super) fn check_arguments(access: &Access, args: &[Val]) -> Result<(), String> {
    let count = if matches!(access.operation(), Operation::Publish | Operation::Discard) {
        2
    } else {
        1
    };
    if args.len() != count
        || args
            .iter()
            .any(|value| !matches!(value,Val::String(s) if !s.is_empty()))
    {
        return Err("upload arguments: session and claim are nonempty handles".into());
    }
    Ok(())
}
pub(super) fn check_answer(access: &Access, results: &[Val]) -> Result<(), String> {
    let [result] = results else {
        return Err("upload answer is one value".into());
    };
    if access.operation() == Operation::Attached {
        return table_link::check_value(&answer(access), result);
    }
    match result {
        Val::Result(Err(Some(value))) => table_link::check_value(&error(), value),
        Val::Result(Ok(Some(value))) => {
            table_link::check_value(&answer(access), value)?;
            if access.operation() == Operation::Publish {
                let json = table_selectors::json(&reference(access), value)?;
                access
                    .declaration()
                    .validate_reference(&Reference::from_value(&json)?)?;
            } else if access.operation() == Operation::Claim
                && !matches!(value.as_ref(),Val::String(s) if !s.is_empty())
            {
                return Err("upload claim answer is empty".into());
            }
            Ok(())
        }
        _ => Err("upload answer has no declared result payload".into()),
    }
}
