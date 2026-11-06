//! E7V — resume-version compatibility.
//!
//! Charter §8.5. A resume manifest attaches saved state to executable code.
//! `resume.rs` in the compiler already answers two of the three questions that
//! must hold before it may:
//!
//! ```text
//! 1. serializability   can this value be encoded?          compiler
//! 2. authority         may it cross this boundary?         compiler
//! 3. version           can THIS code safely interpret it?  here
//! ```
//!
//! Passing one does not imply passing the others. A `Cart` is perfectly
//! serializable and may not be in a public manifest; a public string is
//! serializable and unrestricted and may still be handed to a handler that no
//! longer means what it meant when the document was rendered.
//!
//! # The invariant
//!
//! > A resume manifest may attach state to executable code only when the
//! > platform has verified that the handler, captured data, document parts,
//! > privacy scope and runtime ABI are mutually compatible.
//!
//! # Why identity is content, not a path
//!
//! `checkout/add_to_cart` is a name. The name survives a rewrite of the
//! function it points at, so a manifest that trusts it will hand last week's
//! captures to code that reads them differently — and the failure is silent,
//! because both sides agree about the name. Identity is therefore derived from
//! the normalized implementation, its resolved references, its capture schema
//! and the platform ABI.
//!
//! **Code identity and capture-schema identity are separate.** Two handlers can
//! compile to identical behaviour while taking different captures, and one
//! handler can keep its capture schema while changing behaviour. Collapsing
//! them makes one of those two cases undetectable.
//!
//! # This version is strict on purpose
//!
//! No structural-similarity inference. Two records that look alike are not
//! evidence that one can be read as the other. Compatibility comes from an
//! exact identity match or from an **explicit, checked migration** tied to both
//! schema hashes. Everything else fails closed with a recovery action.
//!
//! # What this crate is not
//!
//! Behaviour, not a compile-time guarantee — like `pw-tasks` (ADR-0016) and
//! `pw-resource`. A passing test here does not make a mixed-build deployment a
//! compile error. The compiler's half of E7V is artifact agreement **within one
//! build**, which is a different question and lives in `pw-core`.

use std::collections::BTreeMap;
use std::fmt;

// --- identity ----------------------------------------------------------------

/// A content hash. Opaque on purpose: nothing may compare parts of one, or
/// order two, or infer proximity from a shared prefix.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash(String);

impl Hash {
    pub fn of(content: &str) -> Hash {
        // FNV-1a. Not cryptographic — this detects a version difference, not an
        // adversary — and it is stable across machines and Rust versions, which
        // `DefaultHasher` is not.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in content.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Hash(format!("{h:016x}"))
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The runtime ABI the manifest was written against.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlatformAbi(pub u32);

/// The coherent set of generated artifacts a document came from.
///
/// Content addressing identifies each *piece*; this identifies the *set*, which
/// is what a streamed patch must agree with.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BuildId(pub String);

/// A handler's content identity. See the module docs on why this is not a path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HandlerId(pub Hash);

/// The set of declarations a handler depends on.
///
/// **Order-insensitive**, and that is the whole reason it is a separate type.
/// `{Stores.get, Carts.add}` is the same dependency set however source
/// traversal happened to discover it, and hashing the discovery order would
/// make an unrelated edit reject every resume in the application.
///
/// Deduplicated, because repeated *use* is a property of the implementation
/// and is carried by [`ImplementationHash`] — which is order-SENSITIVE, since
/// `charge(); send_receipt()` is not `send_receipt(); charge()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencySet(Hash);

impl DependencySet {
    /// Each reference given as its canonical tuple:
    /// `(package, module, namespace, declaration, revision)`.
    pub fn of(references: &[[&str; 5]]) -> DependencySet {
        let mut canonical: Vec<String> = references.iter().map(|r| r.join("\u{3}")).collect();
        canonical.sort_unstable();
        canonical.dedup();
        DependencySet(Hash::of(&canonical.join("\u{2}")))
    }
}

/// Which algorithm produced an [`ImplementationHash`].
///
/// Manifests outlive deployments, so a manifest written under one scheme will
/// meet a runtime using another. Two digests of the same length from different
/// algorithms are not comparable, and comparing them anyway is how a
/// coincidence becomes an attachment.
///
/// ```text
/// 1  normalized source text        superseded
/// 2  semantic tokens + resolved reference identities
/// 3  canonical typed IR            not implemented
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HashScheme(pub u16);

/// The scheme this build produces and understands.
pub const CURRENT_SCHEME: HashScheme = HashScheme(2);

/// What the handler's body does, in order.
///
/// **Order-sensitive** wherever order changes behaviour, and carrying the
/// scheme that produced it.
///
/// Scheme 1 hashed normalized source text. That was the correct trade while
/// E7V was an isolated decision model — it never accepts behaviourally changed
/// code — but it invalidates every open tab when a formatter runs, which is a
/// product defect the moment real tabs depend on resuming. Scheme 2 hashes
/// semantic tokens plus resolved reference identities: whitespace, indentation,
/// comments and source offsets do not participate; token kinds, identifier
/// spellings, operators, literal values, control-flow syntax and statement
/// order do.
///
/// Deliberately still invalidating on local renames, `if` versus an equivalent
/// `match`, and reordered independent pure expressions. Proving those
/// equivalent is an optimizer, and an optimizer inside an identity system is a
/// worse hazard than an occasional reload. Scheme 3 (canonical typed IR) is
/// where that becomes safe.
///
/// The digest is produced by the compiler, not here — this crate never sees
/// source. See `pw-core::resume_artifacts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationHash {
    pub scheme: HashScheme,
    digest: Hash,
}

impl ImplementationHash {
    /// A digest the compiler produced under a named scheme.
    pub fn new(scheme: HashScheme, digest: &str) -> ImplementationHash {
        ImplementationHash {
            scheme,
            digest: Hash(digest.to_string()),
        }
    }

    /// Are these two comparable at all?
    ///
    /// Not the same as equal. Two hashes from different schemes are neither
    /// equal nor unequal — the question does not have an answer, and the
    /// caller must fail closed rather than pick one.
    pub fn comparable_with(&self, other: &ImplementationHash) -> bool {
        self.scheme == other.scheme
    }
}

impl HandlerId {
    /// Derive identity from what actually determines behaviour.
    ///
    /// Four inputs, and each is a separate type because each answers a
    /// different question. The capture schema is included because a handler
    /// that reads its captures differently is a different handler, even with
    /// identical source text.
    pub fn derive(
        implementation: &ImplementationHash,
        dependencies: &DependencySet,
        capture_schema: &SchemaHash,
        abi: &PlatformAbi,
    ) -> HandlerId {
        HandlerId(Hash::of(&format!(
            "{}:{}\u{1}{}\u{1}{}\u{1}{}",
            implementation.scheme.0, implementation.digest, dependencies.0, capture_schema.0, abi.0
        )))
    }
}

/// The shape of what a handler captures.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SchemaHash(pub Hash);

impl SchemaHash {
    /// From the field names and type names, in declaration order.
    pub fn of_fields(fields: &[(&str, &str)]) -> SchemaHash {
        let text: Vec<String> = fields.iter().map(|(n, t)| format!("{n}:{t}")).collect();
        SchemaHash(Hash::of(&text.join(",")))
    }
}

/// How widely a manifest's contents may be read.
///
/// Deliberately **not** `Ord`, and not as a temporary limitation to be fixed
/// later — as a refusal to encode security semantics as a sortable order.
/// Charter §7.8 says labels are a set of restrictions and not a total order,
/// and two sessions are incomparable:
///
/// ```text
/// Session<A> is not below Session<B>
/// Session<B> is not below Session<A>
/// ```
///
/// A total ordering would encode an arbitrary relationship between them that
/// someone later reads as permission to flow. The semantic operations are
/// named instead — see [`PrivacyScope::can_flow_to`].
///
/// When labels become combinations of restrictions this becomes a partial
/// order by set inclusion (`{Session<A>} ⊆ {Session<A>, Secret<C>}`), probably
/// a semilattice. Still not `Ord`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivacyScope {
    /// Served with the public shell; anything cacheable may read it.
    Public,
    /// Scoped to one session.
    Session(String),
    /// Scoped to one user.
    User(String),
    /// Scoped to one organization.
    Organization(String),
}

impl PrivacyScope {
    /// May a value scoped to `self` flow into a region scoped to `destination`?
    ///
    /// ```text
    /// Public     -> Session<A>   allowed   adds a restriction
    /// Session<A> -> Public       forbidden removes one
    /// Session<A> -> Session<A>   allowed
    /// Session<A> -> Session<B>   forbidden different principals
    /// ```
    ///
    /// `Public` is the EMPTY set of restrictions, so it is a subset of every
    /// destination — that is why the one widening-adjacent case is allowed, and
    /// it is allowed in the direction that adds rather than removes.
    ///
    /// **This authorises the DATA, not the CODE.** A public capture resumed
    /// into a session page may still invoke a handler that needs
    /// `session.read`, and that remains subject to capability, placement and
    /// handler-identity checks. "The captured data is public" never means "the
    /// code may run anywhere".
    pub fn can_flow_to(&self, destination: &PrivacyScope) -> bool {
        matches!(self, PrivacyScope::Public) || self == destination
    }

    /// Is this the same scope? Named rather than derived, so a caller reads
    /// what it is asking.
    pub fn is_equivalent_to(&self, other: &PrivacyScope) -> bool {
        self == other
    }
}

/// One resumable unit, as it appears in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeEntry {
    /// Which algorithm produced the identities in this manifest. Carried
    /// separately from the digests, because a digest cannot say what made it.
    pub hash_scheme: HashScheme,
    pub platform_abi: PlatformAbi,
    pub application_build: BuildId,
    pub handler: HandlerId,
    pub capture_schema: SchemaHash,
    pub document_schema: SchemaHash,
    pub privacy_scope: PrivacyScope,
    /// Opaque. Nothing here decodes it — decoding is what the decision
    /// authorises, and doing it first would be the bug.
    pub captures: Vec<u8>,
}

// --- what the running code offers -------------------------------------------

/// An explicit, checked migration between two capture schemas.
///
/// Tied to both hashes, so it cannot be applied to a schema it was not written
/// for. The function is pure and fallible; it may not reach a capability, and
/// the type is what enforces that — it receives bytes and returns bytes.
pub struct Migration {
    pub from: SchemaHash,
    pub to: SchemaHash,
    pub apply: fn(&[u8]) -> Result<Vec<u8>, MigrationError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationError(pub String);

/// What this running build can accept.
#[derive(Default)]
pub struct Runtime {
    pub abi: Vec<PlatformAbi>,
    pub build: Option<BuildId>,
    /// Handlers this build has code for.
    pub handlers: BTreeMap<HandlerId, SchemaHash>,
    /// The document schema each part currently has.
    pub document_schema: Option<SchemaHash>,
    pub scope: Option<PrivacyScope>,
    pub migrations: Vec<Migration>,
}

// --- the decision ------------------------------------------------------------

/// Why a manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The manifest's identities were produced by an algorithm this build does
    /// not implement. NOT "the hashes differ" — they are incomparable, and
    /// comparing them anyway is how a coincidence becomes an attachment.
    UnsupportedHashScheme(HashScheme),
    UnsupportedAbi(PlatformAbi),
    UnknownHandler(HandlerId),
    CaptureSchemaMismatch {
        want: SchemaHash,
        got: SchemaHash,
    },
    DocumentSchemaMismatch,
    PrivacyWidened {
        from: PrivacyScope,
        into: PrivacyScope,
    },
    NoMigration {
        from: SchemaHash,
        to: SchemaHash,
    },
    MigrationFailed(MigrationError),
    MalformedManifest(&'static str),
    MixedBuild {
        document: BuildId,
        patch: BuildId,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::UnsupportedHashScheme(scheme) => write!(
                f,
                "the manifest's identities came from hash scheme {}, which this \
                 build does not implement — they are incomparable, not different",
                scheme.0
            ),
            Refusal::UnsupportedAbi(a) => {
                write!(f, "the manifest's platform ABI {} is not supported", a.0)
            }
            Refusal::UnknownHandler(h) => write!(f, "no code for handler {}", h.0),
            Refusal::CaptureSchemaMismatch { want, got } => {
                write!(
                    f,
                    "the handler reads capture schema {}, the manifest holds {}",
                    want.0, got.0
                )
            }
            Refusal::DocumentSchemaMismatch => {
                f.write_str("the document part has a different shape")
            }
            Refusal::PrivacyWidened { from, into } => {
                write!(f, "a {from:?} manifest may not be resumed into {into:?}")
            }
            Refusal::NoMigration { from, to } => {
                write!(f, "no declared migration from {} to {}", from.0, to.0)
            }
            Refusal::MigrationFailed(e) => write!(f, "the migration failed: {}", e.0),
            Refusal::MalformedManifest(why) => write!(f, "the manifest is malformed: {why}"),
            Refusal::MixedBuild { document, patch } => {
                write!(
                    f,
                    "a patch from build {} cannot apply to a document from build {}",
                    patch.0, document.0
                )
            }
        }
    }
}

/// What to do when a manifest cannot be attached.
///
/// The runtime chooses only among actions valid for the construct in question —
/// a destructive command is never in the same list as a read-only widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Fetch the region again and render it.
    RefetchRegion,
    /// Re-render a private slot from the server.
    RerenderPrivateSlot,
    /// Full navigation.
    ReloadDocument,
    /// Ask the user to act again — never replayed automatically.
    RetryInteraction,
    /// Nothing safe is possible without the user deciding.
    RequireUserConfirmation,
    /// Nothing safe is possible at all.
    RejectIrrecoverable,
}

/// What kind of thing is being resumed. Decides which recoveries are legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Construct {
    /// A read-only public region.
    PublicRegion,
    /// A private slot rendered for one principal.
    PrivateSlot,
    /// Local input the user typed and has not submitted.
    UnsavedInput,
    /// A mutation that has not run yet.
    PendingCommand,
    /// A handle to something held open elsewhere.
    OpenResource,
}

impl Construct {
    /// The recoveries this construct permits, most preferred first.
    ///
    /// A `PendingCommand` never contains an automatic replay: re-running a
    /// mutation the user did not re-request is how a version mismatch becomes a
    /// double charge. It asks for a fresh interaction instead.
    ///
    /// **This is the construct's list, not the answer.** The scope of the
    /// manifest narrows it further — see [`Construct::recovery_for`].
    pub fn recoveries(self) -> &'static [Recovery] {
        match self {
            Construct::PublicRegion => &[Recovery::RefetchRegion, Recovery::ReloadDocument],
            Construct::PrivateSlot => &[Recovery::RerenderPrivateSlot, Recovery::ReloadDocument],
            // Preserved separately when its own schema still matches; otherwise
            // the user is asked before anything is discarded.
            Construct::UnsavedInput => {
                &[Recovery::RequireUserConfirmation, Recovery::ReloadDocument]
            }
            Construct::PendingCommand => &[
                Recovery::RetryInteraction,
                Recovery::RequireUserConfirmation,
            ],
            // A handle is not a description of a resource, it IS the resource.
            // Nothing on the other side can be reconnected to it.
            Construct::OpenResource => &[Recovery::RejectIrrecoverable],
        }
    }

    /// The recovery for this construct holding a manifest of this scope.
    ///
    /// The construct alone is not enough, and the first fuzz run proved it:
    /// a `PublicRegion` carrying a `Session`-scoped manifest was given
    /// `RefetchRegion`, which re-renders a PUBLIC region to recover PRIVATE
    /// state. 611 of 4000 generated cases hit it.
    ///
    /// The pairing is itself a contradiction — a public region should never
    /// hold session state, and if one does the artifact is malformed or forged
    /// — but "should never happen" is not a recovery policy. The scope wins,
    /// because it is the half that says who may see the result.
    pub fn recovery_for(self, scope: &PrivacyScope) -> Recovery {
        let preferred = *self
            .recoveries()
            .first()
            .unwrap_or(&Recovery::RejectIrrecoverable);
        if matches!(scope, PrivacyScope::Public) {
            return preferred;
        }
        match preferred {
            // Refetching a public region cannot restore private state, and
            // asking for it would render the region for whoever asks.
            Recovery::RefetchRegion => Recovery::RerenderPrivateSlot,
            other => other,
        }
    }
}

/// The typed answer. Never a boolean — a boolean cannot carry why, and "why"
/// is what a deployment incident is diagnosed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Attach directly. The captures are valid under the handler's schema.
    Resume,
    /// Attach after applying the named migration.
    Migrate { produced: Vec<u8> },
    /// Do not attach. Take the recovery action.
    Refuse {
        why: Refusal,
        recovery: Recovery,
        /// The causal chain, for a compatibility trace.
        trace: Vec<String>,
    },
}

impl Decision {
    pub fn attaches(&self) -> bool {
        !matches!(self, Decision::Refuse { .. })
    }
}

/// **The decision's causal trace, in words.**
///
/// With the `trace` feature off it holds nothing, and no step's words are
/// compiled: each step is given as a closure that is never called, so its
/// text and its formatting never reach the binary. The browser's build
/// leaves the feature off. No page reads the trace, and its words were
/// bytes every page downloaded; the refusal's code and recovery remain.
#[derive(Default)]
struct Trace(Vec<String>);

impl Trace {
    #[cfg(feature = "trace")]
    fn note(&mut self, words: impl FnOnce() -> String) {
        self.0.push(words());
    }

    #[cfg(not(feature = "trace"))]
    fn note(&mut self, _words: impl FnOnce() -> String) {}
}

/// Decide whether a manifest entry may be attached to this runtime's code.
///
/// Order matters: the cheapest and most fundamental checks first, so a trace
/// reads as a chain of narrowing rather than a list of unrelated failures.
pub fn decide(entry: &ResumeEntry, rt: &Runtime, construct: Construct) -> Decision {
    let mut trace = Trace::default();
    let refuse = |why: Refusal, trace: Trace| Decision::Refuse {
        recovery: construct.recovery_for(&entry.privacy_scope),
        why,
        trace: trace.0,
    };

    // 0. The manifest must be well formed before anything reads it.
    if entry.captures.is_empty() && entry.capture_schema != SchemaHash::of_fields(&[]) {
        trace.note(|| "manifest declares a non-empty schema and carries no bytes".into());
        return refuse(
            Refusal::MalformedManifest("captures are empty under a non-empty schema"),
            trace,
        );
    }

    // 1. The hash scheme, before the ABI, because it governs whether any
    //    identity below can be compared at all. A V1 manifest meeting a V2
    //    runtime is not "a different handler" — it is a question with no
    //    answer, and it fails closed through the ordinary recovery path.
    if entry.hash_scheme != CURRENT_SCHEME {
        trace.note(|| {
            format!(
                "manifest hash scheme {} != this build's {}",
                entry.hash_scheme.0, CURRENT_SCHEME.0
            )
        });
        return refuse(Refusal::UnsupportedHashScheme(entry.hash_scheme), trace);
    }
    trace.note(|| format!("hash scheme {} understood", entry.hash_scheme.0));

    // 2. ABI. An unsupported ABI means nothing below can even be interpreted.
    if !rt.abi.contains(&entry.platform_abi) {
        trace.note(|| format!("abi {} not in {:?}", entry.platform_abi.0, rt.abi));
        return refuse(Refusal::UnsupportedAbi(entry.platform_abi.clone()), trace);
    }
    trace.note(|| format!("abi {} supported", entry.platform_abi.0));

    // 2. Handler identity. A name would match here; content does not.
    let Some(handler_schema) = rt.handlers.get(&entry.handler) else {
        trace.note(|| "no code with this handler identity".into());
        return refuse(Refusal::UnknownHandler(entry.handler.clone()), trace);
    };
    trace.note(|| "handler identity found".into());

    // 3. Privacy, BEFORE any decoding. A widened scope must not reach the
    //    point where bytes are interpreted, even to fail.
    if let Some(into) = &rt.scope
        && !entry.privacy_scope.can_flow_to(into)
    {
        trace.note(|| format!("{:?} does not admit {:?}", entry.privacy_scope, into));
        return refuse(
            Refusal::PrivacyWidened {
                from: entry.privacy_scope.clone(),
                into: into.clone(),
            },
            trace,
        );
    }
    trace.note(|| "privacy scope is equal or narrower".into());

    // 4. The document part must still have the shape the manifest was written
    //    against, or the captures describe positions that no longer exist.
    if let Some(doc) = &rt.document_schema
        && *doc != entry.document_schema
    {
        trace.note(|| "document part schema differs".into());
        return refuse(Refusal::DocumentSchemaMismatch, trace);
    }
    trace.note(|| "document part schema matches".into());

    // 5. Captures. Exact match, or an explicit migration between exactly these
    //    two schemas. No structural inference.
    if *handler_schema == entry.capture_schema {
        trace.note(|| "capture schema matches exactly".into());
        return Decision::Resume;
    }
    let migration = rt
        .migrations
        .iter()
        .find(|m| m.from == entry.capture_schema && m.to == *handler_schema);
    let Some(migration) = migration else {
        trace.note(|| "no migration between these two schemas".into());
        return refuse(
            Refusal::NoMigration {
                from: entry.capture_schema.clone(),
                to: handler_schema.clone(),
            },
            trace,
        );
    };
    trace.note(|| "explicit migration found".into());
    match (migration.apply)(&entry.captures) {
        Ok(produced) => Decision::Migrate { produced },
        Err(e) => {
            trace.note(|| format!("migration returned an error: {}", e.0));
            refuse(Refusal::MigrationFailed(e), trace)
        }
    }
}

/// Proof that [`decide`] said yes.
///
/// # Nothing outside this module can construct one
///
/// The private field is the enforcement, and this doctest is the proof. It is
/// compiled by `cargo test` and must FAIL to compile:
///
/// ```compile_fail
/// use pw_resume::*;
/// // No literal, because `_private` is not visible here.
/// let forged = Authorised {
///     handler: HandlerId(unimplemented!()),
///     captures: vec![],
///     _private: (),
/// };
/// ```
///
/// A grep for `attach(` would be a brittle duplicate of a property the type
/// system already enforces. If somebody later builds a second attachment
/// subsystem entirely, that is an architectural review problem and a grep
/// would not have caught it either.
///
/// The only way to obtain one is [`decide`], and the only thing that accepts
/// one is [`attach`]. That is the whole design: a caller cannot attach a
/// handler without holding the result of a compatibility check, because there
/// is no other constructor.
///
/// A structural test that greps for a bypass would be a lint. This is the
/// same idea moved into the type system, where it cannot be forgotten — the
/// same move as deleting the by-name member fallback rather than documenting
/// that it should not be used.
#[derive(Debug)]
pub struct Authorised {
    handler: HandlerId,
    /// The bytes the handler may read: the originals, or what a migration
    /// produced. Never the manifest's bytes when a migration ran.
    captures: Vec<u8>,
    _private: (),
}

impl Authorised {
    pub fn handler(&self) -> &HandlerId {
        &self.handler
    }
    /// The captures, available only through the authorisation.
    pub fn captures(&self) -> &[u8] {
        &self.captures
    }
}

/// Attach a handler, given proof that it may be.
///
/// Takes `Authorised` by value, so one decision authorises one attachment.
pub fn attach(proof: Authorised) -> (HandlerId, Vec<u8>) {
    (proof.handler, proof.captures)
}

/// Turn a decision into an authorisation, or nothing.
///
/// The single point where a `Decision` becomes permission. `Refuse` yields
/// `None` — there is no path from a refusal to an `Authorised`.
pub fn authorise(entry: &ResumeEntry, decision: &Decision) -> Option<Authorised> {
    match decision {
        Decision::Resume => Some(Authorised {
            handler: entry.handler.clone(),
            captures: entry.captures.clone(),
            _private: (),
        }),
        // The MIGRATED bytes, not the manifest's. Handing the originals to a
        // handler that expects the new schema would be the mismatch the
        // migration exists to prevent, arriving one step later.
        Decision::Migrate { produced } => Some(Authorised {
            handler: entry.handler.clone(),
            captures: produced.clone(),
            _private: (),
        }),
        Decision::Refuse { .. } => None,
    }
}

/// May a streamed patch be applied to this document?
///
/// Separate from [`decide`] because it is a different question with a different
/// answer shape: a patch carries no captures and attaches no handler, but it
/// must belong to the generation of artifacts the document came from. A server
/// that redeploys mid-stream would otherwise send the second half of a page
/// from a build whose markup the first half does not match.
pub fn patch_applies(document: &BuildId, patch: &BuildId) -> Decision {
    if document == patch {
        return Decision::Resume;
    }
    let mut trace = Trace::default();
    trace.note(|| format!("document build {} != patch build {}", document.0, patch.0));
    Decision::Refuse {
        why: Refusal::MixedBuild {
            document: document.clone(),
            patch: patch.clone(),
        },
        // A mixed-generation stream cannot be repaired by re-fetching a region:
        // the document itself belongs to the older generation.
        recovery: Recovery::ReloadDocument,
        trace: trace.0,
    }
}
