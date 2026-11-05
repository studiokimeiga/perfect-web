//! Indexed registry state and atomic patches, used by the memory adapter.
//! This owns no storage or lease and publishes nothing until `apply`.
use super::{
    Descriptor, Generation, GenerationPlan, GenerationState, Manifest, Publication, Reference,
    ReferenceUse, plan_generation,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub struct Registry {
    revision: u64,
    generations: BTreeMap<String, Generation>,
    current: BTreeMap<String, String>,
    collections: BTreeSet<String>,
}
#[derive(Debug)]
pub struct RegistryDraft {
    revision: u64,
    publications: BTreeMap<String, Publication>,
    candidates: BTreeMap<String, Descriptor>,
}
#[derive(Debug)]
pub struct RegistryPatch {
    revision: u64,
    plans: Vec<GenerationPlan>,
}
impl RegistryPatch {
    pub fn plans(&self) -> &[GenerationPlan] {
        &self.plans
    }
}
impl Registry {
    pub fn begin(&self) -> RegistryDraft {
        RegistryDraft {
            revision: self.revision,
            publications: BTreeMap::new(),
            candidates: BTreeMap::new(),
        }
    }
    pub fn generation(&self, generation: &str) -> Option<&Generation> {
        self.generations.get(generation)
    }
    pub fn current_reference(
        &self,
        declaration: &str,
        key: &str,
        kind: &str,
    ) -> Option<&Reference> {
        let generation = self.generations.get(self.current.get(key)?)?;
        generation
            .live_reference(declaration)
            .filter(|reference| reference.kind == kind)
    }
    /// Select the currently LIVE physical generation or reserve a fresh
    /// transaction-local candidate. Independent declarations keep their own
    /// proposals even when their canonical bytes share the chosen generation.
    pub fn propose(
        &self,
        manifest: &Manifest,
        draft: &mut RegistryDraft,
        declaration: &str,
        incoming: Descriptor,
    ) -> Result<Reference, String> {
        self.at_revision(draft.revision)?;
        let declared = manifest
            .declaration(declaration)
            .ok_or("unknown publication declaration")?;
        declared.validate_reference(&incoming.reference)?;
        if incoming.bytes == 0 {
            return Err("publication has no canonical bytes".into());
        }
        if draft.publications.contains_key(declaration) {
            return Err("one declaration publishes its lease twice".into());
        }
        let key = &incoming.reference.key;
        let selected = if let Some(generation) = self.current.get(key) {
            let entry = self
                .generations
                .get(generation)
                .ok_or("current generation is missing")?;
            if entry.state != GenerationState::Live {
                return Err("current generation is retired".into());
            }
            Some(&entry.descriptor)
        } else {
            draft.candidates.get(key)
        };
        let descriptor = if let Some(selected) = selected {
            let mut same = incoming.clone();
            same.reference.generation = selected.reference.generation.clone();
            if &same != selected {
                return Err("canonical bytes change an immutable descriptor".into());
            }
            selected.clone()
        } else {
            let generation = &incoming.reference.generation;
            if self.generations.contains_key(generation)
                || draft
                    .candidates
                    .values()
                    .any(|d| &d.reference.generation == generation)
            {
                return Err("generation identity has already been reserved".into());
            }
            incoming
        };
        draft
            .candidates
            .entry(descriptor.reference.key.clone())
            .or_insert_with(|| descriptor.clone());
        draft.publications.insert(
            declaration.into(),
            Publication {
                declaration: declaration.into(),
                descriptor: descriptor.clone(),
            },
        );
        Ok(descriptor.reference)
    }
    pub fn prepare(
        &self,
        manifest: &Manifest,
        draft: RegistryDraft,
        uses: &[ReferenceUse],
    ) -> Result<RegistryPatch, String> {
        self.at_revision(draft.revision)?;
        type Group = (Vec<Publication>, Vec<ReferenceUse>);
        let mut groups: BTreeMap<String, Group> = BTreeMap::new();
        for publication in draft.publications.into_values() {
            groups
                .entry(publication.descriptor.reference.generation.clone())
                .or_default()
                .0
                .push(publication);
        }
        for usage in uses {
            groups
                .entry(usage.reference.generation.clone())
                .or_default()
                .1
                .push(usage.clone());
        }
        let plans = groups
            .into_iter()
            .map(|(generation, (publications, uses))| {
                plan_generation(
                    manifest,
                    self.generations.get(&generation),
                    &publications,
                    &uses,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let retiring = plans
            .iter()
            .filter(|p| p.generation.state == GenerationState::Tombstone)
            .map(|p| p.generation.descriptor.reference.generation.as_str())
            .collect::<BTreeSet<_>>();
        for plan in &plans {
            if plan.generation.state == GenerationState::Live {
                let reference = &plan.generation.descriptor.reference;
                if self.current.get(&reference.key).is_some_and(|current| {
                    current != &reference.generation && !retiring.contains(current.as_str())
                }) {
                    return Err("two LIVE generations would name the same bytes".into());
                }
            }
        }
        Ok(RegistryPatch {
            revision: draft.revision,
            plans,
        })
    }
    /// Publish only as the enclosing adapter publishes its rows/records. All
    /// fallible checks precede mutation, so a refused/stale patch changes none.
    pub fn apply(&mut self, patch: RegistryPatch) -> Result<(), String> {
        self.at_revision(patch.revision)?;
        if patch.plans.is_empty() {
            return Ok(());
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or("registry revision exhausted")?;
        for plan in &patch.plans {
            let reference = &plan.generation.descriptor.reference;
            if plan.generation.state == GenerationState::Tombstone
                && self.current.get(&reference.key) == Some(&reference.generation)
            {
                self.current.remove(&reference.key);
            }
        }
        for plan in patch.plans {
            let reference = &plan.generation.descriptor.reference;
            if plan.generation.state == GenerationState::Live {
                self.current
                    .insert(reference.key.clone(), reference.generation.clone());
            }
            if plan.collect {
                self.collections.insert(reference.generation.clone());
            }
            self.generations
                .insert(reference.generation.clone(), plan.generation);
        }
        self.revision = next;
        Ok(())
    }
    pub fn pending_collections(&self) -> impl Iterator<Item = &Descriptor> {
        self.collections
            .iter()
            .filter_map(|generation| self.generations.get(generation).map(|g| &g.descriptor))
    }
    /// Completion affects the captured job only. The retained tombstone
    /// reserves its identity; another generation's current pointer is untouched.
    pub fn collected(&mut self, generation: &str) -> Result<(), String> {
        let entry = self
            .generations
            .get(generation)
            .ok_or("unknown collection generation")?;
        if entry.state != GenerationState::Tombstone || !entry.references.is_empty() {
            return Err("a LIVE generation cannot be collected".into());
        }
        self.collections.remove(generation);
        Ok(())
    }
    fn at_revision(&self, revision: u64) -> Result<(), String> {
        if revision != self.revision {
            return Err("registry snapshot changed before publication".into());
        }
        Ok(())
    }
}
