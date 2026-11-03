//! `pw-core` — E1A: the value semantics and boundary ABI the language owns.
//!
//! E0 measured that Koka cannot supply three things the charter requires
//! (ADR-0011):
//!
//! | requirement | what Koka does | what `pw-core` does |
//! |---|---|---|
//! | exhaustive matching (charter §7.1) | enforced only when the effect row excludes `exn` | [`exhaust`] — effect-independent, with named missing cases |
//! | nominal opaque types (§7.1) | erases single-field value structs | [`types::Opaque`] — a distinct type and a distinct ABI slot |
//! | `Option`/`List` distinction (§7.10) | both are `null`, runtime-ambiguous | [`abi`] — decoding is type-directed, never shape-directed |
//! | task-scope non-escape (§7.6) | not modelled at all | [`scope`] — E2A-S, the static half of structured concurrency |
//! | artifact capability compliance (§10.2) | n/a — a Wasm property | [`capability`] — declared vs actually-imported, checked on the built artifact |
//!
//! Every detector emits the single [`diagnostics::Diagnostic`] type, and the
//! public code identifies the violated **invariant** rather than the analysis
//! that found it. Rendering belongs to callers: no semantic rule lives in the
//! CLI.
//!
//! Nothing here depends on Koka, on an effect row, or on a backend
//! representation. That independence is the point.

pub mod abi;
pub mod address_fragment;
pub mod affine;
/// Charter §7.1, §7.10, §8.2 checks that read a type the author wrote.
/// Distinct from `types`, which is the value-type model itself.
pub mod annotations;
pub mod backend;
pub mod binding;
pub mod boundary;
pub mod build;
pub(crate) mod canonical;
pub mod capability;
pub mod check;
pub mod codes;
pub mod contexts;
pub mod contract;
// Track `device-store` (ADR-0366): a value the reader's device keeps.
pub mod device_store;
pub mod diag;
pub mod diagnostics;
pub mod effects;
pub mod exhaust;
pub mod graph;
pub mod hir;
pub mod holds;
pub mod infer;
pub mod koka;
pub mod labels;
pub mod layout;
pub mod layouts;
pub mod lexical;
pub mod lower;
pub mod manifest;
pub mod marko;
pub mod names;
pub mod ontology;
pub mod outcome;
pub mod page_values;
pub mod placement;
pub mod policy;
pub mod predicates;
pub mod privacy;
pub mod program;
pub mod provenance;
pub mod recursion;
pub mod resolve;
pub mod resolved;
pub mod resume;
pub mod resume_artifacts;
pub mod routes;
pub mod rules;
pub mod scope;
pub mod semantic;
pub mod signals;
pub mod signatures;
pub mod streams;
pub mod template_ir;
pub mod types;
// Track `uploads` (ADR-0260): a file a form posts, and its limits.
pub mod uploads;
// NEXT 6c: a command the platform invokes, which no reader presses.
pub mod invoked;
// ADR-0195's ruling 13, the build's half: a page loads from its own origin.
pub mod origins;
// The scheduled-commands track: a command runs when the clock passes its time.
pub mod scheduled;
// 4c (track `bounded-checker`): a read is bounded by construction.
pub mod bounded;
// Proof-guided logical plans over generated table reads.
pub mod read_plans;
// ADR-0350: tables give their generated data operations.
pub mod tables;
pub mod values;
pub mod wit;

pub use abi::{Decoder, Mode, Value};
pub use capability::{CapabilityAudit, CapabilityManifest, RuntimeProfile, audit};
pub use diagnostics::{Detector, Diagnostic, Severity};
pub use exhaust::{Arm, MatchReport, Pattern, check_match, render_witness};
pub use scope::{HandleKind, Op, ScopeGraph, ScopeKind, ScopeViolation};
pub use types::{Adt, Ctor, Program, Type};
