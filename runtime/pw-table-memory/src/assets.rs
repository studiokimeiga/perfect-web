//! Asset reference bookkeeping; storage/claim lifecycle remains host-owned.
use super::*;
use pw_host::assets::{Descriptor, GenerationPlan, Reference, ReferenceUse};

impl MemoryTables {
    /// Explicit trusted host binding. Ordinary construction and server startup
    /// retain their guards until storage/claims/durable lifetimes are proven.
    pub fn new_with_uploads(manifest: TablesJson, uploads: &str) -> Result<Self, Error> {
        Self::make(manifest, Some(uploads))
    }
    /// Resolve committed same-declaration liveness. Never reads open proposals
    /// or staged rows; health failure refuses this path just as row reads do.
    pub fn asset_reference(
        &self,
        declaration: &str,
        key: &str,
        kind: &str,
    ) -> Result<Option<Reference>, Error> {
        let working = self.inner.lock_working()?;
        self.inner.asset_operations.fetch_add(1, Ordering::SeqCst);
        Ok(working
            .registry
            .current_reference(declaration, key, kind)
            .cloned())
    }
    /// Captured-generation cleanup jobs; no storage action is performed here.
    pub fn pending_asset_collections(&self, limit: usize) -> Result<Vec<Descriptor>, Error> {
        let working = self.inner.lock_working()?;
        self.inner.asset_operations.fetch_add(1, Ordering::SeqCst);
        Ok(working
            .registry
            .pending_collections()
            .take(limit)
            .cloned()
            .collect())
    }
    /// Called only after successful captured-generation storage cleanup.
    pub fn asset_collected(&self, generation: &str) -> Result<(), Error> {
        let mut working = self.inner.lock_working()?;
        self.inner.asset_operations.fetch_add(1, Ordering::SeqCst);
        working.registry.collected(generation).map_err(Error::Input)
    }
    /// Registry operations, separate from row primitive statements: each
    /// proposal/resolve/job read/completion, and each planned/applied generation.
    pub fn asset_operations(&self) -> u64 {
        self.inner.asset_operations.load(Ordering::SeqCst)
    }
}
/// A trusted bridge's proposal function on the SAME existing command. It owns
/// no turn/lifetime and cannot publish, prepare or end the command.
pub type AssetPublisher = Arc<dyn Fn(&str, Descriptor) -> Result<Reference, Error> + Send + Sync>;
impl Command {
    pub fn asset_publisher(&self) -> AssetPublisher {
        let inner = Arc::clone(&self.inner);
        let id = self.id;
        Arc::new(move |declaration, descriptor| {
            let borrowed = Command {
                inner: Arc::clone(&inner),
                id,
                ended: true,
            };
            borrowed.propose_asset(declaration, descriptor)
        })
    }
    /// A trusted bridge supplies canonical metadata only after authenticating
    /// the actual session/declaration/claim. This method confers no app grant.
    pub fn propose_asset(
        &self,
        declaration: &str,
        descriptor: Descriptor,
    ) -> Result<Reference, Error> {
        let mut working = self.inner.working_of(self.id)?;
        let Working { registry, open, .. } = &mut *working;
        let open = open.as_mut().ok_or(Error::Ended)?;
        if open.prepared {
            return Err(Error::Ended);
        }
        let manifest = self
            .inner
            .host
            .uploads()
            .ok_or_else(|| Error::Input("uploads are unbound".into()))?;
        let draft = open.asset_draft.as_mut().ok_or(Error::Ended)?;
        self.inner.asset_operations.fetch_add(1, Ordering::SeqCst);
        registry
            .propose(manifest, draft, declaration, descriptor)
            .map_err(Error::Input)
    }
}
impl Prepared {
    /// Final net dispositions for storage/lease preparation. These are plans,
    /// not evidence that storage was kept or that a durable record committed.
    pub fn asset_plans(&self) -> Result<Vec<GenerationPlan>, Error> {
        let working = self.command.inner.working_of(self.command.id)?;
        let open = working.open.as_ref().ok_or(Error::Ended)?;
        Ok(open
            .asset_patch
            .as_ref()
            .map(|p| p.plans().to_vec())
            .unwrap_or_default())
    }
}

/// Each touched key contributes its original/final declared asset fields,
/// independently of whether the row's final payload equals its original.
pub(super) fn uses(
    shape: &Shape,
    tables: &[Data],
    touched: &BTreeMap<(usize, Key), Option<Row>>,
) -> Result<Vec<ReferenceUse>, Error> {
    let mut uses = Vec::new();
    for ((at, key), before) in touched {
        for asset in &shape.tables[*at].assets {
            for (row, before, after) in [(before.as_ref(), 1, 0), (tables[*at].rows.get(key), 0, 1)]
            {
                let Some(row) = row else { continue };
                let value = row
                    .get(&asset.field)
                    .ok_or_else(|| Error::Input("row has no declared asset field".into()))?;
                if value.is_null() {
                    continue;
                }
                let reference: Reference = serde_json::from_value(value.clone())
                    .map_err(|e| Error::Input(format!("asset descriptor: {e}")))?;
                uses.push(ReferenceUse {
                    declaration: asset.upload.clone(),
                    reference,
                    before,
                    after,
                });
            }
        }
    }
    Ok(uses)
}
