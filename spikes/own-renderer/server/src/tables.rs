//! **A program's declared tables, served** (track `data-path`; ADR-0338's
//! D2, the routing ADR-0385 and ADR-0386 left): a program that declares its
//! data in Pleris (`table Entries`, ADR-0350) is served from its build's
//! `tables.json` by the one generic memory adapter (`pw-table-memory`), with
//! no line of Rust of its own.
//!
//! - **The gate is the only way.** The layer supplies no operation by name:
//!   each call of the program links its table functions through the engine's
//!   gate (`call_authorized_with_tables`), and its host map holds the
//!   platform's operations and the functions the gate admits for that
//!   component alone: a query's reads of the published tables (or its
//!   render's snapshot), a command's reads and writes staged in its own
//!   transaction ([`DataLayer::table_reads`], [`Staged::table_ops`]).
//! - **A command is one transaction with the platform's records**
//!   (ADR-0208, ADR-0325, ADR-0334): its writes staged in the adapter's
//!   command; every refusal its commit can make (a bound, a reference) made
//!   before the materializer's transaction ([`Staged::commit`] prepares it);
//!   and its rows published as that transaction's last step before its
//!   COMMIT ([`Staged::publish_within`]): after its state rows, its
//!   interaction's record, its runs and its outbox are written, each of
//!   which may refuse it and roll all of it back, with nothing published.
//!   ADR-0386's decision 5 asked for this before a host might publish the
//!   adapter's writes: the adapter's publication may fail (its storage
//!   failed closed), and a failure after the records committed would have
//!   kept records of writes no reader sees. A COMMIT that fails after the
//!   rows were published fails the tables closed ([`Staged::unpublished`]):
//!   no reader reads a write whose records were lost.
//! - **Which queries a commit makes stale is what the command declares**
//!   (ADR-0007, PW5106), delivered by the outbox as for every layer.
//! - **What a build's tables may be at all** ([`servable`]), whichever
//!   layer serves them: one layer a program (a program that imports a
//!   hand-written layer's operation beside its tables is refused where it is
//!   served; T1 routes several layers in one host, ADR-0338 5.1); no row's
//!   field of a handle's type (PW6901's, again); and no `upload`, whose
//!   images a table keeps nowhere.

use super::*;
use crate::data::{Commit, DataLayer, Ops, Snapshot, Staged};
use pw_table_memory::{Command, MemoryTables, Prepared};

/// **A program's tables in memory**, as its build declares them, and what a
/// node is granted of them: each table's read and write.
pub(crate) struct TablesLayer {
    tables: MemoryTables,
    grants: Vec<&'static str>,
    /// The program's declared uploads, where it declares any (ADR-0440,
    /// L5b): their generations' bytes in memory, lost with the tables, under
    /// a namespace of their own.
    uploads: Option<MemoryUploads>,
}

/// **Where the memory layer keeps its upload generations' bytes**.
struct MemoryUploads {
    storage: crate::blob::MemoryBlobs,
    namespace: pw_host::assets::Namespace,
}

/// The canonical bytes a command published, by the generation the adapter
/// chose for them, until its publication keeps them.
type KeptBytes = Arc<Mutex<std::collections::BTreeMap<String, Arc<[u8]>>>>;

/// **Whether a contract imports a table's function** (`pw:tables/…`).
pub(crate) fn imports_tables(contracts: &[ComponentContract]) -> bool {
    contracts
        .iter()
        .flat_map(|c| &c.imports)
        .any(|i| pw_host::tables::is_table_interface(&i.interface))
}

/// **Whether a build's declared tables may be served, by any layer**, and
/// why not, before anything is served (track `data-path`):
/// - **one layer a program**: a contract importing a hand-written layer's
///   operation beside its tables is refused, by name (T1 routes several
///   layers in one host, ADR-0338 5.1);
/// - **no row holds a handle** (the integrator's ruling 3 of 2026-11-02,
///   PW6901 held again, defence in depth): a field whose type tables.json
///   writes as a session's, a user's or an organization's handle
///   (`User<…>`, bare or `capability.`'s, anywhere in it) refuses the start,
///   named. What a field's record type holds inside it is the compiler's to
///   see; tables.json writes the record's name;
/// - **each upload a table's asset** (ADR-0440, L5b; the integrator's note
///   b before it): an `upload` no table keeps would take a reader's image and
///   keep it nowhere, so it is refused, named; the rest are bound as a layer
///   binds them.
pub(crate) fn servable(
    build: &std::path::Path,
    contracts: &[ComponentContract],
) -> Result<(), String> {
    if let Some((component, import)) = contracts.iter().find_map(|c| {
        c.imports
            .iter()
            .find(|i| {
                i.kind == pw_host::ImportKind::HostCapability && !i.interface.starts_with("pw:")
            })
            .map(|i| (c.component_id.clone(), i.key()))
    }) {
        return Err(format!(
            "`{component}` imports `{import}`, a hand-written data layer's, and the program \
             declares its tables: one host serves one layer of a program until its sources \
             are routed apart (ADR-0338 5.1)"
        ));
    }
    let path = build.join("tables.json");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "{}: {e}: the program imports its tables' functions and its build declares none",
            path.display()
        )
    })?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("tables.json: {e}"))?;
    for table in json["tables"].as_array().into_iter().flatten() {
        for field in table["fields"].as_array().into_iter().flatten() {
            let written = field["type"].as_str().unwrap_or_default();
            if let Some(whose) = handle_named(written) {
                return Err(format!(
                    "tables.json: `{}`'s field `{}` is `{written}`, {whose} handle: a row is read \
                     by every reader of its table, and a handle is its own reader's alone \
                     (PW6901); a row names someone by their id",
                    table["name"].as_str().unwrap_or_default(),
                    field["name"].as_str().unwrap_or_default()
                ));
            }
        }
    }
    // **Its uploads, each a table's asset** (ADR-0440, L5b): a program of
    // tables keeps an upload's image in the table that names it, so one no
    // table keeps is refused, by name, as a reader's image would be kept
    // nowhere; the rest are bound as the layer binds them, or refused saying
    // why.
    let uploads = build.join("uploads.json");
    let uploads_text = match std::fs::read_to_string(&uploads) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", uploads.display())),
    };
    let declared: Vec<serde_json::Value> =
        serde_json::from_str(&uploads_text).map_err(|e| format!("uploads.json: {e}"))?;
    let kept: std::collections::BTreeSet<&str> = json["tables"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|table| table["assets"].as_array().into_iter().flatten())
        .filter_map(|asset| asset["upload"].as_str())
        .collect();
    let unkept = |upload: &&serde_json::Value| {
        !upload["name"]
            .as_str()
            .is_some_and(|name| kept.contains(name))
    };
    if let Some(upload) = declared.iter().find(unkept) {
        let name = upload["name"].as_str().unwrap_or_default();
        return Err(format!(
            "`{name}` is an `upload`, and no table of the program keeps it: the image a reader \
             sends would be kept nowhere; a table that keeps one says `asset <field> from {name}`"
        ));
    }
    if !declared.is_empty() {
        pw_host::tables::Manifest::from_json(&text)
            .and_then(|manifest| manifest.with_uploads(&uploads_text))
            .map_err(|e| format!("uploads.json: {e}"))?;
    }
    Ok(())
}

/// **The generic uploads a build declares** (`uploads.json`, ADR-0440,
/// L5b): its text, where it declares any, which a layer binds its tables
/// to; none where it declares none.
pub(crate) fn declared_uploads(build: &std::path::Path) -> Result<Option<String>, String> {
    let path = build.join("uploads.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let declared: Vec<serde_json::Value> =
        serde_json::from_str(&text).map_err(|e| format!("uploads.json: {e}"))?;
    Ok((!declared.is_empty()).then_some(text))
}

/// **What a node is granted of each upload its tables are bound to**: the
/// declaration's read and write, its claim's acquire and release, as a
/// compiled upload import requires them (`pw_host::assets::Access`).
pub(crate) fn upload_grants(
    manifest: &pw_host::tables::Manifest,
) -> impl Iterator<Item = &'static str> + '_ {
    manifest
        .uploads()
        .into_iter()
        .flat_map(|uploads| uploads.declarations())
        .flat_map(|declaration| {
            let name = declaration.name();
            [
                interned(format!("database.read<{name}>")),
                interned(format!("database.write<{name}>")),
                interned(format!("resource.acquire<{name}.Claim>")),
                interned(format!("resource.release<{name}.Claim>")),
            ]
        })
}

/// **The handle a type, as tables.json writes it, names**: a session's, a
/// user's or an organization's (`Session<…>`, `User<…>`, `Organization<…>`,
/// bare or as `capability.`'s), anywhere in it; none for any other type,
/// `UserId` and a program's own `User` record among them.
fn handle_named(written: &str) -> Option<&'static str> {
    let named = |c: char| c.is_ascii_alphanumeric() || c == '_';
    for (handle, whose) in [
        ("Session<", "a session's"),
        ("User<", "a user's"),
        ("Organization<", "an organization's"),
    ] {
        for (at, _) in written.match_indices(handle) {
            let before = &written[..at];
            let bare = before
                .chars()
                .next_back()
                .is_none_or(|c| !named(c) && c != '.');
            let platform = before.strip_suffix("capability.").is_some_and(|outer| {
                outer
                    .chars()
                    .next_back()
                    .is_none_or(|c| !named(c) && c != '.')
            });
            if bare || platform {
                return Some(whose);
            }
        }
    }
    None
}

impl TablesLayer {
    /// **The layer for a build whose contracts import its tables**: its
    /// `tables.json`, held whole, in a memory adapter of its own. Refused
    /// where the build's tables may not be served ([`servable`]: no tables
    /// declared, a hand-written layer's operation beside them, a row's field
    /// of a handle's type, an upload).
    pub(crate) fn from_build(
        build: &std::path::Path,
        contracts: &[ComponentContract],
    ) -> Result<TablesLayer, String> {
        servable(build, contracts)?;
        let path = build.join("tables.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let declared = declared_uploads(build)?;
        let tables = match &declared {
            Some(metadata) => MemoryTables::new_with_uploads(
                serde_json::from_str(&text).map_err(|e| format!("tables.json: {e}"))?,
                metadata,
            ),
            None => MemoryTables::from_json(&text),
        }
        .map_err(|e| format!("tables.json: {e:?}"))?;
        let uploads = match declared {
            Some(_) => {
                use pw_host::assets::Storage;
                let storage = crate::blob::MemoryBlobs::default();
                let namespace =
                    pw_host::assets::Namespace::from_bytes(crate::identity::random_bytes::<16>());
                storage
                    .enroll(&namespace, true)
                    .map_err(|e| format!("the program's upload storage: {e}"))?;
                Some(MemoryUploads { storage, namespace })
            }
            None => None,
        };
        let grants = tables
            .host_tables()
            .names()
            .flat_map(|table| {
                [
                    interned(format!("database.read<{table}>")),
                    interned(format!("database.write<{table}>")),
                ]
            })
            .chain(upload_grants(tables.host_tables()))
            .collect();
        Ok(TablesLayer {
            tables,
            grants,
            uploads,
        })
    }
}

/// **A capability as the node's grants hold it** (`DataLayer::grants`, whose
/// words are `&'static str`): each kept once a process, however many servers
/// a process builds, so no server leaks more than its tables' names.
pub(crate) fn interned(capability: String) -> &'static str {
    static INTERNED: std::sync::OnceLock<Mutex<std::collections::BTreeSet<&'static str>>> =
        std::sync::OnceLock::new();
    let mut kept = INTERNED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(found) = kept.get(capability.as_str()) {
        return found;
    }
    let leaked: &'static str = Box::leak(capability.into_boxed_str());
    kept.insert(leaked);
    leaked
}

/// A render's snapshot of the tables (NEXT 4c).
struct TablesSnapshot(pw_table_memory::Snapshot);

impl DataLayer for TablesLayer {
    /// No operation by name: a program's table functions are linked through
    /// the gate alone, per component ([`DataLayer::table_reads`]).
    fn reads(&self, _session: &str, _stopped: Option<Stopped>) -> Ops {
        Ops::new()
    }

    fn check_read(&self) -> Result<(), String> {
        self.tables
            .check_health()
            .map_err(|e| format!("the tables refuse reads: {e:?}"))
    }

    /// The published tables as they are now: every query of one render reads
    /// them, whatever commits meanwhile. None where the adapter has failed,
    /// whose every read then fails.
    fn snapshot(&self) -> Option<Snapshot> {
        self.tables
            .snapshot()
            .ok()
            .map(|taken| Box::new(TablesSnapshot(taken)) as Snapshot)
    }

    fn tables(&self) -> Option<&pw_host::tables::Manifest> {
        Some(self.tables.host_tables())
    }

    fn table_reads(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
        snapshot: Option<&Snapshot>,
    ) -> Result<Ops, String> {
        let read = match snapshot {
            Some(snapshot) => match snapshot.downcast_ref::<TablesSnapshot>() {
                Some(TablesSnapshot(taken)) => {
                    self.tables.read_host_fns_at(taken, contract, granted)
                }
                None => return Err("a render's snapshot is not this program's tables'".into()),
            },
            None => self.tables.read_host_fns(contract, granted),
        };
        read.map_err(|e| format!("`{}`'s tables: {e:?}", contract.component_id))
    }

    fn begin<'a>(&'a self, _session: &str) -> Box<dyn Staged + 'a> {
        Box::new(TablesStaging {
            tables: &self.tables,
            turn: match self.tables.begin() {
                Ok(command) => Turn::Open(command),
                Err(e) => Turn::Failed(format!("the tables began no command: {e:?}")),
            },
            uploads: self.uploads.as_ref(),
            kept: KeptBytes::default(),
            collect: Vec::new(),
            published: false,
        })
    }

    fn grants(&self) -> Vec<&'static str> {
        self.grants.clone()
    }

    /// **The committed generation `declaration` names by `key`, and its
    /// bytes** (ADR-0440, L5b): none where no committed row names it (404);
    /// an error where one does and its bytes are missing or not its own
    /// (503, never a 404 or a 304).
    fn read_asset(
        &self,
        declaration: &str,
        key: &str,
        kind: &str,
    ) -> Result<Option<(pw_host::assets::Descriptor, Vec<u8>)>, String> {
        use pw_host::assets::Storage;
        let uploads = self
            .uploads
            .as_ref()
            .ok_or("the program's tables keep no uploads")?;
        let Some(descriptor) = self
            .tables
            .asset_descriptor(declaration, key, kind)
            .map_err(|e| format!("{e:?}"))?
        else {
            return Ok(None);
        };
        match uploads
            .storage
            .get_generation(&uploads.namespace, &descriptor)
            .map_err(|e| format!("the upload generation could not be read: {e}"))?
        {
            Some(bytes) => Ok(Some((descriptor, bytes))),
            // Missing: collected since it was looked up, its row removed
            // meanwhile (404, as PostgreSQL answers under its lock), or named
            // still and its bytes gone (503). A second look tells which (the
            // review of a7c3377b: until then a removal racing the read
            // answered 503).
            None => {
                let missing = |d: &pw_host::assets::Descriptor| {
                    format!(
                        "the upload generation {} is named and its bytes are missing",
                        d.reference.generation
                    )
                };
                match self
                    .tables
                    .asset_descriptor(declaration, key, kind)
                    .map_err(|e| format!("{e:?}"))?
                {
                    None => Ok(None),
                    // Published again meanwhile, another generation: read once.
                    Some(again) if again != descriptor => match uploads
                        .storage
                        .get_generation(&uploads.namespace, &again)
                        .map_err(|e| format!("the upload generation could not be read: {e}"))?
                    {
                        Some(bytes) => Ok(Some((again, bytes))),
                        None => Err(missing(&again)),
                    },
                    Some(_) => Err(missing(&descriptor)),
                }
            }
        }
    }

    /// The generations a commit left unnamed whose collection failed, tried
    /// again a bounded batch at a time.
    fn housekeeping(&self, _now_ms: i64, _sessions: &crate::identity::SessionLifetimes) {
        use pw_host::assets::Storage;
        let Some(uploads) = &self.uploads else { return };
        let pending = match self.tables.pending_asset_collections(500) {
            Ok(pending) => pending,
            Err(e) => {
                return eprintln!("pw dev server: the upload collections could not be read: {e:?}");
            }
        };
        for descriptor in pending {
            let collected = uploads
                .storage
                .collect_generation(&uploads.namespace, &descriptor)
                .map_err(|e| e.to_string())
                .and_then(|()| {
                    self.tables
                        .asset_collected(&descriptor.reference.generation)
                        .map_err(|e| format!("{e:?}"))
                });
            if let Err(why) = collected {
                eprintln!("pw dev server: an upload generation was not collected: {why}");
            }
        }
    }

    fn default_page(&self) -> Option<&'static str> {
        None
    }

    /// **The statements its adapter has run**, each primitive one, reads
    /// and writes (`MemoryTables::statements`): what the conformance suite
    /// holds a read's cost to, below this layer, so a layer that calls the
    /// adapter twice for one read is seen.
    #[cfg(test)]
    fn statements(&self) -> Option<u64> {
        Some(self.tables.statements())
    }
}

#[cfg(test)]
thread_local! {
    /// **A test's: the next publication on this thread fails**, as the
    /// adapter's would where its storage had failed.
    pub(crate) static FAIL_NEXT_PUBLISH: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
    /// **A test's: the next keep of a new generation's bytes on this thread
    /// fails**, as the layer's storage would where it had failed.
    pub(crate) static FAIL_NEXT_KEEP: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// **Where one command's transaction is.**
enum Turn {
    /// Begun: its functions stage in it.
    Open(Command),
    /// Held to what its commit keeps, before the materializer's transaction.
    Prepared(Prepared),
    /// Published, as that transaction's last step.
    Published,
    /// Refused, or failed: it keeps nothing, and says why.
    Failed(String),
}

/// **One command's writes to a program's tables**, staged in the adapter's
/// command until the platform's transaction publishes them, and undone
/// whole where it is dropped before (the adapter's own `Drop`).
pub(crate) struct TablesStaging<'a> {
    /// The adapter, which a COMMIT failing after it published fails closed.
    tables: &'a MemoryTables,
    turn: Turn,
    /// Where its uploads' bytes are kept, where the program declares any.
    uploads: Option<&'a MemoryUploads>,
    /// The bytes it published, until its publication keeps them.
    kept: KeptBytes,
    /// The generations its publication left unnamed, collected after the
    /// commit.
    collect: Vec<pw_host::assets::Descriptor>,
    /// Its rows were published ([`Staged::claims_kept_nothing`]): a commit
    /// that fails after them may have kept them.
    published: bool,
}

impl TablesStaging<'_> {
    /// **`prepared` published, its new generations' bytes kept first**
    /// (ADR-0440, L5b): a generation kept and its row refused is an orphan
    /// the namespace holds; a row published and its bytes not kept would
    /// name nothing, so a keep that fails refuses the publication.
    fn publish_prepared(&mut self, prepared: Prepared) -> Result<(), String> {
        let mut collect = Vec::new();
        if let Some(uploads) = self.uploads {
            use pw_host::assets::Storage;
            let plans = prepared
                .asset_plans()
                .map_err(|e| format!("the tables' upload plans: {e:?}"))?;
            let kept = self
                .kept
                .lock()
                .map_err(|_| "upload bytes are unavailable")?;
            for plan in &plans {
                if plan.keep_new {
                    #[cfg(test)]
                    if FAIL_NEXT_KEEP.with(|fail| fail.replace(false)) {
                        return Err("the upload generation was not kept: a test's failure".into());
                    }
                    let descriptor = &plan.generation.descriptor;
                    let bytes = kept
                        .get(&descriptor.reference.generation)
                        .ok_or("a new upload generation has no bytes to keep")?;
                    uploads
                        .storage
                        .keep_new(&uploads.namespace, descriptor, bytes)
                        .map_err(|e| format!("the upload generation was not kept: {e}"))?;
                }
            }
            collect = plans
                .into_iter()
                .filter(|plan| plan.collect)
                .map(|plan| plan.generation.descriptor)
                .collect();
        }
        // What it leaves unnamed is collected after the commit, and only
        // where it published: a publication refused names what it named.
        prepared
            .publish()
            .map(|_| {
                self.published = true;
                self.collect = collect;
            })
            .map_err(|e| format!("the tables did not publish the commit: {e:?}"))
    }

    /// **The generations its commit left unnamed, collected**: after the
    /// commit, never inside it; one that fails is tried again by the layer's
    /// housekeeping, as the adapter keeps its job.
    fn collect_unnamed(&mut self) {
        let Some(uploads) = self.uploads else { return };
        use pw_host::assets::Storage;
        for descriptor in std::mem::take(&mut self.collect) {
            let collected = uploads
                .storage
                .collect_generation(&uploads.namespace, &descriptor)
                .map_err(|e| e.to_string())
                .and_then(|()| {
                    self.tables
                        .asset_collected(&descriptor.reference.generation)
                        .map_err(|e| format!("{e:?}"))
                });
            if let Err(why) = collected {
                eprintln!("pw dev server: an upload generation was not collected: {why}");
            }
        }
    }
}

impl Staged for TablesStaging<'_> {
    /// No operation by name ([`Staged::table_ops`]).
    fn ops(&self) -> Ops {
        Ops::new()
    }

    fn table_ops(&self, contract: &ComponentContract, granted: &Granted) -> Result<Ops, String> {
        match &self.turn {
            Turn::Open(command) => command
                .host_fns(contract, granted)
                .map_err(|e| format!("`{}`'s tables: {e:?}", contract.component_id)),
            Turn::Failed(why) => Err(why.clone()),
            _ => Err("a command's tables are linked before it commits".into()),
        }
    }

    fn table_predicate_reads(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
    ) -> Result<Ops, String> {
        match &self.turn {
            Turn::Open(command) => command
                .predicate_host_fns(contract, granted)
                .map_err(|e| format!("`{}`'s predicate tables: {e:?}", contract.component_id)),
            Turn::Failed(why) => Err(why.clone()),
            _ => Err("a command's predicate reads before it commits".into()),
        }
    }

    /// Each table the command changed, which the materializer's transaction
    /// records; none where it changed none, and nothing commits. A command
    /// whose writes cannot be read is no command that wrote nothing: it goes
    /// on to its commit, which refuses it.
    fn rows(&self) -> Option<Vec<(String, String)>> {
        match &self.turn {
            Turn::Open(command) => match command.written() {
                Ok(written) if written.is_empty() => None,
                Ok(written) => Some(
                    written
                        .into_iter()
                        .map(|table| (format!("pw:tables/{table}"), "written".to_string()))
                        .collect(),
                ),
                Err(_) => Some(Vec::new()),
            },
            _ => Some(Vec::new()),
        }
    }

    /// **Every refusal the commit can make, before the materializer's
    /// transaction**: each written table within its bound, each reference to
    /// a row that exists (`Command::prepare`). Refused, the command is undone
    /// whole and nothing of it is kept anywhere.
    fn commit(
        &mut self,
        _events: &[data::Handed],
        _invalidated: &[data::Dropped],
        _interaction: Option<&data::Recorded<'_>>,
        _scheduled: &[schedules::Scheduled],
        _completing: Option<schedules::Completing<'_>>,
    ) -> Result<Commit, String> {
        match std::mem::replace(&mut self.turn, Turn::Failed("committing".into())) {
            Turn::Open(command) => match command.prepare() {
                Ok(prepared) => {
                    self.turn = Turn::Prepared(prepared);
                    Ok(Commit::Materializer)
                }
                Err(e) => {
                    let why = format!("the tables refused the commit: {e:?}");
                    self.turn = Turn::Failed(why.clone());
                    Err(why)
                }
            },
            Turn::Failed(why) => {
                self.turn = Turn::Failed(why.clone());
                Err(why)
            }
            other => {
                self.turn = other;
                Err("a command's tables commit once".into())
            }
        }
    }

    /// **Its rows published, as the materializer's transaction's last step
    /// before its COMMIT**: after its state rows, the interaction's record,
    /// the runs and the outbox are written in it. Refused (the adapter's
    /// storage has failed), the transaction rolls back, and nothing of the
    /// command is kept.
    fn publish_within(&mut self) -> Result<(), String> {
        #[cfg(test)]
        if FAIL_NEXT_PUBLISH.with(|fail| fail.replace(false)) {
            // Its command still prepared, undone where the staging is dropped.
            return Err("the tables did not publish the commit: a test's failure".into());
        }
        match std::mem::replace(&mut self.turn, Turn::Published) {
            Turn::Prepared(prepared) => self.publish_prepared(prepared).inspect_err(|why| {
                self.turn = Turn::Failed(why.clone());
            }),
            Turn::Failed(why) => {
                self.turn = Turn::Failed(why.clone());
                Err(why)
            }
            other => {
                self.turn = other;
                Err("a command's tables are published once, after they are prepared".into())
            }
        }
    }

    /// **The materializer's COMMIT failed after its rows were published**
    /// (the integrator's ruling on S1, 2026-11-02): readers may have read
    /// them already, and their records are lost, so a publication cannot be
    /// taken back. The tables fail closed: every read and write of them is
    /// refused from now on, so no reader goes on reading a write without its
    /// records, and the host says so.
    fn unpublished(&mut self, why: &str) {
        eprintln!(
            "pw dev server: a commit's records did not commit after its tables were published: \
             the tables are closed, and every read and write of them is refused: {why}"
        );
        self.tables.fail_closed();
        self.turn = Turn::Failed(format!("the tables are closed: {why}"));
    }

    /// **After the commit, what it staged is the layer's**: published
    /// already, within its transaction, where the host published it there
    /// ([`Staged::publish_within`]); published now otherwise, as a
    /// hand-written layer publishes, and said where it cannot be (the
    /// adapter's storage has failed closed, and every read of it fails).
    fn publish(&mut self) {
        if let Turn::Prepared(_) = self.turn
            && let Turn::Prepared(prepared) = std::mem::replace(&mut self.turn, Turn::Published)
            && let Err(e) = self.publish_prepared(prepared)
        {
            eprintln!("pw dev server: a commit's tables were not published after it: {e}");
            self.turn = Turn::Failed(format!("not published: {e}"));
        }
        self.collect_unnamed();
    }

    /// Kept nothing, unless its rows were published: a commit that failed
    /// after them may have kept them (the tables close), so its claims stay.
    fn claims_kept_nothing(&self) -> bool {
        !self.published
    }

    /// The adapter's publisher on this command, the canonical bytes kept
    /// beside it for the publication's keep. None where the program declares
    /// no uploads.
    fn upload_publisher(&mut self) -> Result<Option<data::UploadPublisher>, String> {
        if self.uploads.is_none() {
            return Ok(None);
        }
        let Turn::Open(command) = &self.turn else {
            return Err("a command's uploads are published while it runs".into());
        };
        let propose = command.asset_publisher();
        let kept = Arc::clone(&self.kept);
        Ok(Some(data::UploadPublisher {
            publish: Arc::new(move |declaration, descriptor, bytes| {
                let reference = propose(declaration, descriptor).map_err(|e| format!("{e:?}"))?;
                kept.lock()
                    .map_err(|_| "upload bytes are unavailable".to_string())?
                    .insert(reference.generation.clone(), bytes);
                Ok(reference)
            }),
            require_receipt: Arc::new(|| Ok(())),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **What tables.json names a handle** (the integrator's ruling 3 of
    /// 2026-11-02, the host's half): a session's, a user's or an
    /// organization's, bare or the platform's, anywhere in a field's type;
    /// never an id, a program's own `User` record, or a type whose name
    /// only ends in one.
    #[test]
    fn a_handle_is_named_wherever_a_fields_type_holds_it() {
        for (written, whose) in [
            ("User<UserId>", "a user's"),
            ("capability.User<UserId>", "a user's"),
            ("Option<capability.User<UserId>>", "a user's"),
            ("Session<SessionId>", "a session's"),
            ("Map<String, Session<SessionId>>", "a session's"),
            ("List<Organization<OrganizationId>>", "an organization's"),
        ] {
            assert_eq!(handle_named(written), Some(whose), "{written}");
        }
        for written in [
            "UserId",
            "Option<UserId>",
            "User",
            "Users<UserId>",
            "SuperUser<UserId>",
            "app.User<Int>",
            "Option<my.capability.User<UserId>>",
        ] {
            assert_eq!(handle_named(written), None, "{written}");
        }
    }
}
