//! E8 — what an effect **is**, as a declaration rather than a spelling.
//!
//! Effect ontology slice 2. Slice 1 gave `effect database.read<T> { .. }` a
//! grammar, a `DeclKind` and a namespace. This resolves a written effect
//! against those declarations, and it is the first production consumer of
//! ADR-0022's semantic provenance.
//!
//! Architect ruling, 2026-08-07:
//!
//! > So slice 2 becomes both: completion of the effect ontology; and the first
//! > production implementation of semantic provenance.
//!
//! # The pipeline
//!
//! ```text
//! database.read<Stores>
//!   → EffectPath(["database","read"]) + TypeArg("Stores")
//!   → EffectDefId + TypeDefId
//!   → EffectInstance { effect, args }
//! ```
//!
//! Everything downstream consumes [`EffectInstance`]. **No checker splits
//! `"database.read"` at the dot** — the dot is read exactly twice, both times
//! here: once when a declaration's name is turned into an [`EffectPath`], and
//! once when a written row entry is. After that there is a resolved identity,
//! and an identity does not need re-parsing.
//!
//! # Why a family is derived and never invented
//!
//! `!{ database }` is a legitimate row: charter §7.5A's rule is that declaring
//! a family accepts its members, so a broad claim is broader than any one
//! operation. But `database` is not itself an effect anybody performs.
//!
//! So a family EXISTS because operations were declared into it. There is no
//! `effect database` and no list of valid family spellings in a checker —
//! `databse.read` is unknown because nothing declared anything under `databse`,
//! which is a fact about the program rather than about a table someone
//! maintained. That is the property `docs/RISK_QUEUE.md` 37 cost us when
//! `LayoutAffect` named nothing in five files.
//!
//! # What this module does NOT do
//!
//! It does not decide whether an effect is permitted, where it may run, or what
//! authority it needs. Those are `effects.rs`, `placement.rs` and `contract.rs`,
//! and each of them is a policy over this answer. This module answers only
//! *which declaration did you write*.

use std::collections::{BTreeMap, BTreeSet};

use crate::hir::{Decl, DeclKind, EffectRef, Hir, Span};
use crate::placement::{ALL_WORLDS, PlacementLookup, World};
use crate::provenance::{Evidence, FactId, FactKind, Route};
use crate::resolve::{DefId, Namespace, Resolution, Workspace};

/// A declared effect's identity.
///
/// A newtype over [`DefId`] rather than a bare one, so an effect cannot be
/// passed where a type or a function is expected. `docs/RISK_QUEUE.md` 34 is
/// what one identity standing in for another costs, and the compiler now has
/// enough different `DefId`s in flight that the distinction is worth a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffectDefId(pub DefId);

/// An effect's name, in segments.
///
/// Built at the boundary — from a declaration's name, or from a row entry's
/// written path — and never again. A checker holding one of these has already
/// been told what the segments are and has no reason to look at a dot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffectPath(Vec<String>);

impl EffectPath {
    /// Read a written path. **The only place the dot is interpreted.**
    pub fn parse(written: &str) -> EffectPath {
        EffectPath(written.split('.').map(str::to_string).collect())
    }

    /// The first segment: `database` in `database.read`.
    ///
    /// A family is the first segment and nothing else. `a.b.c` has family `a`,
    /// which is a decision rather than an accident: the corpus's rule is that
    /// declaring a family accepts its members, and nesting families would make
    /// "which family covers this" a search instead of a lookup.
    pub fn family(&self) -> &str {
        self.0.first().map(String::as_str).unwrap_or_default()
    }

    /// Everything after the family: `read` in `database.read`. Empty when the
    /// effect names only a family, as `log` and `trace` do.
    pub fn operation(&self) -> String {
        self.0[1..].join(".")
    }

    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// The canonical spelling. Round-trips with [`EffectPath::parse`].
    pub fn text(&self) -> String {
        self.0.join(".")
    }
}

/// What one type argument turned out to name.
///
/// **Resolved from where it was written, or not resolved.** Architect ruling,
/// 2026-08-07:
///
/// > Do not extend ambient visibility to effect arguments. […] If `Payments`
/// > isn't imported or otherwise in scope, that's a source error.
///
/// There was a third case, `Unscoped`: the program declares it somewhere and
/// this unit does not import it. It existed because 31 corpus rows named an
/// argument they had not imported, and it is gone because those 31 were
/// repaired rather than accommodated — assumption A-017, retired the same day
/// it was written. The ruling on why:
///
/// > Don't keep a safe-looking fallback simply because it currently prevents
/// > tests from failing.
///
/// Modules count as resolved, and the reason is in `capability.rs`: the corpus
/// writes `database.read<Stores>` and `Stores` is a MODULE — the domain being
/// read. A first version of the capability-argument rule accepted only types
/// and reported two working corpus files as defective.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypeArgument {
    /// A `type` or `opaque type`, resolved through the workspace from where
    /// the row was written. The only one carrying an identity.
    Type { written: String, def: DefId },
    /// A module this unit can see. An external contract, exactly as an opaque
    /// type is, and one with no `DefId` because a module is not a declaration.
    Module { written: String },
    /// Nothing in this program declares it. Kept rather than dropped — the
    /// authority the program asked for is not erased by being unresolvable,
    /// and `PW5200` is what makes the mistake visible where it can be repaired.
    Unresolved { written: String },
}

impl TypeArgument {
    pub fn written(&self) -> &str {
        match self {
            TypeArgument::Type { written, .. }
            | TypeArgument::Module { written }
            | TypeArgument::Unresolved { written } => written,
        }
    }

    /// Did it resolve from where it was written? The question `PW5200` asks.
    pub fn is_resolved(&self) -> bool {
        !matches!(self, TypeArgument::Unresolved { .. })
    }
}

/// **What an effect DOES to the frame**, as distinct from what it is called.
///
/// Architect ruling, 2026-08-07:
///
/// > `style.mutate<LayoutAffect>` is not a "layout-family" effect, but it IS
/// > layout-affecting. […] give resolved `EffectInstance`s semantic
/// > traits/facets that phase checking consumes […] rather than
/// > `family == "layout"`.
///
/// The family was a proxy for meaning and it was too coarse: `style.mutate`
/// with a layout-affecting argument and one without share a family, so no rule
/// keyed on the family could tell them apart. `forbidden_in_phase("animate",
/// "layout")` therefore could not catch the exact case its own comment
/// described.
///
/// **Declared, not derived from a spelling.** Each effect names its impacts in
/// its own declaration, and an impact may be conditional on a type argument:
///
/// ```pleris
/// effect style.mutate<T> {
///     impact style_write
///     impact layout_write when LayoutAffect
/// }
/// ```
///
/// The condition is resolved to a `DefId` once, when the ontology is built, so
/// matching it against an instance's argument is identity comparison. No
/// checker holds the spelling `LayoutAffect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Facet {
    /// Reads geometry. Forces layout if one is pending.
    LayoutRead,
    /// Writes something that can invalidate layout.
    LayoutWrite,
    /// Writes something that costs only paint.
    PaintWrite,
    /// Changes the document tree.
    DomWrite,
    /// Runs on the compositor, off the main thread.
    CompositorOperation,
}

impl Facet {
    /// The spelling an `impact` clause uses. One name per facet, because a
    /// facet is a language concept and an alias would give it two.
    pub fn named(name: &str) -> Option<Facet> {
        Some(match name {
            "layout_read" => Facet::LayoutRead,
            "layout_write" => Facet::LayoutWrite,
            "paint_write" => Facet::PaintWrite,
            "dom_write" => Facet::DomWrite,
            "compositor" => Facet::CompositorOperation,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Facet::LayoutRead => "layout_read",
            Facet::LayoutWrite => "layout_write",
            Facet::PaintWrite => "paint_write",
            Facet::DomWrite => "dom_write",
            Facet::CompositorOperation => "compositor",
        }
    }
}

/// One `impact` clause: a facet, and the argument it is conditional on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impact {
    pub facet: Facet,
    /// `impact layout_write when LayoutAffect` — the marker, resolved to a
    /// declaration when the declaring module can see it. `None` means the
    /// impact is unconditional.
    ///
    /// A `DefId` rather than a name, so matching an instance's argument is
    /// identity comparison. The written spelling is carried beside it only so
    /// a diagnostic can name what failed to resolve.
    ///
    /// An unresolved marker is a **compile error** — `PW5204`. Architect
    /// ruling, 2026-08-07:
    ///
    /// > Silently dropping `impact layout_write when LayoutAffect` because
    /// > `LayoutAffect` failed to resolve recreates the same class of problem
    /// > that `LayoutAffect` already exposed: source looks meaningful, compiler
    /// > quietly assigns it no meaning.
    ///
    /// It was inert for one commit. Inert is better than a textual fallback and
    /// still wrong: a package could lose an import and the facet would stop
    /// applying with nothing said.
    pub when: Option<Marker>,
}

/// The marker an `impact .. when ..` clause names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub written: String,
    /// `None` when nothing in the declaring module's scope declares it, which
    /// is `PW5204` and never a spelling match.
    pub def: Option<DefId>,
    pub span: Span,
}

/// **A resolved effect.** What every analysis downstream consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectInstance {
    pub effect: EffectDefId,
    pub args: Vec<TypeArgument>,
    /// What this instance does to the frame, after its conditional impacts have
    /// been matched against its resolved arguments.
    pub facets: BTreeSet<Facet>,
    /// The row entry this came from, for a diagnostic that underlines it.
    pub span: Span,
}

/// What a row entry resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// One declared operation: `database.read<Stores>`.
    Operation(EffectInstance),
    /// A whole family: `!{ database }`, which covers every operation declared
    /// under it. Not an effect anything performs — a claim about a row.
    Family { name: String, span: Span },
}

/// Why a written effect resolved to nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectError {
    /// Nothing is declared under this first segment. `databse.read`.
    UnknownFamily {
        family: String,
        /// The nearest declared family, when one is close enough to be a typo.
        nearest: Option<String>,
        span: Span,
    },
    /// The family exists; this operation is not one of its members.
    /// `database.reed`.
    UnknownOperation {
        family: String,
        operation: String,
        /// Every operation the family DOES declare, so the message can list
        /// them rather than only reject.
        declared: Vec<String>,
        nearest: Option<String>,
        span: Span,
    },
    /// The effect exists and binds a different number of type parameters.
    /// `database.read` with none, `database.read<A, B>` with two.
    WrongArity {
        effect: String,
        expected: usize,
        found: usize,
        span: Span,
    },
}

impl EffectError {
    pub fn span(&self) -> &Span {
        match self {
            EffectError::UnknownFamily { span, .. }
            | EffectError::UnknownOperation { span, .. }
            | EffectError::WrongArity { span, .. } => span,
        }
    }
}

/// One `effect` declaration, as the program wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDecl {
    pub def: EffectDefId,
    pub path: EffectPath,
    /// How many type parameters it binds. `effect log` binds none;
    /// `effect database.read<T>` binds one.
    pub arity: usize,
    /// The `capability` clause, as written. `None` for `capability none` — an
    /// effect that needs no host authority, which is a real and common case
    /// and not a missing declaration.
    pub capability: Option<String>,
    /// The `placement` clause: where this effect is *meaningful*, as distinct
    /// from where it is *authorised*.
    ///
    /// Architect ruling, 2026-08-07:
    ///
    /// > If an effect has `capability none` we still need to know that
    /// > `layout.measure` is browser-only. […] For browser-semantic effects
    /// > with no capability, the declaration itself supplies the placement
    /// > restriction.
    ///
    /// This is the fact `World::worlds_for` holds today as a hard-coded table,
    /// and the reason its `dom`/`style`/`layout` entries were read as "needs a
    /// host capability" when they only ever meant "only means anything in a
    /// browser". Empty means unconstrained — an effect that can happen
    /// anywhere, which is the right default because a checker must not reject
    /// what it has not been taught.
    pub placement: Vec<World>,
    /// What this effect does to the frame — see [`Facet`]. Empty for an effect
    /// with no frame consequence, which is most of them.
    pub impacts: Vec<Impact>,
    pub span: Span,
}

/// **Every effect the program declares.**
#[derive(Debug, Clone, Default)]
pub struct Ontology {
    by_name: BTreeMap<String, EffectDecl>,
    /// First segment → the operations declared under it, in sorted order.
    ///
    /// Derived from the declarations, never written down. A family exists
    /// because something was declared into it.
    families: BTreeMap<String, BTreeSet<String>>,
    /// Effect names declared more than once in the checked set.
    ///
    /// Detected rather than absorbed. `by_name` keeps one declaration per
    /// spelling by design — the effect vocabulary is one flat namespace, which
    /// is what makes `database.read` mean the same thing in every row — so a
    /// second declaration of one name is a conflict somebody has to resolve
    /// and not a map entry that quietly wins.
    conflicts: BTreeMap<String, Vec<EffectDefId>>,
}

impl Ontology {
    /// Collect every `effect` declaration in the program.
    pub fn build(hirs: &[&Hir]) -> Ontology {
        Ontology::build_with(hirs, &Workspace::default())
    }

    /// [`Ontology::build`], resolving each `impact .. when ..` marker through
    /// the workspace so a facet condition is a `DefId` rather than a name.
    pub fn build_with(hirs: &[&Hir], ws: &Workspace) -> Ontology {
        // No program-wide name set. There was one — every type, opaque type
        // and module in the checked set — and it existed only to distinguish
        // "declared somewhere" from "declared nowhere". Both are now the same
        // answer: an argument resolves from where it was written or it does
        // not resolve.
        let mut out = Ontology::default();

        for (unit, hir) in hirs.iter().enumerate() {
            for (id, decl) in hir.all_decls() {
                if decl.kind != DeclKind::Effect || decl.name.is_empty() {
                    continue;
                }
                let def = EffectDefId(DefId { unit, decl: id.0 });
                let path = EffectPath::parse(&decl.name);
                out.families
                    .entry(path.family().to_string())
                    .or_default()
                    .insert(path.operation());
                if let Some(first) = out.by_name.get(&decl.name) {
                    out.conflicts
                        .entry(decl.name.clone())
                        .or_insert_with(|| vec![first.def])
                        .push(def);
                    continue;
                }
                out.by_name.insert(
                    decl.name.clone(),
                    EffectDecl {
                        def,
                        path,
                        arity: decl.type_params.len(),
                        capability: capability_clause(decl),
                        placement: placement_clause(decl),
                        impacts: impact_clauses(decl, ws, unit),
                        span: hir.decl_span(id),
                    },
                );
            }
        }
        out
    }

    /// Effect names two declarations claim. Empty in a well-formed program.
    pub fn conflicts(&self) -> &BTreeMap<String, Vec<EffectDefId>> {
        &self.conflicts
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn declarations(&self) -> impl Iterator<Item = &EffectDecl> {
        self.by_name.values()
    }

    pub fn get(&self, name: &str) -> Option<&EffectDecl> {
        self.by_name.get(name)
    }

    /// The declaration a WRITTEN effect names: `database.read<Stores>` finds
    /// `database.read`.
    ///
    /// The type argument is stripped here and nowhere else, for the same
    /// reason the dot is read here and nowhere else. A caller holding a row's
    /// text has one question — which declaration is this — and giving it a way
    /// to ask means it never has to take the string apart itself.
    ///
    /// **Membership, not visibility.** A contract is derived from INFERRED
    /// effects, which arrive as spellings with no unit attached: the effect
    /// came from a callee in another module, and "which file wrote this row"
    /// has no answer by then. [`Ontology::resolve`] is the visibility-checked
    /// form and takes a unit precisely because it has one.
    pub fn declared_for(&self, written: &str) -> Option<&EffectDecl> {
        self.by_name
            .get(written.split('<').next().unwrap_or(written))
    }

    /// The facets of an effect **as inference carries it**: a written
    /// spelling, with no `EffectInstance` to hand.
    ///
    /// `Source` holds an effect as text — that is how E2D's inference has
    /// always propagated one — so the phase rule needs a way in from a string.
    /// The string is taken apart HERE, in the one module allowed to, using the
    /// same argument resolution [`Ontology::argument`] uses, and everything
    /// downstream receives facets.
    ///
    /// Retiring this means carrying `EffectInstance` through inference instead
    /// of a spelling, which is E9's to do when effect rows become real.
    pub fn facets_of(&self, ws: &Workspace, unit: usize, written: &str) -> BTreeSet<Facet> {
        let Some(decl) = self.declared_for(written) else {
            return BTreeSet::new();
        };
        let args: Vec<DefId> = written
            .split_once('<')
            .map(|(_, rest)| rest.trim_end_matches('>'))
            .into_iter()
            .flat_map(|a| a.split(','))
            .filter_map(|a| match ws.resolve_in(unit, Namespace::Type, a.trim()) {
                Resolution::Local(def) | Resolution::Imported { def, .. } => Some(def),
                _ => None,
            })
            .collect();
        decl.impacts
            .iter()
            .filter(|i| match &i.when {
                None => true,
                Some(m) => m.def.is_some_and(|marker| args.contains(&marker)),
            })
            .map(|i| i.facet)
            .collect()
    }

    /// The declaration an instance names.
    pub fn declaration(&self, of: EffectDefId) -> Option<&EffectDecl> {
        self.by_name.values().find(|d| d.def == of)
    }

    /// Is `name` a family something was declared into?
    pub fn has_family(&self, name: &str) -> bool {
        self.families.contains_key(name)
    }

    /// **Resolve one written row entry.**
    ///
    /// The whole pipeline, in one place, recording an edge per transition.
    ///
    /// `unit` is where the row was written. It decides visibility: an effect
    /// declared in a module this file does not import is not in scope, exactly
    /// as a type is not. `usize::MAX` means "no unit", and then only the
    /// ontology's own membership answers — which is what a caller holding a
    /// row and no workspace position honestly has.
    pub fn resolve(
        &self,
        ws: &Workspace,
        unit: usize,
        entry: &EffectRef,
        evidence: &mut Evidence,
    ) -> Result<Resolved, EffectError> {
        let path = EffectPath::parse(&entry.path);

        // A bare family: `!{ database }`. Legitimate, and not an operation.
        if path.operation().is_empty() && !self.by_name.contains_key(&entry.path) {
            if self.families.contains_key(path.family()) {
                return Ok(Resolved::Family {
                    name: path.family().to_string(),
                    span: entry.span.clone(),
                });
            }
            return Err(EffectError::UnknownFamily {
                family: path.family().to_string(),
                nearest: nearest(path.family(), self.families.keys().cloned()),
                span: entry.span.clone(),
            });
        }

        let Some(decl) = self.by_name.get(&entry.path) else {
            // Which HALF is wrong decides the message. The family is checked
            // first because "no such family" and "that family has no such
            // operation" send a reader to two different places, and a rule
            // that said only "unknown effect" would send them to neither.
            let Some(operations) = self.families.get(path.family()) else {
                return Err(EffectError::UnknownFamily {
                    family: path.family().to_string(),
                    nearest: nearest(path.family(), self.families.keys().cloned()),
                    span: entry.span.clone(),
                });
            };
            let operation = path.operation();
            return Err(EffectError::UnknownOperation {
                nearest: nearest(&operation, operations.iter().cloned()),
                family: path.family().to_string(),
                operation,
                declared: operations.iter().cloned().collect(),
                span: entry.span.clone(),
            });
        };

        // Visibility, if the caller said where it stands. An effect declared
        // in a module this file neither owns nor imports is out of scope, and
        // reporting it as unknown is the same answer resolution gives a type.
        //
        // BEFORE the fact is recorded, and the order is load-bearing. Recorded
        // first, a resolution that then failed would leave an edge behind
        // saying it succeeded — and every `must contain` assertion written
        // against this graph would be satisfied by a compiler that resolved
        // nothing. ADR-0022: provenance RECORDS what the analysis concluded,
        // and until this line the analysis has concluded nothing.
        if unit != usize::MAX && !self.visible(ws, unit, &entry.path, decl) {
            return Err(EffectError::UnknownFamily {
                family: path.family().to_string(),
                nearest: nearest(path.family(), self.families.keys().cloned()),
                span: entry.span.clone(),
            });
        }

        // How the name was reached. `Qualified` because an effect name is a
        // path against declarations, resolved whole — never by matching a
        // final segment, which is the route ADR-0022 exists to forbid.
        let resolved = evidence.record(
            FactKind::ResolvedEffect {
                path: entry.path.clone(),
                to: decl.def,
                via: Route::Qualified,
            },
            entry.span.clone(),
            vec![],
        );

        if entry.args.len() != decl.arity {
            return Err(EffectError::WrongArity {
                effect: entry.path.clone(),
                expected: decl.arity,
                found: entry.args.len(),
                span: entry.span.clone(),
            });
        }

        let args: Vec<TypeArgument> = entry
            .args
            .iter()
            .map(|a| self.argument(ws, unit, a, &entry.span, resolved, evidence))
            .collect();

        // What this instance does to the frame. An unconditional impact always
        // applies; a conditional one applies when one of the resolved arguments
        // IS the declaration the condition named — compared by `DefId`, so no
        // checker downstream ever reads a marker's spelling.
        let facets: BTreeSet<Facet> = decl
            .impacts
            .iter()
            .filter(|i| match &i.when {
                None => true,
                // By `DefId`. An unresolved marker contributes nothing AND is
                // reported as `PW5204` — see `check_effect_declarations`. There
                // is deliberately no spelling comparison in this expression.
                Some(m) => m.def.is_some_and(|marker| {
                    args.iter()
                        .any(|a| matches!(a, TypeArgument::Type { def, .. } if *def == marker))
                }),
            })
            .map(|i| i.facet)
            .collect();

        for f in &facets {
            evidence.record(
                FactKind::EffectFacet {
                    effect: entry.path.clone(),
                    facet: f.name().to_string(),
                },
                entry.span.clone(),
                vec![resolved],
            );
        }

        Ok(Resolved::Operation(EffectInstance {
            effect: decl.def,
            args,
            facets,
            span: entry.span.clone(),
        }))
    }

    /// One type argument, resolved and recorded.
    ///
    /// Through the WORKSPACE first, from the unit that wrote the row, so a
    /// resolved argument carries the identity of the declaration this file can
    /// actually see. The program-wide set is consulted only to distinguish
    /// [`TypeArgument::Unscoped`] from [`TypeArgument::Unresolved`], which is
    /// a distinction and not a fallback: neither answer becomes a `DefId`.
    fn argument(
        &self,
        ws: &Workspace,
        unit: usize,
        written: &str,
        span: &Span,
        cause: FactId,
        evidence: &mut Evidence,
    ) -> TypeArgument {
        let in_scope = (unit != usize::MAX)
            .then(|| match ws.resolve_in(unit, Namespace::Type, written) {
                Resolution::Local(def) | Resolution::Imported { def, .. } => {
                    Some(TypeArgument::Type {
                        written: written.to_string(),
                        def,
                    })
                }
                _ if ws.sees_module(unit, written) => Some(TypeArgument::Module {
                    written: written.to_string(),
                }),
                _ => None,
            })
            .flatten();

        let arg = in_scope.unwrap_or(TypeArgument::Unresolved {
            written: written.to_string(),
        });
        // Recorded either way. That an argument did NOT resolve is a fact
        // about the program, and a graph that only held successes could not
        // distinguish "checked and found" from "never looked".
        evidence.record(
            FactKind::ResolvedTypeArgument {
                written: written.to_string(),
                resolved: arg.is_resolved(),
            },
            span.clone(),
            vec![cause],
        );
        arg
    }

    /// Can `unit` see this effect declaration?
    ///
    /// Through the workspace, in the effect namespace. Not by membership in
    /// the ontology: the ontology holds every effect in the checked set, and
    /// "this program declares it somewhere" is the ambient union assumption
    /// A-009 removed.
    fn visible(&self, ws: &Workspace, unit: usize, name: &str, decl: &EffectDecl) -> bool {
        // The effect's own module always sees it, and so does a module that
        // imports it by name or wholesale.
        if let Resolution::Local(def) | Resolution::Imported { def, .. } =
            ws.resolve_in(unit, Namespace::Effect, name)
        {
            return EffectDefId(def) == decl.def;
        }
        // A workspace with no module for this unit answers nothing — a
        // synthetic body, or a caller checking one file in isolation. The
        // ontology's own membership is then the honest answer.
        ws.module_of(unit).is_none()
    }
}

/// What an effect's type argument may name, **from where it was written**.
///
/// Architect ruling, 2026-08-07:
///
/// > Effect names resolve through that prelude; their arguments resolve through
/// > ordinary lexical/module visibility. […] If `Payments` isn't imported or
/// > otherwise in scope, that's a source error.
///
/// One implementation, used by [`Ontology::argument`] and by `PW5200`. They
/// were two — the ontology asked the workspace and `capability.rs` asked a
/// whole-program set — which meant the ruling was obeyed by the analysis and
/// not by the rule that reports it, so a file naming an out-of-scope argument
/// got no error at all.
///
/// Modules count for the reason `capability.rs` records: `database.read<Stores>`
/// names the domain being read, and `Stores` is a module.
pub fn argument_names_visible_from(ws: &Workspace, unit: usize, hirs: &[&Hir]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (owner, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if !matches!(decl.kind, DeclKind::Type | DeclKind::Opaque) || decl.name.is_empty() {
                continue;
            }
            if matches!(
                ws.resolve_in(unit, Namespace::Type, &decl.name),
                Resolution::Local(_) | Resolution::Imported { .. }
            ) {
                out.insert(decl.name.clone());
            }
            if (owner == unit || decl.visibility.as_deref() != Some("private"))
                && let Some(module) = hir.module_of(id)
            {
                let qualified = format!("{module}.{}", decl.name);
                if matches!(ws.resolve_path_in(unit,Namespace::Type,&qualified),Resolution::Local(def)|Resolution::Imported{def,..} if def.unit==owner && def.decl==id.0)
                {
                    out.insert(qualified);
                }
            }
        }
        for (id, _) in hir.all_decls() {
            if let Some(m) = hir.module_of(id)
                && ws.sees_module(unit, m)
            {
                out.insert(m.to_string());
            }
        }
    }
    out
}

/// `capability database.read<T>` → `Some("database.read<T>")`;
/// `capability none` → `None`.
///
/// `none` is a real answer, not a missing one. Charter §7.5A's `log` and
/// `trace` perform an effect and require no host authority, and an ontology
/// that could not say so would force every effect to invent a capability.
fn capability_clause(decl: &Decl) -> Option<String> {
    let value = decl.policy("capability")?.value.trim();
    (value != "none" && !value.is_empty()).then(|| value.to_string())
}

/// Every `impact` clause on a declaration.
///
/// `impact style_write` is unconditional; `impact layout_write when LayoutAffect`
/// applies only when an argument resolves to that marker. The marker is looked
/// up from the DECLARING unit, because that is where it is written.
fn impact_clauses(decl: &Decl, ws: &Workspace, unit: usize) -> Vec<Impact> {
    decl.policies
        .iter()
        .filter(|p| p.name == "impact")
        .filter_map(|p| {
            let value = p.value.trim();
            let (facet, marker) = match value.split_once(" when ") {
                Some((f, m)) => (f.trim(), Some(m.trim())),
                None => (value, None),
            };
            Some(Impact {
                facet: Facet::named(facet)?,
                when: marker.map(|m| Marker {
                    written: m.to_string(),
                    def: match ws.resolve_in(unit, Namespace::Type, m) {
                        Resolution::Local(def) | Resolution::Imported { def, .. } => Some(def),
                        _ => None,
                    },
                    span: p.span.clone(),
                }),
            })
        })
        .collect()
}

/// **Where an effect is meaningful, for `placement.rs`'s solver.**
///
/// The declaration is now the only source of this fact. `World::worlds_for`
/// held it as a hard-coded family→world table until 2026-08-07 and the two
/// agreed at the moment of deletion — `tests/placement_migration.rs` is the
/// frozen baseline that says so.
///
/// The three answers are the ruling's, and the middle one is why they are three
/// rather than two:
///
/// ```text
/// declared, with a placement clause   Known      the constraint
/// declared, with none                 Unrestricted  meaningful anywhere
/// not declared                        Blocked    no answer exists
/// ```
///
/// `Blocked` names the diagnostic that owns it rather than carrying a message,
/// because the mistake is in the effect row and `check_effect_rows` already
/// reports it there. A placement rule inventing its own wording would deliver
/// one defect twice in two vocabularies.
impl crate::placement::Placements for Ontology {
    fn placement_of(&self, effect: &str) -> PlacementLookup {
        match self.declared_for(effect) {
            Some(d) if d.placement.is_empty() => PlacementLookup::Unrestricted,
            Some(d) => PlacementLookup::Known(d.placement.clone()),
            // Which of the two unknown-effect codes owns it is the same
            // question `resolve` answers, asked without a unit: a family
            // something was declared into, with an operation nobody declared,
            // is a different mistake from a family that does not exist.
            None => PlacementLookup::Blocked {
                code: if self.has_family(EffectPath::parse(effect).family()) {
                    crate::codes::UNKNOWN_EFFECT_OPERATION.id
                } else {
                    crate::codes::UNKNOWN_EFFECT_FAMILY.id
                },
            },
        }
    }
}

/// `placement browser` → `[Browser]`; `placement browser, edge, origin` →
/// three.
///
/// A LIST, because `network.fetch` is meaningful anywhere with a request
/// context and only build time has none. An enum would have forced either a
/// fourth world named "anywhere with a request" or a table somewhere else
/// saying which set each keyword abbreviates, and the second is what
/// `worlds_for` already is.
///
/// A word that names no world is dropped rather than guessed at. The
/// diagnostic for it belongs with the other `PW52xx` unknown-name rules and
/// is step 5; silently defaulting to "everywhere" here would make a typo into
/// a widening, which is the wrong direction for a placement constraint.
fn placement_clause(decl: &Decl) -> Vec<World> {
    let Some(p) = decl.policy("placement") else {
        return Vec::new();
    };
    p.value
        .split(',')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .filter_map(|w| ALL_WORLDS.iter().copied().find(|world| world.name() == w))
        .collect()
}

/// The closest declared spelling, when one is close enough to be a typo.
///
/// The same bound `capability.rs` uses, and for the same reason: a suggestion
/// that is merely the alphabetically first candidate sends the reader to an
/// unrelated declaration, which is worse than no suggestion.
fn nearest(written: &str, candidates: impl Iterator<Item = String>) -> Option<String> {
    let budget = (written.len() / 3).max(1);
    candidates
        .map(|c| (distance(written, &c), c))
        .filter(|(d, _)| *d <= budget && *d > 0)
        .min_by_key(|(d, c)| (*d, c.len()))
        .map(|(_, c)| c)
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            row[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(row[j] + 1);
        }
        std::mem::swap(&mut prev, &mut row);
    }
    prev[b.len()]
}

/// Report every effect DECLARATION whose impact condition names nothing.
///
/// `PW5204`. Separate from [`check_effect_rows`] because the mistake is in the
/// declaration, not in any program that writes the effect — a platform package
/// with this defect silently stops applying a facet, and every application
/// checked against it looks fine.
pub fn check_effect_declarations(
    ontology: &Ontology,
    out: &mut Vec<crate::diagnostics::Diagnostic>,
    unit: usize,
) {
    use crate::codes;
    use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};

    for decl in ontology.declarations() {
        if decl.def.0.unit != unit {
            continue;
        }
        for impact in &decl.impacts {
            let Some(marker) = &impact.when else { continue };
            if marker.def.is_some() {
                continue;
            }
            out.push(Diagnostic {
                code: codes::UNRESOLVED_IMPACT_MARKER.id,
                invariant: codes::UNRESOLVED_IMPACT_MARKER.invariant,
                reason: "impact_condition_names_nothing",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!(
                    "`{}` conditions `{}` on `{}`, which this module cannot see",
                    decl.path.text(),
                    impact.facet.name(),
                    marker.written
                ),
                primary_span: marker.span.clone(),
                related: vec![Related::new(
                    marker.span.clone(),
                    format!(
                        "`{}` names no type here, so the impact would never apply",
                        marker.written
                    ),
                )],
                explanation: Some(
                    "An impact condition is matched by resolved identity, never by spelling — \
                     which is what stops a marker from meaning whatever a reader assumes. A \
                     marker that resolves to nothing therefore matches nothing, and the facet \
                     silently stops applying: the program still checks clean and a phase rule \
                     quietly sees an effect with no consequences. That is exactly what \
                     `style.mutate<LayoutAffect>` did for three milestones while `LayoutAffect` \
                     was declared nowhere."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: format!(
                        "import the module that declares `{}`, or declare it here",
                        marker.written
                    ),
                    replacement: None,
                }],
            });
        }
    }
}

/// Report every effect row in one unit against the ontology.
///
/// **The consumer that makes the ontology load-bearing.** Until this ran, an
/// effect resolved to a declaration only where a contract asked; a row could
/// name nothing at all and no diagnostic said so — which is the state
/// `docs/RISK_QUEUE.md` 37 describes, where `LayoutAffect` named nothing in
/// five files for three milestones.
///
/// The three codes are separate because they send a reader to three different
/// places. A family typo, an operation typo and a wrong argument count are not
/// the same mistake, and "unknown effect" would be the message for all three.
pub fn check_effect_rows(
    ontology: &Ontology,
    ws: &Workspace,
    unit: usize,
    hir: &Hir,
    out: &mut Vec<crate::diagnostics::Diagnostic>,
) {
    use crate::codes;
    use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};

    // **No exemption for a program with no effect declarations at all.**
    //
    // There was one: a checked set with an empty ontology returned here in
    // silence, on the reasoning that "your effect does not exist" is the wrong
    // thing to say when the truth is "no vocabulary was supplied". The
    // architect ruled against it on 2026-08-07, deciding the same question for
    // placement:
    //
    // > A pure standalone file can still check without a web platform package.
    // > But if it writes `!{ database.read<Stores> }` without a selected or
    // > imported platform environment that declares `database.read`, the
    // > compiler should say, effectively: `database.read` has no meaning in
    // > this program. It should NOT say: I recognize that string from an old
    // > compiler table, so I'll assume origin.
    //
    // What made the exemption look harmless was the table underneath it —
    // placement and capability still had answers for an undeclared effect, so
    // silence here cost nothing visible. With `worlds_for` deleted the silence
    // is the whole failure: the row is unreported, placement is `Blocked`, and
    // a contract comes out with no placements and no explanation.
    //
    // Longer term a project selects `pw-std` and `pw-platform-web` through its
    // configuration and ordinary programs never see this. That is a project
    // environment concern and not a reason for the compiler to answer from a
    // vocabulary the program did not select.

    for (_, decl) in hir.all_decls() {
        for entry in decl.declared_effects.iter().flatten() {
            let mut evidence = Evidence::default();
            let Err(e) = ontology.resolve(ws, unit, entry, &mut evidence) else {
                continue;
            };
            let (code, message, label, repair) = match &e {
                EffectError::UnknownFamily {
                    family, nearest, ..
                } => (
                    codes::UNKNOWN_EFFECT_FAMILY,
                    match nearest {
                        Some(n) => format!("unknown effect family `{family}`; did you mean `{n}`?"),
                        None => format!("unknown effect family `{family}`"),
                    },
                    format!("no package in scope declares an effect under `{family}`"),
                    match nearest {
                        Some(n) => format!("did you mean `{n}`?"),
                        None => "declare the effect, or select a package that does".to_string(),
                    },
                ),
                EffectError::UnknownOperation {
                    family,
                    operation,
                    declared,
                    nearest,
                    ..
                } => (
                    codes::UNKNOWN_EFFECT_OPERATION,
                    match nearest {
                        Some(n) => {
                            format!(
                                "`{family}` declares no operation `{operation}`; did you mean `{n}`?"
                            )
                        }
                        None => format!("`{family}` declares no operation `{operation}`"),
                    },
                    format!(
                        "`{family}` declares: {}",
                        declared
                            .iter()
                            .map(|d| format!("`{family}.{d}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    match nearest {
                        Some(n) => format!("did you mean `{family}.{n}`?"),
                        None => format!("use one of the operations `{family}` declares"),
                    },
                ),
                EffectError::WrongArity {
                    effect,
                    expected,
                    found,
                    ..
                } => (
                    codes::EFFECT_ARITY_MISMATCH,
                    format!(
                        "`{effect}` takes {expected} type argument{}, and {found} {} given",
                        if *expected == 1 { "" } else { "s" },
                        if *found == 1 { "was" } else { "were" }
                    ),
                    match (expected, found) {
                        (1, 0) => format!(
                            "`{effect}` alone does not say WHICH — omission is not a wildcard"
                        ),
                        (0, _) => format!("`{effect}` binds no type parameter"),
                        _ => format!("`{effect}` binds {expected}"),
                    },
                    if *expected > *found {
                        format!("write `{effect}<..>` naming what it applies to")
                    } else {
                        format!("write `{effect}` with {expected} argument(s)")
                    },
                ),
            };
            out.push(Diagnostic {
                code: code.id,
                invariant: code.invariant,
                reason: "effect_row_does_not_resolve",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: e.span().clone(),
                related: vec![Related::new(e.span().clone(), label)],
                explanation: Some(
                    "An effect is a DECLARATION, not a spelling. Before the ontology, \
                     `log` and a misspelled `databse` were indistinguishable to every \
                     checker, because nothing said what a family was — and \
                     `style.mutate<LayoutAffect>` appeared in five corpus files while \
                     `LayoutAffect` was declared nowhere, so a distinction the platform \
                     called `the point` rested on a spelling no checker could resolve."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: repair,
                    replacement: None,
                }],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_read_once_and_then_asked() {
        // The property the module exists for. After `parse`, the segments are
        // answers rather than something to re-derive.
        let p = EffectPath::parse("database.read");
        assert_eq!(p.family(), "database");
        assert_eq!(p.operation(), "read");
        assert_eq!(p.text(), "database.read");

        // A family-only effect has no operation, and that is not an error.
        let log = EffectPath::parse("log");
        assert_eq!(log.family(), "log");
        assert_eq!(log.operation(), "");
    }

    #[test]
    fn capability_none_is_an_answer_and_not_a_missing_clause() {
        let with = Decl {
            policies: vec![crate::hir::Policy {
                name: "capability".into(),
                value: "database.read<T>".into(),
                roots: Vec::new(),
                keys: Vec::new(),
                span: 0..0,
            }],
            ..bare()
        };
        assert_eq!(
            capability_clause(&with).as_deref(),
            Some("database.read<T>")
        );

        let without = Decl {
            policies: vec![crate::hir::Policy {
                name: "capability".into(),
                value: "none".into(),
                roots: Vec::new(),
                keys: Vec::new(),
                span: 0..0,
            }],
            ..bare()
        };
        assert_eq!(capability_clause(&without), None);
        // Which is NOT the same as never having written the clause — but both
        // mean "no host authority", so they agree here on purpose.
        assert_eq!(capability_clause(&bare()), None);
    }

    fn bare() -> Decl {
        Decl {
            name: "database.read".into(),
            name_span: 0..0,
            kind: DeclKind::Effect,
            params: vec![],
            ret: None,
            variants: None,
            fields: None,
            opaque_of: None,
            invariant: None,
            type_params: vec![],
            policies: vec![],
            imports: vec![],
            visibility: None,
            declared_effects: None,
            body: None,
            terms: None,
            children: vec![],
            mutable: false,
        }
    }
}
