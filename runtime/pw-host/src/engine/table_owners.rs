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
    // A transition grants only the old owner's read/delete side. Narrow it
    // to one ordinary caller before applying every existing argument/row check.
    let narrowed;
    let caller = if let Caller::IdentityTransition {
        previous_guest,
        current,
    } = caller
    {
        let old_owner_access = match access {
            Access::Read(read) => {
                read.source_key.as_ref().unwrap_or(&read.key).first() == Some(&handle.field)
            }
            Access::Write(write) => {
                matches!(write.primitive, Primitive::Delete | Primitive::DeletePrefix)
            }
        };
        let previous = matches!(args.first(), Some(Val::String(given)) if given == previous_guest);
        let user = if previous && old_owner_access {
            previous_guest
        } else {
            current
        };
        narrowed = Caller::User(user.clone());
        &narrowed
    } else {
        caller
    };
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
            if read.primitive.range() {
                let [_, Val::List(requests)] = args.as_slice() else {
                    return Err(format!(
                        "`{import}` takes its reader and selector-and-mark records"
                    ));
                };
                let columns = read.count_columns()?;
                let (first, following) = columns.split_first().ok_or("a range has no reader")?;
                let keys = requests
                    .iter()
                    .map(|request| {
                        let rest = table_link::exact_row(following, request)?;
                        let mut fields = vec![(first.wit.clone(), Val::String(user.clone()))];
                        fields.extend(rest.iter().cloned());
                        Ok(Val::Record(fields))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                return Ok(vec![Val::List(keys)]);
            }
            // `(handle, list of what follows it)`: each of the list's keys
            // with the handle in its first field's place.
            let (selectors, limit) = table_link::window_arguments(read, args.as_slice())?;
            let [_, Val::List(rest)] = selectors else {
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
            let mut normalized = vec![Val::List(keys)];
            if let Some(limit) = limit {
                normalized.push(Val::S64(limit));
            }
            Ok(normalized)
        }
        Access::Write(write) if matches!(write.primitive, Primitive::Insert | Primitive::Put) => {
            let [_, row] = args.as_slice() else {
                return Err(format!(
                    "`{import}` is given its owner's handle and one row"
                ));
            };
            // A row that is not exactly the table's is refused here, before
            // any adapter reads it (the review's note 7a): an owner check
            // that passed what it could not read would trust a decoder
            // further on to refuse it.
            let fields = table_link::exact_row(&write.row, row).map_err(|why| {
                format!("`{import}` was given a row that is not exactly its table's: {why}")
            })?;
            let wit = crate::tables::wit_name(&handle.field);
            match fields.iter().find(|(name, _)| *name == wit) {
                Some((_, Val::String(owner))) if owner == user => Ok(vec![row.clone()]),
                _ => Err(format!(
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
    if read.primitive.counts() {
        return Ok(out);
    }
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
            let params = table_link::range_params(read, params)?;
            let words = read.primitive.by_words();
            if read.many() {
                let [Type::String, Type::List(rest)] = params else {
                    return Err(format!(
                        "it takes ({}), not its reader's handle and one list of what follows it",
                        table_link::listed(params)
                    ));
                };
                if read.primitive.range() {
                    table_link::record(&read.count_columns()?[1..], &rest.ty())?;
                } else {
                    table_link::selector(words, &read.by[1..], &rest.ty())?;
                }
            } else {
                table_link::fields(words, &read.by, params)?;
            }
            if read.primitive.counts() {
                return table_link::count_results(read, results);
            }
            if read.primitive.related() {
                return table_link::related_results(read, results);
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
            target: None,
            source_key: None,
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

    /// `Notes`' `put`, its owner `author`.
    fn put() -> Access {
        Access::Write(Write {
            import: "pw:tables/notes#put".into(),
            table: "Notes".into(),
            primitive: Primitive::Put,
            by: Vec::new(),
            row: vec![
                string("author"),
                string("id"),
                string("to"),
                string("draft"),
            ],
            key: vec!["author".into(), "id".into()],
            unique: Vec::new(),
            handle: Some(Handle {
                field: "author".into(),
                seen: None,
            }),
        })
    }

    #[test]
    fn an_owned_row_is_written_by_its_owner_alone() {
        let put = put();
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

    #[test]
    fn a_row_not_exactly_its_tables_is_refused_before_any_adapter() {
        // The review's note 7a: the owner check passed a row it could not
        // read, trusting a decoder further on to refuse it.
        let put = put();
        let alice = Caller::User("alice".into());
        let field = |name: &str, value: &str| (name.to_string(), Val::String(value.into()));
        let rows = [
            // A field the table does not declare, after its own.
            Val::Record(vec![
                field("author", "alice"),
                field("id", "n1"),
                field("to", "bob"),
                field("draft", "d"),
                field("extra", "x"),
            ]),
            // Its owner missing: nothing to hold to its writer.
            Val::Record(vec![
                field("id", "n1"),
                field("to", "bob"),
                field("draft", "d"),
            ]),
            // Its fields out of their order.
            Val::Record(vec![
                field("id", "n1"),
                field("author", "alice"),
                field("to", "bob"),
                field("draft", "d"),
            ]),
            // No record.
            Val::String("alice".into()),
        ];
        for row in rows {
            let refused = admit(&put, &alice, vec![Val::String("alice".into()), row.clone()]);
            assert!(
                refused
                    .as_ref()
                    .is_err_and(|why| why.contains("a row that is not exactly its table's")),
                "{row:?}: {refused:?}"
            );
        }
        // The neighbour: exactly the table's row, written by its owner.
        assert_eq!(
            admit(
                &put,
                &alice,
                vec![Val::String("alice".into()), note("alice", "d")]
            ),
            Ok(vec![note("alice", "d")])
        );
    }
    #[test]
    fn a_transition_reads_and_deletes_old_owner_rows_but_only_writes_new_rows() {
        let transition = Caller::IdentityTransition {
            previous_guest: "guest".into(),
            current: "alice".into(),
        };
        let Access::Write(write) = put() else {
            unreachable!()
        };
        let own = Access::Read(Read {
            target: None,
            source_key: None,
            import: "pw:tables/notes#get".into(),
            table: "Notes".into(),
            primitive: Primitive::Get,
            by: vec![string("author"), string("id")],
            row: write.row.clone(),
            key: write.key.clone(),
            order: None,
            sorted_by: Vec::new(),
            unique: false,
            bounded: None,
            handle: write.handle.clone(),
        });
        let old = vec![Val::String("guest".into()), Val::String("n1".into())];
        assert_eq!(admit(&own, &transition, old.clone()), Ok(old.clone()));
        let mut delete = write.clone();
        delete.primitive = Primitive::Delete;
        delete.by = vec![string("author"), string("id")];
        assert_eq!(
            admit(&Access::Write(delete), &transition, old.clone()),
            Ok(old)
        );
        assert_eq!(
            admit(
                &put(),
                &transition,
                vec![Val::String("alice".into()), note("alice", "d")]
            ),
            Ok(vec![note("alice", "d")])
        );
        assert!(
            admit(
                &put(),
                &transition,
                vec![Val::String("guest".into()), note("guest", "d")]
            )
            .is_err(),
            "previous guest has no put authority"
        );
        assert!(
            admit(
                &put(),
                &transition,
                vec![Val::String("alice".into()), note("guest", "d")]
            )
            .is_err(),
            "current cannot forge old owner"
        );
        assert!(
            admit(&by_to(), &transition, vec![Val::String("guest".into())]).is_err(),
            "previous guest has no sharer-index authority"
        );
        assert!(
            admit(
                &own,
                &transition,
                vec![Val::String("bob".into()), Val::String("n1".into())]
            )
            .is_err(),
            "no third user's rows"
        );
        assert!(
            admit(
                &own,
                &Caller::User("alice".into()),
                vec![Val::String("guest".into()), Val::String("n1".into())]
            )
            .is_err(),
            "ordinary callers gain no transition privilege"
        );
    }
    #[test]
    fn a_sharers_count_result_is_not_projected_as_a_stored_row() {
        let Access::Read(mut count) = by_to() else {
            unreachable!()
        };
        count.primitive = Primitive::CountMany;
        count.by.push(string("id"));
        let counts = vec![Val::List(vec![Val::Record(vec![
            (
                "key".into(),
                Val::Record(vec![
                    ("to".into(), Val::String("bob".into())),
                    ("id".into(), Val::String("n1".into())),
                ]),
            ),
            ("count".into(), Val::S64(2)),
        ])])];
        assert_eq!(seen(&Access::Read(count), counts.clone()), Ok(counts));
    }

    #[test]
    fn identity_relation_reads_use_membership_ownership_not_the_public_targets_key() {
        let caller = Caller::IdentityTransition {
            previous_guest: "guest".into(),
            current: "account".into(),
        };
        let mut shared = by_to();
        if let Access::Read(read) = &mut shared {
            read.target = Some("Posts".into());
            read.source_key = Some(read.key.clone());
            // A legal public target key may have the same spelling as the
            // source sharer field. It grants no old-guest sharer authority.
            read.key = vec!["to".into()];
            read.primitive = Primitive::RelatedLast;
            read.handle.as_mut().unwrap().seen = None;
        }
        assert!(
            admit(
                &shared,
                &caller,
                vec![Val::String("guest".into()), Val::S64(1)]
            )
            .is_err(),
            "old guest must not read foreign-owned memberships through a sharer"
        );
        assert!(
            admit(
                &shared,
                &caller,
                vec![Val::String("account".into()), Val::S64(1)]
            )
            .is_ok(),
            "current account retains normal sharer access"
        );
        let mut owner = shared;
        if let Access::Read(read) = &mut owner {
            read.handle.as_mut().unwrap().field = "author".into();
            read.by[0] = string("author");
        }
        assert!(
            admit(
                &owner,
                &caller,
                vec![Val::String("guest".into()), Val::S64(1)]
            )
            .is_ok(),
            "old guest retains own membership reads even when target key differs"
        );
    }
}
