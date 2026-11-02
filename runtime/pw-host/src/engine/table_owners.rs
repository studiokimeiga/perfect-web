//! **An owned table's function is its caller's** (track `row-ownership`,
//! ADR-0384 H1).
//!
//! A table that states `owned by` gives functions that take the reader's
//! handle (`capability.User<UserId>`) in the place of the field they lead
//! with: the owner, or a field the table shares with. On the wire a handle
//! is its user's id (ADR-0270), so a component could pass any id there; the
//! checker refuses a program that makes one (PW5037), and this module is
//! what makes a forged one, or a stale or hand-built component's, fail at
//! run time too, whichever adapter serves the table:
//!
//! - [`admit`], before any adapter is asked: the handle a call passes is
//!   its caller's, the user the host serves it for ([`Caller`]), and a call
//!   no one makes reads and writes no one's rows; an `insert`'s or a
//!   `put`'s row is its owner's, the handle's (PostgreSQL's `WITH CHECK`);
//!   then the arguments as the table's plain function takes them, a set
//!   form's handle given back to each of its keys, so an adapter and
//!   [`super::table_link::check_answer`] read them as any table's;
//! - [`seen`], after the answer is held to the row: a reader the table
//!   shares with is answered the seen row, the private fields left out,
//!   never passed on;
//! - [`check_import`], where it links: the import is typed as the owned
//!   function is, the handle a string beside a set form's list and an
//!   `insert`'s or a `put`'s row, and a sharer's read answering the seen row.

use wasmtime::component::Val;
use wasmtime::component::types::Type;

use super::table_link;
use crate::tables::{Access, Caller, Column, Primitive};

/// **The arguments an owned table's function was given, as its table's
/// plain function takes them**, once its handle is its caller's (see the
/// module). An unowned table's are given back as they came.
pub fn admit(access: &Access, caller: &Caller, args: Vec<Val>) -> Result<Vec<Val>, String> {
    let Some(handle) = access.handle() else {
        return Ok(args);
    };
    let import = access.import();
    let Caller::User(user) = caller else {
        return Err(format!(
            "`{import}` reads or writes an owned table's rows, and no one makes this call: a \
             call no reader makes reads and writes no one's rows"
        ));
    };
    match args.first() {
        Some(Val::String(given)) if given == user => {}
        Some(Val::String(_)) => {
            return Err(format!(
                "`{import}` was given a handle that is not its caller's: a handle is the \
                 platform's to make (ADR-0263), and an owned table's rows are read and written \
                 by their reader's alone"
            ));
        }
        _ => {
            return Err(format!(
                "`{import}` is given its reader's handle first, a string, and was not"
            ));
        }
    }
    match access {
        Access::Read(read) if read.many() => {
            // `(handle, list of what follows it)`: each of the list's keys
            // with the handle in its first field's place.
            let [_, Val::List(rest)] = args.as_slice() else {
                return Err(format!(
                    "`{import}` is given its reader's handle and one list of what follows it"
                ));
            };
            let (first, following) = read
                .by
                .split_first()
                .ok_or_else(|| format!("`{import}` reads by nothing"))?;
            let keys = rest
                .iter()
                .map(|item| keyed(first, following, user, item))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|why| format!("`{import}`: {why}"))?;
            Ok(vec![Val::List(keys)])
        }
        Access::Write(write) if matches!(write.primitive, Primitive::Insert | Primitive::Put) => {
            let [_, row] = args.as_slice() else {
                return Err(format!(
                    "`{import}` is given its owner's handle and one row"
                ));
            };
            let owner = table_link::exact_row(&write.row, row)
                .ok()
                .and_then(|fields| {
                    let wit = crate::tables::wit_name(&handle.field);
                    fields.iter().find(|(name, _)| *name == wit)
                })
                .map(|(_, value)| value);
            match owner {
                Some(Val::String(owner)) if owner == user => Ok(vec![row.clone()]),
                // A row that is not exactly the table's is the write
                // decoder's to refuse, in its own words.
                None => Ok(vec![row.clone()]),
                Some(_) => Err(format!(
                    "`{import}` was given a row whose `{}` is not its writer: an owned row is \
                     written by its owner alone",
                    handle.field
                )),
            }
        }
        // A keyed read and a delete take the handle in the place of the
        // field they lead with already.
        _ => Ok(args),
    }
}

/// One key of an owned set form: the caller's id in `first`'s place, then
/// what the list held, a field's value or a record of the fields after it.
fn keyed(first: &Column, following: &[Column], user: &str, item: &Val) -> Result<Val, String> {
    let mut fields = vec![(first.wit.clone(), Val::String(user.to_string()))];
    match (following, item) {
        ([one], value) => fields.push((one.wit.clone(), value.clone())),
        (_, Val::Record(given)) => fields.extend(given.iter().cloned()),
        _ => return Err("a key of several fields is a record of them".into()),
    }
    Ok(Val::Record(fields))
}

/// **What a reader the table shares with is answered** (see the module):
/// each row of a read whose handle has a seen row, the private fields left
/// out, after the engine held the whole row to its read. Any other answer
/// is given back as it came.
pub fn seen(access: &Access, out: Vec<Val>) -> Result<Vec<Val>, String> {
    let Access::Read(read) = access else {
        return Ok(out);
    };
    let Some(seen) = read.handle.as_ref().and_then(|h| h.seen.as_ref()) else {
        return Ok(out);
    };
    let shown = |row: Val| -> Result<Val, String> {
        let Val::Record(fields) = row else {
            return Err("a row is a record".into());
        };
        let kept: Vec<(String, Val)> = fields
            .into_iter()
            .filter(|(name, _)| seen.iter().any(|c| c.wit == *name))
            .collect();
        if kept.len() != seen.len() {
            return Err("a row lacks a field of its seen row".into());
        }
        Ok(Val::Record(kept))
    };
    out.into_iter()
        .map(|answer| match answer {
            Val::Option(Some(row)) => Ok(Val::Option(Some(Box::new(shown(*row)?)))),
            Val::List(rows) => Ok(Val::List(
                rows.into_iter().map(shown).collect::<Result<_, _>>()?,
            )),
            other => Ok(other),
        })
        .collect()
}

/// **Is this owned function's import typed as it is given?** A read takes
/// its fields with the handle, a string, first, or the handle and one list
/// of what follows it; and answers the seen row where its reader is one the
/// table shares with. An `insert` and a `put` take the handle and a row.
/// What else a write takes and answers is [`table_link::check_import`]'s.
pub fn check_import(access: &Access, params: &[Type], results: &[Type]) -> Result<(), String> {
    match access {
        Access::Read(read) => {
            let words = read.primitive.by_words();
            if read.many() {
                let [Type::String, Type::List(rest)] = params else {
                    return Err(format!(
                        "it takes ({}), not its reader's handle and one list of what follows it",
                        table_link::listed(params)
                    ));
                };
                table_link::selector(words, &read.by[1..], &rest.ty())?;
            } else {
                table_link::fields(words, &read.by, params)?;
            }
            let answered = read
                .handle
                .as_ref()
                .and_then(|h| h.seen.as_ref())
                .unwrap_or(&read.row);
            let (wrapper, row) = match (read.one(), results) {
                (true, [Type::Option(row)]) => ("option", row.ty()),
                (false, [Type::List(rows)]) => ("list", rows.ty()),
                _ => {
                    return Err(format!(
                        "it answers ({}), not {} of its rows",
                        table_link::listed(results),
                        if read.one() { "an option" } else { "a list" }
                    ));
                }
            };
            table_link::record(answered, &row).map_err(|why| format!("its {wrapper}'s row: {why}"))
        }
        Access::Write(write) if matches!(write.primitive, Primitive::Insert | Primitive::Put) => {
            let [Type::String, rest @ ..] = params else {
                return Err(format!(
                    "it takes ({}), not its owner's handle and a row",
                    table_link::listed(params)
                ));
            };
            table_link::write_import(write, rest, results)
        }
        Access::Write(write) => table_link::write_import(write, params, results),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::{Handle, Read, Stored, Write};

    fn string(name: &str) -> Column {
        Column::new(
            name,
            Stored::String {
                min_length: None,
                max_length: None,
            },
        )
    }

    /// `Notes`' read by `to`, a sharer's, its seen row without `draft`.
    fn by_to() -> Access {
        let row = vec![
            string("author"),
            string("id"),
            string("to"),
            string("draft"),
        ];
        Access::Read(Read {
            import: "pw:tables/notes#by-to".into(),
            table: "Notes".into(),
            primitive: Primitive::Index,
            by: vec![string("to")],
            row: row.clone(),
            key: vec!["author".into(), "id".into()],
            order: None,
            sorted_by: Vec::new(),
            unique: false,
            bounded: None,
            handle: Some(Handle {
                field: "to".into(),
                seen: Some(row[..3].to_vec()),
            }),
        })
    }

    fn note(author: &str, draft: &str) -> Val {
        Val::Record(vec![
            ("author".into(), Val::String(author.into())),
            ("id".into(), Val::String("n1".into())),
            ("to".into(), Val::String("bob".into())),
            ("draft".into(), Val::String(draft.into())),
        ])
    }

    #[test]
    fn a_sharer_is_answered_the_seen_row() {
        let shown = seen(&by_to(), vec![Val::List(vec![note("alice", "secret")])]);
        assert_eq!(
            shown,
            Ok(vec![Val::List(vec![Val::Record(vec![
                ("author".into(), Val::String("alice".into())),
                ("id".into(), Val::String("n1".into())),
                ("to".into(), Val::String("bob".into())),
            ])])])
        );
    }

    #[test]
    fn a_handle_is_its_callers_or_nothing_is_read() {
        let user = |u: &str| Caller::User(u.into());
        let bob = vec![Val::String("bob".into())];
        assert_eq!(admit(&by_to(), &user("bob"), bob.clone()), Ok(bob.clone()));
        let forged = admit(&by_to(), &user("eve"), bob.clone());
        assert!(
            forged
                .as_ref()
                .is_err_and(|why| why.contains("not its caller's")),
            "{forged:?}"
        );
        let nobody = admit(&by_to(), &Caller::Nobody, bob);
        assert!(
            nobody
                .as_ref()
                .is_err_and(|why| why.contains("no one makes this call")),
            "{nobody:?}"
        );
    }

    #[test]
    fn an_owned_row_is_written_by_its_owner_alone() {
        let row = vec![
            string("author"),
            string("id"),
            string("to"),
            string("draft"),
        ];
        let put = Access::Write(Write {
            import: "pw:tables/notes#put".into(),
            table: "Notes".into(),
            primitive: Primitive::Put,
            by: Vec::new(),
            row,
            key: vec!["author".into(), "id".into()],
            unique: Vec::new(),
            handle: Some(Handle {
                field: "author".into(),
                seen: None,
            }),
        });
        let alice = Caller::User("alice".into());
        let given = |author: &str| vec![Val::String("alice".into()), note(author, "d")];
        assert_eq!(
            admit(&put, &alice, given("alice")),
            Ok(vec![note("alice", "d")])
        );
        let forged = admit(&put, &alice, given("bob"));
        assert!(
            forged
                .as_ref()
                .is_err_and(|why| why.contains("is not its writer")),
            "{forged:?}"
        );
    }
}
