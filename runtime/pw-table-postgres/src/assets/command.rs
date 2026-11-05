//! Indexed transactional references and storage effects on the command lease.
use super::*;
use crate::pool::Lease;
use crate::{Command, Inner, Open, Prepared};
use pw_host::assets::{
    Descriptor, Generation, GenerationPlan, GenerationState, Publication, Reference, ReferenceUse,
    plan_generation,
};

#[derive(Default)]
pub(crate) struct Transaction {
    publications: BTreeMap<String, (Publication, Arc<[u8]>)>,
    candidates: BTreeMap<String, Descriptor>,
    pub plans: Vec<GenerationPlan>,
    pub receipt_required: bool,
}
pub(crate) fn namespace_key(namespace: &Namespace) -> i64 {
    crate::migrate::lock_key(&format!("pw_upload_lifetime:{}", namespace.hex()))
}
pub(crate) fn acquire_namespace(
    conn: &mut Conn,
    counters: &Counters,
    namespace: &Namespace,
) -> Result<i64, Error> {
    let key = namespace_key(namespace);
    conn.batch(counters, "SET LOCAL lock_timeout = '5s'")
        .map_err(database)?;
    conn.query(counters, "SELECT pg_advisory_lock($1)", &[Box::new(key)])
        .map_err(database)?;
    Ok(key)
}
pub(crate) fn release_namespace(
    conn: &mut Conn,
    counters: &Counters,
    key: i64,
) -> Result<(), Error> {
    let rows = conn
        .query(counters, "SELECT pg_advisory_unlock($1)", &[Box::new(key)])
        .map_err(database)?;
    if !matches!(
        rows.first().map(|r| r.try_get::<_, bool>(0)),
        Some(Ok(true))
    ) {
        return Err(Error::Database(
            "upload namespace lock was not released".into(),
        ));
    }
    Ok(())
}
pub(crate) fn lock_lifetime(inner: &Inner, lease: &mut Lease) -> Result<Option<i64>, Error> {
    let Some(binding) = &inner.uploads else {
        return Ok(None);
    };
    let conn = lease.conn()?;
    conn.batch(&inner.counters, "BEGIN").map_err(database)?;
    let key = acquire_namespace(conn, &inner.counters, &binding.namespace)?;
    conn.batch(&inner.counters, "COMMIT").map_err(database)?;
    binding
        .store
        .enroll(&binding.namespace, false)
        .map_err(|e| Error::Refused(format!("upload storage namespace: {e}")))?;
    Ok(Some(key))
}

fn descriptor(row: &postgres::Row) -> Result<Descriptor, Error> {
    let width: u32 = u32::try_from(row.get::<_, i64>(3))
        .map_err(|_| Error::Drift("invalid asset width".into()))?;
    let height: u32 = u32::try_from(row.get::<_, i64>(4))
        .map_err(|_| Error::Drift("invalid asset height".into()))?;
    let bytes: u64 = u64::try_from(row.get::<_, i64>(5))
        .map_err(|_| Error::Drift("invalid asset byte count".into()))?;
    let descriptor = Descriptor {
        reference: Reference {
            generation: row.get(0),
            key: row.get(1),
            kind: row.get(2),
            width,
            height,
        },
        bytes,
    };
    descriptor.reference.validate().map_err(Error::Drift)?;
    if bytes == 0 {
        return Err(Error::Drift("empty asset descriptor".into()));
    }
    Ok(descriptor)
}
const COLUMNS: &str = "generation,key,kind,width,height,bytes";
impl Command {
    /// Trusted bridge proposal after actual session/declaration/claim checks.
    pub fn propose_asset(
        &self,
        declaration: &str,
        incoming: Descriptor,
        bytes: Arc<[u8]>,
    ) -> Result<Reference, Error> {
        let manifest = self
            .inner
            .host
            .uploads()
            .ok_or_else(|| Error::Input("uploads are unbound".into()))?;
        manifest
            .declaration(declaration)
            .ok_or_else(|| Error::Input("unknown upload declaration".into()))?
            .validate_reference(&incoming.reference)
            .map_err(Error::Input)?;
        if incoming.bytes == 0 || incoming.bytes != bytes.len() as u64 {
            return Err(Error::Input("canonical asset byte count differs".into()));
        }
        let mut open = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
        if open.ended || open.prepared {
            return Err(Error::Ended);
        }
        if open.assets.publications.contains_key(declaration) {
            return Err(Error::Input(
                "one declaration publishes its held claim twice".into(),
            ));
        }
        let sql = format!(
            "SELECT {COLUMNS} FROM {} WHERE key=$1 AND state='live' LIMIT 2",
            self.inner.names.qualified("pw_asset_generations")
        );
        let conn = self.inner.command_conn(&mut open)?;
        let current = conn
            .query(
                &self.inner.counters,
                &sql,
                &[Box::new(incoming.reference.key.clone())],
            )
            .map_err(database)?;
        if current.len() > 1 {
            return Err(Error::Drift(
                "multiple LIVE generations for one hash".into(),
            ));
        }
        let current = current.first().map(descriptor).transpose()?;
        let selected = current
            .as_ref()
            .or_else(|| open.assets.candidates.get(&incoming.reference.key));
        let selected = if let Some(selected) = selected {
            let mut same = incoming.clone();
            same.reference.generation = selected.reference.generation.clone();
            if &same != selected {
                return Err(Error::Input(
                    "publication changes an immutable descriptor".into(),
                ));
            }
            selected.clone()
        } else {
            if open
                .assets
                .candidates
                .values()
                .any(|d| d.reference.generation == incoming.reference.generation)
            {
                return Err(Error::Input("generation identity already proposed".into()));
            }
            let sql = format!(
                "SELECT 1 FROM {} WHERE generation=$1",
                self.inner.names.qualified("pw_asset_generations")
            );
            let conn = self.inner.command_conn(&mut open)?;
            if !conn
                .query(
                    &self.inner.counters,
                    &sql,
                    &[Box::new(incoming.reference.generation.clone())],
                )
                .map_err(database)?
                .is_empty()
            {
                return Err(Error::Input("generation identity already reserved".into()));
            }
            incoming
        };
        open.assets
            .candidates
            .entry(selected.reference.key.clone())
            .or_insert_with(|| selected.clone());
        open.assets.publications.insert(
            declaration.into(),
            (
                Publication {
                    declaration: declaration.into(),
                    descriptor: selected.clone(),
                },
                bytes,
            ),
        );
        open.assets.receipt_required = true;
        Ok(selected.reference)
    }
    pub fn require_upload_receipt(&self) -> Result<(), Error> {
        if self.inner.uploads.is_none() {
            return Err(Error::Input("uploads are unbound".into()));
        }
        let mut open = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
        if open.ended || open.prepared {
            return Err(Error::Ended);
        }
        open.assets.receipt_required = true;
        Ok(())
    }
}
impl Prepared {
    pub fn asset_plans(&self) -> Result<Vec<GenerationPlan>, Error> {
        let open = self
            .command
            .state
            .lock()
            .map_err(|_| Error::StoragePoisoned)?;
        if open.ended {
            return Err(Error::Ended);
        }
        Ok(open.assets.plans.clone())
    }
}
fn uses(inner: &Inner, open: &Open) -> Result<Vec<ReferenceUse>, Error> {
    let mut out = Vec::new();
    for ((table, _), (before, after)) in &open.touched {
        for asset in &inner.shape.tables[*table].assets {
            for (row, before, after) in [(before, 1, 0), (after, 0, 1)] {
                let Some(row) = row else { continue };
                let value = row
                    .get(&asset.field)
                    .ok_or_else(|| Error::Input("asset field missing from touched row".into()))?;
                if value.is_null() {
                    continue;
                }
                out.push(ReferenceUse {
                    declaration: asset.upload.clone(),
                    reference: Reference::from_value(value).map_err(Error::Input)?,
                    before,
                    after,
                });
            }
        }
    }
    Ok(out)
}
fn counts(
    inner: &Inner,
    conn: &mut Conn,
    generations: &[String],
) -> Result<BTreeMap<String, Generation>, Error> {
    let sql = format!(
        "SELECT {COLUMNS},state FROM {} WHERE generation=ANY($1)",
        inner.names.qualified("pw_asset_generations")
    );
    let mut out = BTreeMap::new();
    for row in conn
        .query(&inner.counters, &sql, &[Box::new(generations.to_vec())])
        .map_err(database)?
    {
        let d = descriptor(&row)?;
        let state = match row.get::<_, String>(6).as_str() {
            "live" => GenerationState::Live,
            "tombstone" => GenerationState::Tombstone,
            _ => return Err(Error::Drift("unknown generation state".into())),
        };
        let generation = d.reference.generation.clone();
        if out
            .insert(
                generation,
                Generation {
                    descriptor: d,
                    state,
                    references: BTreeMap::new(),
                },
            )
            .is_some()
        {
            return Err(Error::Drift("duplicate generation descriptor".into()));
        }
    }
    let sql = format!(
        "SELECT generation,declaration,count FROM {} WHERE generation=ANY($1)",
        inner.names.qualified("pw_asset_references")
    );
    for row in conn
        .query(&inner.counters, &sql, &[Box::new(generations.to_vec())])
        .map_err(database)?
    {
        let generation: String = row.get(0);
        let declaration: String = row.get(1);
        let count = u64::try_from(row.get::<_, i64>(2))
            .map_err(|_| Error::Drift("negative asset reference count".into()))?;
        if count == 0 {
            return Err(Error::Drift("zero stored asset reference count".into()));
        }
        let held = out
            .get_mut(&generation)
            .ok_or_else(|| Error::Drift("references lack their descriptor".into()))?;
        if held.references.insert(declaration, count).is_some() {
            return Err(Error::Drift("duplicate declaration count".into()));
        }
    }
    Ok(out)
}
fn one(rows: Vec<postgres::Row>) -> Result<(), Error> {
    if rows.len() != 1 {
        return Err(Error::Refused(
            "asset registry statement was suppressed".into(),
        ));
    }
    Ok(())
}
fn persist(inner: &Inner, conn: &mut Conn, plan: &GenerationPlan) -> Result<(), Error> {
    let d = &plan.generation.descriptor;
    let r = &d.reference;
    let bytes = i64::try_from(d.bytes)
        .map_err(|_| Error::Input("asset byte count exceeds PostgreSQL bigint".into()))?;
    let sql = format!(
        "INSERT INTO {} AS g (generation,key,kind,width,height,bytes,state) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(generation) DO UPDATE SET state=EXCLUDED.state WHERE g.key=EXCLUDED.key AND g.kind=EXCLUDED.kind AND g.width=EXCLUDED.width AND g.height=EXCLUDED.height AND g.bytes=EXCLUDED.bytes RETURNING 1",
        inner.names.qualified("pw_asset_generations")
    );
    one(conn
        .query(
            &inner.counters,
            &sql,
            &[
                Box::new(r.generation.clone()),
                Box::new(r.key.clone()),
                Box::new(r.kind.clone()),
                Box::new(i64::from(r.width)),
                Box::new(i64::from(r.height)),
                Box::new(bytes),
                Box::new(if plan.generation.state == GenerationState::Live {
                    "live".to_string()
                } else {
                    "tombstone".to_string()
                }),
            ],
        )
        .map_err(database)?)?;
    let sql = format!(
        "DELETE FROM {} WHERE generation=$1 RETURNING declaration",
        inner.names.qualified("pw_asset_references")
    );
    conn.query(&inner.counters, &sql, &[Box::new(r.generation.clone())])
        .map_err(database)?;
    for (declaration, count) in &plan.generation.references {
        let count = i64::try_from(*count)
            .map_err(|_| Error::Input("asset count exceeds PostgreSQL bigint".into()))?;
        let sql = format!(
            "INSERT INTO {} (generation,declaration,count) VALUES($1,$2,$3) RETURNING 1",
            inner.names.qualified("pw_asset_references")
        );
        one(conn
            .query(
                &inner.counters,
                &sql,
                &[
                    Box::new(r.generation.clone()),
                    Box::new(declaration.clone()),
                    Box::new(count),
                ],
            )
            .map_err(database)?)?;
    }
    if plan.collect {
        let sql = format!(
            "INSERT INTO {} (generation) VALUES($1) ON CONFLICT(generation) DO UPDATE SET generation=EXCLUDED.generation RETURNING 1",
            inner.names.qualified("pw_asset_collections")
        );
        one(conn
            .query(&inner.counters, &sql, &[Box::new(r.generation.clone())])
            .map_err(database)?)?;
    }
    Ok(())
}
pub(crate) fn prepare(inner: &Inner, open: &mut Open) -> Result<(), Error> {
    let Some(manifest) = inner.host.uploads() else {
        return Ok(());
    };
    type Group = (Vec<Publication>, Vec<ReferenceUse>);
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for (p, _) in open.assets.publications.values() {
        groups
            .entry(p.descriptor.reference.generation.clone())
            .or_default()
            .0
            .push(p.clone());
    }
    for u in uses(inner, open)? {
        groups
            .entry(u.reference.generation.clone())
            .or_default()
            .1
            .push(u);
    }
    if groups.is_empty() {
        return Ok(());
    }
    let generations = groups.keys().cloned().collect::<Vec<_>>();
    verify_rows(inner, open)?;
    let conn = inner.command_conn(open)?;
    let held = counts(inner, conn, &generations)?;
    let mut plans = Vec::new();
    for (generation, (publications, uses)) in groups {
        plans.push(
            plan_generation(manifest, held.get(&generation), &publications, &uses)
                .map_err(Error::Input)?,
        );
    }
    // Retire captured old descriptors before any new LIVE pointer of that hash.
    plans.sort_by_key(|p| p.generation.state == GenerationState::Live);
    for p in &plans {
        persist(inner, conn, p)?;
    }
    verify_plans(inner, conn, &plans)?;
    open.assets.plans = plans;
    Ok(())
}
pub(crate) fn keep(inner: &Inner, assets: &Transaction) -> Result<(), Error> {
    let Some(binding) = &inner.uploads else {
        return Ok(());
    };
    for plan in &assets.plans {
        if plan.keep_new {
            let bytes = assets
                .publications
                .iter()
                .find(|(declaration, (p, _))| {
                    plan.used_publications.contains(*declaration)
                        && p.descriptor == plan.generation.descriptor
                })
                .map(|(_, (_, bytes))| bytes)
                .ok_or_else(|| {
                    Error::Input("fresh generation has no used held publication".into())
                })?;
            binding
                .store
                .keep_new(&binding.namespace, &plan.generation.descriptor, bytes)
                .map_err(|e| Error::Refused(format!("upload keep: {e}")))?;
        }
    }
    Ok(())
}
/// Called ONLY after a known successful COMMIT, on the SAME still-held lease.
/// A failed fresh inspection, storage deletion or completion leaves the job and
/// must not turn an applied command into a refusal.
pub(crate) fn collect_committed(inner: &Inner, open: &mut Open) -> Result<usize, Error> {
    let Some(binding) = &inner.uploads else {
        return Ok(0);
    };
    let captures = open
        .assets
        .plans
        .iter()
        .filter(|p| p.collect)
        .map(|p| p.generation.descriptor.clone())
        .collect::<Vec<_>>();
    if captures.is_empty() {
        return Ok(0);
    }
    let conn = open.lease.as_mut().ok_or(Error::Ended)?.conn()?;
    let mut done = 0;
    for descriptor in captures {
        if collect_one(inner, conn, binding, &descriptor)? {
            done += 1;
        }
    }
    Ok(done)
}

fn collect_one(
    inner: &Inner,
    conn: &mut Conn,
    binding: &Binding,
    captured: &Descriptor,
) -> Result<bool, Error> {
    let sql = format!(
        "SELECT g.generation,g.key,g.kind,g.width,g.height,g.bytes FROM {} g JOIN {} j USING(generation) WHERE g.generation=$1 AND g.state='tombstone' AND NOT EXISTS(SELECT 1 FROM {} r WHERE r.generation=g.generation)",
        inner.names.qualified("pw_asset_generations"),
        inner.names.qualified("pw_asset_collections"),
        inner.names.qualified("pw_asset_references")
    );
    let latest = conn
        .query(
            &inner.counters,
            &sql,
            &[Box::new(captured.reference.generation.clone())],
        )
        .map_err(database)?;
    if latest.is_empty() {
        return Ok(false);
    }
    if latest.len() != 1 || descriptor(&latest[0])? != *captured {
        return Err(Error::Drift(
            "captured collection descriptor changed".into(),
        ));
    }
    binding
        .store
        .collect_generation(&binding.namespace, captured)
        .map_err(|e| Error::Refused(format!("upload collection: {e}")))?;
    let sql = format!(
        "DELETE FROM {} WHERE generation=$1 RETURNING generation",
        inner.names.qualified("pw_asset_collections")
    );
    let rows = conn
        .query(
            &inner.counters,
            &sql,
            &[Box::new(captured.reference.generation.clone())],
        )
        .map_err(database)?;
    if rows.len() != 1 {
        return Err(Error::Refused(
            "captured collection completion was suppressed".into(),
        ));
    }
    Ok(true)
}
/// A namespace reader closes its connection on every unexpected unwind or
/// acquisition failure. A normal release is explicit before returning the loan.
struct NamespaceLoan {
    lease: Lease,
    released: bool,
}
impl Drop for NamespaceLoan {
    fn drop(&mut self) {
        if !self.released {
            self.lease.discard();
        }
    }
}
impl PgTables {
    /// Resolve authoritative current same-declaration liveness and read/check
    /// bytes under namespace exclusion. Callers decide HTTP headers only after
    /// this succeeds: no reference is None; LIVE but missing/unreadable is Err.
    pub fn read_asset(
        &self,
        declaration: &str,
        key: &str,
        kind: &str,
    ) -> Result<Option<(Descriptor, Vec<u8>)>, Error> {
        let binding = self
            .inner
            .uploads
            .as_ref()
            .ok_or_else(|| Error::Input("uploads are unbound".into()))?;
        let declared = self
            .inner
            .host
            .uploads()
            .and_then(|m| m.declaration(declaration))
            .ok_or_else(|| Error::Input("unknown upload declaration".into()))?;
        let mut loan = NamespaceLoan {
            lease: self.inner.pool.take()?,
            released: false,
        };
        let lifetime = lock_lifetime(&self.inner, &mut loan.lease)?;
        let answer = (|| {
            let conn = loan.lease.conn()?;
            let sql = format!(
                "SELECT g.generation,g.key,g.kind,g.width,g.height,g.bytes FROM {} g JOIN {} r USING(generation) WHERE g.key=$1 AND g.kind=$2 AND g.state='live' AND r.declaration=$3 AND r.count>0 LIMIT 2",
                self.inner.names.qualified("pw_asset_generations"),
                self.inner.names.qualified("pw_asset_references")
            );
            let rows = conn
                .query(
                    &self.inner.counters,
                    &sql,
                    &[
                        Box::new(key.to_string()),
                        Box::new(kind.to_string()),
                        Box::new(declaration.to_string()),
                    ],
                )
                .map_err(database)?;
            if rows.is_empty() {
                return Ok(None);
            }
            if rows.len() != 1 {
                return Err(Error::Drift("multiple current asset descriptors".into()));
            }
            let d = descriptor(&rows[0])?;
            declared
                .validate_reference(&d.reference)
                .map_err(Error::Drift)?;
            let bytes = binding
                .store
                .get_generation(&binding.namespace, &d)
                .map_err(|e| Error::Refused(format!("LIVE upload storage unavailable: {e}")))?
                .ok_or_else(|| Error::Refused("LIVE upload storage is missing".into()))?;
            if bytes.len() as u64 != d.bytes {
                return Err(Error::Refused("LIVE upload byte count differs".into()));
            }
            Ok(Some((d, bytes)))
        })();
        if let Some(key) = lifetime {
            self.inner.unfence(&mut loan.lease, key);
        }
        loan.released = true;
        answer
    }
    /// Retry at most `limit` captured durable jobs, same namespace->host-turn
    /// order as commands. Each deletion rechecks latest committed no-ref state.
    pub fn collect_assets(&self, limit: usize) -> Result<usize, Error> {
        if limit == 0 {
            return Ok(0);
        }
        let binding = self
            .inner
            .uploads
            .as_ref()
            .ok_or_else(|| Error::Input("uploads are unbound".into()))?;
        let command = self.begin()?;
        let mut open = command.state.lock().map_err(|_| Error::StoragePoisoned)?;
        let conn = open.lease.as_mut().ok_or(Error::Ended)?.conn()?;
        let sql = format!(
            "SELECT g.generation,g.key,g.kind,g.width,g.height,g.bytes FROM {} g JOIN {} j USING(generation) WHERE g.state='tombstone' AND NOT EXISTS(SELECT 1 FROM {} r WHERE r.generation=g.generation) ORDER BY g.generation LIMIT $1",
            self.inner.names.qualified("pw_asset_generations"),
            self.inner.names.qualified("pw_asset_collections"),
            self.inner.names.qualified("pw_asset_references")
        );
        let rows = conn
            .query(
                &self.inner.counters,
                &sql,
                &[Box::new(i64::try_from(limit).unwrap_or(i64::MAX))],
            )
            .map_err(database)?;
        let descriptors = rows.iter().map(descriptor).collect::<Result<Vec<_>, _>>()?;
        let mut done = 0;
        for d in descriptors {
            if collect_one(&self.inner, conn, binding, &d)? {
                done += 1;
            }
        }
        drop(open);
        drop(command);
        Ok(done)
    }
}

/// Re-read only key/asset columns at touched keys, in one indexed VALUES batch
/// per asset table. This detects actual SQL rewrites without fetching unrelated
/// rows or any row's non-asset payload. Input refs cannot stand in for stored refs.
fn verify_rows(inner: &Inner, open: &mut Open) -> Result<(), Error> {
    use crate::shape::{Kind, TableShape};
    use crate::{bind_atom, decode, ident, row_key};
    type AssetRow = (crate::shape::Key, Option<serde_json::Map<String, Value>>);
    let mut by_table: BTreeMap<usize, Vec<AssetRow>> = BTreeMap::new();
    for ((table, key), (_, after)) in &open.touched {
        let shape = &inner.shape.tables[*table];
        if shape.assets.is_empty() {
            continue;
        }
        let fields = after
            .as_ref()
            .map(|row| {
                shape
                    .assets
                    .iter()
                    .map(|a| {
                        row.get(&a.field)
                            .cloned()
                            .map(|v| (a.field.clone(), v))
                            .ok_or_else(|| Error::Input("touched row has no asset field".into()))
                    })
                    .collect::<Result<serde_json::Map<String, Value>, Error>>()
            })
            .transpose()?;
        by_table
            .entry(*table)
            .or_default()
            .push((key.clone(), fields));
    }
    for (at, touched) in by_table {
        let table = &inner.shape.tables[at];
        let mut columns = table
            .key
            .iter()
            .map(|&f| table.columns[f].clone())
            .collect::<Vec<_>>();
        for asset in &table.assets {
            columns.push(
                table
                    .columns
                    .iter()
                    .find(|f| f.name == asset.field)
                    .ok_or_else(|| Error::Schema("asset column missing".into()))?
                    .clone(),
            );
        }
        let projection = TableShape {
            name: table.name.clone(),
            assets: table.assets.clone(),
            columns,
            key: (0..table.key.len()).collect(),
            order: None,
            indexes: Vec::new(),
            references: Vec::new(),
            bounded: None,
        };
        let mut params: Vec<crate::Param> = Vec::new();
        let mut values = Vec::new();
        for (key, _) in &touched {
            if key.len() != table.key.len() {
                return Err(Error::Schema("touched key shape differs".into()));
            }
            let mut tuple = Vec::new();
            for (&field, atom) in table.key.iter().zip(key) {
                if params.len() >= 65535 {
                    return Err(Error::Input(
                        "too many touched asset keys for one statement".into(),
                    ));
                }
                params.push(bind_atom(&table.columns[field], atom)?);
                tuple.push(format!(
                    "${}::{}",
                    params.len(),
                    table.columns[field].kind.sql()
                ));
            }
            values.push(format!("({})", tuple.join(",")));
        }
        let aliases = (0..table.key.len())
            .map(|i| format!("k{i}"))
            .collect::<Vec<_>>();
        let conditions = table
            .key
            .iter()
            .zip(&aliases)
            .map(|(&f, alias)| format!("t.{}=k.{alias}", ident(&table.columns[f].name)))
            .collect::<Vec<_>>();
        let selected = projection
            .columns
            .iter()
            .map(|c| {
                if c.kind == Kind::Json {
                    format!("t.{}::text", ident(&c.name))
                } else {
                    format!("t.{}", ident(&c.name))
                }
            })
            .collect::<Vec<_>>();
        let sql = format!(
            "SELECT {} FROM {} t JOIN (VALUES {}) k({}) ON {}",
            selected.join(","),
            inner.names.qualified(&table.name),
            values.join(","),
            aliases.join(","),
            conditions.join(" AND ")
        );
        let conn = inner.command_conn(open)?;
        let mut actual = BTreeMap::new();
        for raw in conn
            .query(&inner.counters, &sql, &params)
            .map_err(database)?
        {
            let row = decode(&projection, &raw, 0)?;
            let key = row_key(&row, &projection.columns, &projection.key)?;
            let fields = table
                .assets
                .iter()
                .map(|a| {
                    row.get(&a.field)
                        .cloned()
                        .map(|v| (a.field.clone(), v))
                        .ok_or_else(|| Error::Drift("actual row has no asset field".into()))
                })
                .collect::<Result<serde_json::Map<String, Value>, Error>>()?;
            if actual.insert(key, fields).is_some() {
                return Err(Error::Drift("duplicate stored asset key".into()));
            }
        }
        for (key, expected) in touched {
            if actual.remove(&key) != expected {
                return Err(Error::Refused(
                    "actual row asset fields differ from staged writes".into(),
                ));
            }
        }
        if !actual.is_empty() {
            return Err(Error::Drift(
                "asset projection returned unrelated rows".into(),
            ));
        }
    }
    Ok(())
}
fn verify_plans(inner: &Inner, conn: &mut Conn, plans: &[GenerationPlan]) -> Result<(), Error> {
    if plans.is_empty() {
        return Ok(());
    }
    let ids = plans
        .iter()
        .map(|p| p.generation.descriptor.reference.generation.clone())
        .collect::<Vec<_>>();
    let actual = counts(inner, conn, &ids)?;
    if actual.len() != plans.len()
        || plans.iter().any(|p| {
            actual.get(&p.generation.descriptor.reference.generation) != Some(&p.generation)
        })
    {
        return Err(Error::Refused(
            "actual asset registry differs from prepared generations/counts".into(),
        ));
    }
    let sql = format!(
        "SELECT generation FROM {} WHERE generation=ANY($1)",
        inner.names.qualified("pw_asset_collections")
    );
    let actual = conn
        .query(&inner.counters, &sql, &[Box::new(ids)])
        .map_err(database)?
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect::<std::collections::BTreeSet<_>>();
    let expected = plans
        .iter()
        .filter(|p| p.collect)
        .map(|p| p.generation.descriptor.reference.generation.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if actual != expected {
        return Err(Error::Refused(
            "actual captured collection jobs differ".into(),
        ));
    }
    Ok(())
}
/// Receipt/outbox hooks may modify rows/registry after prepare. Deferred hooks
/// must finish, then actual touched refs and plans must still match, before the
/// final caller/session clock fence and COMMIT. Kept orphan bytes stay unserved.
pub(crate) fn verify_before_commit(
    inner: &Inner,
    open: &mut Open,
    assets: &Transaction,
) -> Result<(), Error> {
    if inner.uploads.is_none() {
        return Ok(());
    }
    let conn = inner.command_conn(open)?;
    conn.batch(&inner.counters, "SET CONSTRAINTS ALL IMMEDIATE")
        .map_err(database)?;
    verify_rows(inner, open)?;
    let conn = inner.command_conn(open)?;
    verify_plans(inner, conn, &assets.plans)
}
