//! Durable namespace and immutable metadata; command lifecycle remains gated.
use crate::pool::{Conn, Counters};
use crate::sql::Names;
use crate::{Error, Options, PgTables, Uploads, database};
use pw_core::tables::{Schema, TablesJson};
use pw_host::assets::{Manifest, Namespace, Storage};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(crate) struct Binding {
    pub namespace: Namespace,
    pub store: Arc<dyn Storage>,
}
pub(crate) struct Configuration {
    pub store: Arc<dyn Storage>,
    candidate: Namespace,
    declarations: BTreeMap<String, Value>,
    bindings: BTreeMap<(String, String), Value>,
}
impl Configuration {
    pub fn new(input: Uploads, tables: &TablesJson, manifest: &Manifest) -> Result<Self, Error> {
        let declarations = manifest
            .declarations()
            .map(|d| serde_json::to_value(d).map(|v| (d.name().into(), v)))
            .collect::<Result<_, _>>()
            .map_err(|e| Error::Schema(e.to_string()))?;
        let mut bindings = BTreeMap::new();
        for table in &tables.tables {
            for asset in &table.schema.assets {
                let field = table
                    .schema
                    .fields
                    .iter()
                    .find(|f| f.name == asset.field)
                    .ok_or_else(|| Error::Schema("upload binding has no field".into()))?;
                bindings.insert(
                    (table.schema.name.clone(), asset.field.clone()),
                    json!({"upload":asset.upload,"field":field}),
                );
            }
        }
        Ok(Self {
            store: input.store,
            candidate: input.fresh_namespace,
            declarations,
            bindings,
        })
    }
}
impl PgTables {
    /// Explicit trusted upload binding. Enrollment does not enable commands;
    /// their row/storage/receipt lifetime remains closed until integrated.
    pub fn open_with_uploads(
        url: &str,
        schema: Option<&str>,
        manifest: TablesJson,
        options: Options,
        uploads: Uploads,
    ) -> Result<Self, Error> {
        Self::make(url, schema, manifest, options, Some(uploads))
    }
    pub fn upload_namespace(&self) -> Option<&Namespace> {
        self.inner.uploads.as_ref().map(|b| &b.namespace)
    }
    pub fn check_upload_storage(&self) -> Result<(), Error> {
        if let Some(binding) = &self.inner.uploads {
            binding
                .store
                .enroll(&binding.namespace, false)
                .map_err(storage_error)?;
        }
        Ok(())
    }
}
fn storage_error(error: std::io::Error) -> Error {
    Error::Schema(format!("upload storage namespace: {error}"))
}
fn value(text: &str) -> Result<Value, Error> {
    serde_json::from_str(text).map_err(|e| Error::Drift(format!("upload metadata: {e}")))
}
fn insert(
    conn: &mut Conn,
    counters: &Counters,
    sql: &str,
    params: &[crate::values::Param],
) -> Result<(), Error> {
    let rows = conn.query(counters, sql, params).map_err(database)?;
    if rows.len() != 1 {
        return Err(Error::Schema(
            "upload metadata insert was suppressed".into(),
        ));
    }
    Ok(())
}
/// Called after platform tables exist, inside the migration transaction, BEFORE
/// any app DDL or its equal-hash early return. No whole dynamic registry load.
pub(crate) fn enroll(
    conn: &mut Conn,
    counters: &Counters,
    names: &Names,
    config: Option<&Configuration>,
    recorded: Option<&Schema>,
) -> Result<Option<Namespace>, Error> {
    let Metadata {
        namespace,
        declarations,
        bindings,
    } = read_metadata(conn, counters, names)?;
    let qualified = |table: &str| names.qualified(table);
    let Some(config) = config else {
        if recorded.is_some_and(|schema| schema.tables.iter().any(|table| !table.assets.is_empty()))
        {
            return Err(Error::Schema(
                "recorded upload bindings have lost their enrollment metadata".into(),
            ));
        }
        if namespace.is_some() || !declarations.is_empty() || !bindings.is_empty() {
            return Err(Error::Schema(
                "upload metadata cannot be removed by an unbound manifest".into(),
            ));
        }
        return Ok(None);
    };
    if namespace.is_none() && (!declarations.is_empty() || !bindings.is_empty()) {
        return Err(Error::Drift("upload metadata has no namespace".into()));
    }
    for (name, old) in &declarations {
        if config.declarations.get(name) != Some(old) {
            return Err(Error::Schema(format!(
                "upload declaration {name} changed or was removed"
            )));
        }
    }
    for (binding, old) in &bindings {
        if config.bindings.get(binding) != Some(old) {
            return Err(Error::Schema(format!(
                "upload binding {}.{} changed or was removed",
                binding.0, binding.1
            )));
        }
    }
    for (table, field) in config.bindings.keys() {
        if !bindings.contains_key(&(table.clone(), field.clone()))
            && recorded.is_some_and(|schema| schema.tables.iter().any(|t| &t.name == table))
        {
            return Err(Error::Schema(format!(
                "new upload binding {table}.{field} on an existing table needs explicit conversion/version fencing"
            )));
        }
    }
    let namespace = match namespace {
        Some(namespace) => {
            config
                .store
                .enroll(&namespace, false)
                .map_err(storage_error)?;
            namespace
        }
        None => {
            config
                .store
                .enroll(&config.candidate, true)
                .map_err(storage_error)?;
            insert(
                conn,
                counters,
                &format!(
                    "INSERT INTO {}(namespace) VALUES($1) RETURNING 1",
                    qualified("pw_asset_namespace")
                ),
                &[Box::new(config.candidate.hex().to_string())],
            )?;
            config.candidate.clone()
        }
    };
    for (name, declaration) in &config.declarations {
        if !declarations.contains_key(name) {
            insert(
                conn,
                counters,
                &format!(
                    "INSERT INTO {}(name,declaration) VALUES($1,$2::text::json) RETURNING 1",
                    qualified("pw_asset_declarations")
                ),
                &[Box::new(name.clone()), Box::new(declaration.to_string())],
            )?;
        }
    }
    for ((table, field), binding) in &config.bindings {
        if !bindings.contains_key(&(table.clone(), field.clone())) {
            let upload = binding["upload"]
                .as_str()
                .ok_or_else(|| Error::Schema("upload binding has no declaration".into()))?;
            insert(
                conn,
                counters,
                &format!(
                    "INSERT INTO {}(table_name,field_name,upload,field) VALUES($1,$2,$3,$4::text::json) RETURNING 1",
                    qualified("pw_asset_bindings")
                ),
                &[
                    Box::new(table.clone()),
                    Box::new(field.clone()),
                    Box::new(upload.to_string()),
                    Box::new(binding["field"].to_string()),
                ],
            )?;
        }
    }
    Ok(Some(namespace))
}

#[derive(PartialEq)]
struct Metadata {
    namespace: Option<Namespace>,
    declarations: BTreeMap<String, Value>,
    bindings: BTreeMap<(String, String), Value>,
}
fn read_metadata(conn: &mut Conn, counters: &Counters, names: &Names) -> Result<Metadata, Error> {
    let qualified = |table: &str| names.qualified(table);
    let rows = conn
        .query(
            counters,
            &format!("SELECT namespace FROM {}", qualified("pw_asset_namespace")),
            &[],
        )
        .map_err(database)?;
    if rows.len() > 1 {
        return Err(Error::Drift("multiple upload namespaces recorded".into()));
    }
    let namespace = rows
        .first()
        .map(|r| r.get::<_, String>(0))
        .map(|s| {
            Namespace::parse(&s)
                .ok_or_else(|| Error::Drift("invalid upload namespace recorded".into()))
        })
        .transpose()?;
    let mut declarations = BTreeMap::new();
    for r in conn
        .query(
            counters,
            &format!(
                "SELECT name,declaration::text FROM {}",
                qualified("pw_asset_declarations")
            ),
            &[],
        )
        .map_err(database)?
    {
        declarations.insert(r.get::<_, String>(0), value(&r.get::<_, String>(1))?);
    }
    let mut bindings = BTreeMap::new();
    for r in conn
        .query(
            counters,
            &format!(
                "SELECT table_name,field_name,upload,field::text FROM {}",
                qualified("pw_asset_bindings")
            ),
            &[],
        )
        .map_err(database)?
    {
        bindings.insert(
            (r.get::<_, String>(0), r.get::<_, String>(1)),
            json!({"upload":r.get::<_,String>(2),"field":value(&r.get::<_,String>(3))?}),
        );
    }
    Ok(Metadata {
        namespace,
        declarations,
        bindings,
    })
}
/// RETURNING a row count cannot attest to its values, and a later app-schema
/// record hook can change a prior insert. Read exact persisted values after
/// the whole migration, while its transaction/lock still owns the metadata.
pub(crate) fn verify(
    conn: &mut Conn,
    counters: &Counters,
    names: &Names,
    config: Option<&Configuration>,
    namespace: Option<&Namespace>,
) -> Result<(), Error> {
    let expected = match (config, namespace) {
        (Some(config), Some(namespace)) => Metadata {
            namespace: Some(namespace.clone()),
            declarations: config.declarations.clone(),
            bindings: config.bindings.clone(),
        },
        (None, None) => Metadata {
            namespace: None,
            declarations: BTreeMap::new(),
            bindings: BTreeMap::new(),
        },
        _ => {
            return Err(Error::Schema(
                "upload enrollment configuration is incomplete".into(),
            ));
        }
    };
    let stored = read_metadata(conn, counters, names)?;
    if stored != expected {
        return Err(Error::Schema(
            "upload persisted enrollment metadata changed before commit".into(),
        ));
    }
    Ok(())
}
