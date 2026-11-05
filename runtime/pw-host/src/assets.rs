//! Generic upload declaration identity and data references; no application writes.
use crate::tables::{Field, Stored, closed_case_tags, wit_name};
use crate::{ComponentContract, Granted, ImportKind};
use serde::{Deserialize, Serialize};
mod memory;
pub use memory::{Registry, RegistryDraft, RegistryPatch};

mod registry;
pub use registry::{
    Descriptor, Generation, GenerationPlan, GenerationState, Publication, ReferenceUse,
    plan_generation,
};

use std::collections::{BTreeMap, BTreeSet};

/// Mirrored from the compiler upload declaration ceiling; cross-crate witness pins it.
pub const MOST_BYTES: u64 = 100_000_000;

/// A declaration read only through a validated manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Declaration {
    name: String,
    module: Option<String>,
    source: Option<String>,
    route: String,
    serves: String,
    max_bytes: u64,
    types: Vec<String>,
    max_width: u32,
    max_height: u32,
}
impl Declaration {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn source(&self) -> &str {
        self.source.as_deref().expect("validated source")
    }
    pub fn route(&self) -> &str {
        &self.route
    }
    pub fn serves(&self) -> &str {
        &self.serves
    }
    pub fn kinds(&self) -> &[String] {
        &self.types
    }
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }
    pub fn max_width(&self) -> u32 {
        self.max_width
    }
    pub fn max_height(&self) -> u32 {
        self.max_height
    }
    pub fn interface(&self) -> String {
        format!("pw:uploads/{}", wit_name(&self.name))
    }
    pub fn validate_reference(&self, reference: &Reference) -> Result<(), String> {
        reference.whole()?;
        if !self.types.contains(&reference.kind)
            || reference.width > self.max_width
            || reference.height > self.max_height
        {
            return Err(format!(
                "{} reference exceeds its declared kind or dimensions",
                self.name
            ));
        }
        Ok(())
    }
    fn whole(&self) -> Result<(), String> {
        if !identifier(&self.name)
            || self.module.as_deref() != Some(&self.name)
            || !self.source.as_deref().is_some_and(identifier)
        {
            return Err(format!(
                "{} is not a generic upload in its own module and explicit source",
                self.name
            ));
        }
        if !path(&self.route) || !path(&self.serves) || self.route == self.serves {
            return Err(format!(
                "{} has ambiguous or invalid upload paths",
                self.name
            ));
        }
        let distinct = self.types.iter().collect::<BTreeSet<_>>();
        if self.max_bytes == 0
            || self.max_bytes > MOST_BYTES
            || self.max_width == 0
            || self.max_height == 0
            || distinct.is_empty()
            || distinct.len() != self.types.len()
            || self
                .types
                .iter()
                .any(|t| !matches!(t.as_str(), "png" | "jpeg" | "webp" | "gif"))
        {
            return Err(format!("{} has invalid upload limits or kinds", self.name));
        }
        Ok(())
    }
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|part| {
            part.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}
fn path(s: &str) -> bool {
    s.len() > 1
        && s.starts_with('/')
        && !s.ends_with('/')
        && s[1..].split('/').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        })
}

/// Runtime-checked declarations, indexed by emitted interface identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    declarations: BTreeMap<String, Declaration>,
}
impl Manifest {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let declarations: Vec<Declaration> =
            serde_json::from_str(text).map_err(|e| format!("uploads.json: {e}"))?;
        let mut indexed = BTreeMap::new();
        let mut paths = BTreeSet::new();
        let mut names = BTreeSet::new();
        for declaration in declarations {
            declaration.whole()?;
            if !names.insert(declaration.name.clone())
                || indexed.contains_key(&declaration.interface())
            {
                return Err("uploads.json repeats an upload declaration identity".into());
            }
            // Route and serving prefixes must identify one declaration, even
            // where a forged manifest bypassed the compiler's path checks.
            for prefix in [&declaration.route, &declaration.serves] {
                if paths.iter().any(|old: &String| {
                    old == prefix
                        || prefix.starts_with(&format!("{old}/"))
                        || old.starts_with(&format!("{prefix}/"))
                }) {
                    return Err("uploads.json has overlapping upload paths".into());
                }
                paths.insert(prefix.clone());
            }
            indexed.insert(declaration.interface(), declaration);
        }
        Ok(Self {
            declarations: indexed,
        })
    }
    pub fn declarations(&self) -> impl Iterator<Item = &Declaration> {
        self.declarations.values()
    }
    pub fn declaration(&self, name: &str) -> Option<&Declaration> {
        self.declarations.values().find(|d| d.name == name)
    }
    pub fn access(&self, interface: &str, function: &str) -> Result<Access, String> {
        let declaration = self
            .declarations
            .get(interface)
            .ok_or_else(|| format!("upload gate: no declaration for {interface}"))?;
        let operation = match function {
            "attached" => Operation::Attached,
            "claim" => Operation::Claim,
            "publish" => Operation::Publish,
            "discard" => Operation::Discard,
            _ => return Err(format!("upload gate: no operation {interface}#{function}")),
        };
        Ok(Access {
            declaration: declaration.clone(),
            operation,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Attached,
    Claim,
    Publish,
    Discard,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    declaration: Declaration,
    operation: Operation,
}
impl Access {
    pub fn declaration(&self) -> &Declaration {
        &self.declaration
    }
    pub fn operation(&self) -> Operation {
        self.operation
    }
    pub fn capabilities(&self) -> Vec<String> {
        let name = self.declaration.name();
        let mut caps = vec![format!(
            "database.{}<{name}>",
            if matches!(self.operation, Operation::Publish | Operation::Discard) {
                "write"
            } else {
                "read"
            }
        )];
        if self.operation != Operation::Attached {
            caps.push(format!(
                "resource.{}<{name}.Claim>",
                if self.operation == Operation::Claim {
                    "acquire"
                } else {
                    "release"
                }
            ));
        }
        caps
    }
}
pub fn is_upload_interface(interface: &str) -> bool {
    interface == "pw:uploads"
        || interface.starts_with("pw:uploads/")
        || interface.starts_with("pw:uploads@")
}

/// Every declared import, including an unused one, has both grant axes.
pub fn gate_links(
    contract: &ComponentContract,
    granted: &Granted,
    manifest: Option<&Manifest>,
    command: bool,
) -> Result<BTreeMap<String, Access>, String> {
    let mut functions = BTreeMap::new();
    for import in contract
        .imports
        .iter()
        .filter(|i| is_upload_interface(&i.interface))
    {
        let manifest = manifest.ok_or("upload gate: generic uploads are not bound")?;
        let access = manifest.access(&import.interface, &import.name)?;
        if import.kind != ImportKind::HostCapability {
            return Err("upload gate: operation is not a platform import".into());
        }
        if access.operation != Operation::Attached && !command {
            return Err("upload gate: claims link only into commands".into());
        }
        let required = access.capabilities();
        if !required.contains(&import.capability) {
            return Err("upload gate: import claims another declaration's capability".into());
        }
        for capability in required {
            if !contract
                .required_capabilities
                .iter()
                .any(|c| c.name() == capability)
                || granted.handle(&capability).is_none()
            {
                return Err(format!(
                    "upload gate: {capability} is required in the contract and granted here"
                ));
            }
        }
        if functions.insert(import.key(), access).is_some() {
            return Err("upload gate: operation imported twice".into());
        }
    }
    Ok(functions)
}

/// A descriptor is data, never proof of a lease or an adoptable live generation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub key: String,
    pub generation: String,
    pub kind: String,
    pub width: u32,
    pub height: u32,
}
impl Reference {
    pub fn from_value(value: &serde_json::Value) -> Result<Self, String> {
        let reference: Self = serde_json::from_value(value.clone())
            .map_err(|e| format!("invalid asset reference: {e}"))?;
        reference.whole()?;
        Ok(reference)
    }
    fn whole(&self) -> Result<(), String> {
        let hex = |s: &str, n| {
            s.len() == n
                && s.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        };
        if !hex(&self.key, 64)
            || !hex(&self.generation, 32)
            || !matches!(self.kind.as_str(), "png" | "jpeg" | "webp" | "gif")
            || self.width == 0
            || self.height == 0
        {
            return Err("asset reference has invalid hash, generation, kind or dimensions".into());
        }
        Ok(())
    }
}

/// Exact generated Ref or one Option<Ref>; nominal identity is checked by the compiler.
pub(crate) fn reference_shape(stored: &Stored, declaration: &Declaration) -> bool {
    let stored = match stored {
        Stored::Option(inner) => inner.as_ref(),
        other => other,
    };
    let Stored::Record(fields) = stored else {
        return false;
    };
    let plain_string = |s: &Stored| {
        matches!(
            s,
            Stored::String {
                min_length: None,
                max_length: None
            }
        )
    };
    let plain_int = |s: &Stored| {
        matches!(
            s,
            Stored::Int {
                at_least: None,
                at_most: None
            }
        )
    };
    matches!(fields.as_slice(),[Field{name:key,stored:k},Field{name:generation,stored:g},Field{name:kind,stored:Stored::Cases(cases)},Field{name:width,stored:w},Field{name:height,stored:h}]
        if key=="key"&&generation=="generation"&&kind=="kind"&&width=="width"&&height=="height"&&plain_string(k)&&plain_string(g)&&plain_int(w)&&plain_int(h)
        && closed_case_tags(cases.iter().map(|c|(c.name.as_str(),c.carries.len()))).as_deref()==Some(declaration.kinds()))
}
