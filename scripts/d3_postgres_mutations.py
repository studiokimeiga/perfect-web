#!/usr/bin/env python3
"""Mutation controls for D3 (ADR-0338): any program's declared tables on
PostgreSQL, behind the memory adapter's interface (`runtime/pw-table-postgres`).

Each mutant undoes one guarantee and must be killed by a test of the crate,
at a native assertion (the strict producer of `d2_writes_mutations.py`:
each test binary's complete named inventory, every failure an assertion's):

1. ADR-0338's named controls for D3: a set read looped per key; a
   transaction at the connection's default; a migration applied twice;
   drift accepted;
2. the schema: the platform's tables applied twice, the migration's lock
   not taken, a record's hash or a table made by another hand accepted, a
   destructive change made unstated, an older build taking a newer database
   back, a refinement with no check, a string's bytes counted for its code
   points, nested values kept as `jsonb` (a negative zero's sign lost);
3. the primitives: rows in another order, the key's holder answered after a
   unique value's, a refused insert answered as written, a put refused by
   its own row, a unique value refused by the database (the transaction
   aborted), a growth write past the bound, `all` past its bound, a selector,
   a written row or a stored row unchecked, a statement prepared every call;
4. the commands: two at once, writes outside a transaction, reads outside
   it, a prepared command writing on, a row put back taken for a write,
   references held at each write or only at the commit;
5. the commit: the records committed outside the command's transaction, an
   interaction recorded twice, a live record overwritten, a refused commit
   answered committed, an unanswered one answered refused, a serialization
   failure taken for another error or not run again, the outbox's ids out of
   commit order;
6. renders: seats unbounded, a snapshot reading the latest commit;
7. the independent review's fixes (2026-11-02), each undone: the migration at
   the connection's default isolation, a key field's retype taken for a
   field's, a backfill's string or least `Int` written as a literal the
   server reads otherwise, a rule, an inheriting table, triggers not firing
   and an unlogged table taken for no drift, two objects given one name, a
   write whose answer cannot be read leaving its command whole, an outbox
   row lost, and a nested float read back inexactly (serde_json without
   `float_roundtrip`, in the workspace's manifest: last, as it rebuilds every
   crate that reads JSON).

They need a database: `PW_TABLES_DATABASE_URL` (or `PW_STORE_DATABASE_URL`,
`PW_FEED_DATABASE_URL`), a throwaway one, which each test uses in a schema
of its own. Without it every test passes doing nothing, and no mutant could
be killed, so this refuses to run.

Run whole from the repository root (`just e14-d3-postgres`). Every source is
restored in finally; the unmutated baseline must pass before any mutant
means anything. Words given as arguments run only the mutants whose
description holds one of them, for a quick look while working; the record is
a whole run.
"""
import os
import pathlib
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
CRATE = ROOT / "runtime/pw-table-postgres"
LIB = CRATE / "src/lib.rs"
SQL = CRATE / "src/sql.rs"
MIGRATE = CRATE / "src/migrate.rs"
SHAPE = CRATE / "src/shape.rs"
VALUES = CRATE / "src/values.rs"
POOL = CRATE / "src/pool.rs"
# The workspace's manifest: serde_json's `float_roundtrip` (the review of 2026-11-02).
CARGO = ROOT / "Cargo.toml"

MUTANTS = [
    ('upload enrollment erases asset history after metadata loss', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '        if recorded.is_some_and(|schema| schema.tables.iter().any(|table| !table.assets.is_empty()))', '        if false && recorded.is_some_and(|schema| schema.tables.iter().any(|table| !table.assets.is_empty()))'),
    ('upload enrollment accepts rewritten persisted metadata after schema recording', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '    if stored != expected {', '    if false && stored != expected {'),
    ('upload enrollment skips final metadata verification for unbound manifests', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '        (None, None) => Metadata {\n            namespace: None,\n            declarations: BTreeMap::new(),\n            bindings: BTreeMap::new(),\n        },', '        (None, None) => return Ok(()),'),
    ('upload enrollment accepts changed immutable base declarations', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '        if config.declarations.get(name) != Some(old) {', '        if false && config.declarations.get(name) != Some(old) {'),
    ('upload enrollment accepts changed immutable field associations', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '        if config.bindings.get(binding) != Some(old) {', '        if false && config.bindings.get(binding) != Some(old) {'),
    ('upload enrollment enrolls structural references on existing tables', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '        if !bindings.contains_key(&(table.clone(), field.clone()))\n            && recorded.is_some_and(|schema| schema.tables.iter().any(|t| &t.name == table))', '        if false && !bindings.contains_key(&(table.clone(), field.clone()))\n            && recorded.is_some_and(|schema| schema.tables.iter().any(|t| &t.name == table))'),
    ('upload enrollment does not verify existing namespace markers', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '            config\n                .store\n                .enroll(&namespace, false)\n                .map_err(storage_error)?;', '            let _ = &namespace;'),
    ('upload enrollment commits fresh namespace rows without storage enrollment', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '            config\n                .store\n                .enroll(&config.candidate, true)\n                .map_err(storage_error)?;', '            let _ = &config.candidate;'),
    ('upload enrollment accepts suppressed metadata inserts', ROOT / 'runtime/pw-table-postgres/src/assets.rs',
     '    if rows.len() != 1 {', '    if false && rows.len() != 1 {'),
    ('upload enrollment allows collisions with asset platform relation names', ROOT / 'runtime/pw-table-postgres/src/shape.rs',
     ' || name.starts_with("pw_asset_")', ' || false && name.starts_with("pw_asset_")'),
    ('upload enrollment enables unfinished command lifetimes after enrollment alone', ROOT / 'runtime/pw-table-postgres/src/lib.rs',
     '        if self.inner.uploads.is_some() {', '        if false && self.inner.uploads.is_some() {'),
    ('closed cases PostgreSQL additive columns use a different encoding', MIGRATE,
     '    let (kind, nullable) = crate::shape::column_of(&field.stored);', '    let (kind, nullable) = match &field.stored {\n        Stored::Option(inner) if matches!(**inner, Stored::Cases(_)) => (crate::shape::Kind::Json, true),\n        _ => crate::shape::column_of(&field.stored),\n    };'),
    ('closed cases PostgreSQL accepts unknown stored tags', ROOT / 'runtime/pw-table-postgres/src/shape.rs',
     '            .is_some_and(|tag| case_tags(cases).is_some_and(|tags| tags.iter().any(|t| t == tag))),', '            .is_some_and(|_tag| case_tags(cases).is_some()),'),
    ('closed cases PostgreSQL stores scalar tags as JSON', ROOT / 'runtime/pw-table-postgres/src/shape.rs',
     '        Stored::String { .. } | Stored::Cases(_) => (Kind::Bytes, false),', '        Stored::String { .. } => (Kind::Bytes, false),\n        Stored::Cases(_) => (Kind::Json, false),'),
    ('closed cases PostgreSQL serves assets without a binding', ROOT / 'runtime/pw-table-postgres/src/shape.rs',
     '        .any(|table| !table.schema.assets.is_empty())', '        .any(|table| false && !table.schema.assets.is_empty())'),
    ('relations scalar ignores its whole source selector', SQL,
     'WHERE {} AND {matched}) ORDER BY {order} LIMIT ${parameter}::bigint",',
     'WHERE ({} OR true) AND {matched}) ORDER BY {order} LIMIT ${parameter}::bigint",'),
    ('relations match uses another type-compatible target field', SQL,
     'ident(&target.columns[relation.target_match].name)',
     'ident(&target.columns.iter().find(|c| c.name == "text").unwrap_or(&target.columns[relation.target_match]).name)'),
    ('relations scalar join duplicates target rows for duplicate edges', SQL,
     'FROM {to} t WHERE EXISTS (SELECT 1 FROM {from} s WHERE {} AND {matched}) ORDER BY {order} LIMIT ${parameter}::bigint",',
     'FROM {to} t JOIN {from} s ON {matched} WHERE {} ORDER BY {order} LIMIT ${parameter}::bigint",'),
    ('relations set clips globally instead of inside each source selector', SQL,
     'WHERE {} AND {matched}) ORDER BY {order} LIMIT ${parameter}::bigint) w \\\n             ORDER BY k.pw_slot, {}",',
     'WHERE {} AND {matched}) ORDER BY {order} OFFSET (${parameter}::bigint * 0)) w \\\n             ORDER BY k.pw_slot, {} LIMIT ${parameter}::bigint",'),
    ('relations correct scalar answers fetch all matches before host clipping', LIB,
     '        let rows = conn\n            .query(&inner.counters, sql, &params)\n            .map_err(database)?;\n        let mut last = 0;\n        for row in rows {',
     '        let expanded = if !function.primitive.many() {\n            let parameter = function.by.len() + 1;\n            sql.replace(&format!(" LIMIT ${parameter}::bigint"), &format!(" OFFSET (${parameter}::bigint * 0)"))\n        } else { sql.to_string() };\n        let mut rows = conn.query(&inner.counters, &expanded, &params).map_err(database)?;\n        if !function.primitive.many() { rows.truncate(usize::try_from(bound).unwrap_or(usize::MAX)); }\n        let mut last = 0;\n        for row in rows {'),
    ('relations correct scalar answers fetch per-author windows before host clipping', LIB,
     '        let rows = conn\n            .query(&inner.counters, sql, &params)\n            .map_err(database)?;\n        let mut last = 0;\n        for row in rows {',
     '        let expanded = if !function.primitive.many() {\n            let parameter = function.by.len() + 1;\n            let (head, rest) = sql.split_once(" FROM ").ok_or_else(|| Error::Schema("relation select".into()))?;\n            let (filtered, ordered) = rest.rsplit_once(" ORDER BY ").ok_or_else(|| Error::Schema("relation order".into()))?;\n            let (order, _) = ordered.rsplit_once(" LIMIT ").ok_or_else(|| Error::Schema("relation limit".into()))?;\n            let matched = ident(&target.columns[relation.target_match].name);\n            format!("{head} FROM (SELECT t.*, row_number() OVER (PARTITION BY t.{matched} ORDER BY {order}) AS pw_author_rank FROM {filtered}) t WHERE t.pw_author_rank <= ${parameter}::bigint ORDER BY {order}")\n        } else { sql.to_string() };\n        let mut rows = conn.query(&inner.counters, &expanded, &params).map_err(database)?;\n        if !function.primitive.many() { rows.truncate(usize::try_from(bound).unwrap_or(usize::MAX)); }\n        let mut last = 0;\n        for row in rows {'),
    ('relations correct set answers run one SQL query per source selector', LIB,
     '        let rows = conn\n            .query(&inner.counters, sql, &params)\n            .map_err(database)?;\n        let mut last = 0;\n        for row in rows {\n            let (slot, offset) = if function.primitive.many() {\n                let slot: i64 = row.try_get(0).map_err(database)?;\n                (\n                    usize::try_from(slot)\n                        .ok()\n                        .and_then(|s| s.checked_sub(1))\n                        .ok_or_else(|| Error::Database("relation slot is invalid".into()))?,\n                    1,\n                )\n            } else {\n                (0, 0)\n            };',
     '        let rows = if function.primitive.many() {\n            let mut rows = Vec::new();\n            for (slot,key) in keys.iter().enumerate() {\n                let mut params = function.by.iter().zip(key).map(|(&field,atom)| bind_array(&source.columns[field],&[atom])).collect::<Result<Vec<Param>,Error>>()?;\n                params.push(Box::new(bound));\n                rows.extend(conn.query(&inner.counters,sql,&params).map_err(database)?.into_iter().map(|row| (slot,1,row)));\n            }\n            rows\n        } else {\n            conn.query(&inner.counters,sql,&params).map_err(database)?.into_iter().map(|row| (0,0,row)).collect()\n        };\n        let mut last = 0;\n        for (slot,offset,row) in rows {'),
    ('ranges declared String order is replaced by the primary key', SQL,
     '        Primitive::CountAfter => {\n            let order = &table.columns[table.order.expect("validated range order")];',
     '        Primitive::CountAfter => {\n            let order = &table.columns[table.order.expect("validated range order")];\n            // Deliberate type-compatible wrong-field comparison for String order.\n            let order = if matches!(order.kind, Kind::Bytes) {\n                &table.columns[table.key[0]]\n            } else { order };'),
    ('ranges scalar ignores the order mark', SQL,
     '"SELECT count(*) AS pw_range_count FROM {t} t WHERE {} AND t.{} > ${}::{}",',
     '"SELECT count(*) AS pw_range_count FROM {t} t WHERE {} AND (t.{} > ${}::{} OR true)",'),
    ('ranges set includes equal order marks', SQL,
     '"t.{} > k.c{}",',
     '"t.{} >= k.c{}",'),
    ('ranges set omits zero ranges with an inner join', SQL,
     '\n                     LEFT JOIN {t} t ON {} GROUP BY k.pw_slot ORDER BY k.pw_slot',
     '\n                     INNER JOIN {t} t ON {} GROUP BY k.pw_slot ORDER BY k.pw_slot'),
    ('ranges different marks collapse to the same equality key', SHAPE,
     '    Ok((columns, keys.into_iter().collect()))',
     '    let mut by = std::collections::BTreeMap::new();\n    for key in keys { by.entry(key[..function.by.len()].to_vec()).or_insert(key); }\n    Ok((columns, by.into_values().collect()))'),
    ('ranges correct set answers run one SQL query per pair', LIB,
     '    let (columns, keys) = shape::typed_ranges(table, function, args)?;\n    if keys.is_empty() {\n        return Ok(Value::Array(Vec::new()));\n    }',
     '    let (columns, keys) = shape::typed_ranges(table, function, args)?;\n    if keys.is_empty() {\n        return Ok(Value::Array(Vec::new()));\n    }\n    if function.primitive.many() {\n        let mut counts = Vec::new();\n        for key in &keys {\n            let params = columns.iter().zip(key).map(|(column, atom)| bind_array(column, &[atom])).collect::<Result<Vec<Param>, Error>>()?;\n            let rows = conn.query(&inner.counters, sql, &params).map_err(database)?;\n            let count: i64 = rows[0].try_get(1).map_err(database)?;\n            let key = Value::Object(columns.iter().map(|c| c.name.clone()).zip(key.iter().map(shape::Atom::value)).collect());\n            counts.push(serde_json::json!({"key":key,"count":count}));\n        }\n        return Ok(Value::Array(counts));\n    }'),
    ('ranges correct scalar answer materializes every SQL row first', LIB,
     '    let rows = conn\n        .query(&inner.counters, sql, &params)\n        .map_err(database)?;\n    if rows.len() != keys.len() {',
     '    if !function.primitive.many() {\n        let unbounded = sql.replace("count(*) AS pw_range_count", "t.*");\n        let rows = conn.query(&inner.counters, &unbounded, &params).map_err(database)?;\n        return Ok(Value::from(rows.len() as i64));\n    }\n    let rows = conn\n        .query(&inner.counters, sql, &params)\n        .map_err(database)?;\n    if rows.len() != keys.len() {'),
    ('windows materialize unrestricted rows then clip after SQL', LIB,
     '    let rows = conn\n        .query(&inner.counters, sql, &params)\n        .map_err(database)?;\n    if function.primitive == Primitive::Count {',
     '    // Deliberate false optimization: fetch every scalar-window row, then\n    // crop it before the gate. Only the real returned-row/byte oracle sees it.\n    let unbounded = if window_limit.is_some() && !function.primitive.many() {\n        let parameter = function.by.len() + 1;\n        sql.replace(&format!(" LIMIT ${parameter}::bigint"), &format!(" OFFSET (${parameter}::bigint * 0)"))\n    } else { sql.as_str().to_string() };\n    let mut rows = conn.query(&inner.counters, &unbounded, &params).map_err(database)?;\n    if let Some(limit) = window_limit && !function.primitive.many() {\n        rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));\n    }\n    if function.primitive == Primitive::Count {'),
    ('windows set limit is global instead of per key', SQL,
     '                 (SELECT {} FROM {t} t WHERE {} ORDER BY {} LIMIT ${}::bigint) w \\\n                 ORDER BY k.pw_slot, {}"',
     '                 (SELECT {} FROM {t} t WHERE {} ORDER BY {5} OFFSET (${6}::bigint * 0)) w \\\n                 ORDER BY k.pw_slot, {7} LIMIT ${6}::bigint"'),
    ('windows last reverses the order column but not key ties', SQL,
     '.map(|&field| format!("{alias}.{} {direction}", ident(&table.columns[field].name)))',
     '.map(|&field| { let direction = if Some(field) == table.order { direction } else { "ASC" }; format!("{alias}.{} {direction}", ident(&table.columns[field].name)) })'),
    ('windows zero shortcuts skip selector validation before SQL', LIB,
     '    let mut count_keys = None;',
     '    if window_limit.is_some_and(|n| n <= 0) { return Ok(Value::Array(Vec::new())); }\n    let mut count_keys = None;'),
    ('counts count the absent left-join row as a match', SQL,
     'SELECT k.pw_slot, count(t.{}) FROM unnest({})',
     'SELECT k.pw_slot, count(*) + (count(t.{}) * 0) FROM unnest({})'),
    ('counts omit missing selectors with an inner join', SQL,
     '\n                 LEFT JOIN {t} t ON {} GROUP BY k.pw_slot ORDER BY k.pw_slot',
     '\n                 INNER JOIN {t} t ON {} GROUP BY k.pw_slot ORDER BY k.pw_slot'),
    ('counts scalar query ignores its selector', SQL,
     '"SELECT count(*) FROM {t} t WHERE {}",',
     '"SELECT count(*) FROM {t} t WHERE ({} OR true)",'),
    ('counts row accounting is not observed', POOL,
     '            .returned_rows\n            .fetch_add(rows.len() as u64, Ordering::Relaxed);',
     '            .returned_rows\n            .fetch_add(0, Ordering::Relaxed);'),
    ('counts byte accounting is not observed', POOL,
     'counters.returned_bytes.fetch_add(bytes, Ordering::Relaxed);',
     'counters.returned_bytes.fetch_add(0, Ordering::Relaxed);'),
    ("a shared-outbox platform index name is not reserved", SQL,
     '    "pw_outbox_trimmed_pkey",\n', ''),
    # 1. ADR-0338's named controls for D3
    ("a set read is looped per key", LIB,
     "            let keys = typed_selections(table, function, import, args)?;\n"
     "            if keys.is_empty() {\n",
     "            let keys = typed_selections(table, function, import, args)?;\n"
     "            for _ in 1..keys.len() {\n"
     "                let _ = conn.query(&inner.counters, \"SELECT 1\", &[]);\n"
     "            }\n"
     "            if keys.is_empty() {\n"),
    ("a command's transaction is the connection's default", LIB,
     '            Isolation::Serializable => "BEGIN ISOLATION LEVEL SERIALIZABLE",\n',
     '            Isolation::Serializable => "BEGIN",\n'),
    ("a migration is applied twice", MIGRATE,
     "    let ddl = match &recorded {\n",
     "    let ddl = match &None::<Recorded> {\n"),
    ("drift is accepted", MIGRATE,
     "            if was != is {\n",
     "            if false && was != is {\n"),
    # 2. the schema
    ("the platform's tables are applied twice", MIGRATE,
     "        if applied {\n",
     "        if false && applied {\n"),
    ("the migration's lock is not taken", MIGRATE,
     '        "SELECT pg_advisory_xact_lock($1)",\n',
     '        "SELECT $1::bigint",\n'),
    ("a record's hash is not held to its schema's", MIGRATE,
     "            if schema.hash != hash {\n",
     "            if false && schema.hash != hash {\n"),
    ("a table made by another hand is taken for none", MIGRATE,
     '                .find(|(t, w, ..)| !t.is_empty() && w == "table")\n',
     '                .find(|(t, w, ..)| !t.is_empty() && w == "no table")\n'),
    ("a field is dropped though no migrate states it", MIGRATE,
     "                    match stated_drop {\n",
     "                    match stated_drop.or(Some(usize::MAX)) {\n"),
    ("an older build takes a newer database back", MIGRATE,
     "    if !stated_applies {\n",
     "    if false && !stated_applies {\n"),
    # The review of PR #66: a table a change adds was shaped alone, so its
    # reference to a table held, or added with it, resolved against nothing.
    ("a table a change adds is shaped alone", MIGRATE,
     "            tables_sql.push(sql::create_table(names, shape));\n",
     "            let alone = built.tables.iter().filter(|t| t.name == shape.name);\n"
     "            let alone: Vec<TableSchema> = alone.cloned().collect();\n"
     "            tables_sql.push(sql::create_table(names, &shapes_of(&alone)?[0]));\n"),
    ("a refinement is held by no check", SQL,
     "            if let Some(check) = check_of(names, column) {\n",
     "            if let Some(check) = check_of(names, column).filter(|_| false) {\n"),
    ("a string's bytes are counted for its code points", MIGRATE,
     "               WHERE get_byte($1, i) & 192 <> 128 $$;\",\n",
     "               WHERE true $$;\",\n"),
    ("nested values are kept as jsonb", SHAPE,
     '            Kind::Json => "json",\n',
     '            Kind::Json => "jsonb",\n'),
    # 3. the primitives
    ("rows come in another order than tables.json's", SQL,
     '        .map(|&f| format!("{alias}.{} ASC", ident(&table.columns[f].name)))\n',
     '        .map(|&f| format!("{alias}.{} DESC", ident(&table.columns[f].name)))\n'),
    ("a write is answered by a unique value's holder before its key's", SQL,
     '        "SELECT {}, CASE {} END AS pw_rank FROM {t} t WHERE {condition} ORDER BY pw_rank LIMIT 1",\n',
     '        "SELECT {}, CASE {} END AS pw_rank FROM {t} t WHERE {condition} ORDER BY pw_rank DESC LIMIT 1",\n'),
    ("a refused insert is answered as written", LIB,
     "                    if count == 1 {\n",
     "                    if count == 1 || holder.is_some() {\n"),
    ("a put is refused by its own row's unique value", SQL,
     '        condition = format!("{condition} AND NOT ({key})");\n',
     '        condition = format!("{condition} AND true");\n'),
    ("a unique value is left to the database, which aborts the transaction", SQL,
     "    for index in table.indexes.iter().filter(|i| i.unique) {\n",
     "    for index in table.indexes.iter().filter(|i| i.unique && false) {\n"),
    ("a growth write passes the bound", SQL,
     '        "(SELECT count(*) FROM (SELECT 1 FROM {} LIMIT {bound}) AS rows) < {bound}",\n',
     '        "(SELECT count(*) FROM (SELECT 1 FROM {} LIMIT {bound}) AS rows) <= {bound}",\n'),
    ("all answers past its bound", LIB,
     "        && rows.len() as u64 > bound\n",
     "        && rows.len() as u64 > bound + 1\n"),
    ("a selector is not held to its declared type", SHAPE,
     "            if !validates(&atom.value(), &column.stored) {\n",
     "            if false && !validates(&atom.value(), &column.stored) {\n"),
    ("a written row is not held to its table", SHAPE,
     "        if !validates(v, &column.stored) {\n",
     "        if false && !validates(v, &column.stored) {\n"),
    ("a stored row is answered unchecked", VALUES,
     "        if !validates(&value, &column.stored) {\n",
     "        if false && !validates(&value, &column.stored) {\n"),
    ("a statement is prepared on every call", POOL,
     "        let statement = match self.prepared.get(sql) {\n",
     "        let statement = match None::<&Statement> {\n"),
    # 4. the commands
    ("two commands of a host run at once", LIB,
     "        while turn.0.is_some() {\n",
     "        while false && turn.0.is_some() {\n"),
    ("a command writes outside a transaction", LIB,
     "        if !open.begun {\n"
     "            if let Err(error) = self.begin_command(conn) {\n",
     "        if false && !open.begun {\n"
     "            if let Err(error) = self.begin_command(conn) {\n"),
    ("a command reads outside its transaction", LIB,
     "        let conn = self.command_conn(open)?;\n"
     "        let answer = read(self, conn, import, args);\n",
     "        let _ = self.command_conn(open)?;\n"
     "        let answer = self.read_published(import, args);\n"),
    ("a prepared command writes on", LIB,
     "        if open.ended || open.prepared {\n",
     "        if open.ended {\n"),
    ("a row put back as it was is a write", LIB,
     "            .filter(|(_, (before, now))| !same_rows(before, now))\n",
     "            .filter(|(_, (_before, _now))| true)\n"),
    ("references are held at the commit, not where the command prepares", LIB,
     "                && let Some(hold) = &inner.hold_references\n",
     "                && let Some(hold) = &None::<String>\n"),
    ("references are held at each write", SQL,
     "                         DEFERRABLE INITIALLY DEFERRED\",\n",
     "                         NOT DEFERRABLE\",\n"),
    # 5. the commit
    ("the records commit outside the command's transaction", LIB,
     "            let conn = inner.command_conn(open)?;\n"
     "            let ran = (|| {\n",
     "            let conn = inner.command_conn(open)?;\n"
     "            conn.batch(&inner.counters, \"COMMIT\").map_err(database)?;\n"
     "            conn.batch(&inner.counters, \"BEGIN\").map_err(database)?;\n"
     "            let ran = (|| {\n"),
    ("an interaction is recorded twice", LIB,
     "                    if recorded.is_empty() {\n",
     "                    if false && recorded.is_empty() {\n"),
    ("a live interaction's record is overwritten", LIB,
     "                 at_ms = EXCLUDED.at_ms WHERE i.at_ms < $1 RETURNING 1\"\n",
     "                 at_ms = EXCLUDED.at_ms WHERE true RETURNING 1\"\n"),
    ("a refused commit is answered as committed", LIB,
     '            if let Err(e) = conn.batch(&inner.counters, "COMMIT") {\n',
     '            if let Some(e) = conn.batch(&inner.counters, "COMMIT").err().filter(|_| false) {\n'),
    ("a commit never answered is answered refused", LIB,
     "                    _ => Error::OutcomeUnknown(pg(&e)),\n",
     "                    _ => Error::Refused(pg(&e)),\n"),
    ("a serialization failure is taken for another error", LIB,
     "        *code == SqlState::T_R_SERIALIZATION_FAILURE || *code == SqlState::T_R_DEADLOCK_DETECTED\n",
     "        *code == SqlState::T_R_DEADLOCK_DETECTED\n"),
    ("a serialization failure is not run again", LIB,
     "                Err(Error::Serialization(_)) if attempt < ATTEMPTS => attempt += 1,\n",
     "                Err(Error::Serialization(_)) if false && attempt < ATTEMPTS => attempt += 1,\n"),
    ("the outbox's ids are taken out of commit order", LIB,
     '                "WITH ordered AS (SELECT pg_advisory_xact_lock($1), pg_notify($6, \'\')) \\\n',
     '                "WITH ordered AS (SELECT $1::bigint, pg_notify($6, \'\')) \\\n'),
    # 6. renders
    ("render seats are unbounded", POOL,
     "        while places.taken >= self.bound {\n",
     "        while false && places.taken >= self.bound {\n"),
    ("a render's snapshot reads the latest commit", LIB,
     'pub const READ_SNAPSHOT: &str = "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY";\n',
     'pub const READ_SNAPSHOT: &str = "BEGIN READ ONLY";\n'),
    # 7. the independent review (2026-11-02), each fix undone; the last
    # rebuilds every crate that reads JSON, once.
    ("the migration reads at the connection's default isolation", MIGRATE,
     "    conn.batch(counters, \"BEGIN ISOLATION LEVEL READ COMMITTED\")\n",
     "    conn.batch(counters, \"BEGIN\")\n"),
    ("a key field retyped is taken for a field retyped", MIGRATE,
     "                    if unbounded(&g.stored) != unbounded(&f.stored) && now.key.contains(&to) =>\n",
     "                    if unbounded(&g.stored) != unbounded(&f.stored) && now.key.contains(&to) && false =>\n"),
    ("a backfilled string is a literal the server's string syntax reads", MIGRATE,
     "            format!(\"pg_catalog.decode('{hex}', 'hex')\")\n",
     "            format!(\"'\\\\x{hex}'::bytea\")\n"),
    ("the least Int backfilled is a negated literal", MIGRATE,
     "        Literal::Int(n) => format!(\"'{n}'::bigint\"),\n",
     "        Literal::Int(n) => format!(\"{n}::bigint\"),\n"),
    ("a rule is no drift", MIGRATE,
     "  JOIN pg_catalog.pg_class c ON c.oid = r.ev_class\n",
     "  JOIN pg_catalog.pg_class c ON c.oid = r.ev_class AND false\n"),
    ("a table inheriting one is no drift", MIGRATE,
     "  JOIN pg_catalog.pg_class c ON c.oid = i.inhparent\n",
     "  JOIN pg_catalog.pg_class c ON c.oid = i.inhparent AND false\n"),
    ("triggers not firing are no drift", MIGRATE,
     "                             FROM pg_catalog.pg_trigger t WHERE t.tgrelid = c.oid AND t.tgenabled <> 'O')\n",
     "                             FROM pg_catalog.pg_trigger t WHERE t.tgrelid = c.oid AND t.tgenabled <> 'O' AND false)\n"),
    ("an unlogged table is no drift", MIGRATE,
     "CASE WHEN c.relpersistence <> 'p' THEN",
     "CASE WHEN false THEN"),
    ("two objects given one name are made, one replacing the other", SHAPE,
     "    crate::sql::names_held(&tables)?;\n",
     "    let _ = crate::sql::names_held(&tables);\n"),
    ("a write whose answer cannot be read leaves its command whole", LIB,
     "            Err(unread) => {\n                open.failed = Some(unread.clone());\n",
     "            Err(unread) => {\n"),
    ("a commit keeps fewer outbox rows than it carries", LIB,
     "                    if rows.len() != handed.len() {\n",
     "                    if false && rows.len() != handed.len() {\n"),
    ("a nested float is read back by serde_json's inexact parser", CARGO,
     "serde_json = { version = \"1.0.151\", features = [\"float_roundtrip\"] }\n",
     "serde_json = \"1.0.151\"\n"),
    ('an owned command predicate reads published rows instead of staging', LIB,
     '            let data = Arc::new(move |args: &[Value]| {\n                inner\n                    .command_read(&state, &bound, args)\n                    .map_err(|error| format!("`{bound}`: {error:?}"))\n            });\n',
     '            let data = Arc::new(move |args: &[Value]| {\n                inner\n                    .read_published(&bound, args)\n                    .map_err(|error| format!("`{bound}`: {error:?}"))\n            });\n'),
    ('an owned query predicate reads published rows instead of its snapshot', LIB,
     '        one_predicate(contract).map_err(|e| format!("{e:?}"))?;\n        self.functions(contract, granted)\n',
     '        one_predicate(contract).map_err(|e| format!("{e:?}"))?;\n        PgTables { inner: Arc::clone(&self.state.inner) }.read_host_fns(contract, granted).map_err(|e| format!("{e:?}"))\n'),
    ('an owned query body reads published rows instead of its snapshot', LIB,
     '        self.read_host_fns(contract, granted)\n            .map_err(|e| format!("{e:?}"))\n',
     '        PgTables { inner: Arc::clone(&self.state.inner) }.read_host_fns(contract, granted).map_err(|e| format!("{e:?}"))\n'),
    ('postgres predicate callbacks accept a nonpredicate contract', LIB,
     '        [only] if only.kind == "predicate" => Ok(()),\n',
     '        [_] => Ok(()),\n'),
    ('postgres operation accounting hides a data statement', POOL,
     '        counters\n            .operation_statements\n            .fetch_add(1, Ordering::Relaxed);\n        let rows = self.client.query(&statement, &refs(params))?;\n',
     '        let rows = self.client.query(&statement, &refs(params))?;\n'),
    ('postgres operation accounting counts a transaction boundary as data', POOL,
     '        if !matches!(\n            sql,\n',
     '        if true || !matches!(\n            sql,\n'),
]

# Each test binary apart: names can repeat between binaries, and the native
# assertion validator expects one complete inventory each.
TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "asset_enrollment"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "compiled_cases"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "compiled_assets"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "compiled_relations"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "compiled_ranges"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "compiled_windows"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "counts"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "compiled_counts"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--lib"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "compiled_owned"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "commands"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "durability"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "migrations"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "parity"],
    # Track `durable-tables`: the platform's runs and sessions, the fence.
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "platform"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "primitives"],
]
# Every test binary of the crate is one of them.
assert sorted(t[-1] for t in TESTS if "--test" in t) == sorted(p.stem for p in (CRATE / "tests").glob("*.rs"))

SOURCES = [MIGRATE] + sorted((CRATE / "tests").rglob("*.rs")) + sorted((ROOT / "runtime/pw-table-memory/tests/support").glob("*.rs"))
INVENTORY = {}


def database():
    return next(
        (os.environ[name] for name in
         ("PW_TABLES_DATABASE_URL", "PW_STORE_DATABASE_URL", "PW_FEED_DATABASE_URL")
         if os.environ.get(name)),
        None,
    )


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
    if not database():
        print("FAIL: no database is named (PW_TABLES_DATABASE_URL, PW_STORE_DATABASE_URL or")
        print("PW_FEED_DATABASE_URL); without one every test passes doing nothing, and no")
        print("mutant can mean anything")
        return 2
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
