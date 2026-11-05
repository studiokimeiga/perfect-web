//! Pure transitions of one immutable generation; adapters own atomic persistence.
use super::{Manifest, Reference};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    pub reference: Reference,
    pub bytes: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationState {
    Live,
    Tombstone,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generation {
    pub descriptor: Descriptor,
    pub state: GenerationState,
    pub references: BTreeMap<String, u64>,
}
impl Generation {
    pub fn live_reference(&self, declaration: &str) -> Option<&Reference> {
        (self.state == GenerationState::Live
            && self.references.get(declaration).is_some_and(|n| *n > 0))
        .then_some(&self.descriptor.reference)
    }
}
/// A transaction-owned proposal, supplied AFTER the lease bridge authenticates
/// session/declaration/claim and incoming-file policy. No token is persisted here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publication {
    pub declaration: String,
    pub descriptor: Descriptor,
}
/// Original and final uses in touched rows; before/after come from the adapter,
/// not from an application's asserted reference count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceUse {
    pub declaration: String,
    pub reference: Reference,
    pub before: u64,
    pub after: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationPlan {
    pub generation: Generation,
    pub keep_new: bool,
    pub collect: bool,
    pub used_publications: BTreeSet<String>,
}

/// Plan one generation from one committed snapshot and final touched-row uses.
/// `bytes` counts canonical stored bytes; incoming-file admission has already
/// happened at the authenticated lease bridge, before canonical rewriting.
pub fn plan_generation(
    manifest: &Manifest,
    existing: Option<&Generation>,
    publications: &[Publication],
    uses: &[ReferenceUse],
) -> Result<GenerationPlan, String> {
    let descriptor = existing
        .map(|g| &g.descriptor)
        .or_else(|| publications.first().map(|p| &p.descriptor))
        .ok_or("asset reference names an unknown generation")?
        .clone();
    descriptor.reference.whole()?;
    if descriptor.bytes == 0 {
        return Err("an immutable descriptor has no stored bytes".into());
    }
    let mut sources = BTreeSet::new();
    let mut check_declaration = |name: &str| -> Result<(), String> {
        let declaration = manifest
            .declaration(name)
            .ok_or_else(|| format!("asset generation names unknown declaration {name}"))?;
        declaration.validate_reference(&descriptor.reference)?;
        sources.insert(declaration.source().to_string());
        Ok(())
    };
    let old = existing.map(|g| g.references.clone()).unwrap_or_default();
    if let Some(generation) = existing {
        match generation.state {
            GenerationState::Live if old.is_empty() => {
                return Err("LIVE generation has no references".into());
            }
            GenerationState::Tombstone if !old.is_empty() => {
                return Err("retired generation still has references".into());
            }
            _ => {}
        }
        for (declaration, count) in &old {
            if *count == 0 {
                return Err("generation stores a zero reference count".into());
            }
            check_declaration(declaration)?;
        }
    }
    let retired = existing.is_some_and(|g| g.state == GenerationState::Tombstone);
    let mut fresh = BTreeSet::new();
    for publication in publications {
        if retired {
            return Err("a retired generation identity cannot be published again".into());
        }
        if publication.descriptor != descriptor {
            return Err("publication changes an immutable generation descriptor".into());
        }
        if !fresh.insert(publication.declaration.clone()) {
            return Err("one declaration publishes its lease twice".into());
        }
        check_declaration(&publication.declaration)?;
    }
    let mut deltas: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut final_uses = BTreeSet::new();
    for usage in uses {
        if usage.reference != descriptor.reference {
            return Err("row reference differs from the immutable generation descriptor".into());
        }
        check_declaration(&usage.declaration)?;
        if usage.after > 0 {
            let committed = !retired && old.get(&usage.declaration).is_some_and(|n| *n > 0);
            if !committed && !fresh.contains(&usage.declaration) {
                return Err(
                    "reference is not LIVE for this declaration and has no fresh publication"
                        .into(),
                );
            }
            final_uses.insert(usage.declaration.clone());
        }
        let (before, after) = deltas.entry(usage.declaration.clone()).or_default();
        *before = before
            .checked_add(usage.before)
            .ok_or("original reference uses overflow")?;
        *after = after
            .checked_add(usage.after)
            .ok_or("final reference uses overflow")?;
    }
    if sources.len() > 1 {
        return Err("one generation cannot cross declared data sources".into());
    }
    let mut references = old;
    for (declaration, (before, after)) in deltas {
        let committed = references.get(&declaration).copied().unwrap_or(0);
        let final_count = committed
            .checked_sub(before)
            .ok_or("original uses exceed committed references")?
            .checked_add(after)
            .ok_or("reference count overflows")?;
        if final_count == 0 {
            references.remove(&declaration);
        } else {
            references.insert(declaration, final_count);
        }
    }
    let live = !references.is_empty();
    let used_publications = fresh.intersection(&final_uses).cloned().collect();
    Ok(GenerationPlan {
        generation: Generation {
            descriptor,
            state: if live {
                GenerationState::Live
            } else {
                GenerationState::Tombstone
            },
            references,
        },
        keep_new: existing.is_none() && live,
        collect: existing.is_some_and(|g| g.state == GenerationState::Live) && !live,
        used_publications,
    })
}
