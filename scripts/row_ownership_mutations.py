#!/usr/bin/env python3
"""Mutation controls for track `row-ownership` (ADR-0384's H1 and H2):
whose a table's rows are, read and written by their reader alone, and a
predicate the program gives a body, asked by the host.

Each mutant undoes one guarantee and must be killed by a test:
1. the compiler: an owned read takes the reader's handle, never a plain id;
   a sharer is answered the seen row; an owned table gives no `all`; tables.json
   names each function's reader; PW0367, PW0368 and PW0369 each refuse, an
   owned table's bound among PW0368's; a reference stays within one owner's
   rows (PW6924, each of its three shapes); `private` is a declaration's
   visibility where a declaration's word follows;
2. the host: a handle is its caller's, and a call no one makes reads no
   owned row; an owned row is written by its owner alone, read by its owner
   field wherever that is in the row, and a row not exactly the table's is
   refused; a set form's handle is given back to each key; a sharer is
   answered the seen row; the manifest is held whole for its owners, its bound
   and its references among it; an owned import is typed as owned;
3. predicates (H2): a body answers a Bool and only reads (PW6905); a query
   may require one, keyed by its reader, never cached shared or keyed by its
   arguments alone, and no materialization depends on it (PW6907); a query
   that requires is read by a page's binding alone, never called, named or
   bound elsewhere (PW6926); a predicate with a body is required where it is
   seen and asked by no page (PW6925); a requirement names the component
   that answers it, and the ABI names it too; the engine asks the program's
   predicate itself, before the export runs, from the program's compiled
   predicates, each one `predicate` export requiring nothing, under the
   export's limits and one budget, and the deployment never, on any path;
   its reads are its command's transaction's, which no other command enters
   between the predicate and the body.

The review of the track (2026-11-02) found three of these unheld: an
owner check reading a row's first field, and a predicate asked after the
body, each survived every test; its findings' regressions hold them now.
Its second review found two more ways past a query's predicate, the ABI
blind to who answers one, and `Predicate::new` unheld by any test.

Run whole from the repository root (`just e14-row-ownership`). Every source
is restored in finally; the unmutated baseline must pass before any mutant
means anything. Words given as arguments run only the mutants whose
description holds one of them, for a quick look while working; the record is
a whole run.
"""
import pathlib
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLES = ROOT / "compiler/pw-core/src/tables.rs"
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"
VALUES = ROOT / "compiler/pw-core/src/values.rs"
PREDICATES = ROOT / "compiler/pw-core/src/predicates.rs"
CHECK = ROOT / "compiler/pw-core/src/check.rs"
CONTRACT = ROOT / "compiler/pw-core/src/contract.rs"
OWNERS = ROOT / "runtime/pw-host/src/engine/table_owners.rs"
LINK = ROOT / "runtime/pw-host/src/engine/table_link.rs"
HOST_TABLES = ROOT / "runtime/pw-host/src/tables.rs"
HOST = ROOT / "runtime/pw-host/src/lib.rs"
PROGRAM_PREDICATES = ROOT / "runtime/pw-host/src/engine/program_predicates.rs"
MEMORY = ROOT / "runtime/pw-table-memory/src/lib.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
IDENTITY = SERVER.with_name("identity.rs")

MUTANTS = [
    ('counts scalar and set function names silently alias', TABLES,
     'o.name == p.name && o.primitive == p.primitive && o.by == p.by',
     'o.name == p.name'),
    ('counts manifest admits a set with no selector after its reader', HOST_TABLES,
     'function.primitive == Primitive::CountMany && function.by.len() <= 1',
     'false && function.primitive == Primitive::CountMany && function.by.len() <= 1'),
    ('counts generated record aliases are accepted', TABLES,
     '*first != &generated.what',
     'false && *first != &generated.what'),
    ('counts negative scalar answers are accepted', LINK,
     'Val::S64(n) if *n >= 0 => Ok(())',
     'Val::S64(n) if *n >= 0 || true => Ok(())'),
    ('counts missing result keys are accepted', LINK,
     '    if seen != asked {',
     '    if false && seen != asked {'),
    ('counts reversed selector order is accepted', LINK,
     '        if last.as_ref().is_some_and(|last| *last >= selected) {',
     '        if false && last.as_ref().is_some_and(|last| *last >= selected) {'),
    ('counts selector results are projected as stored shared rows', OWNERS,
     '    if read.primitive.counts() {\n        return Ok(out);\n    }',
     '    if false && read.primitive.counts() {\n        return Ok(out);\n    }'),
    ("identity transition authority escapes its lifecycle root", HOST,
     '        if identity != transition {\n',
     '        if identity != transition && false {\n'),
    ("identity transition may put the previous guests rows", OWNERS,
     '        let user = if previous && old_owner_access {\n',
     '        let user = if previous && (old_owner_access || true) {\n'),
    ("identity transition source uses its public presentation field", IDENTITY,
     '                previous_guest: self.previous_guest,\n',
     '                previous_guest: guest_user(self.guest),\n'),
    ("identity transition target uses its public presentation field", IDENTITY,
     '                session: self.target_session.clone(),\n',
     '                session: self.session.into(),\n'),
    ("identity transition scope is never retired", IDENTITY,
     '                IDENTITY_TRANSITIONS.with(|held| {\n                    held.borrow_mut().pop();\n                });\n',
     '                IDENTITY_TRANSITIONS.with(|held| {\n                    let _ = held.borrow().len();\n                });\n'),
    ("identity lifecycle skips its real target actor fence", SERVER,
     '            || (self.invoked_by_platform(component_id) && !lifecycle)\n',
     '            || self.invoked_by_platform(component_id)\n'),
    ("identity lifecycle receipt replay skips target authority", SERVER,
     '        self.command_answer(component, session, &[], Some("identity-joined"))\n',
     '        self.command_answer_bound(component, session, &[], Some("identity-joined"))\n'),
    ("identity lifecycle treats an authenticated admission as a guest", IDENTITY,
     '            && matches!(admitted, ReaderIdentity::Guest | ReaderIdentity::Anonymous);\n',
     '            && (matches!(admitted, ReaderIdentity::Guest | ReaderIdentity::Anonymous) || true);\n'),
    ("a local guest command ignores changed authority", IDENTITY,
     '        if self.is_local() && self.principals.cached_reader(&self.session) == self.reader {\n',
     '        if self.is_local() {\n'),
    ("anonymous guest rows lose their admitted caller", IDENTITY,
     '        .then(|| guest_user(session))\n',
     '        .then(|| None::<String>).flatten()\n'),
    ("anonymous row authority borrows the mutable cache", IDENTITY,
     '        matches!(\n            self.invocation_reader(session),\n            Some(ReaderIdentity::Anonymous)\n        )\n',
     '        matches!(self.bound_reader(session), ReaderIdentity::Anonymous)\n'),
    # 1. the compiler
    ("an owned read takes a plain id", TABLES,
     '    let handle = |f: &str| DeclaredType::new("capability.User", vec![ty(f).unwrap_or_else(unit)]);\n',
     "    let handle = |f: &str| ty(f).unwrap_or_else(unit);\n"),
    ("a sharer is answered the whole row", TABLES,
     "        (Reading::Sharer, Some(s)) => DeclaredType::new(s.name.clone(), Vec::new()),\n",
     "        (Reading::Sharer, Some(_)) => row_ty.clone(),\n"),
    ("an owned table gives all", TABLES,
     "    if let Some(b) = &t.bounded\n        && t.owner.is_none()\n",
     "    if let Some(b) = &t.bounded\n        && (t.owner.is_none() || true)\n"),
    ("tables.json names no reader", TABLES,
     "        Primitive::Insert | Primitive::Put => Some(owner.to_string()),\n        _ => by.first().cloned(),\n",
     "        Primitive::Insert | Primitive::Put => None.filter(|_: &String| owner.is_empty()),\n        _ => None.filter(|_: &String| by.is_empty()),\n"),
    ("PW0367 an owner may be no principal's id", TABLES,
     "            Some((_, Some(ty))) if !principal_id(sigs, ty) => Err(format!(\n",
     "            Some((_, Some(ty))) if false && !principal_id(sigs, ty) => Err(format!(\n"),
    ("PW0368 an owned key may lead with another", TABLES,
     "        && k.value.first().is_some_and(|f| f != owner)\n",
     "        && k.value.first().is_some_and(|f| f != owner && false)\n"),
    ("PW0368 an owned index may lead with no reader", TABLES,
     "        if index.fields.first().is_some_and(|f| !is_reader(f)) {\n",
     "        if index.fields.first().is_some_and(|f| !is_reader(f) && false) {\n"),
    ("PW0368 a unique index may hold no owner", TABLES,
     "        if index.unique && !index.fields.iter().any(|f| f == owner) {\n",
     "        if false && index.unique && !index.fields.iter().any(|f| f == owner) {\n"),
    ("PW0368 all of an owned table is not refused", TABLES,
     '    if member == "all"\n        && let Some(owner) = t.owner_field()\n',
     '    if member == "all"\n        && let Some(owner) = t.owner_field().filter(|_| false)\n'),
    ("PW0369 a private field may be the order of a shared table", TABLES,
     "        } else if !sharers.is_empty() && t.order.as_ref().is_some_and(|o| o.value == *name) {\n",
     "        } else if false && !sharers.is_empty() && t.order.as_ref().is_some_and(|o| o.value == *name) {\n"),
    ("PW0369 a private field may be the owner", TABLES,
     "        } else if name == owner {\n",
     "        } else if false && name == owner {\n"),
    ("private before a declaration's word heads a clause", GRAMMAR,
     "        if DECL_STARTERS.contains(&next) || UI_NOUNS.contains(&next) {\n",
     "        if false && (DECL_STARTERS.contains(&next) || UI_NOUNS.contains(&next)) {\n"),
    # The review's findings 1 and 2 (probes A and B).
    ("PW0368 an owned table may state a bound", TABLES,
     "    if let Some(b) = &t.bounded {\n        out.push(read_refusal(\n",
     "    if let Some(b) = t.bounded.as_ref().filter(|_| false) {\n        out.push(read_refusal(\n"),
    ("PW6924 an owned table refers to another's rows by any field", TABLES,
     "        (Some(owner), Some(_)) if field == owner => return None,\n",
     "        (Some(_), Some(_)) => return None,\n"),
    ("PW6924 a table no one owns refers into an owned one", TABLES,
     "        (None, None) => return None,\n        (Some(owner), Some(_)) if field == owner => return None,\n",
     "        (None, _) => return None,\n        (Some(owner), Some(_)) if field == owner => return None,\n"),
    ("PW6924 an owned table refers to one no one owns", TABLES,
     "        (None, None) => return None,\n        (Some(owner), Some(_)) if field == owner => return None,\n        (Some(owner), Some(by)) => (\n",
     "        (_, None) => return None,\n        (Some(owner), Some(_)) if field == owner => return None,\n        (Some(owner), Some(by)) => (\n"),
    # 2. the host
    ("a forged handle is believed", OWNERS,
     "        Some(Val::String(given)) if given == user => {}\n",
     "        Some(Val::String(given)) if given == user || true => {}\n"),
    ("a call no one makes reads owned rows", OWNERS,
     "    let Caller::User(user) = caller else {\n",
     "    let nobody = String::new();\n    let user = match caller { Caller::User(user) => user, Caller::Nobody | Caller::IdentityTransition { .. } => &nobody };\n    if false {\n"),
    ("an owned row may be written for another", OWNERS,
     "                Some((_, Val::String(owner))) if owner == user => Ok(vec![row.clone()]),\n",
     "                Some((_, Val::String(owner))) if owner == user || true => Ok(vec![row.clone()]),\n"),
    # The review's finding 6: every fixture put its owner first.
    ("the owner check reads a row's first field", OWNERS,
     "            let wit = crate::tables::wit_name(&handle.field);\n",
     "            let wit = fields.first().map(|(name, _)| name.clone()).unwrap_or_default();\n"),
    # The review's note 7a.
    ("a row not exactly its table's is passed", OWNERS,
     "            let fields = table_link::exact_row(&write.row, row).map_err(|why| {\n                format!(\"`{import}` was given a row that is not exactly its table's: {why}\")\n            })?;\n",
     "            let Ok(fields) = table_link::exact_row(&write.row, row) else {\n                return Ok(vec![row.clone()]);\n            };\n"),
    ("a set form's handle is not given back to its keys", OWNERS,
     "            Ok(vec![Val::List(keys)])\n",
     "            let _ = keys;\n            Ok(args)\n"),
    ("a sharer is answered the private fields", OWNERS,
     "    let Some(seen) = read.handle.as_ref().and_then(|h| h.seen.as_ref()) else {\n",
     "    let Some(seen) = read.handle.as_ref().and_then(|h| h.seen.as_ref()).filter(|_| false) else {\n"),
    ("the manifest holds no owners", HOST_TABLES,
     "            table.owned_whole()?;\n",
     "            let _ = table.owned_whole();\n"),
    ("the manifest holds no bound of an owned table", HOST_TABLES,
     "        if let Some(bound) = self.bounded {\n",
     "        if let Some(bound) = self.bounded.filter(|_| false) {\n"),
    ("the manifest lets an owned table refer by any field", HOST_TABLES,
     "                    (Some(owner), Some(_)) => field == owner,\n",
     "                    (Some(_), Some(_)) => true,\n"),
    ("the manifest lets a reference cross into or out of an owned table", HOST_TABLES,
     "                    (Some(owner), Some(_)) => field == owner,\n                    _ => false,\n",
     "                    (Some(owner), Some(_)) => field == owner,\n                    _ => true,\n"),
    ("an owned import is typed as an unowned one", LINK,
     "    if access.handle().is_some() {\n",
     "    if access.handle().is_some() && false {\n"),
    ("the engine admits no handle", HOST,
     "                            Some(function) => table_owners::admit(function, &caller, args)\n",
     "                            Some(function) => Ok::<Vec<Val>, String>({ let _ = (function, &caller); args })\n"),
    # 3. predicates
    ("a predicate's body is not held to its Bool", VALUES,
     "        || (decl.kind == DeclKind::Predicate && decl.body.is_some())\n",
     "        || (decl.kind == DeclKind::Predicate && decl.body.is_some() && false)\n"),
    ("PW6905 a predicate's body may write", PREDICATES,
     '                !matches!(family, "database.read" | "session.read")\n',
     '                !matches!(family, "database.read" | "session.read") && false\n'),
    ("a query may not require a predicate", CHECK,
     "                crate::hir::DeclKind::Command | crate::hir::DeclKind::Query\n",
     "                crate::hir::DeclKind::Command\n"),
    ("a requirement names no component", CONTRACT,
     "                requirement.component = asked_by(hirs, ws, unit, &requirement.predicate);\n",
     "                requirement.component = asked_by(hirs, ws, unit, &requirement.predicate).filter(|_| false);\n"),
    # The review's findings 4 and 7b (probes G and H), and a page's ask.
    ("PW6925 a requires that sees no body is the deployment's", PREDICATES,
     "                let Some((at, given, declared)) = bodied(hirs, ws, &predicate.name) else {\n",
     "                let Some((at, given, declared)) = bodied(hirs, ws, &predicate.name).filter(|_| false) else {\n"),
    ("PW6925 a page asks a predicate with a body", PREDICATES,
     "            if declared.body.is_none() {\n",
     "            if declared.body.is_none() || true {\n"),
    ("PW6907 a query that requires a predicate is cached shared", PREDICATES,
     '                    Some(("shared", c)) => (\n',
     '                    Some(("shared", c)) if false => (\n'),
    # The second review's findings 1 to 4.
    ("PW6907 a query that requires a predicate is keyed without its reader", PREDICATES,
     "                    Some(_) => continue,\n                    None => (\n",
     "                    Some(_) | None => continue,\n                    #[allow(unreachable_patterns)]\n                    None => (\n"),
    ("PW6907 a materialization depends on a query that requires", PREDICATES,
     "                    let Some((def, query)) = guarded_query(hirs, ws, unit, &key.name) else {\n",
     "                    let Some((def, query)) = guarded_query(hirs, ws, unit, &key.name).filter(|_| false) else {\n"),
    ("PW6926 a query that requires is called or named anywhere", PREDICATES,
     "                Expr::Name(_) | Expr::Field { .. } => (crate::infer::path_of(body, e), true),\n",
     "                Expr::Name(_) | Expr::Field { .. } => (String::new(), true),\n"),
    ("PW6926 a name a body binds is taken for the query", PREDICATES,
     "                    if scope.as_ref().is_some_and(|l| l.binder(e).is_some()) {\n",
     "                    if scope.as_ref().is_some_and(|l| l.binder(e).is_some()) && false {\n"),
    ("PW6926 a query that requires is bound outside a page", PREDICATES,
     '                } if keyword == "query" && !binds => match modifiers.first() {\n',
     '                } if keyword == "query" && !binds && false => match modifiers.first() {\n'),
    ("PW6926 a page's binding is refused too", PREDICATES,
     "        let binds = matches!(\n            decl.kind,\n",
     "        let binds = false && matches!(\n            decl.kind,\n"),
    ("the ABI names no requirement's answerer", CONTRACT,
     "                if let Some(component) = &requirement.component {\n                    text.push('@');\n",
     "                if let Some(component) = requirement.component.as_ref().filter(|_| false) {\n                    text.push('@');\n"),
    # The engine asks the program's predicate (the review's findings 3, 5
    # and 6; probes E and L).
    ("the deployment is asked the program's predicate", PROGRAM_PREDICATES,
     "            Some(component) => match predicates.by_component.get(component) {\n",
     "            Some(_) => deployment(&requirement.predicate, &selected),\n            Some(component) => match predicates.by_component.get(component) {\n"),
    ("a program predicate missing from the call is the deployment's", PROGRAM_PREDICATES,
     "                None => Err(format!(\n                    \"its body, component `{component}`, is not among the program's predicates \\\n                     this call was given\"\n                )),\n",
     "                None => deployment(&requirement.predicate, &selected).or(Err(format!(\n                    \"its body, component `{component}`, is not among the program's predicates \\\n                     this call was given\"\n                ))),\n"),
    ("the deployment's path answers the program's predicate", HOST,
     "            let answer = match &requirement.component {\n                Some(component) => Err(format!(\n",
     "            let answer = match &requirement.component {\n                Some(_) => evaluate(&requirement.predicate, &selected),\n                Some(component) => Err(format!(\n"),
    ("a program predicate runs unbounded", PROGRAM_PREDICATES,
     "        budget,\n        &host,\n        Some(tables.manifest()),\n",
     "        &Limits::unbounded(),\n        &host,\n        Some(tables.manifest()),\n"),
    ("a predicate's fuel is not the export's", PROGRAM_PREDICATES,
     "    if budget.fuel.is_some() {\n        budget.fuel = Some(left);\n    }\n",
     "    let _ = left;\n"),
    ('a predicate is asked after the export runs', HOST,
     '            let left = self.authorize_with_tables_as(\n                contract, limits, platform, tables, predicates, caller, export, args, deployment,\n            )?;\n            let mut host = platform.clone();\n            host.extend(tables.functions(contract, granted)?);\n            run(\n                &self.engine,\n                &self.component,\n                contract,\n                granted,\n                &left,\n                &host,\n                Some(tables.manifest()),\n                caller,\n                export,\n                args,\n            )\n            .map_err(InvocationError::from)\n',
     '            let mut host = platform.clone();\n            host.extend(tables.functions(contract, granted)?);\n            let ran = run(\n                &self.engine,\n                &self.component,\n                contract,\n                granted,\n                limits,\n                &host,\n                Some(tables.manifest()),\n                caller,\n                export,\n                args,\n            )\n            .map_err(InvocationError::from);\n            let left = self.authorize_with_tables_as(\n                contract, limits, platform, tables, predicates, caller, export, args, deployment,\n            )?;\n            ran\n'),
    ("a table's function among the platform's is linked", HOST,
     "                .find(|key| crate::tables::is_table_import(key))\n            {\n                return Err(format!(\n",
     "                .find(|key| crate::tables::is_table_import(key) && false)\n            {\n                return Err(format!(\n"),
    ("a predicate's reads leave its command's transaction", MEMORY,
     "                    .command_read(id, &bound, args)\n                    .map_err(|error| format!(\"`{bound}`: {error:?}\"))\n            });\n            out.insert(import, table_read_host::host_fn(read, data));\n",
     "                    .read_published(&bound, args)\n                    .map_err(|error| format!(\"`{bound}`: {error:?}\"))\n            });\n            let _ = id;\n            out.insert(import, table_read_host::host_fn(read, data));\n"),
    ("a predicate's reads are given to a command", MEMORY,
     '        [only] if only.kind == "predicate" => Ok(()),\n',
     '        [only] if only.kind == "predicate" || true => Ok(()),\n'),
    # `Predicate::new`, the engine's hold on a program predicate's contract
    # (the second review's note 4).
    ("a program predicate's contract may hold several exports", PROGRAM_PREDICATES,
     "        let [export] = contract.exports.as_slice() else {\n",
     "        let Some(export) = contract.exports.first() else {\n"),
    ("a program predicate's contract may be another export's", PROGRAM_PREDICATES,
     '        if export.kind != "predicate" {\n',
     '        if false && export.kind != "predicate" {\n'),
    ("a program predicate may require one itself", PROGRAM_PREDICATES,
     "        if !place.authorization.is_empty() {\n",
     "        if false && !place.authorization.is_empty() {\n"),
    # ADR-0384's "a concurrent change between predicate and body": what the
    # memory adapter proves is its one open command.
    ("a second command begins while one is open", MEMORY,
     "        while working.open.is_some() {\n            working = working.wait()?;\n        }\n",
     "        let _ = working.open.is_some();\n"),
    ("a query snapshot's body reads the published state", MEMORY,
     "        .read_host_fns_at(self, contract, granted)\n",
     "        .read_host_fns(contract, granted)\n"),
    ("a query snapshot's predicate reads the published state", MEMORY,
     "        one_predicate(contract).map_err(|error| format!(\"{error:?}\"))?;\n        self.functions(contract, granted)\n",
     "        one_predicate(contract).map_err(|error| format!(\"{error:?}\"))?;\n        MemoryTables { inner: Arc::clone(&self.inner) }.read_host_fns(contract, granted).map_err(|error| format!(\"{error:?}\"))\n"),
    ('a denied compiled predicate loses its typed refusal', PROGRAM_PREDICATES,
     '                refused: denied.then(|| name.to_string()),\n',
     '                refused: None.filter(|_| denied),\n'),
    ('an evaluation error becomes an explicit refusal', PROGRAM_PREDICATES,
     '        let denied = answer == Ok(false);\n',
     '        let denied = true;\n'),
    ('the server runs a reader as nobody', SERVER,
     '            Some(principal) => pw_host::tables::Caller::User(principal.user),\n',
     '            Some(principal) => { let _ = principal; pw_host::tables::Caller::Nobody },\n'),
    ('the server loses a compiled predicate refusal', SERVER,
     '        if let Some(predicate) = error.refused() {\n',
     '        if let Some(predicate) = None::<&str> {\n'),
    ('the server gives a predicate its command write callbacks', SERVER,
     '        self.0.table_predicate_reads(contract, granted)\n',
     '        self.0.table_ops(contract, granted)\n'),
    ('a standalone table query is given no snapshot', SERVER,
     '        let standalone = if render.is_none() && self.data.tables().is_some() {\n',
     '        let standalone = if false && render.is_none() && self.data.tables().is_some() {\n'),
    ('a qualified compiled predicate loses its declared name', PROGRAM_PREDICATES,
     '                .map(|predicate| predicate.contract.exports[0].name.as_str())\n',
     '                .map(|_| requirement.predicate.as_str())\n'),
    ('a reused guarded value bypasses its current predicate', SERVER,
     '                    if reused {\n',
     '                    if false && reused {\n'),
    ('a joined guarded flight bypasses its current predicate', SERVER,
     '            let reused = !matches!(&fetched, pw_resource::Fetched::Fresh(_));\n',
     '            let reused = matches!(&fetched, pw_resource::Fetched::FromCache(_));\n'),
    ('a query denial contaminates its refreshing command', SERVER,
     '                    |predicate, bound| self.identity.holds(predicate, bound, session, host),\n                )\n                .map(|_| ())\n',
     '                    |predicate, bound| self.identity.holds(predicate, bound, session, host),\n                )\n                .map(|_| ())\n                .map_err(|error| { if let Some(predicate) = error.refused() { identity::record_refusal(predicate); } error })\n'),
    ('a typed query denial is flattened before its page', SERVER,
     '            Some(predicate) => Self::Refused(predicate.to_string()),\n',
     '            Some(_) => Self::Failed(error.to_string()),\n'),
    ('a refused query loses its authored words', SERVER,
     '                    let says = server\n                        .words\n                        .get(&predicate)\n                        .map(String::as_str)\n                        ',
     '                    let says = None::<&str>\n                        '),
    ('a failed joined query loses its typed denial', SERVER,
     "                    // Ask this invocation's requirements for its own typed refusal.\n                    self.authorize_reused_answer(resource, session, args)?;\n",
     ''),
]

# Each Cargo test binary separately: names can repeat between binaries, and
# the shared native assertion validator expects one complete inventory.
TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "tables"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "table_counts"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_counts"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server", "identity::transition_tests::"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "invoked_by"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server", "tests::protected_migration::"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server", "required_queries"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "table_owners"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "predicate_bodies"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-host", "--features", "engine", "--lib"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_owners"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_predicates"],
]

SOURCES = [
    ROOT / "compiler/pw-core/tests/tables.rs",
    ROOT / "compiler/pw-core/tests/table_counts.rs",
    ROOT / "runtime/pw-table-memory/tests/compiled_counts.rs",
    ROOT / "runtime/pw-table-memory/tests/support/counts.rs",
    SERVER,
    IDENTITY,
    ROOT / "compiler/pw-core/tests/invoked_by.rs",
    ROOT / "spikes/own-renderer/server/src/tests/protected_migration.rs",
    ROOT / "spikes/own-renderer/server/src/tests/required_queries.rs",
    ROOT / "spikes/own-renderer/server/src/tests/tables.rs",
    ROOT / "compiler/pw-core/tests/table_owners.rs",
    ROOT / "compiler/pw-core/tests/predicate_bodies.rs",
    *sorted((ROOT / "compiler/pw-syntax/src").glob("*.rs")),
    *sorted((ROOT / "runtime/pw-host/src").rglob("*.rs")),
    ROOT / "runtime/pw-table-memory/tests/compiled_owners.rs",
    ROOT / "runtime/pw-table-memory/tests/compiled_predicates.rs",
    ROOT / "runtime/pw-table-memory/tests/support/owned.rs",
    ROOT / "runtime/pw-table-memory/tests/support/assert_result.rs",
]
INVENTORY = {}


def run_tests():
    passed, failed = 0, 0
    for cmd in TESTS:
        full = [*cmd, "--", "--format=pretty"]
        try:
            child = producer_process.run(full, cwd=ROOT, timeout=900)
        except subprocess.TimeoutExpired:
            return False, passed, failed
        key = tuple(cmd)
        result = cargo_assertion_results.validate(child.returncode, child.stdout,
            root=ROOT, sources=SOURCES, expected=INVENTORY.get(key))
        if not result.valid:
            print(child.stdout[-4000:], flush=True)
            return False, passed, failed
        INVENTORY.setdefault(key, cargo_assertion_results.names(child.stdout))
        passed += result.passed
        failed += result.failed
    return True, passed, failed


def main():
    import mutation_baseline  # keeps every command and reports a red baseline
    built, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1
    chosen = [m for m in MUTANTS if not sys.argv[1:] or any(w in m[0] for w in sys.argv[1:])]
    survivors = 0
    for what, path, anchor, replacement in chosen:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}", flush=True)
            survivors += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            built, passed, failed = run_tests()
        if not built:
            verdict = "INVALID (build, infrastructure or native assertion producer)"
            survivors += 1
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
    print(f"{len(chosen) - survivors} of {len(chosen)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
