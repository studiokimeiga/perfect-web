//! The compatibility decision, compiled for the browser.
//!
//! One implementation, not two. `runtime/pw-resume` decides; this exposes that
//! decision to a page as `wasm32-unknown-unknown` with a hand-rolled ABI — no
//! `wasm-bindgen`, because the project pins its dependency set and the
//! interface is four integers and two byte buffers.
//!
//! # Why not a JavaScript port
//!
//! A second implementation of a security decision is two things that can
//! disagree, and the disagreement is silent: both sides return a boolean and
//! only one of them is right. The whole reason `decide` returns a typed
//! `Decision` is so the answer carries its reason, and a port would carry a
//! different one.
//!
//! # The ABI
//!
//! ```text
//! alloc(len)                -> ptr    the caller writes the manifest here
//! decide_manifest(ptr, len) -> code   0 resume, 1 migrate, 2.. a refusal
//! last_recovery()           -> code   which recovery the refusal chose
//! ```
//!
//! The manifest is the same `|`-separated encoding the fuzzer's harness uses,
//! so a browser input and a fuzz input are the same bytes.
//!
//! The decision's causal trace is not exported. No page read it, and this
//! build compiles none of its words (`pw-resume`'s `trace` feature, off
//! here), which were a tenth of every page's decision. A refusal is told by
//! its code, which `pleris.dev/e/PWR<code>` explains, and by its recovery
//! (ADR-0400).

use std::sync::Mutex;

use pw_resume::*;

static RECOVERY: Mutex<u32> = Mutex::new(0);

/// **The handlers this build compiled**, each by its identity and the
/// capture schema its document presents (ADR-0132). Until 2026-10-02 this was
/// the store page's two handlers, by name, written here; any other handler a
/// program declared built, and its button never attached.
static KNOWN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// **Tell the decision what this build compiled**: one `identity|capture`
/// line per handler, as the server's build lists it. Replaces what was
/// known, and returns how many handlers that is.
///
/// From the build the runtime was served with, never from the document: a
/// document cached from an older build names handlers this build may not
/// have, and the decision is what refuses them.
///
/// # Safety
///
/// `ptr` must point at `len` bytes the caller obtained from [`alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn know(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let text = String::from_utf8_lossy(bytes);
    let mut known = KNOWN.lock().unwrap();
    known.clear();
    // A `#` line is the table's word on the build and its pages
    // (ADR-0300), never a handler: no identity begins with one.
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (identity, capture) = line.split_once('|').unwrap_or((line, ""));
        known.push((identity.to_string(), capture.to_string()));
    }
    known.len() as u32
}

/// **What this build says of the page's documents** (ADR-0300): their
/// document schema and their scope, from the handler table the runtime was
/// served with, never from the document. `None` until told.
static DOCUMENT: Mutex<Option<(String, String)>> = Mutex::new(None);

/// **Tell the decision what this build says of this page's documents**:
/// `schema|scope`, as the handler table's `#page` line gives them for the
/// page the document is. Replaces what was known; returns 1.
///
/// Told nothing, a document's schema and scope are not compared, as no
/// build has spoken for them.
///
/// # Safety
///
/// `ptr` must point at `len` bytes the caller obtained from [`alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn know_document(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let text = String::from_utf8_lossy(bytes);
    let (schema, scope) = text.split_once('|').unwrap_or((&text, "public"));
    *DOCUMENT.lock().unwrap() = Some((schema.to_string(), scope.trim().to_string()));
    1
}

/// A scope as a manifest or the table writes it: `public`, `session:<id>`,
/// `user:<id>`, and anything else an organization's.
fn scope_of(s: &str) -> PrivacyScope {
    match s {
        "public" => PrivacyScope::Public,
        s if s.starts_with("session:") => PrivacyScope::Session(s[8..].to_string()),
        s if s.starts_with("user:") => PrivacyScope::User(s[5..].to_string()),
        s => PrivacyScope::Organization(s.to_string()),
    }
}

/// Bytes the caller may write a manifest into. Leaked deliberately: a page
/// makes a handful of decisions, and a free() across the ABI is more ways to
/// be wrong than it is worth.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = vec![0u8; len];
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// 0 resume · 1 migrate · 2 unsupported scheme · 3 unsupported ABI ·
/// 4 unknown handler · 5 privacy widened · 6 document schema · 7 no migration ·
/// 8 migration failed · 9 malformed · 10 mixed build
/// # Safety
///
/// `ptr` must point at `len` bytes the caller obtained from [`alloc`]. The ABI
/// is two integers wide on purpose — there is nothing here for a caller to get
/// subtly wrong except the length, and a wrong length is a wrong manifest,
/// which the decision refuses rather than trusts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn decide_manifest(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let text = String::from_utf8_lossy(bytes).to_string();
    let (entry, rt, construct) = parse(&text);

    let d = decide(&entry, &rt, construct);

    // The authorisation is taken here, inside the same call, so a page cannot
    // attach without it: the ABI has no way to produce an `Authorised` and no
    // way to attach except through a `Resume`/`Migrate` answer.
    let code = match &d {
        Decision::Resume => {
            let _ = authorise(&entry, &d).map(attach);
            0
        }
        Decision::Migrate { .. } => {
            let _ = authorise(&entry, &d).map(attach);
            1
        }
        Decision::Refuse { why, recovery, .. } => {
            *RECOVERY.lock().unwrap() = match recovery {
                Recovery::RefetchRegion => 0,
                Recovery::RerenderPrivateSlot => 1,
                Recovery::ReloadDocument => 2,
                Recovery::RetryInteraction => 3,
                Recovery::RequireUserConfirmation => 4,
                Recovery::RejectIrrecoverable => 5,
            };
            match why {
                Refusal::UnsupportedHashScheme(_) => 2,
                Refusal::UnsupportedAbi(_) => 3,
                Refusal::UnknownHandler(_) => 4,
                Refusal::PrivacyWidened { .. } => 5,
                Refusal::DocumentSchemaMismatch => 6,
                Refusal::NoMigration { .. } => 7,
                Refusal::MigrationFailed(_) => 8,
                Refusal::MalformedManifest(_) => 9,
                Refusal::MixedBuild { .. } => 10,
                Refusal::CaptureSchemaMismatch { .. } => 7,
            }
        }
    };
    if d.attaches() {
        *RECOVERY.lock().unwrap() = u32::MAX;
    }
    code
}

#[unsafe(no_mangle)]
pub extern "C" fn last_recovery() -> u32 {
    *RECOVERY.lock().unwrap()
}

/// `scheme|abi|build|handler|capture|document|scope|captures|construct`
fn parse(text: &str) -> (ResumeEntry, Runtime, Construct) {
    let f: Vec<&str> = text.split('|').collect();
    let field = |i: usize| f.get(i).copied().unwrap_or_default();
    // The schema of NOTHING is the schema of no fields, not the schema of one
    // field named "". `decide` refuses a manifest that carries no capture
    // bytes under a non-empty schema — correctly — so a handler that captures
    // nothing was malformed by construction, and the only symptom was a button
    // that never attached.
    let schema = |s: &str| {
        if s.is_empty() {
            SchemaHash::of_fields(&[])
        } else {
            SchemaHash::of_fields(&[(s, "T")])
        }
    };

    let scheme = HashScheme(field(0).parse().unwrap_or(0));
    let abi = PlatformAbi(field(1).parse().unwrap_or(0));
    let scope = scope_of(field(6));
    let handler = HandlerId::derive(
        &ImplementationHash::new(scheme, field(3)),
        &DependencySet::of(&[["app", "store", "Term", field(3), "r1"]]),
        &schema(field(4)),
        &abi,
    );
    // What this build knows: each handler it compiled, by identity, at
    // scheme 2 (ADR-0132). E7-L's claim is that the exact handler is loaded,
    // which a table of every compiled handler keeps: one identity among many
    // is authorised only by its own. A handler that captures nothing has the
    // schema of nothing, which is still a schema.
    let known = |name: &str, capture: &str| {
        (
            HandlerId::derive(
                &ImplementationHash::new(CURRENT_SCHEME, name),
                &DependencySet::of(&[["app", "store", "Term", name, "r1"]]),
                &schema(capture),
                &PlatformAbi(1),
            ),
            schema(capture),
        )
    };
    let entry = ResumeEntry {
        hash_scheme: scheme,
        platform_abi: abi,
        application_build: BuildId(field(2).to_string()),
        handler,
        capture_schema: schema(field(4)),
        document_schema: schema(field(5)),
        privacy_scope: scope,
        captures: field(7).as_bytes().to_vec(),
    };
    let told = DOCUMENT.lock().unwrap().clone();
    let rt = Runtime {
        abi: vec![PlatformAbi(1)],
        build: Some(BuildId("B1".into())),
        handlers: KNOWN
            .lock()
            .unwrap()
            .iter()
            .map(|(identity, capture)| known(identity, capture))
            .collect(),
        // What the build says of this page's documents (ADR-0300), and
        // nothing where it has said nothing. Until ADR-0300 the constants
        // `cart-doc` and `Public`, which every manifest also said.
        document_schema: told.as_ref().map(|(s, _)| schema(s)),
        scope: told.as_ref().map(|(_, scope)| scope_of(scope)),
        migrations: Vec::new(),
    };
    let construct = match field(8) {
        "private" => Construct::PrivateSlot,
        "command" => Construct::PendingCommand,
        "resource" => Construct::OpenResource,
        _ => Construct::PublicRegion,
    };
    (entry, rt, construct)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One decision over `manifest`, as the browser asks it.
    fn decide_text(manifest: &str) -> u32 {
        unsafe { decide_manifest(manifest.as_ptr(), manifest.len()) }
    }

    fn know_text(table: &str) -> u32 {
        unsafe { know(table.as_ptr(), table.len()) }
    }

    /// `scheme|abi|build|handler|capture|document|scope|captures|construct`
    fn manifest(handler: &str, capture: &str) -> String {
        let bytes = if capture.is_empty() { "" } else { "x" };
        format!("2|1|B1|{handler}|{capture}|cart-doc|public|{bytes}|region")
    }

    /// ADR-0132, in one test because the table is one process's: what the
    /// decision knows is what it was told the build compiled, and nothing
    /// else.
    #[test]
    fn the_decision_knows_what_the_build_compiled_and_nothing_else() {
        // Told nothing: every handler is refused as unknown, the closed side.
        assert_eq!(know_text(""), 0);
        assert_eq!(decide_text(&manifest("e1ab9fca1f6fc15b", "item.id")), 4);

        // Told the build's table: its handlers resume, by identity and capture.
        assert_eq!(
            know_text("e1ab9fca1f6fc15b|item.id\n5e53c6a9aaee307a|\n"),
            2
        );
        assert_eq!(decide_text(&manifest("e1ab9fca1f6fc15b", "item.id")), 0);
        assert_eq!(decide_text(&manifest("5e53c6a9aaee307a", "")), 0);

        // An identity the build lacks is refused, however it is presented.
        assert_eq!(decide_text(&manifest("ffffffffffffffff", "item.id")), 4);
        // And so is a name: the store's handlers are not known by name.
        assert_eq!(decide_text(&manifest("add_to_cart", "cart")), 4);

        // Told again, the table is replaced, not added to.
        assert_eq!(know_text("5e53c6a9aaee307a|\n"), 1);
        assert_eq!(decide_text(&manifest("e1ab9fca1f6fc15b", "item.id")), 4);

        // A table's `#` lines are no handlers (ADR-0300).
        assert_eq!(
            know_text("#build|b1\n#page|t.P|s1|public\n5e53c6a9aaee307a|\n"),
            1
        );

        // ADR-0300: what the build says of the page's documents, compared.
        // Told nothing, neither is: the manifest's own words pass.
        let as_page = |document: &str, scope: &str| {
            format!("2|1|b1|5e53c6a9aaee307a||{document}|{scope}||region")
        };
        assert_eq!(decide_text(&as_page("cart-doc", "public")), 0);
        // Told a public page of schema `s1`: its documents resume.
        assert_eq!(unsafe { know_document(b"s1|public".as_ptr(), 9) }, 1);
        assert_eq!(decide_text(&as_page("s1", "public")), 0);
        // A document of another schema is refused (6), its parts elsewhere.
        assert_eq!(decide_text(&as_page("s2", "public")), 6);
        // A session's document's handler, in a public page, is refused (5).
        assert_eq!(decide_text(&as_page("s1", "session:")), 5);
        // Told a session page: a session's document and a public one resume.
        assert_eq!(unsafe { know_document(b"s1|session:".as_ptr(), 11) }, 1);
        assert_eq!(decide_text(&as_page("s1", "session:")), 0);
        assert_eq!(decide_text(&as_page("s1", "public")), 0);
        // And a user's does not flow into a session's page.
        assert_eq!(decide_text(&as_page("s1", "user:")), 5);
    }
}
