//! **The server's claim bridge** (ADR-0440, L5b): a compiled program's upload
//! imports, bound for one command run to its invocation's ledger, and for a
//! query to its session's own leases. The engine holds each import to its
//! declared ABI and its grants (`pw_host::assets::gate_links`) before any of
//! these is called; what may be claimed, and in which session, the ledger
//! decides, and a handle that is not the request's session's is refused.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use pw_host::assets::{Declaration, Manifest, Reference};
use pw_host::engine::{HostFn, Val};

use super::generic::Invocation;
use super::{Leases, Shown};
use crate::data::{Requested, UploadPublisher};
use crate::identity::ReaderIdentity;

/// The operations an upload's interface exports to a program.
const OPERATIONS: [&str; 4] = ["claim", "publish", "discard", "attached"];

fn key(declaration: &Declaration, operation: &str) -> String {
    format!("{}#{operation}", declaration.interface())
}

fn text(args: &[Val], at: usize, what: &str) -> Result<String, String> {
    match args.get(at) {
        Some(Val::String(value)) => Ok(value.clone()),
        _ => Err(format!("{what} is given no text at {at}")),
    }
}

fn some(value: Val) -> Val {
    Val::Result(Ok(Some(Box::new(value))))
}

/// The upload's declared `NotFound`, as its import's ABI cases it.
fn not_found() -> Val {
    Val::Result(Err(Some(Box::new(Val::Variant("not-found".into(), None)))))
}

fn shown(shown: &Shown) -> Val {
    Val::Record(vec![
        ("src".into(), Val::String(shown.src.clone())),
        ("width".into(), Val::S64(i64::from(shown.width))),
        ("height".into(), Val::S64(i64::from(shown.height))),
    ])
}

fn reference(reference: &Reference) -> Val {
    Val::Record(vec![
        ("key".into(), Val::String(reference.key.clone())),
        ("generation".into(), Val::String(reference.generation.clone())),
        ("kind".into(), Val::Variant(reference.kind.clone(), None)),
        ("width".into(), Val::S64(i64::from(reference.width))),
        ("height".into(), Val::S64(i64::from(reference.height))),
    ])
}

fn held(invocation: &Mutex<Invocation>) -> Result<std::sync::MutexGuard<'_, Invocation>, String> {
    invocation
        .lock()
        .map_err(|_| "the upload invocation is unavailable".to_string())
}

/// **One command run's upload imports**, each declaration's, bound to its
/// invocation and to the layer's publication in the command's transaction.
pub(crate) fn command_ops(
    manifest: &Manifest,
    invocation: &Arc<Mutex<Invocation>>,
    publisher: &UploadPublisher,
) -> BTreeMap<String, HostFn> {
    let mut ops: BTreeMap<String, HostFn> = BTreeMap::new();
    for declaration in manifest.declarations() {
        let name = declaration.name().to_string();
        let (inv, marked, upload) = (Arc::clone(invocation), publisher.clone(), name.clone());
        ops.insert(
            key(declaration, "claim"),
            Arc::new(move |args: &[Val]| {
                let handle = text(args, 0, "upload.claim")?;
                match held(&inv)?.claim(&upload, &handle)? {
                    // A claim taken: the commit carries its record.
                    Some(token) => {
                        (marked.require_receipt)()?;
                        Ok(vec![some(Val::String(token))])
                    }
                    None => Ok(vec![not_found()]),
                }
            }),
        );
        let (inv, layer, upload) = (Arc::clone(invocation), publisher.clone(), name.clone());
        ops.insert(
            key(declaration, "publish"),
            Arc::new(move |args: &[Val]| {
                let handle = text(args, 0, "upload.publish")?;
                let token = text(args, 1, "upload.publish")?;
                let published = held(&inv)?.publish(&upload, &handle, &token, |d, desc, bytes| {
                    (layer.publish)(d, desc, bytes)
                })?;
                Ok(vec![published.as_ref().map_or_else(not_found, |r| some(reference(r)))])
            }),
        );
        let (inv, upload) = (Arc::clone(invocation), name.clone());
        ops.insert(
            key(declaration, "discard"),
            Arc::new(move |args: &[Val]| {
                let handle = text(args, 0, "upload.discard")?;
                let token = text(args, 1, "upload.discard")?;
                let discarded = held(&inv)?.discard(&upload, &handle, &token)?;
                Ok(vec![discarded.as_ref().map_or_else(not_found, |s| some(shown(s)))])
            }),
        );
        let (inv, upload) = (Arc::clone(invocation), name);
        ops.insert(
            key(declaration, "attached"),
            Arc::new(move |args: &[Val]| {
                let handle = text(args, 0, "upload.attached")?;
                let attached = held(&inv)?.attached(&upload, &handle)?;
                Ok(vec![Val::List(attached.iter().map(shown).collect())])
            }),
        );
    }
    ops
}

/// **A query's upload imports**: each declaration's `attached`, the
/// session's own live lease of it for the reader the query admitted.
pub(crate) fn query_ops(
    manifest: &Manifest,
    leases: &Leases,
    session: &str,
    reader: &ReaderIdentity,
) -> BTreeMap<String, HostFn> {
    let mut ops: BTreeMap<String, HostFn> = BTreeMap::new();
    for declaration in manifest.declarations() {
        let (leases, asked, reader, upload) = (
            Arc::clone(leases),
            Requested::by(session),
            reader.clone(),
            declaration.name().to_string(),
        );
        ops.insert(
            key(declaration, "attached"),
            Arc::new(move |args: &[Val]| {
                let handle = text(args, 0, "upload.attached")?;
                let cookie = asked.session("upload.attached", &handle)?;
                let attached = leases.shown_for(cookie, &upload, &reader);
                Ok(vec![Val::List(attached.iter().map(shown).collect())])
            }),
        );
    }
    ops
}

/// **The upload imports of a command no reader runs** (a Clock's): it
/// claims nothing, so each refuses, by name.
pub(crate) fn refusing_ops(manifest: &Manifest) -> BTreeMap<String, HostFn> {
    let mut ops: BTreeMap<String, HostFn> = BTreeMap::new();
    for declaration in manifest.declarations() {
        for operation in OPERATIONS {
            let what = format!("`{}`'s {operation}", declaration.name());
            ops.insert(
                key(declaration, operation),
                Arc::new(move |_: &[Val]| {
                    Err(format!("{what} is a reader's: a command no reader runs claims no upload"))
                }),
            );
        }
    }
    ops
}

#[cfg(test)]
mod tests;
