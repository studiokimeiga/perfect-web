//! Running `pw-core`'s algorithms over real `.pw` source.
//!
//! `docs/NEXT.md` item 7. The algorithms in [`crate::exhaust`],
//! [`crate::types`], [`crate::scope`] and [`crate::abi`] were written and
//! tested against hand-built inputs; this is what feeds them HIR built from
//! source. It consumes ids and spans only — never syntax nodes (ADR-0012).
//!
//! # Scope of what is checked
//!
//! Deliberately narrow, and narrow in a direction that cannot produce false
//! positives: a `match` is checked **only** when its scrutinee type is known
//! from a signature or an annotated binding *in the same program*. Anything
//! else is skipped in silence. A checker that guessed a type in order to
//! report more would be reporting fiction, and the corpus exists to catch
//! exactly that.

use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};
use crate::exhaust::{self, Arm, Pattern as EPat};
use crate::hir::{self, AttrValue, Body, Decl, DeclKind, Expr, ExprId, Hir, Node, Pattern as HPat};
use crate::placement::{ALL_WORLDS, Demand, Placements, World, solve};
use crate::privacy::{Label, Restriction};
use crate::scope::{HandleKind, Op, ScopeGraph, ScopeKind, ScopeViolation};
use crate::signatures::Signatures;
use crate::types::{Ctor, Program, Type};

/// One source file that has been parsed and lowered.
pub struct Unit {
    pub path: String,
    pub src: String,
    pub hir: Hir,
}

/// Check every unit against the shared environment.
pub fn check_units(units: &[Unit]) -> Vec<(String, Vec<Diagnostic>)> {
    let hirs: Vec<&Hir> = units.iter().map(|u| &u.hir).collect();
    let workspace = crate::resolve::Workspace::build(&hirs);
    let sigs = Signatures::build(&workspace, &hirs);
    check_with(units, &hirs, &workspace, &sigs)
}

/// The existing semantic analyses, using the program's shared environment.
pub(crate) fn check_with(
    units: &[Unit],
    hirs: &[&Hir],
    workspace: &crate::resolve::Workspace,
    sigs: &Signatures,
) -> Vec<(String, Vec<Diagnostic>)> {
    // E2B: one workspace module graph, built before any semantic analysis.
    // Resolution failures are reported per unit, so a file importing a module
    // that does not exist says so instead of failing later in a checker that
    // cannot explain why.
    // E2C: resolution and signatures are prepared once by the caller.
    // E2D: effects inferred over the whole program, so a helper declared in
    // another module still contributes to its caller's row.
    let mut inference = crate::effects::Inference::new(sigs, workspace);
    inference.run(hirs);
    check_inferred(units, hirs, workspace, sigs, &inference)
}

/// The exact privacy derivation for one prepared semantic program.
/// All facts own their IDs and labels; no inference or HIR borrow escapes.
pub(crate) struct PrivacyFacts {
    summaries: BTreeMap<crate::resolve::DefId, Label>,
    reads: Reads,
    labels: BTreeMap<crate::resolve::DefId, Label>,
}

impl PrivacyFacts {
    pub(crate) fn holds(&self, def: crate::resolve::DefId) -> Label {
        self.reads.holds(def)
    }
    pub(crate) fn labels(&self) -> &BTreeMap<crate::resolve::DefId, Label> {
        &self.labels
    }
}

pub(crate) fn privacy_facts(
    hirs: &[&Hir],
    sigs: &Signatures,
    inference: &crate::effects::Inference<'_>,
) -> PrivacyFacts {
    let summaries = summaries(hirs, sigs);
    let reads = Reads::of(hirs, sigs, inference).answering(&summaries);
    let labels = reads.labels();
    PrivacyFacts {
        summaries,
        reads,
        labels,
    }
}

pub(crate) fn check_inferred(
    units: &[Unit],
    hirs: &[&Hir],
    workspace: &crate::resolve::Workspace,
    sigs: &Signatures,
    inference: &crate::effects::Inference<'_>,
) -> Vec<(String, Vec<Diagnostic>)> {
    check_inferred_prepared(units, hirs, workspace, sigs, inference).0
}

pub(crate) fn check_inferred_prepared(
    units: &[Unit],
    hirs: &[&Hir],
    workspace: &crate::resolve::Workspace,
    sigs: &Signatures,
    inference: &crate::effects::Inference<'_>,
) -> (crate::program::Diagnostics, PrivacyFacts) {
    // E7: what the whole program says about a type — is it a resource, and is
    // it produced only by a scoped declaration. Both are needed before any one
    // file can be asked what its handlers may capture.
    let manifest = crate::boundary::TypeFacts::build(hirs, sigs);
    // E8 step 2: what a capability's type argument may name, PER UNIT.
    //
    // Whole-program until 2026-08-07, which was assumption A-017 and is now
    // retired: an argument resolves from where it was written, exactly as a
    // type in an expression does.
    let visible_types: Vec<std::collections::BTreeSet<String>> = (0..units.len())
        .map(|u| crate::ontology::argument_names_visible_from(workspace, u, hirs))
        .collect();
    // E8 slice 2: what each effect a row names actually IS. One ontology for
    // the whole program, beside the signatures, because both answer questions
    // about the same rows.
    let ontology = crate::ontology::Ontology::build_with(hirs, workspace);
    // E6: every route the program declares, so a link can be checked against
    // what exists rather than against a naming convention.
    let routes = crate::routes::table(hirs);
    // E6: the resource dependency graph, over every unit at once. A fragment
    // in one module depends on a query in another, so a per-file graph would
    // report a dangling edge for exactly the case the milestone is about.
    let graph = crate::graph::Graph::build(hirs, workspace);
    // ADR-0101: what a command's write has to reach, over every unit, since
    // a command in one module writes what a query in another reads.
    let readers = cached_readers(inference, &graph, hirs);
    // ADR-0207: every data source the program declares.
    let sources = declared_sources(hirs);
    let mut resolution: BTreeMap<usize, Vec<Diagnostic>> = BTreeMap::new();
    for e in &workspace.errors {
        resolution
            .entry(e.unit)
            .or_default()
            .push(resolve_diagnostic(e));
    }
    for (i, u) in units.iter().enumerate() {
        let per_unit = resolution.entry(i).or_default();
        per_unit.extend(unresolved_uses(workspace, hirs, i, &u.hir));
        // ADR-0072: an element named with a capital letter is a view.
        per_unit.extend(view_elements(workspace, hirs, i, &u.hir));
        // ADR-0204: an element holds the children HTML permits, as it
        // renders them.
        per_unit.extend(nesting(workspace, hirs, i, &u.hir));
        // ADR-0207: a data source states what it guarantees, and nothing
        // asks it for more.
        per_unit.extend(sources_give_what_is_asked(
            inference,
            &sources,
            &visible_types[i],
            i,
            &u.hir,
        ));
        // ADR-0214: an opaque type's `.value` is its representation in its
        // own module, and nothing else there.
        per_unit.extend(representation_unshadowed(sigs, i, &u.hir));
        // ADR-0263: a session's, a user's or an organization's handle is
        // the platform's to make.
        per_unit.extend(handles_are_the_platforms(workspace, sigs, i, &u.hir));
        // ADR-0264: and a session's never reaches the browser.
        per_unit.extend(sessions_stay_on_the_server(workspace, sigs, i, &u.hir));
        // ADR-0212: a `query` reads a query or a resource, and a
        // `subscription` a subscription.
        per_unit.extend(reads_name_their_kind(workspace, hirs, sigs, i, &u.hir));
        per_unit.extend(dependencies_stated_once(&u.hir));
        // ADR-0148: a streamed query is read by a `<stream>`, which shows
        // each state its query can be in.
        per_unit.extend(crate::streams::check(workspace, hirs, sigs, i, &u.hir));
        // ADR-0094: a template writes no code.
        per_unit.extend(code_in_markup(&u.hir));
        // ADR-0167: a comment in markup is `<!-- -->`.
        per_unit.extend(comment_as_text(&u.hir));
        // ADR-0183: a page states its title.
        per_unit.extend(titles(&u.hir));
        // ADR-0186: and what it says of itself to what reads it unshown.
        per_unit.extend(metadata(&u.hir));
        // ADR-0189: a `<link>` is one HTML allows where it is written.
        per_unit.extend(links(&u.hir));
        // ADR-0195's ruling 13: and a page loads from its own origin.
        per_unit.extend(crate::origins::check(&u.hir));
        // ADR-0185: an id names one element, a reference names one shown
        // with it, and ARIA is what WAI-ARIA defines.
        per_unit.extend(ids_and_aria(&u.hir));
        // ADR-0089: a policy's value is one its domain has.
        per_unit.extend(policy_values(workspace, i, &u.hir));
        // ADR-0107: a cache key names each parameter its entry depends on.
        per_unit.extend(keys_name_what_is_read(&u.hir));
        // ADR-0114: what is built before any request reads none of its values.
        per_unit.extend(built_pages_read_no_parameter(&u.hir));
        // ADR-0110: a resumable handler reads what it captures.
        per_unit.extend(handlers_read_their_captures(&u.hir));
        // ADR-0113: and performs only what the browser may.
        per_unit.extend(handlers_run_in_the_browser(
            inference, &ontology, workspace, hirs, i, &u.hir,
        ));
        // ADR-0098: a name is written once where it is declared.
        per_unit.extend(declared_once(&u.hir));
        // ADR-0091: a listener binds its declaration's key.
        per_unit.extend(listener_keys(&u.hir, &u.src));
        // ADR-0092: a graph clause belongs to a declaration that can mean it.
        per_unit.extend(clauses_in_place(&u.hir));
        // ADR-0105: a command invalidates the entry it speculates on; and
        // ADR-0101, what it writes, where it does not speculate on it.
        let speculated = speculations(workspace, hirs, i);
        per_unit.extend(speculation_reconciled(&graph, &speculated, &u.hir));
        per_unit.extend(writes_invalidated(
            inference,
            &graph,
            &readers,
            &speculated,
            i,
            &u.hir,
        ));
        // ADR-0090: the declaration rules. Only `pw check` ran them, beside
        // this function; `pw build` checks through here, and compiled what
        // they refuse: R-015's `retry forever` became a component.
        per_unit.extend(crate::rules::check(&u.hir));
        // ADR-0047: a name used as a value resolves too, in lexical scope.
        let names = crate::names::check(workspace, hirs, i, &u.src);
        per_unit.extend(names.diagnostics);
        // ADR-0247: and a clause the walk reads in a block is judged as one
        // heading a declaration is.
        per_unit.extend(clause_values(workspace, i, &u.hir, &names.clauses));
        // Every effect row, against the declarations. Reported beside the
        // resolution failures because that is what it is: a name in a row that
        // resolves to nothing is the same class of mistake as a name in an
        // expression that does.
        crate::ontology::check_effect_rows(&ontology, workspace, i, &u.hir, per_unit);
        // And the declarations themselves: an impact condition naming nothing
        // makes a facet silently inapplicable.
        crate::ontology::check_effect_declarations(&ontology, per_unit, i);
        // E10, ADR-0025: an optimistic transition is a separate execution root
        // with its own effect row, and its context permits none.
        named_roots_respect_their_context(inference, i, &u.hir, per_unit);
        derived_values_are_pure(inference, sigs, i, &u.hir, per_unit);
        optimistic_targets_are_resource_entries(sigs, workspace, hirs, i, per_unit);
    }

    // Declaration name → its privacy label, across every unit. A `page` in one
    // file renders a `session query` declared in another, and that is the
    // corpus's canonical private-in-shared-cache case (assumption A-009).
    // Keyed by RESOLVED IDENTITY, not by spelling.
    //
    // It was `BTreeMap<String, Label>` keyed by bare declaration name, so a
    // page rendering `Cart` was given the join of EVERY `Cart` in the program.
    // Joining is the safe direction — an unrelated declaration could only make
    // a page look more private, never less — which is exactly why it never
    // produced a visibly wrong answer and stayed in place. It is still
    // `docs/RISK_QUEUE.md` 34's shape: meaning resolved from a spelling.
    //
    // And a declaration's label is joined with what it reads through what it
    // calls (ADR-0118): a `session query` read through a helper, or through a
    // public query, is read.
    //
    // And what calling each declaration makes from its body (ADR-0129): the
    // value labels a sink reads follow a call into its callee. Made first,
    // since a secret a declaration answers, by its type or by what its body
    // makes, is held where it runs and in what keeps it (ADR-0282).
    let PrivacyFacts {
        summaries,
        reads,
        labels,
    } = privacy_facts(hirs, sigs, inference);
    let resources = reads.of_resources(hirs);
    // And what a view's handlers capture of what it is given (ADR-0136): a
    // page that uses the view writes it into its own document.
    let captured = crate::resume::captured_params(hirs, sigs);
    // And what each view needs provided, and which hold signals (ADR-0144).
    let provision = crate::signals::Provision::of(hirs, sigs);
    // And each command a page's handler calls (ADR-0154): what a browser
    // sends, and can send twice.
    let sent = crate::resume::sent_commands(hirs, sigs);
    // 4c: what keys each source read, and what loops over it, over every
    // unit, since a loop in one module calls a read in another.
    let bounded = crate::bounded::Reads::of(hirs, workspace, sigs, inference, &sources);
    let paths: Vec<&str> = units.iter().map(|u| u.path.as_str()).collect();
    let diagnostics = units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let mut out = resolution.remove(&i).unwrap_or_default();
            out.extend(sent_without_idempotency(&u.hir, i, &sent));
            // ADR-0406: a command a reader can run says who may.
            out.extend(who_may_run(&u.hir, i, &sent));
            // ADR-0157: a command is one request a page's handler sends, and
            // what it answers is whether it committed.
            out.extend(called_outside_a_handler(&u.hir, i, sigs));
            // ADR-0160: a page is served at its route, which gives its
            // parameters, and one route is one page's.
            out.extend(crate::routes::parameters_agree(&u.hir, i, sigs));
            out.extend(crate::routes::declared_twice(hirs, i));
            // ADR-0302: a predicate is declared once, with one set of words.
            out.extend(crate::predicates::declared_twice(hirs, i));
            // Track `uploads` (ADR-0260): an upload's clauses, its paths, and
            // the forms that post a file.
            out.extend(crate::uploads::check(hirs, i));
            // Tables obey the same prepared semantic environment as every
            // other declaration and can be reached only through given functions.
            out.extend(crate::tables::check(hirs, i, workspace, sigs));
            // NEXT 6c: a command the platform invokes, which no reader
            // presses (PW64xx).
            out.extend(crate::invoked::check(hirs, i, workspace, &sent));
            // The scheduled-commands track: a schedule names a command the
            // clock invokes, in a command, at its time (PW65xx).
            out.extend(crate::scheduled::check(hirs, i, workspace, sigs));
            // 4c, a read is bounded by construction: no read in a loop over
            // data is keyed by its element (PW0405), a set read's source
            // batches (PW0406), and a keyed read names its set form (PW0407).
            out.extend(bounded.check(i, &u.src));
            // ADR-0163: a page says when its address names nothing.
            out.extend(not_found_names_a_case(&u.hir, i, sigs, workspace));
            // ADR-0295: and when it is another address of the page.
            out.extend(redirect_names_a_case(&u.hir, i, sigs, workspace));
            // ADR-0302: a predicate the program declares says what a refusal
            // by it is told, and is given what it takes.
            out.extend(crate::predicates::check(&u.hir, i, sigs, workspace));
            // ADR-0307: a control is shown where its command's predicates hold.
            out.extend(crate::holds::check(hirs, workspace, sigs, i));
            out.extend(answer_read_for_a_value(&u.hir, i, sigs));
            // ADR-0280: a handler navigates once its command commits.
            out.extend(navigations(workspace, &u.hir, i, sigs));
            // ADR-0311: and a page's canonical address is a page's.
            out.extend(canonical_addresses(workspace, hirs, i, sigs));
            // Track `todomvc` (ADR-0367): a page binds a signal to its
            // address's fragment (PW61xx).
            out.extend(crate::address_fragment::check(hirs, i));
            // Track `device-store` (ADR-0366): a value the device keeps, and
            // the `Device` label that keeps it there (PW68xx).
            out.extend(crate::device_store::check(hirs, i, sigs, &summaries));
            // ADR-0303: a page is shown in its layout.
            out.extend(crate::layouts::check(hirs, workspace, i));
            // ADR-0243's amendment: markup is no operand.
            out.extend(markup_as_an_operand(&u.hir));
            out.extend(check_unit_with(
                &labels,
                &reads,
                &summaries,
                &captured,
                &provision,
                sigs,
                inference,
                &manifest,
                &routes,
                &graph,
                &resources,
                &ontology,
                workspace,
                i,
                &visible_types[i],
                u,
            ));
            out.sort_by_key(|d| d.primary_span.start);
            // A note found in another file names it (ADR-0321).
            crate::diagnostics::resolve_files(&mut out, i, &paths);
            (u.path.clone(), out)
        })
        .collect();
    (
        diagnostics,
        PrivacyFacts {
            summaries,
            reads,
            labels,
        },
    )
}

/// **A command a page's handler calls declares `idempotent_by`**
/// (ADR-0154): its request can be delivered twice, by a retrying proxy, a
/// flaky network or a client's own retry, and the platform gives each press
/// an interaction to tell the second from the first.
fn sent_without_idempotency(
    hir: &Hir,
    unit: usize,
    sent: &BTreeMap<crate::resolve::DefId, Vec<(usize, hir::Span)>>,
) -> Vec<Diagnostic> {
    let code = crate::codes::SENT_WITHOUT_IDEMPOTENCY;
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        if decl.kind != hir::DeclKind::Command || decl.policy("idempotent_by").is_some() {
            continue;
        }
        let def = crate::resolve::DefId { unit, decl: id.0 };
        let Some(calls) = sent.get(&def) else {
            continue;
        };
        out.push(Diagnostic {
            code: code.id,
            invariant: code.invariant,
            reason: "sent_without_idempotency",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!(
                "`{}` is called by a page's handler, and declares no `idempotent_by`",
                decl.name
            ),
            primary_span: hir.decl_span(id),
            related: calls
                .iter()
                // Each in its own file, a page's (ADR-0321).
                .map(|(at, span)| {
                    Related::in_unit(*at, span.clone(), "a press sends it from here".to_string())
                })
                .collect(),
            explanation: Some(
                "A browser's request can reach the server twice: a retrying proxy, a \
                 flaky network or a client's own retry sends it again after the first \
                 arrived (RFC 9110 lets a client retry only a request it knows to be \
                 idempotent). Each press carries an interaction, and `idempotent_by \
                 InteractionId` runs the command once for it (ADR-0121)."
                    .to_string(),
            ),
            // Both steps: `idempotent_by` names a type, and a program with no
            // `InteractionId` that only adds the clause is refused next
            // (PW0335, "names no type visible here").
            repairs: vec![Repair {
                description: "declare `idempotent_by InteractionId`, and `opaque type InteractionId = String` if the program has none".to_string(),
                replacement: None,
            }],
        });
    }
    out
}

/// **A command says who may run it** (ADR-0406, PW0367).
///
/// The server runs a command for any request that names it, whether or not a
/// page shows its button (`/command/<id>` admits every command it hosts but
/// one the platform invokes), so who may run it belongs to the command. A
/// command a reader can run declares `requires`: the predicates a request must
/// hold, or `Anyone`, alone. One the platform runs says `invoked_by` instead,
/// and PW6401 holds it to no `requires`. `Anyone` is the platform's: a program
/// that declared its own would be read as everyone's.
fn who_may_run(
    hir: &Hir,
    unit: usize,
    sent: &BTreeMap<crate::resolve::DefId, Vec<(usize, hir::Span)>>,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        match decl.kind {
            hir::DeclKind::Predicate if decl.name == crate::policy::ANYONE => {
                out.push(anyone_declared(hir, id, decl));
            }
            hir::DeclKind::Command if decl.policy("invoked_by").is_none() => {
                let presses = sent
                    .get(&crate::resolve::DefId { unit, decl: id.0 })
                    .map_or(&[][..], Vec::as_slice);
                out.extend(runner_unstated(hir, id, decl, presses));
            }
            _ => {}
        }
    }
    out
}

/// What a request to a reader-run command must hold, unsaid: no `requires`,
/// or `Anyone` with arguments or beside another predicate. A `requires` that
/// does not parse is its clause's own refusal.
fn runner_unstated(
    hir: &Hir,
    id: hir::DeclId,
    decl: &hir::Decl,
    presses: &[(usize, hir::Span)],
) -> Option<Diagnostic> {
    let (reason, message, at, repairs) = match decl.policy("requires") {
        None => (
            "runner_unstated",
            format!(
                "`{}` runs for any reader's request, and says nothing of who may run it",
                decl.name
            ),
            decl.name_span.clone(),
            vec![
                "declare `requires` with what a reader must hold, such as `requires SignedIn`",
                "or `requires Anyone`, if anyone may run it",
            ],
        ),
        Some(requires) => {
            let predicates = crate::policy::predicates(&requires.value).ok()?;
            let anyone = predicates
                .iter()
                .find(|p| p.name == crate::policy::ANYONE)?;
            let (reason, message) = if !anyone.arguments.is_empty() {
                (
                    "anyone_given_arguments",
                    "gives `Anyone` arguments, and it takes none",
                )
            } else if predicates.len() > 1 {
                (
                    "anyone_beside_others",
                    "requires `Anyone` beside another predicate",
                )
            } else {
                return None;
            };
            (
                reason,
                format!("`{}` {message}", decl.name),
                requires.span.clone(),
                vec!["write `requires Anyone` alone, or name only what a reader must hold"],
            )
        }
    };
    let related = if presses.is_empty() {
        vec![Related::new(
            hir.decl_span(id),
            format!(
                "a request that names `{}` runs it, with or without a page",
                decl.name
            ),
        )]
    } else {
        presses
            .iter()
            .map(|(unit, span)| {
                Related::in_unit(
                    *unit,
                    span.clone(),
                    "a press sends it from here".to_string(),
                )
            })
            .collect()
    };
    Some(Diagnostic {
        code: crate::codes::WHO_MAY_RUN.id,
        invariant: crate::codes::WHO_MAY_RUN.invariant,
        reason,
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message,
        primary_span: at,
        related,
        explanation: Some(WHO_MAY_RUN_WHY.to_string()),
        repairs: repairs
            .into_iter()
            .map(|description| Repair {
                description: description.to_string(),
                replacement: None,
            })
            .collect(),
    })
}

const WHO_MAY_RUN_WHY: &str = "The server runs a command for any request that names it, whether \
     or not a page shows its button: a request is an address and a body, and anyone can send \
     one. So who may run a command is the command's to say. `requires` names what every request \
     must hold, as `requires SignedIn, OwnsPost(post)`; `requires Anyone` says anyone may, and \
     stands alone. A command the platform runs says `invoked_by` instead.";

/// `predicate Anyone`, declared by a program: `Anyone` is the platform's, and
/// nothing asks it, so a program's own would be read as everyone's.
fn anyone_declared(hir: &Hir, id: hir::DeclId, decl: &hir::Decl) -> Diagnostic {
    Diagnostic {
        code: crate::codes::WHO_MAY_RUN.id,
        invariant: crate::codes::WHO_MAY_RUN.invariant,
        reason: "anyone_declared",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message:
            "`predicate Anyone` is declared, and `Anyone` is the platform's: every reader holds it"
                .to_string(),
        primary_span: decl.name_span.clone(),
        related: vec![Related::new(
            hir.decl_span(id),
            "a `requires Anyone` would not ask this".to_string(),
        )],
        explanation: Some(WHO_MAY_RUN_WHY.to_string()),
        repairs: vec![Repair {
            description: "name the predicate otherwise".to_string(),
            replacement: None,
        }],
    }
}

/// **A page says when its address names nothing** (ADR-0163, PW0342):
/// `not_found_on Type.Case` is a page's, names a case of a declared type,
/// and a query the page reads can answer it. Otherwise the clause would
/// never fire, and the page would answer 503 for the absence it names.
fn not_found_names_a_case(
    hir: &Hir,
    unit: usize,
    sigs: &Signatures,
    ws: &crate::resolve::Workspace,
) -> Vec<Diagnostic> {
    let code = crate::codes::NOT_FOUND_NAMES_A_CASE;
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        let Some(p) = decl.policy("not_found_on") else {
            continue;
        };
        let value = p.value.trim();
        let refused = |message: String, repair: &str| Diagnostic {
            code: code.id,
            invariant: code.invariant,
            reason: "not_found_names_a_case",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message,
            primary_span: p.span.clone(),
            related: vec![Related::new(
                hir.decl_span(id),
                format!("a policy of `{}`", decl.name),
            )],
            explanation: Some(
                "A page whose queries fail is answered 503: it cannot be shown now. A page \
                 whose address names nothing is answered 404, and only the page can say \
                 which of its queries' declared errors means that (ADR-0163)."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: repair.to_string(),
                replacement: None,
            }],
        };
        if decl.kind != DeclKind::Page {
            out.push(refused(
                format!(
                    "`not_found_on {value}` is a page's: `{}` is not one",
                    decl.name
                ),
                "move it to the page whose address it is about",
            ));
            continue;
        }
        let (ty, _) = match crate::routes::not_found_case(ws, sigs, unit, decl) {
            Ok(Some(found)) => found,
            Ok(None) => continue,
            Err(why) => {
                out.push(refused(
                    format!("`not_found_on {value}`: {why}"),
                    "name a case of a declared error, `StoreError.NotFound`",
                ));
                continue;
            }
        };
        let Some(b) = decl.body else { continue };
        let answers = crate::page_values::query_bindings(ws, unit, hir.body(b))
            .iter()
            .any(|(_, query, _)| crate::routes::error_of(sigs, *query) == Some(ty));
        if !answers {
            out.push(refused(
                format!(
                    "`not_found_on {value}`: no query `{}` reads can answer it",
                    decl.name
                ),
                "name an error a query the page reads declares",
            ));
        }
    }
    out
}

/// **A page says when its address is another address of the page**
/// (ADR-0295, PW0350): `redirect_on Type.Case permanent` (or `temporary`) is
/// a page's, names a case of a declared type that a query the page reads can
/// answer, and is not the case its `not_found_on` names. The case carries
/// one value, of the type of the one parameter the page's route carries,
/// and the host answers 308 (or 307) to the route filled with it. Otherwise
/// the clause would never fire, or the host could not say where the page is.
fn redirect_names_a_case(
    hir: &Hir,
    unit: usize,
    sigs: &Signatures,
    ws: &crate::resolve::Workspace,
) -> Vec<Diagnostic> {
    let code = crate::codes::REDIRECT_NAMES_A_CASE;
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        let Some(p) = decl.policy("redirect_on") else {
            continue;
        };
        let value = p.value.trim();
        let refused = |message: String, repair: &str| Diagnostic {
            code: code.id,
            invariant: code.invariant,
            reason: "redirect_names_a_case",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message,
            primary_span: p.span.clone(),
            related: vec![Related::new(
                hir.decl_span(id),
                format!("a policy of `{}`", decl.name),
            )],
            explanation: Some(
                "A page whose address is another address of the page, as a dictionary's \
                 simplified form is its traditional one's, answers a redirect there: 308 \
                 where the move is permanent, 307 where it is not. Only the page can say \
                 which of its queries' declared errors means that, and what it carries \
                 must fill the page's route (ADR-0295)."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: repair.to_string(),
                replacement: None,
            }],
        };
        if decl.kind != DeclKind::Page {
            out.push(refused(
                format!(
                    "`redirect_on {value}` is a page's: `{}` is not one",
                    decl.name
                ),
                "move it to the page whose address it is about",
            ));
            continue;
        }
        let (ty, case, _) = match crate::routes::redirect_case(ws, sigs, unit, decl) {
            Ok(Some(found)) => found,
            Ok(None) => continue,
            Err(why) => {
                out.push(refused(
                    format!("`redirect_on {value}`: {why}"),
                    "name a case of a declared error and how it moves: \
                     `KiokunError.Moved permanent`",
                ));
                continue;
            }
        };
        if let Ok(Some((absent, absent_case))) = crate::routes::not_found_case(ws, sigs, unit, decl)
            && absent == ty
            && absent_case == case
        {
            out.push(refused(
                format!(
                    "`redirect_on {value}`: `{case}` is the case `not_found_on` names, and a \
                     page that is absent is not elsewhere"
                ),
                "name the case that means the page is at another address",
            ));
            continue;
        }
        let Some(b) = decl.body else { continue };
        let answers = crate::page_values::query_bindings(ws, unit, hir.body(b))
            .iter()
            .any(|(_, read, _)| crate::routes::error_of(sigs, *read) == Some(ty));
        if !answers {
            out.push(refused(
                format!(
                    "`redirect_on {value}`: no query `{}` reads can answer it",
                    decl.name
                ),
                "name an error a query the page reads declares",
            ));
            continue;
        }
        // Where the host sends the reader: the page's route, its one `{name}`
        // filled with what the case carries.
        let holes = decl
            .policy("route")
            .map(|r| crate::routes::route_holes(&r.value))
            .unwrap_or_default();
        let [hole] = holes.as_slice() else {
            out.push(refused(
                format!(
                    "`redirect_on {value}`: `{}`'s route carries {} parameters, and a \
                     case carries one value to fill it",
                    decl.name,
                    holes.len()
                ),
                "give the page a route with one `{parameter}`",
            ));
            continue;
        };
        let carried = sigs
            .type_decl(ty)
            .and_then(|t| t.variants.as_ref())
            .and_then(|cases| cases.iter().find(|(c, _)| *c == case))
            .map(|(_, fields)| fields.as_slice())
            .unwrap_or_default();
        let page = crate::resolve::DefId { unit, decl: id.0 };
        let parameter = sigs.by_def(page).and_then(|sig| {
            let at = sig.names.iter().position(|n| n == hole)?;
            sig.params.get(at)?.as_ref()?.resolved()
        });
        let fits = match (carried, parameter) {
            ([one], Some(param)) => one.resolved().is_some_and(|t| t.same_as(param)),
            _ => false,
        };
        if !fits {
            out.push(refused(
                format!(
                    "`redirect_on {value}`: `{case}` carries {}, and `{}`'s route carries \
                     one `{hole}`{}",
                    match carried {
                        [] => "nothing".to_string(),
                        [one] => format!("a `{one}`"),
                        many => format!("{} values", many.len()),
                    },
                    decl.name,
                    match parameter {
                        Some(t) => format!(", a `{}`", t.display_name()),
                        None => String::new(),
                    },
                ),
                "carry one value of the type of the route's parameter: `Moved(String)`",
            ));
        }
    }
    out
}

/// The command `e` calls, if it is a call to one: its name as written, and
/// its definition.
fn command_called(
    body: &Body,
    sigs: &Signatures,
    unit: usize,
    e: ExprId,
) -> Option<(String, crate::resolve::DefId)> {
    let Expr::Call { callee, .. } = body.expr(e) else {
        return None;
    };
    let path = crate::infer::path_of(body, *callee);
    let def = crate::page_values::resolve_term(sigs.workspace(), unit, &path)?;
    (sigs.kind_of(def) == Some(DeclKind::Command)).then_some((path, def))
}

/// **A command is called only by a page's handler** (ADR-0157). It is one
/// request: its `requires`, `idempotent_by`, `transaction`, `invalidates` and
/// `emits` describe that request. Called from another declaration, its body
/// would run without them, so what it invalidates would stay stale and what
/// it emits would go unheard; and its caller is answered whether it
/// committed, which only a request is.
fn called_outside_a_handler(hir: &Hir, unit: usize, sigs: &Signatures) -> Vec<Diagnostic> {
    let code = crate::codes::COMMAND_OUTSIDE_A_HANDLER;
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        let Some(b) = decl.body else { continue };
        let body = hir.body(b);
        let sent: BTreeSet<ExprId> = crate::resume::handlers_in(body)
            .into_iter()
            .flat_map(|h| body.walk_from(h))
            .collect();
        for e in body.walk() {
            if sent.contains(&e) {
                continue;
            }
            let Some((command, _)) = command_called(body, sigs, unit, e) else {
                continue;
            };
            out.push(Diagnostic {
                code: code.id,
                invariant: code.invariant,
                reason: "command_outside_a_handler",
                detector: Detector::Signature,
                severity: Severity::Error,
                message: format!(
                    "`{command}` is a command, and `{}` calls it: only a page's handler \
                     sends a command",
                    decl.name
                ),
                primary_span: body.expr_span(e),
                related: Vec::new(),
                explanation: Some(
                    "A command is one request a page's handler sends: its `requires`, \
                     `idempotent_by`, `transaction`, `invalidates` and `emits` describe that \
                     request. Called from another declaration, its body would run without \
                     them: what it invalidates would stay stale, and what it emits would go \
                     unheard. And its caller is answered whether it committed, which only a \
                     request is (ADR-0157)."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: format!(
                        "move what `{}` shares with `{command}` into a `fn`, and call that",
                        decl.name
                    ),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// **What a command answers carries no value** (ADR-0157): `Ok`, or the
/// error it declares. A handler that binds the value binds nothing: it
/// reaches the page from the query the command invalidates, so the page has
/// one source for what it shows. Refused where it is written, in the
/// handler's terms, rather than by the backend, in the value's.
fn answer_read_for_a_value(hir: &Hir, unit: usize, sigs: &Signatures) -> Vec<Diagnostic> {
    use crate::lexical::{Binder, Lexical};
    let code = crate::codes::ANSWER_READ_FOR_A_VALUE;
    let declares_result = |def| {
        sigs.by_def(def)
            .and_then(|s| s.result())
            .and_then(|r| r.as_builtin())
            == Some(crate::resolved::Builtin::Result)
    };
    let refused = |message: String, at: hir::Span, repair: String| Diagnostic {
        code: code.id,
        invariant: code.invariant,
        reason: "answer_read_for_a_value",
        detector: Detector::Signature,
        severity: Severity::Error,
        message,
        primary_span: at,
        related: Vec::new(),
        explanation: Some(
            "A command's caller is answered `Ok`, or an error the command declares \
             (ADR-0157). The value it produced reaches the page from the query it \
             invalidates, so the page has one source for what it shows: a value read \
             from the answer would be a second, which the query's next change would \
             contradict."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: repair,
            replacement: None,
        }],
    };
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        let Some(b) = decl.body else { continue };
        let body = hir.body(b);
        let handlers = crate::resume::handlers_in(body);
        if handlers.is_empty() {
            continue;
        }
        let within: Vec<ExprId> = handlers.iter().flat_map(|h| body.walk_from(*h)).collect();
        // Each name a `let` binds to a command's answer, by its pattern.
        let mut bound: BTreeMap<hir::PatternId, String> = BTreeMap::new();
        for e in &within {
            if let Expr::Let {
                pat: Some(p),
                init: Some(i),
                ..
            } = body.expr(*e)
                && let HPat::Bind { name, .. } = body.pat(*p)
                && let Some((command, def)) = command_called(body, sigs, unit, *i)
            {
                // A command declared without a `Result` answers nothing at
                // all.
                if !declares_result(def) {
                    out.push(refused(
                        format!(
                            "`{name}` is bound to what `{command}` answered, which is \
                             nothing: `{command}` declares no `Result`"
                        ),
                        body.pat_span(*p),
                        format!(
                            "call `{command}` without binding its answer, and read the \
                             value from the page's query"
                        ),
                    ));
                }
                bound.insert(*p, command);
            }
        }
        let lexical = Lexical::build(decl, body);
        for e in &within {
            let Expr::Match { scrutinee, arms } = body.expr(*e) else {
                continue;
            };
            let answered = match command_called(body, sigs, unit, *scrutinee) {
                Some((command, def)) => declares_result(def).then_some(command),
                None => match lexical.binder(*scrutinee) {
                    Some(Binder::Pattern(p)) => bound.get(&p).cloned(),
                    _ => None,
                },
            };
            let Some(command) = answered else { continue };
            for arm in arms {
                for payload in ok_payloads(body, arm.pat) {
                    let what = match body.pat(payload) {
                        HPat::Bind { name, .. } => format!("`{name}` is bound to nothing"),
                        _ => "its pattern matches nothing".to_string(),
                    };
                    out.push(refused(
                        format!("`{command}` answers whether it committed, not its value: {what}"),
                        body.pat_span(payload),
                        "write `Ok(_)`, and read the value from the page's query".to_string(),
                    ));
                }
            }
        }
    }
    out
}

/// What an `Ok` in pattern `p` matches its payload with, through `|`, where
/// that is more than `_`. Read off the spelling because the value matched
/// is a command's answer, a `Result` (ADR-0157), whose cases are `Ok` and
/// `Err`.
fn ok_payloads(body: &Body, p: hir::PatternId) -> Vec<hir::PatternId> {
    match body.pat(p) {
        HPat::Ctor { path, args } if path == "Ok" || path == "Result.Ok" => args
            .iter()
            .copied()
            .filter(|a| !matches!(body.pat(*a), HPat::Wild))
            .collect(),
        HPat::Or(ps) => ps.iter().flat_map(|q| ok_payloads(body, *q)).collect(),
        _ => Vec::new(),
    }
}

/// **A handler navigates once its command commits** (ADR-0280): `navigate
/// Page(args)` names a page (PW5042), whose parameters type its arguments as
/// a call's are, and is written in a handler, last in the `Ok` arm of the
/// command's answer nearest it (PW5043). An `Ok` is a commit, so the page is
/// left only once one is made; an `Err` arm, a speculation, or code before
/// the answer would leave it on a refusal or before it.
/// **Markup is no operand** (ADR-0243's amendment of 2026-10-09). A view's
/// markup is shown where it is written: `<p>a</p> + 1` checked, rendered the
/// paragraph, and dropped the sum, and `<h1>a</h1><p>b</p>` was read as that
/// until two roots on one line became one region. An arithmetic, comparison
/// or logical operator over markup is refused where it is written.
fn markup_as_an_operand(hir: &Hir) -> Vec<Diagnostic> {
    use crate::hir::{BinOp, UnOp};
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        let Some(b) = decl.body else { continue };
        let body = hir.body(b);
        for e in body.walk() {
            let operands: Vec<ExprId> = match body.expr(e) {
                Expr::Binary {
                    op:
                        BinOp::Add
                        | BinOp::Sub
                        | BinOp::Mul
                        | BinOp::Div
                        | BinOp::Rem
                        | BinOp::Cmp(_)
                        | BinOp::And
                        | BinOp::Or,
                    lhs,
                    rhs,
                } => vec![*lhs, *rhs],
                Expr::Unary {
                    op: UnOp::Neg | UnOp::Not,
                    operand,
                } => vec![*operand],
                _ => continue,
            };
            if !operands
                .iter()
                .any(|o| matches!(body.expr(*o), Expr::Template { .. }))
            {
                continue;
            }
            let code = crate::codes::MARKUP_AS_AN_OPERAND;
            out.push(Diagnostic {
                code: code.id,
                invariant: code.invariant,
                reason: "markup_as_an_operand",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: "markup is an operand here: it is shown where it is written, and \
                          what it is combined with is dropped"
                    .to_string(),
                primary_span: body.expr_span(e),
                related: Vec::new(),
                explanation: Some(
                    "A view's markup is not a value: the renderer shows each region where \
                     the view writes it, and an operator over one computes nothing anyone \
                     sees. Two elements on one line are one region (`<h1>a</h1><p>b</p>`); \
                     a value is shown inside markup, `<p>{a + 1}</p>`."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "write the value inside the markup, in braces".to_string(),
                    replacement: None,
                }],
            });
        }
    }
    out
}

fn navigations(
    ws: &crate::resolve::Workspace,
    hir: &Hir,
    unit: usize,
    sigs: &Signatures,
) -> Vec<Diagnostic> {
    use crate::lexical::{Binder, Lexical};
    use crate::resolve::{Namespace, Resolution};
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        let Some(b) = decl.body else { continue };
        let body = hir.body(b);
        let written: Vec<ExprId> = body
            .walk()
            .into_iter()
            .filter(
                |e| matches!(body.expr(*e), Expr::Keyword { keyword, .. } if keyword == "navigate"),
            )
            .collect();
        if written.is_empty() {
            continue;
        }
        // What each names: a page, by its declaration.
        for e in &written {
            let Expr::Keyword { modifiers, .. } = body.expr(*e) else {
                continue;
            };
            let name = modifiers.first().cloned().unwrap_or_default();
            // A page is rendered, so named among the views; what it names
            // otherwise is said by its kind.
            let kind = [Namespace::Ui, Namespace::Term].into_iter().find_map(|ns| {
                let found = match name.contains('.') {
                    true => ws.resolve_path_in(unit, ns, &name),
                    false => ws.resolve_in(unit, ns, &name),
                };
                match found {
                    Resolution::Local(def) | Resolution::Imported { def, .. } => sigs.kind_of(def),
                    _ => None,
                }
            });
            if kind == Some(DeclKind::Page) {
                continue;
            }
            let what = match kind {
                Some(k) => format!("names {}", described(k)),
                None if name.is_empty() => "names nothing".to_string(),
                None => "names nothing this module sees".to_string(),
            };
            let code = crate::codes::NAVIGATE_NAMES_NO_PAGE;
            out.push(Diagnostic {
                code: code.id,
                invariant: code.invariant,
                reason: "navigate_names_no_page",
                detector: Detector::Signature,
                severity: Severity::Error,
                message: format!("`navigate {name}` {what}: a navigation goes to a page"),
                primary_span: body.expr_span(*e),
                related: Vec::new(),
                explanation: Some(
                    "`navigate` takes the page by its declaration, and its arguments as the \
                     page's parameters, so the address it goes to is one the page is served \
                     at, with values of the types its route gives."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "name a page this module declares or imports".to_string(),
                    replacement: None,
                }],
            });
        }
        // Where each may be: last in the `Ok` arm of the command's answer
        // nearest it, in a handler.
        let handlers = crate::resume::handlers_in(body);
        let lexical = Lexical::build(decl, body);
        // Each name a `let` binds to a command's answer, by its pattern.
        let mut answers: BTreeSet<hir::PatternId> = BTreeSet::new();
        for h in &handlers {
            for e in body.walk_from(*h) {
                if let Expr::Let {
                    pat: Some(p),
                    init: Some(i),
                    ..
                } = body.expr(e)
                    && command_called(body, sigs, unit, *i).is_some()
                {
                    answers.insert(*p);
                }
            }
        }
        let is_answer = |e: ExprId| {
            command_called(body, sigs, unit, e).is_some()
                || matches!(lexical.binder(e), Some(Binder::Pattern(p)) if answers.contains(&p))
        };
        let mut allowed = BTreeSet::new();
        for h in &handlers {
            let start = match body.expr(*h) {
                Expr::Lambda { body: inner, .. } => *inner,
                _ => *h,
            };
            after_a_commit(body, &is_answer, start, false, &mut allowed);
        }
        for e in written {
            if allowed.contains(&e) {
                continue;
            }
            let inside = handlers.iter().any(|h| body.walk_from(*h).contains(&e));
            let message = match inside {
                false => "`navigate` is written outside a handler: a page is left by a \
                          handler, once its command commits"
                    .to_string(),
                true => "`navigate` is not last in the `Ok` arm of a command's answer: a \
                         page is left only once its command has committed, and nothing \
                         runs after it"
                    .to_string(),
            };
            let code = crate::codes::NAVIGATE_BEFORE_A_COMMIT;
            out.push(Diagnostic {
                code: code.id,
                invariant: code.invariant,
                reason: "navigate_before_a_commit",
                detector: Detector::Signature,
                severity: Severity::Error,
                message,
                primary_span: body.expr_span(e),
                related: Vec::new(),
                explanation: Some(
                    "A command's `Ok` is its commit (ADR-0157). Written there, last, the \
                     navigation follows it: the page it goes to is read after the commit, \
                     and a refusal, whose `Err` arm runs, leaves the page where it is."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "write it last in the `Ok` arm of `match command(..) { .. }`, \
                                  or link to the page"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// Each `navigate` in `e` that is last in the `Ok` arm of the command's
/// answer nearest it (`last`: `e` is in that arm's last place), into
/// `allowed` (ADR-0280). A block's last statement, each branch of an `if`
/// and each arm of a `match` on anything but an answer are in its place; an
/// answer's own `match` decides it again, by its arm; anything else is not.
fn after_a_commit(
    body: &Body,
    is_answer: &dyn Fn(ExprId) -> bool,
    e: ExprId,
    last: bool,
    allowed: &mut BTreeSet<ExprId>,
) {
    match body.expr(e) {
        Expr::Keyword { keyword, .. } if keyword == "navigate" => {
            if last {
                allowed.insert(e);
            }
        }
        Expr::Match { scrutinee, arms } => {
            after_a_commit(body, is_answer, *scrutinee, false, allowed);
            let answer = is_answer(*scrutinee);
            for arm in arms {
                let place = match answer {
                    true => is_ok(body, arm.pat),
                    false => last,
                };
                after_a_commit(body, is_answer, arm.body, place, allowed);
            }
        }
        Expr::Block { stmts } => {
            for (i, s) in stmts.iter().enumerate() {
                after_a_commit(body, is_answer, *s, last && i + 1 == stmts.len(), allowed);
            }
        }
        Expr::If { cond, then, els } => {
            after_a_commit(body, is_answer, *cond, false, allowed);
            after_a_commit(body, is_answer, *then, last, allowed);
            if let Some(els) = els {
                after_a_commit(body, is_answer, *els, last, allowed);
            }
        }
        _ => {
            for c in body.children(e) {
                after_a_commit(body, is_answer, c, false, allowed);
            }
        }
    }
}

/// Whether an arm's pattern is the answer's `Ok`, through `|`.
fn is_ok(body: &Body, p: hir::PatternId) -> bool {
    match body.pat(p) {
        HPat::Ctor { path, .. } => path == "Ok" || path == "Result.Ok",
        HPat::Or(ps) => !ps.is_empty() && ps.iter().all(|q| is_ok(body, *q)),
        _ => false,
    }
}

/// Uses of a name nothing declares.
///
/// Import failures alone are not enough. `R-004` materializes a `session query
/// Cart` declared in a file it never imports: with only import checking, that
/// is silence — and silence is what let the fixture look uncaught rather than
/// unresolved.
///
/// Deliberately conservative about what counts as a use: a bare name that is
/// not a local binding, and a qualified path whose head names a module. A
/// method call on a value (`line.item_name`) is a field access, not a path, and
/// is left alone until types exist.
fn unresolved_uses(
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    use crate::resolve::Resolution;

    let mut out = Vec::new();
    // A declaration can call itself and its siblings. An `import` binds
    // nothing by its name, as the name check reads it: until ADR-0226 it put
    // its module's name in scope here, so `String.nope(s)` was a local's
    // field and checked clean wherever `String` was imported.
    // Nor does a table (track `tables`): its name is its module's, and
    // `Shops.all` in `module Shops` is a call into the module, resolved
    // below, not a field of a local.
    let declared: std::collections::BTreeSet<String> = hir
        .all_decls()
        .filter(|(_, d)| {
            !matches!(
                d.kind,
                crate::hir::DeclKind::Import | crate::hir::DeclKind::Table
            )
        })
        .map(|(_, d)| d.name.clone())
        .collect();
    for (decl_id, decl) in hir.all_decls() {
        // An interface's terms too, in their own arena (ADR-0240).
        let Some(body_id) = decl.terms_body() else {
            continue;
        };
        let body = hir.body(body_id);
        // Which binding a callee's name means where the call is written: a
        // call to a function value out of its binding's scope names nothing
        // (ADR-0066). The set below held every name the body bound anywhere,
        // so such a call passed.
        let lexical = crate::lexical::Lexical::build_in(hir, decl_id);

        let mut in_scope = crate::resolve::local_bindings(body);
        in_scope.extend(decl.params.iter().map(|p| p.name.clone()));
        in_scope.extend(declared.iter().cloned());

        // **The body, PLUS every embedded transition.**
        //
        // Architect ruling, 2026-08-10:
        //
        // > Being inside a policy never exempts an executable term from
        // > resolution. […] There should not be a separate
        // > "optimistic-expression resolver."
        //
        // A transition is not reachable from `body.root` — deliberately, so
        // the declaration's effect row does not absorb code that runs in
        // another execution context — so it is added here by name rather than
        // found by walking. It is the SAME rule below, not a second one.
        let mut reachable = body.walk();
        for (policy, root) in decl.term_roots() {
            reachable.extend(body.walk_from(root.root));
            // A listener's argument names a parameter or `_`, and its own
            // rule says so (PW5104, ADR-0091).
            if root.context == crate::hir::ExecutionContext::Listener {
                continue;
            }

            // **Every name inside a named root must come from somewhere.**
            //
            // Architect ruling, 2026-08-10, locking the semantic requirement:
            //
            // > Every identifier visible inside optimistic/rollback code must
            // > arise from ordinary lexical scope or from an explicit binder in
            // > the construct itself. No special-name lookup.
            //
            // The binders are the ROOT's own, so they scope to it and nothing
            // else. An optimistic clause's target is a separate root with no
            // binders — it is evaluated before the binding exists, so
            // `Cart(cart)` names nothing, which is correct.
            let mut scope = in_scope.clone();
            scope.extend(root.binders.iter().map(|(n, _)| n.clone()));
            scope.extend(crate::resolve::local_bindings_from(body, root.root));

            // An optimistic clause's target may leave a key unnamed, `_`:
            // the speculation reaches the entry the page shows, whatever its
            // key there (ADR-0222, ruling 0105-a).
            let wildcards = target_wildcards(body, root);
            // An `invalidates` key's argument written `_` is every value at
            // its position (ADR-0256, ADR-0195's ruling 10). An `emits` key's
            // is no value: an event is given one at each parameter.
            if every_value(body, root.root) {
                match root.context {
                    crate::hir::ExecutionContext::Invalidated => continue,
                    crate::hir::ExecutionContext::Emitted => {
                        out.push(emitted_no_value(
                            hirs, workspace, unit, hir, decl, body, root.root,
                        ));
                        continue;
                    }
                    _ => {}
                }
            }
            // A module a path starts with, `Carts` in `Carts.with_line(..)`,
            // is no term: the path is the qualified call's, below. Until
            // ADR-0226 an import's name was in scope here, which is what let
            // it pass, and every member of an imported module with it.
            let heads: BTreeSet<ExprId> = body
                .walk_from(root.root)
                .into_iter()
                .filter_map(|x| match body.expr(x) {
                    Expr::Field { base, .. } => match body.expr(*base) {
                        Expr::Name(h) if workspace.sees_module(unit, h) => Some(*base),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            for nid in body.walk_from(root.root) {
                let Expr::Name(n) = body.expr(nid) else {
                    continue;
                };
                if wildcards.contains(&nid) || heads.contains(&nid) {
                    continue;
                }
                if scope.contains(n)
                    || crate::resolve::INTRINSIC_CALLS.contains(&n.as_str())
                    || !matches!(
                        workspace.resolve(unit, n),
                        crate::resolve::Resolution::Unresolved
                    )
                {
                    continue;
                }
                out.push(unresolved_in_policy_term(
                    hir,
                    decl,
                    body,
                    nid,
                    n,
                    &policy.name,
                ));
            }
        }

        for id in reachable {
            let Expr::Call { callee, .. } = body.expr(id) else {
                continue;
            };
            let path = path_of(body, *callee);
            // A name some scope binds is the value it holds, whatever a
            // declaration elsewhere is called (ADR-0066).
            let bound = match &lexical {
                Some(lx) if matches!(body.expr(*callee), Expr::Name(_)) => {
                    lx.binder(*callee).is_some()
                }
                _ => path
                    .split('.')
                    .next()
                    .is_some_and(|h| in_scope.contains(h) && !declared.contains(h)),
            };
            if !bound && let Some(def) = names_no_term(workspace, hirs, unit, &path) {
                out.push(not_a_term(hirs, hir, decl, body, id, &path, def));
                continue;
            }
            let Some((head, _)) = path.split_once('.') else {
                // **A BARE call.**
                //
                // Examined here since 2026-08-10, and not before — which is why
                // four accepted programs called names that do not exist and
                // checked clean since E4. Every analysis produces an answer for
                // a call it cannot resolve: inference contributes no effects,
                // privacy no label, placement no constraint, the contract no
                // capability. Each of those is indistinguishable from *this
                // call is harmless*.
                //
                // The subject is narrow on purpose. Two earlier attempts at
                // this rule reported sixteen and then eleven corpus errors and
                // were reverted rather than landed over-broad, because a
                // call-shaped construct can have real meaning that resolution
                // does not supply: a language constructor, a policy operator, a
                // CSS value function. `tests/semantic_ownership.rs` is the gate
                // that made the class small enough to report — it names every
                // owner, and the residue it leaves is what this reports.
                // **A bare call two whole modules declare** (ADR-0326): the
                // lowering refused it, and nothing here did, so a program
                // checked clean that did not build. Named where it is
                // written, with each module.
                if !bound
                    && matches!(body.expr(*callee), Expr::Name(_))
                    && let Resolution::Ambiguous(defs) =
                        workspace.resolve_in(unit, crate::resolve::Namespace::Term, &path)
                {
                    out.push(ambiguous_bare_call(
                        workspace, hir, decl, body, id, &path, &defs,
                    ));
                    continue;
                }
                let unowned = match &lexical {
                    // A local in scope where the call is written is the value
                    // it holds; any other name must be a declaration's.
                    Some(lx) if matches!(body.expr(*callee), Expr::Name(_)) => {
                        lx.binder(*callee).is_none()
                            && bare_call_is_unowned(workspace, unit, &path, &declared)
                    }
                    _ => bare_call_is_unowned(workspace, unit, &path, &in_scope),
                };
                if unowned {
                    // `Circle(3)`: a case, alone. The types it sees that
                    // declare one: one is the case's type, as for a case
                    // without a payload (ADR-0198, ADR-0195's ruling 5);
                    // several are named, and none is a name that resolves
                    // to nothing.
                    // In declaration order, as a case without a payload
                    // names them.
                    let owners: Vec<String> = workspace
                        .visible_types(unit)
                        .into_iter()
                        .filter_map(|def| Some((def, crate::resolve::declaration(hirs, def)?)))
                        .filter(|(_, d)| {
                            d.variants
                                .as_ref()
                                .is_some_and(|vs| vs.iter().any(|v| v.name == path))
                        })
                        .collect::<BTreeMap<_, _>>()
                        .into_values()
                        .map(|d| d.name.clone())
                        .collect();
                    // Several: the typer's, by the type expected where the
                    // call is written (ADR-0201).
                    if owners.is_empty() {
                        // In a table's own module, a read or a write it does
                        // not give (track `tables`).
                        match crate::tables::undeclared(
                            workspace,
                            hirs,
                            unit,
                            None,
                            &path,
                            body.expr_span(id),
                        ) {
                            Some(d) => out.push(d),
                            None => out.push(unresolved_bare_call(hir, decl, body, id, &path)),
                        }
                    }
                }
                continue;
            };
            // Only a head that looks like a module: either the workspace has
            // it, or it is capitalised and is not a local. Anything else is a
            // field access on a value.
            let known_module = workspace.sees_module(unit, head);
            let module_shaped = head.chars().next().is_some_and(char::is_uppercase)
                || matches!(
                    head,
                    "secrets" | "style" | "clock" | "log" | "database" | "dom"
                );
            if in_scope.contains(head) || (!known_module && !module_shaped) {
                continue;
            }
            // `StoreError.DecodeFailed` is a CONSTRUCTOR on a type in scope,
            // not a member of a module. Both are written `Head.member`, and
            // treating the second as the first reported every union constructor
            // in the corpus as undeclared.
            if !known_module
                && matches!(
                    workspace.resolve(unit, head),
                    Resolution::Local(_) | Resolution::Imported { .. }
                )
            {
                continue;
            }
            if workspace.resolve_path(unit, &path) != Resolution::Unresolved {
                continue;
            }
            // A read or a write a table does not give (track `tables`):
            // its own refusal, which names what it gives.
            if let Some((head, member)) = path.rsplit_once('.')
                && let Some(d) = crate::tables::undeclared(
                    workspace,
                    hirs,
                    unit,
                    Some(head),
                    member,
                    body.expr_span(id),
                )
            {
                out.push(d);
                continue;
            }

            out.push(Diagnostic {
                code: crate::codes::UNRESOLVED_NAME.id,
                invariant: crate::codes::UNRESOLVED_NAME.invariant,
                reason: "unresolved_use",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!("`{path}` does not resolve"),
                primary_span: body.expr_span(id),
                related: vec![Related::new(
                    hir.decl_span(decl_id_of(hir, decl)),
                    format!("used inside `{}`", decl.name),
                )],
                explanation: Some(format!(
                    "`{head}` is not a module this file can see. Names are visible \
                     through lexical scope, module membership, or an explicit import \
                     — external implementation is allowed, a missing declaration is \
                     not."
                )),
                repairs: vec![Repair {
                    description: format!("add `import {head}`, or declare it"),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// **What a call names, when it names no term** (ADR-0087): the declaration
/// a view, a page, an event or an effect is, where no function or type has
/// the name. A call names a term, or a type it builds (`resolve::Namespace`);
/// the rest are rendered, emitted or performed, and never called. A
/// materialization is a term since ADR-0273, read by `query`, and called by
/// nothing still: the materializer runs it.
fn names_no_term(
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    path: &str,
) -> Option<crate::resolve::DefId> {
    use crate::resolve::{Namespace, Resolution};
    let found = |ns| match path.contains('.') {
        true => workspace.resolve_path_in(unit, ns, path),
        false => workspace.resolve_in(unit, ns, path),
    };
    if let Resolution::Local(d) | Resolution::Imported { def: d, .. } = found(Namespace::Term)
        && crate::resolve::declaration(hirs, d).is_some_and(|x| x.kind == DeclKind::Materialize)
    {
        return Some(d);
    }
    if [Namespace::Term, Namespace::Type]
        .into_iter()
        .any(|ns| found(ns) != Resolution::Unresolved)
    {
        return None;
    }
    [Namespace::Ui, Namespace::Event, Namespace::Effect]
        .into_iter()
        .find_map(|ns| match found(ns) {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => Some(d),
            _ => None,
        })
}

fn not_a_term(
    hirs: &[&Hir],
    hir: &Hir,
    decl: &Decl,
    body: &Body,
    id: ExprId,
    path: &str,
    def: crate::resolve::DefId,
) -> Diagnostic {
    let kind = crate::resolve::declaration(hirs, def).map(|d| d.kind);
    let (what, how) = match kind {
        Some(DeclKind::View) => ("a view", "Markup renders a view; it is not called."),
        Some(DeclKind::Component) => (
            "a component",
            "Markup renders a component; it is not called.",
        ),
        Some(DeclKind::Page) => (
            "a page",
            "A page is reached by its route; it is not called.",
        ),
        Some(DeclKind::Layout) => (
            "a layout",
            "A page is shown in its layout, which it names with `layout`; it is not called.",
        ),
        Some(DeclKind::Materialize) => (
            "a materialization",
            "The materializer decides when a materialization runs, from the \
             dependency graph; nothing in a program calls one.",
        ),
        Some(DeclKind::Event) => (
            "an event",
            "A command emits an event by declaring `emits`, and a resource \
             listens with `invalidates_on`; an event is never a value.",
        ),
        _ => (
            "an effect",
            "An effect is performed by calling an operation whose row declares \
             it; its name appears only in effect rows.",
        ),
    };
    Diagnostic {
        code: crate::codes::NOT_A_TERM.id,
        invariant: crate::codes::NOT_A_TERM.invariant,
        reason: "not_a_term",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: format!("`{path}` is {what}, which a call cannot name"),
        primary_span: body.expr_span(id),
        related: vec![Related::new(
            hir.decl_span(decl_id_of(hir, decl)),
            format!("called inside `{}`", decl.name),
        )],
        // Until 2026-09-26 such a call checked: every analysis answered for
        // it as it does for a call to nothing.
        explanation: Some(format!(
            "A call names a function, a data operation, or a type it builds. \
             {how} Every analysis would answer for such a call as it does for a \
             call to nothing, and the value it is said to produce would have no \
             type."
        )),
        repairs: vec![Repair {
            description: format!("use `{path}` as {what} is used, or call a function"),
            replacement: None,
        }],
    }
}

/// **A policy's value is one its domain has** (ADR-0089).
///
/// The table in `crate::policy` says what each head's value is. Nothing held
/// a value to it until 2026-09-26: `cache Shared` checked, and the rule that
/// keeps a session's data out of a shared cache read it as no shared cache;
/// `placement originn` checked, and the declared world was dropped for a
/// derived one; `retry nope(..)` and `key nope` checked. A word, a duration, a
/// world, a flag and an operator's arguments are the table's to judge; a
/// parameter, a type and a declaration a value names are resolved here.
fn policy_values(workspace: &crate::resolve::Workspace, unit: usize, hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        for p in &decl.policies {
            out.extend(policy_value(
                workspace,
                unit,
                hir,
                id,
                &p.name,
                p.value.trim(),
                &p.span,
                "a policy of",
            ));
        }
    }
    out
}

/// **A clause written in a block is judged as one heading a declaration
/// is** (ADR-0247), by the same table: the words of ruling 0047-a, checked
/// against the closed set. The names check reads such a clause, `scope
/// component` in a resource's block, and until ADR-0247 nothing judged what
/// it held: `scope bogus` checked, and the scope graph, which reads it,
/// found nothing to refuse.
fn clause_values(
    workspace: &crate::resolve::Workspace,
    unit: usize,
    hir: &Hir,
    clauses: &[crate::names::Clause],
) -> Vec<Diagnostic> {
    clauses
        .iter()
        .filter_map(|c| {
            policy_value(
                workspace,
                unit,
                hir,
                c.decl,
                &c.head,
                &c.value,
                &c.span,
                "a clause in",
            )
        })
        .collect()
}

/// `head value`, written in the declaration `id` at `span`, against
/// `head`'s domain: PW0335 where the domain has no such value.
#[allow(clippy::too_many_arguments)]
fn policy_value(
    workspace: &crate::resolve::Workspace,
    unit: usize,
    hir: &Hir,
    id: crate::hir::DeclId,
    head: &str,
    value: &str,
    span: &crate::hir::Span,
    of: &str,
) -> Option<Diagnostic> {
    use crate::policy::{Domain, ValueFault};
    use crate::resolve::Resolution;
    let decl = hir.decl(id);
    // A table's `key` names its row's fields, which `tables::check` holds
    // (track `tables`); a query's names its parameters.
    let fault = match crate::policy::domain_on(head, decl.kind) {
        Some(domain @ (Domain::ParamRef | Domain::ParamList)) => value
            .split(',')
            .map(str::trim)
            .find(|n| {
                !decl.params.iter().any(|q| q.name == *n)
                    && !(domain == Domain::ParamRef
                        && crate::privacy::Label::PARTITIONS.contains(n))
            })
            .map(|n| format!("`{n}` is not a parameter of `{}`", decl.name)),
        // Authorization predicates are deployment vocabulary, not Pleris
        // declarations. Their arguments ARE Pleris command parameters,
        // and must bind to the values the host will receive. Anything
        // richer would execute application code before authorization.
        Some(Domain::PredicateRef) => {
            if decl.kind != crate::hir::DeclKind::Command {
                Some(
                    "`requires` is an invocation precondition and belongs to a command".to_string(),
                )
            } else {
                match crate::policy::predicates(value) {
                    Err(why) => Some(why),
                    Ok(predicates) => predicates.into_iter().find_map(|predicate| {
                        predicate.arguments.into_iter().find_map(|argument| {
                            (!decl.params.iter().any(|p| p.name == argument)).then(|| {
                                format!(
                                    "`{}` receives command parameters, and `{argument}` is not a parameter of `{}`",
                                    predicate.name, decl.name
                                )
                            })
                        })
                    }),
                }
            }
        }
        // Resolved as a written type is, so the language's own types
        // are types too: `idempotent_by Int` was "no type visible
        // here" when only declarations were asked (ADR-0089).
        Some(Domain::TypeRef) => {
            let resolved = crate::lower::type_fragment(value).is_some_and(|t| {
                crate::resolved::resolve(workspace, unit, None, &decl.type_params, &t, span.clone())
                    .resolved()
                    .is_some()
            });
            (!resolved).then(|| format!("`{value}` names no type visible here"))
        }
        Some(Domain::DeclRef) => (workspace.resolve(unit, value) == Resolution::Unresolved)
            .then(|| format!("`{value}` names no declaration visible here")),
        _ => crate::policy::value_fault(head, value).map(|f| match f {
            ValueFault::Word(words) => format!("`{value}` is not one of {}", listed(words)),
            ValueFault::Duration => {
                format!("`{value}` is not a duration: a count and a unit, `30.seconds`")
            }
            ValueFault::Length => {
                format!("`{value}` is not a length: a count and a CSS unit, `24.px`")
            }
            ValueFault::ZeroBudget => {
                format!("`{value}` is no time: every request would end before it starts")
            }
            ValueFault::World(w) => format!(
                "`{w}` is not a world: {}",
                listed(
                    &crate::placement::ALL_WORLDS
                        .iter()
                        .map(|w| w.name())
                        .collect::<Vec<_>>()
                )
            ),
            ValueFault::Flag => format!("`{head}` takes no value"),
            ValueFault::Count => format!(
                "`{value}` is not a count: a whole number greater than zero, as digits, \
                 `5_000_000`"
            ),
            ValueFault::HostOp(why) => format!(
                "`{value}` is no operation a host provides, \
                 `\"namespace:package/interface#name\"`: {why}"
            ),
            ValueFault::Operator(name, ops) => {
                format!(
                    "`{name}` is none of `{}`'s operators: {}",
                    head,
                    listed(&ops)
                )
            }
            ValueFault::Unclosed(name) => {
                format!("`{name}`'s arguments are not closed with `)`")
            }
            ValueFault::UnknownArgument(op, a) => format!("`{op}` takes no argument `{a}`"),
            ValueFault::ArgumentTwice(op, a) => format!("`{op}` is given `{a}` twice"),
            ValueFault::MissingArgument(op, a) => format!("`{op}` is not given `{a}`"),
            ValueFault::ArgumentKind(op, a, kind, v) => {
                format!("`{op}`'s `{a}` is {}, and this is `{v}`", kind.describe())
            }
            ValueFault::Positional(op, v) => {
                format!("`{op}` takes its arguments by name, and `{v}` names none")
            }
            ValueFault::ArgumentWord(op, v, words) => {
                format!("`{v}` is none of what `{op}` takes: {}", listed(words))
            }
        }),
    };
    let why = fault?;
    let written = match value {
        "" => head.to_string(),
        _ => format!("{head} {value}"),
    };
    Some(Diagnostic {
        code: crate::codes::POLICY_VALUE.id,
        invariant: crate::codes::POLICY_VALUE.invariant,
        reason: "policy_value",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: format!("`{written}`: {why}"),
        primary_span: span.clone(),
        related: vec![Related::new(
            hir.decl_span(id),
            format!("{of} `{}`", decl.name),
        )],
        // Until 2026-09-26 such a value checked, and each reader of the clause
        // decided alone what it meant.
        explanation: Some(
            "A policy's value is one its domain has: a word the policy lists, a \
             duration, a length, a world, a parameter of the declaration, a type, an \
             authorization predicate over command parameters, or an operator given \
             its arguments by name, wherever the policy is written. A value the \
             compiler does not understand is not a policy nobody wrote: each reader \
             of the clause would decide alone what it meant, and `cache Shared` \
             would be no shared cache to the privacy rule, and no cache to the \
             manifest."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: "write a value the policy has".to_string(),
            replacement: None,
        }],
    })
}

/// **A clause belongs to a declaration that reads it** (ADR-0092, ADR-0216).
///
/// A command emits and invalidates; a resource or a materialization listens;
/// a materialization depends; a cached read is kept for its freshness; and so
/// on for every head (`crate::policy::declared_by`, which fails closed).
/// Until 2026-09-26 each graph clause checked on any declaration: the graph
/// drew a query's `emits`, which nothing emits, and left out a `fn`'s, which
/// is not in the graph. Until ADR-0216 every other head did: `freshness` on a
/// command, `retry` on a function.
fn clauses_in_place(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        for p in &decl.policies {
            let Some((kinds, whom)) = crate::policy::declared_by(&p.name) else {
                continue;
            };
            if kinds.contains(&decl.kind) {
                continue;
            }
            out.push(Diagnostic {
                code: crate::codes::CLAUSE_OUT_OF_PLACE.id,
                invariant: crate::codes::CLAUSE_OUT_OF_PLACE.invariant,
                reason: "clause_out_of_place",
                detector: Detector::ResourceGraph,
                severity: Severity::Error,
                message: format!(
                    "`{}` is {}, and `{}` belongs to {whom}",
                    decl.name,
                    described(decl.kind),
                    p.name
                ),
                primary_span: p.span.clone(),
                related: vec![Related::new(
                    hir.decl_span(id),
                    format!("`{}` is declared here", decl.name),
                )],
                explanation: Some(
                    "A clause is read by the declarations it is written for: a command \
                     emits and invalidates, a cached read is kept for its freshness and \
                     retried, a materialization depends. On another declaration nothing \
                     reads it, and the program would not do what it says (ADR-0216)."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: format!("move `{}` to {whom}, or remove it", p.name),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// A cached reader of the database, whose entries a write has to reach
/// (ADR-0101).
struct Reader {
    /// Its graph node, `Module.Name`.
    path: String,
    name: String,
    unit: usize,
    /// Its declaration's span, in its own unit's file (ADR-0321).
    span: hir::Span,
    /// The domains it reads: `Carts` for `database.read<Carts>`.
    reads: BTreeSet<String>,
    /// A materialization, rebuilt only when an event reaches it (ADR-0103).
    fragment: bool,
    /// For a materialization, each dependency that reads the database, with
    /// what it reads. Empty for a query.
    built_from: Vec<(String, BTreeSet<String>)>,
}

/// Every query, subscription and resource that reads the database and
/// declares no staleness window, and every materialization built from one
/// that reads it, whatever its window.
///
/// A positive `freshness` is left out: its entries may be that stale by its
/// own declaration, and expire. **(ruling needed)** A materialization does
/// not expire: it is rebuilt only when an event reaches it (ADR-0103).
fn cached_readers(
    inference: &crate::effects::Inference,
    graph: &crate::graph::Graph,
    hirs: &[&Hir],
) -> Vec<Reader> {
    let mut out = Vec::new();
    // What each resource reads, by its node, for the fragments built on it.
    let mut read_by: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if !matches!(
                decl.kind,
                DeclKind::Query | DeclKind::Subscription | DeclKind::Resource
            ) {
                continue;
            }
            let reads: BTreeSet<String> = inference
                .effective_effects(unit, hir, id)
                .iter()
                .filter_map(|e| crate::effects::database_domain(e, "read"))
                .map(str::to_string)
                .collect();
            if reads.is_empty() {
                continue;
            }
            let path = crate::graph::path_of(hir, id);
            read_by.insert(path.clone(), (decl.name.clone(), reads.clone()));
            if decl
                .policy("freshness")
                .is_some_and(|f| crate::policy::duration(&f.value).is_some_and(|ms| ms > 0))
            {
                continue;
            }
            out.push(Reader {
                path,
                name: decl.name.clone(),
                unit,
                span: hir.decl_span(id),
                reads,
                fragment: false,
                built_from: Vec::new(),
            });
        }
    }
    // A materialization may read another (ADR-0255, ADR-0195's ruling 10):
    // what one reads, the one reading it reads too. Each round reads what
    // the last made known, until none is added; the build refuses a cycle
    // (PW5109), and a round past their number would be one.
    let materializations: Vec<(usize, crate::hir::DeclId)> = hirs
        .iter()
        .enumerate()
        .flat_map(|(unit, hir)| {
            hir.all_decls()
                .filter(|(_, d)| d.kind == DeclKind::Materialize)
                .map(move |(id, _)| (unit, id))
        })
        .collect();
    for _ in 0..materializations.len() {
        let mut added = false;
        for &(unit, id) in &materializations {
            let hir = hirs[unit];
            let path = crate::graph::path_of(hir, id);
            // What it reads through what it depends on, and through its own
            // statements: a materialize block may hold them after its
            // policies.
            let reads: BTreeSet<String> = graph
                .edges
                .iter()
                .filter(|e| e.kind == crate::graph::EdgeKind::Reads && e.from == path)
                .filter_map(|e| read_by.get(&e.to))
                .flat_map(|(_, r)| r.iter().cloned())
                .chain(
                    inference
                        .effective_effects(unit, hir, id)
                        .iter()
                        .filter_map(|e| crate::effects::database_domain(e, "read"))
                        .map(str::to_string),
                )
                .collect();
            if reads.is_empty() {
                continue;
            }
            let entry = read_by
                .entry(path)
                .or_insert_with(|| (hir.decl(id).name.clone(), BTreeSet::new()));
            let before = entry.1.len();
            entry.1.extend(reads);
            added |= entry.1.len() > before;
        }
        if !added {
            break;
        }
    }
    for (unit, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if decl.kind != DeclKind::Materialize {
                continue;
            }
            let path = crate::graph::path_of(hir, id);
            let built_from: Vec<(String, BTreeSet<String>)> = graph
                .edges
                .iter()
                .filter(|e| e.kind == crate::graph::EdgeKind::Reads && e.from == path)
                .filter_map(|e| read_by.get(&e.to).cloned())
                .collect();
            // What it reads, the rounds above gathered.
            let Some((_, reads)) = read_by.get(&path).cloned() else {
                continue;
            };
            out.push(Reader {
                path,
                name: decl.name.clone(),
                unit,
                span: hir.decl_span(id),
                reads,
                fragment: true,
                built_from,
            });
        }
    }
    out
}

/// **A command invalidates what it writes** (ADR-0101).
///
/// Invalidation is explicit (ADR-0007): the platform learns that an entry
/// changed only from a command's `invalidates` and `emits`. A command that
/// performs `database.write<T>` reaches each cached reader of `T`, by name or
/// through an event the reader listens for. Until 2026-09-26 one that
/// declared neither checked, and every page kept the cart from before.
fn writes_invalidated(
    inference: &crate::effects::Inference,
    graph: &crate::graph::Graph,
    readers: &[Reader],
    speculated: &[Speculation],
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    use crate::graph::{EdgeKind, NodeKind};
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        if decl.kind != DeclKind::Command {
            continue;
        }
        let path = crate::graph::path_of(hir, id);
        if clauses_refused(graph, &path) {
            continue;
        }
        let writes: BTreeSet<String> = inference
            .effective_effects(unit, hir, id)
            .iter()
            .filter_map(|e| crate::effects::database_domain(e, "write"))
            .map(str::to_string)
            .collect();
        if writes.is_empty() {
            continue;
        }
        let sources = decl
            .body
            .map(|b| inference.infer_at(unit, hir.body(b)).sources)
            .unwrap_or_default();
        for r in readers {
            // An entry the command speculates on is PW5107's (ADR-0105).
            if speculated
                .iter()
                .any(|s| s.command == path && s.target == r.path)
            {
                continue;
            }
            let shared: Vec<&str> = writes.intersection(&r.reads).map(String::as_str).collect();
            // A fragment is rebuilt when an event reaches it, directly or
            // through what it reads (ADR-0102), and the materializer reads
            // no command's `invalidates` (ADR-0103).
            let fragment = r.fragment;
            let reached = if fragment {
                graph.edges.iter().any(|e| {
                    e.from == path
                        && e.kind == EdgeKind::Emits
                        && graph.affected_by(&e.to).contains(&r.path)
                })
            } else {
                graph.invalidates(&path, &r.path)
            };
            if shared.is_empty() || reached {
                continue;
            }
            let reader = if r.unit == unit {
                r.name.clone()
            } else {
                r.path.clone()
            };
            // The call that writes, when the body makes one.
            let at = sources
                .iter()
                .find(|s| crate::effects::database_domain(&s.effect, "write") == Some(shared[0]))
                .map(|s| s.span.clone())
                .unwrap_or_else(|| decl.name_span.clone());
            let mut related = vec![Related::new(
                hir.decl_span(id),
                format!("`{}` is the command", decl.name),
            )];
            if fragment {
                let via: Vec<&str> = r
                    .built_from
                    .iter()
                    .filter(|(_, reads)| shared.iter().any(|d| reads.contains(*d)))
                    .map(|(name, _)| name.as_str())
                    .collect();
                // What it is built from that reads the write, or itself.
                let source = if via.is_empty() {
                    "which reads it".to_string()
                } else {
                    format!("which is built from `{}`", via.join("`, `"))
                };
                // In its own file, which may be another module's (ADR-0321):
                // until 2026-10-12 a reader in another file had no note.
                related.push(Related::in_unit(
                    r.unit,
                    r.span.clone(),
                    if via.is_empty() {
                        format!("`{}` reads `{}`", r.name, shared.join("`, `"))
                    } else {
                        format!(
                            "`{}` is built from `{}`, which reads `{}`",
                            r.name,
                            via.join("`, `"),
                            shared.join("`, `")
                        )
                    },
                ));
                let events: Vec<String> = graph
                    .nodes
                    .iter()
                    .filter(|n| matches!(n.kind, NodeKind::Event))
                    .filter(|n| graph.affected_by(&n.path).contains(&r.path))
                    .map(|n| format!("`{}`", n.name))
                    .collect();
                out.push(Diagnostic {
                    code: crate::codes::WRITE_NOT_INVALIDATED.id,
                    invariant: crate::codes::WRITE_NOT_INVALIDATED.invariant,
                    reason: "write_not_invalidated",
                    detector: Detector::ResourceGraph,
                    severity: Severity::Error,
                    message: format!(
                        "`{}` writes `{}`, and no event it emits reaches `{reader}`, {source}",
                        decl.name,
                        shared.join("`, `")
                    ),
                    primary_span: at,
                    related,
                    explanation: Some(format!(
                        "A materialization is rebuilt only when an event reaches it: \
                         `regenerate` is `on_invalidation`, so it has no staleness window, \
                         and the materializer reads committed events, not a command's \
                         `invalidates`. `{reader}` reads what `{}` writes, itself or through \
                         what it depends on, so after the write it keeps what it rendered \
                         until an event reaches it, by its own `invalidates_on` or through \
                         what it depends on.",
                        decl.name
                    )),
                    repairs: vec![Repair {
                        description: if events.is_empty() {
                            format!(
                                "emit an event, and have `{reader}` listen for it with \
                                 `invalidates_on`"
                            )
                        } else {
                            format!("emit {}, which reaches `{reader}`", events.join(" or "))
                        },
                        replacement: None,
                    }],
                });
                continue;
            }
            related.push(Related::in_unit(
                r.unit,
                r.span.clone(),
                format!("`{}` reads `{}`", r.name, shared.join("`, `")),
            ));
            let events: Vec<String> = graph
                .edges
                .iter()
                .filter(|e| e.kind == EdgeKind::InvalidatedBy && e.from == r.path)
                .filter_map(|e| graph.node(&e.to).map(|n| format!("`{}`", n.name)))
                .collect();
            out.push(Diagnostic {
                code: crate::codes::WRITE_NOT_INVALIDATED.id,
                invariant: crate::codes::WRITE_NOT_INVALIDATED.invariant,
                reason: "write_not_invalidated",
                detector: Detector::ResourceGraph,
                severity: Severity::Error,
                message: format!(
                    "`{}` writes `{}` and does not invalidate `{reader}`, which reads it",
                    decl.name,
                    shared.join("`, `")
                ),
                primary_span: at,
                related,
                explanation: Some(format!(
                    "Invalidation is explicit (ADR-0007): the platform learns that an entry \
                     changed only from what a command declares. `invalidates` names a \
                     resource whose entries it drops, and `emits` a typed event, which drops \
                     the entries of each resource listening for it with `invalidates_on`. \
                     `{reader}` reads what `{}` writes and declares no staleness window, so \
                     after the write its entries, the page bindings they fill and the \
                     fragments built from them keep the value from before, and nothing \
                     refreshes them.",
                    decl.name
                )),
                repairs: vec![
                    Repair {
                        description: format!(
                            "declare `invalidates {}(..)` on `{}`",
                            r.name, decl.name
                        ),
                        replacement: None,
                    },
                    Repair {
                        description: if events.is_empty() {
                            format!(
                                "or emit an event, and have `{reader}` listen for it \
                                 with `invalidates_on`"
                            )
                        } else {
                            format!(
                                "or emit {}, which `{reader}` listens for",
                                events.join(" or ")
                            )
                        },
                        replacement: None,
                    },
                ],
            });
        }
    }
    out
}

/// Does a clause of `command` name nothing, or a declaration of another
/// kind? That is PW5100's or PW5103's to report, and what it meant to
/// invalidate is not known, so the rules that ask what a command reaches
/// set the command aside.
fn clauses_refused(graph: &crate::graph::Graph, command: &str) -> bool {
    use crate::graph::{EdgeKind, NodeKind};
    graph
        .dangling
        .iter()
        .any(|d| d.from == command && matches!(d.kind, EdgeKind::Invalidates | EdgeKind::Emits))
        || graph.edges.iter().any(|e| {
            e.from == command
                && match (e.kind, graph.node(&e.to).map(|n| &n.kind)) {
                    (EdgeKind::Invalidates, Some(NodeKind::Resource { .. }))
                    | (EdgeKind::Emits, Some(NodeKind::Event)) => false,
                    (kind, _) => matches!(kind, EdgeKind::Invalidates | EdgeKind::Emits),
                }
        })
}

/// The entry an optimistic clause speculates on (ADR-0105).
struct Speculation {
    /// The command's graph node, and the target's.
    command: String,
    target: String,
    /// The command's declaration.
    decl: hir::DeclId,
    /// The target resource, as written: `Cart`.
    written: String,
    /// The target entry, `Cart(current_session())`, and its clause.
    at: crate::diagnostics::Span,
    clause: crate::diagnostics::Span,
}

/// Every optimistic clause of a command in `unit`, with the resource its
/// target names. A target that names no resource is PW0331's.
fn speculations(
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
) -> Vec<Speculation> {
    use crate::resolve::{Namespace, Resolution};
    let hir = hirs[unit];
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        if decl.kind != DeclKind::Command {
            continue;
        }
        let Some(body) = decl.terms_body().map(|b| hir.body(b)) else {
            continue;
        };
        for (policy, target, _) in decl.optimistic_clauses() {
            let Expr::Call { callee, .. } = body.expr(target.root) else {
                continue;
            };
            let written = path_of(body, *callee);
            // As `resource_value_type` reads it: a bare name in the term
            // namespace, a qualified path whole.
            let resolution = match written.contains('.') {
                true => workspace.resolve_path(unit, &written),
                false => workspace.resolve_in(unit, Namespace::Term, &written),
            };
            let (Resolution::Local(def) | Resolution::Imported { def, .. }) = resolution else {
                continue;
            };
            if !crate::resolve::declaration(hirs, def).is_some_and(|d| {
                matches!(
                    d.kind,
                    DeclKind::Query | DeclKind::Resource | DeclKind::Subscription
                )
            }) {
                continue;
            }
            out.push(Speculation {
                command: crate::graph::path_of(hir, id),
                target: crate::graph::path_of(hirs[def.unit], hir::DeclId(def.decl)),
                decl: id,
                written,
                at: body.expr_span(target.root),
                clause: policy.span.clone(),
            });
        }
    }
    out
}

/// **A command invalidates the entry it speculates on** (ADR-0105).
///
/// ADR-0025: an optimistic transition shows a speculative value, and the
/// command's success reconciles it. Both backends reconcile through the
/// entry: the Marko backend gives a binding its command's result when the
/// command `invalidates` the query, and the dev server's page learns the new
/// value when an event reaches the entry. Until 2026-09-26 a command
/// speculating on `Cart` with neither clause checked, and the speculation
/// stayed on the page as if it had been committed.
fn speculation_reconciled(
    graph: &crate::graph::Graph,
    speculated: &[Speculation],
    hir: &Hir,
) -> Vec<Diagnostic> {
    use crate::graph::EdgeKind;
    let mut out = Vec::new();
    for s in speculated {
        if clauses_refused(graph, &s.command) || graph.invalidates(&s.command, &s.target) {
            continue;
        }
        let decl = hir.decl(s.decl);
        let events: Vec<String> = graph
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::InvalidatedBy && e.from == s.target)
            .filter_map(|e| graph.node(&e.to).map(|n| format!("`{}`", n.name)))
            .collect();
        out.push(Diagnostic {
            code: crate::codes::OPTIMISTIC_NOT_RECONCILED.id,
            invariant: crate::codes::OPTIMISTIC_NOT_RECONCILED.invariant,
            reason: "optimistic_not_reconciled",
            detector: Detector::ResourceGraph,
            severity: Severity::Error,
            message: format!(
                "`{}` speculates on `{}` and does not invalidate it",
                decl.name, s.written
            ),
            primary_span: s.at.clone(),
            related: vec![
                Related::new(s.clause.clone(), "the speculation".to_string()),
                Related::new(
                    hir.decl_span(s.decl),
                    format!("`{}` is the command", decl.name),
                ),
            ],
            explanation: Some(format!(
                "An optimistic transition shows a speculative value until the command \
                 answers (ADR-0025). If it succeeds, what it committed replaces the \
                 speculation, and that happens through the entry: a command that \
                 `invalidates` it, or emits an event it listens for. `{}` does neither \
                 for `{}`, so the speculation stays on the page as if it had been \
                 committed, whatever was.",
                decl.name, s.written
            )),
            repairs: vec![
                Repair {
                    description: format!(
                        "declare `invalidates {}(..)` on `{}`",
                        s.written, decl.name
                    ),
                    replacement: None,
                },
                Repair {
                    description: if events.is_empty() {
                        format!(
                            "or emit an event, and have `{}` listen for it with \
                             `invalidates_on`",
                            s.written
                        )
                    } else {
                        format!(
                            "or emit {}, which `{}` listens for",
                            events.join(" or "),
                            s.written
                        )
                    },
                    replacement: None,
                },
            ],
        });
    }
    out
}

/// **A cache key names each parameter its entry depends on** (ADR-0107).
///
/// A query's entry is shared by every call whose `key` is equal, and a
/// subscription's stream by every call whose `dedupe_by` is. A parameter the
/// body reads and the key leaves out gives one call another's answer:
/// `Other(1, 2)` and `Other(1, 3)`, keyed by `id`, share an entry. PW5004
/// holds a key to the privacy partitions a value depends on; this holds it
/// to the parameters. A read is any use of the parameter in the body, which
/// errs toward keying: a parameter read only for a trace is keyed too.
fn keys_name_what_is_read(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        if !matches!(
            decl.kind,
            DeclKind::Query | DeclKind::Subscription | DeclKind::Resource
        ) {
            continue;
        }
        let Some(body) = decl.body.map(|b| hir.body(b)) else {
            continue;
        };
        let Some(lexical) = crate::lexical::Lexical::build_in(hir, id) else {
            continue;
        };
        // Each parameter the body reads, with its first read.
        let mut read: BTreeMap<usize, ExprId> = BTreeMap::new();
        for e in body.walk() {
            if let Some(crate::lexical::Binder::Param(i)) = lexical.binder(e) {
                read.entry(i).or_insert(e);
            }
        }
        for head in ["key", "dedupe_by"] {
            let Some(p) = decl.policy(head) else {
                continue;
            };
            let named: Vec<&str> = p.value.split(',').map(str::trim).collect();
            // A component naming neither a parameter nor a partition is
            // PW0335's, and what the key meant is not known.
            if named.iter().any(|n| {
                !decl.params.iter().any(|q| q.name == *n)
                    && !crate::privacy::Label::PARTITIONS.contains(n)
            }) {
                continue;
            }
            for (&i, &at) in &read {
                let Some(param) = decl.params.get(i) else {
                    continue;
                };
                if named.contains(&param.name.as_str()) {
                    continue;
                }
                out.push(Diagnostic {
                    code: crate::codes::KEY_OMITS_A_READ.id,
                    invariant: crate::codes::KEY_OMITS_A_READ.invariant,
                    reason: "key_omits_a_read",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message: format!(
                        "`{}`'s `{head}` omits `{}`, which its body reads",
                        decl.name, param.name
                    ),
                    primary_span: p.span.clone(),
                    related: vec![Related::new(
                        body.expr_span(at),
                        format!("`{}` is read here", param.name),
                    )],
                    explanation: Some(format!(
                        "Every call whose `{head}` is equal shares one entry: a cached \
                         answer, or one stream. `{}` reads `{}`, so two calls that differ \
                         only in `{}` are given one answer, the first's.",
                        decl.name, param.name, param.name
                    )),
                    repairs: vec![Repair {
                        description: format!(
                            "key the entry by `{}` too: `{head} {}, {}`",
                            param.name,
                            p.value.trim(),
                            param.name
                        ),
                        replacement: None,
                    }],
                });
            }
        }
    }
    out
}

/// **A resumable handler reads what it captures** (ADR-0110).
///
/// A resumable handler runs later, in the browser, with what it captured and
/// nothing else: its captures are written into the document when the page
/// renders, and read back when it runs. A name bound where the page renders,
/// a parameter or an `{#each}` item, is not bound where the handler runs,
/// unless it is captured. A declaration's name is not a binding, and neither
/// is what the handler binds itself. Until 2026-09-26 such a handler checked
/// and `pw emit-handlers` refused it: "`item` is not bound here".
fn handlers_read_their_captures(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        let Some(body) = decl.body.map(|b| hir.body(b)) else {
            continue;
        };
        let Some(lexical) = crate::lexical::Lexical::build_in(hir, id) else {
            continue;
        };
        for lambda in body.walk() {
            let Expr::Lambda {
                descriptor: Some(d),
                body: inner,
                ..
            } = body.expr(lambda)
            else {
                continue;
            };
            if !crate::resume::is_resumable(body, *d) {
                continue;
            }
            let captured: BTreeSet<String> = crate::resume::capture_roots(body, &lexical, lambda)
                .into_keys()
                .collect();
            let own = crate::resolve::local_bindings_from(body, lambda);
            // A page's signal is the browser's, and the handler reaches it
            // through its context: it is not captured (ADR-0130).
            let signal = |b: Option<crate::lexical::Binder>| {
                b.is_some_and(|b| {
                    body.signals.iter().any(|s| {
                        matches!(body.expr(*s), Expr::Let { pat: Some(p), .. }
                            if b == crate::lexical::Binder::Pattern(*p))
                    })
                })
            };
            let mut reported: BTreeSet<&str> = BTreeSet::new();
            for e in body.walk_from(*inner) {
                // A name, and a record's shorthand field, `Pick { n: 1, item }`,
                // which reads one: each with whether a scope binds it.
                let reads: Vec<(&str, crate::diagnostics::Span, bool)> = match body.expr(e) {
                    Expr::Name(n) => {
                        let b = lexical.binder(e);
                        vec![(n.as_str(), body.expr_span(e), b.is_some() && !signal(b))]
                    }
                    Expr::Record { fields, .. } => fields
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.value.is_none())
                        .map(|(i, f)| {
                            let b = lexical.shorthand(e, i);
                            (f.name.as_str(), f.span.clone(), b.is_some() && !signal(b))
                        })
                        .collect(),
                    _ => continue,
                };
                for (n, at, bound) in reads {
                    if !bound || captured.contains(n) || own.contains(n) || !reported.insert(n) {
                        continue;
                    }
                    out.push(Diagnostic {
                        code: crate::codes::HANDLER_READS_UNCAPTURED.id,
                        invariant: crate::codes::HANDLER_READS_UNCAPTURED.invariant,
                        reason: "handler_reads_uncaptured",
                        detector: Detector::PatternMatrix,
                        severity: Severity::Error,
                        message: format!(
                            "`{}`'s handler reads `{n}`, which it does not capture",
                            decl.name
                        ),
                        primary_span: at,
                        related: vec![Related::new(
                            body.expr_span(*d),
                            "what the handler captures".to_string(),
                        )],
                        explanation: Some(format!(
                            "A resumable handler runs later, in the browser, with what it \
                         captured and nothing else: its captures are written into the \
                         document when the page renders, and read back when it runs. `{n}` \
                         is bound where `{}` renders, so where the handler runs it has no \
                         value. `pw emit-handlers` refuses the handler; this says so where \
                         the program is checked.",
                            decl.name
                        )),
                        repairs: vec![Repair {
                            description: format!("capture it: `resumable(captures = {{ {n} }})`"),
                            replacement: None,
                        }],
                    });
                }
            }
        }
    }
    out
}

/// **A resumable handler runs in the browser** (ADR-0113).
///
/// It is loaded and run there when its element is pressed, and it reaches
/// the origin through a command: the command is its own component, with its
/// own contract (`contract.rs`), so a call to one performs nothing here. What
/// the handler performs itself is the browser's to grant. Until 2026-09-26 a
/// handler calling `Carts.add` itself checked, and nothing asked where it
/// ran: the page's contract leaves its handlers out, and the handler has none.
/// A function named as a handler performs, in the browser, an effect the
/// browser cannot grant (ADR-0199): what `(e) => save(e)` would report.
fn named_handler_outside_the_browser(
    decl: &Decl,
    body: &Body,
    handler: ExprId,
    path: &str,
    effect: &str,
) -> Diagnostic {
    Diagnostic {
        code: crate::codes::DECLARED_PLACEMENT_CANNOT_GRANT.id,
        invariant: crate::codes::DECLARED_PLACEMENT_CANNOT_GRANT.invariant,
        reason: "handler_performs_outside_the_browser",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!(
            "`{}`'s handler `{path}` performs `{effect}`, which the browser cannot grant",
            decl.name
        ),
        primary_span: body.expr_span(handler),
        related: Vec::new(),
        explanation: Some(
            "A handler runs in the browser, and a function named as one runs there with \
             it: what it performs, the browser must grant. A command named as one is a \
             request, which the command performs where it runs (ADR-0199)."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: format!(
                "perform `{effect}` in a command, and name the command as the handler"
            ),
            replacement: None,
        }],
    }
}

fn handlers_run_in_the_browser(
    inference: &crate::effects::Inference,
    platform: &dyn Placements,
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    use crate::resolve::Resolution;
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        let Some(body) = decl.body.map(|b| hir.body(b)) else {
            continue;
        };
        // Every handler (ADR-0134), written `resumable(..)` or not.
        let handlers = crate::resume::handlers_in(body);
        if handlers.is_empty() {
            continue;
        }
        let lexical = crate::lexical::Lexical::build(decl, body);
        for lambda in handlers {
            // A declaration named as the handler (ADR-0199): a command is a
            // request, which it performs; any other runs here, and performs
            // what a call of it would, its row or, with none, what its body
            // does. A local's value is no handler, refused as one.
            if matches!(body.expr(lambda), Expr::Name(_) | Expr::Field { .. }) {
                if crate::resume::root_name(body, lambda)
                    .is_some_and(|r| lexical.binder(r).is_some())
                {
                    continue;
                }
                let path = path_of(body, lambda);
                let resolution = match path.contains('.') {
                    true => workspace.resolve_path(unit, &path),
                    false => workspace.resolve(unit, &path),
                };
                let (Resolution::Local(def) | Resolution::Imported { def, .. }) = resolution else {
                    continue;
                };
                if crate::resolve::declaration(hirs, def)
                    .is_none_or(|d| d.kind == DeclKind::Command)
                {
                    continue;
                }
                for effect in inference.effects_of_def(def).into_iter().flatten() {
                    if World::Browser.grants(effect, platform) == crate::placement::Grant::No {
                        out.push(named_handler_outside_the_browser(
                            decl, body, lambda, &path, effect,
                        ));
                    }
                }
                continue;
            }
            let Expr::Lambda { body: inner, .. } = body.expr(lambda) else {
                continue;
            };
            // The calls to a command: a request, which the command performs.
            let commands: Vec<crate::diagnostics::Span> = body
                .walk_from(*inner)
                .into_iter()
                .filter(|e| {
                    let Expr::Call { callee, .. } = body.expr(*e) else {
                        return false;
                    };
                    // A call through a local is the local's, never a
                    // command of its spelling's (ADR-0063).
                    if matches!(body.expr(*callee), Expr::Name(_))
                        && lexical.binder(*callee).is_some()
                    {
                        return false;
                    }
                    let path = path_of(body, *callee);
                    let resolution = match path.contains('.') {
                        true => workspace.resolve_path(unit, &path),
                        false => workspace.resolve(unit, &path),
                    };
                    matches!(
                        resolution,
                        Resolution::Local(def) | Resolution::Imported { def, .. }
                            if crate::resolve::declaration(hirs, def)
                                .is_some_and(|c| c.kind == DeclKind::Command)
                    )
                })
                .map(|e| body.expr_span(e))
                .collect();
            let mut reported = BTreeSet::new();
            for s in inference.infer_rooted(unit, body, *inner).sources {
                let requested = commands
                    .iter()
                    .any(|c| s.span.start >= c.start && s.span.end <= c.end);
                if requested
                    || World::Browser.grants(&s.effect, platform) != crate::placement::Grant::No
                    || !reported.insert(s.effect.clone())
                {
                    continue;
                }
                out.push(Diagnostic {
                    code: crate::codes::DECLARED_PLACEMENT_CANNOT_GRANT.id,
                    invariant: crate::codes::DECLARED_PLACEMENT_CANNOT_GRANT.invariant,
                    reason: "handler_performs_outside_the_browser",
                    detector: Detector::PatternMatrix,
                    severity: Severity::Error,
                    message: format!(
                        "`{}`'s handler performs `{}`, which the browser cannot grant",
                        decl.name, s.effect
                    ),
                    primary_span: s.span.clone(),
                    related: vec![Related::new(
                        body.expr_span(lambda),
                        "a resumable handler runs in the browser".to_string(),
                    )],
                    explanation: Some(format!(
                        "A resumable handler is loaded and run in the browser when its \
                         element is pressed. It reaches the origin through a command, \
                         which is its own component with its own contract, so what the \
                         command performs is not the handler's. `{}` is performed by the \
                         handler itself, and the browser cannot grant it.",
                        s.effect
                    )),
                    repairs: vec![Repair {
                        description: "move it into a command, and call the command".to_string(),
                        replacement: None,
                    }],
                });
            }
        }
    }
    out
}

/// **What is built before any request reads no request's value** (ADR-0114).
///
/// `placement build` makes a declaration's output a file produced before any
/// request exists (charter §9.3: "public, deterministic, build-known"), so a
/// parameter, which a request supplies, has no value when it is built. Until
/// 2026-09-26 a build-placed page rendering its `id` checked.
fn built_pages_read_no_parameter(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        if declared_world(hir, decl) != Some(World::Build) || decl.params.is_empty() {
            continue;
        }
        let Some(body) = decl.body.map(|b| hir.body(b)) else {
            continue;
        };
        let Some(lexical) = crate::lexical::Lexical::build_in(hir, id) else {
            continue;
        };
        let mut reported = BTreeSet::new();
        for e in body.walk() {
            let Some(crate::lexical::Binder::Param(i)) = lexical.binder(e) else {
                continue;
            };
            let Some(param) = decl.params.get(i) else {
                continue;
            };
            if !reported.insert(i) {
                continue;
            }
            out.push(Diagnostic {
                code: crate::codes::BUILT_BEFORE_ITS_PARAMETERS.id,
                invariant: crate::codes::BUILT_BEFORE_ITS_PARAMETERS.invariant,
                reason: "built_before_its_parameters",
                detector: Detector::PatternMatrix,
                severity: Severity::Error,
                message: format!(
                    "`{}` is built before any request, and reads `{}`, which a request supplies",
                    decl.name, param.name
                ),
                primary_span: body.expr_span(e),
                related: vec![Related::new(
                    hir.decl_span(id),
                    format!("`{}` is placed at build", decl.name),
                )],
                explanation: Some(format!(
                    "`placement build` makes `{}`'s output a file produced before any \
                     request exists, and served as it is to every one. `{}` is supplied by \
                     a request, so when the file is built it has no value, and no one \
                     value could be right for every reader.",
                    decl.name, param.name
                )),
                repairs: vec![Repair {
                    description: format!(
                        "place `{}` where requests are, or build it from what the build knows",
                        decl.name
                    ),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// **A `query` reads a query or a resource, and a `subscription` a
/// subscription** (ADR-0212, ruling 0108-a). `let v = query helper(id)` over
/// a `fn` checked, and the page kept the function's answer as an entry, with
/// a freshness, a key and a cache the function never declared.
fn reads_name_their_kind(
    ws: &crate::resolve::Workspace,
    hirs: &[&Hir],
    sigs: &Signatures,
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    use crate::resolve::{Namespace, Resolution};
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else {
            continue;
        };
        let body = hir.body(body_id);
        for id in body.walk() {
            let Expr::Keyword {
                keyword, modifiers, ..
            } = body.expr(id)
            else {
                continue;
            };
            let (reads, expected): (&[DeclKind], &str) = match keyword.as_str() {
                "query" => (
                    &[DeclKind::Query, DeclKind::Resource],
                    "a query or a resource",
                ),
                "subscription" => (&[DeclKind::Subscription], "a subscription"),
                _ => continue,
            };
            let Some(name) = modifiers.first() else {
                continue;
            };
            let found = match name.contains('.') {
                true => ws.resolve_path_in(unit, Namespace::Term, name),
                false => ws.resolve_in(unit, Namespace::Term, name),
            };
            let (Resolution::Local(def) | Resolution::Imported { def, .. }) = found else {
                continue;
            };
            let Some(kind) = sigs.kind_of(def) else {
                continue;
            };
            if reads.contains(&kind) {
                continue;
            }
            // **A materialization that derives its value is read as a query
            // is** (ADR-0273), by another that derives its own, and by a page,
            // which the host serves its kept value (ADR-0277): a public one,
            // kept for every reader. One that derives none is a fragment the
            // host renders.
            if kind == DeclKind::Materialize && keyword == "query" {
                let derives = sigs.by_def(def).is_some_and(|s| s.returns.is_some());
                let deriving = decl.kind == DeclKind::Materialize && decl.ret.is_some();
                let page = decl.kind == DeclKind::Page;
                let public = hirs
                    .get(def.unit)
                    .map(|h| h.decl(crate::hir::DeclId(def.decl)))
                    .and_then(|m| m.policy("partition"))
                    .is_some_and(|p| p.value.trim() == "public");
                if derives && (deriving || (page && public)) {
                    continue;
                }
                let (message, repair) = if derives && page {
                    (
                        format!(
                            "`query {name}` reads a materialization private to its reader, \
                             which a page does not read yet: the host keeps a public one"
                        ),
                        format!("make `{name}` `partition public`, or read the queries it reads"),
                    )
                } else if derives {
                    (
                        format!(
                            "`query {name}` reads a materialization, which {} does not read: \
                             a page reads one, or a materialization that derives its own",
                            described(decl.kind)
                        ),
                        "read the queries it reads".to_string(),
                    )
                } else {
                    (
                        format!(
                            "`query {name}` reads a materialization that declares no type: a \
                             fragment the host renders, which derives no value"
                        ),
                        format!("declare `{name}`'s type, and derive it in its body"),
                    )
                };
                out.push(Diagnostic {
                    code: crate::codes::READ_NAMES_ANOTHER_KIND.id,
                    invariant: crate::codes::READ_NAMES_ANOTHER_KIND.invariant,
                    reason: "read_names_another_kind",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message,
                    primary_span: body.expr_span(id),
                    related: Vec::new(),
                    explanation: Some(
                        "A materialization that declares its type derives it in its body, \
                         and another that derives its own reads it as a query is (ADR-0273). \
                         Until ADR-0273 no materialization was a value."
                            .to_string(),
                    ),
                    repairs: vec![Repair {
                        description: repair,
                        replacement: None,
                    }],
                });
                continue;
            }
            out.push(Diagnostic {
                code: crate::codes::READ_NAMES_ANOTHER_KIND.id,
                invariant: crate::codes::READ_NAMES_ANOTHER_KIND.invariant,
                reason: "read_names_another_kind",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!(
                    "`{keyword} {name}` reads {}, and a `{keyword}` reads {expected}",
                    described(kind)
                ),
                primary_span: body.expr_span(id),
                related: Vec::new(),
                explanation: Some(format!(
                    "A `{keyword}` is kept as an entry: its key, its freshness and \
                     its cache are what {expected} declares. {} declares none of \
                     them. Until ADR-0212 it was read as one all the same.",
                    capitalized(described(kind))
                )),
                repairs: vec![Repair {
                    description: match kind {
                        DeclKind::Fn => format!("call `{name}(..)` without `{keyword}`"),
                        _ => format!("name {expected}"),
                    },
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// **A materialization that derives its value states what it depends on by
/// reading it** (ADR-0273): `query R(..)` in its body, which the graph takes
/// as its edges, as a page's. A `depends_on` beside it would state a fact
/// the body states, a second place to keep it, and refused. One that
/// declares no type keeps its `depends_on`: a fragment the host renders.
fn dependencies_stated_once(hir: &Hir) -> Vec<Diagnostic> {
    hir.all_decls()
        .filter(|(_, d)| d.kind == DeclKind::Materialize && d.ret.is_some())
        .filter_map(|(_, d)| Some((d, d.policy("depends_on")?)))
        .map(|(d, p)| Diagnostic {
            code: crate::codes::DEPENDENCY_STATED_TWICE.id,
            invariant: crate::codes::DEPENDENCY_STATED_TWICE.invariant,
            reason: "dependency_stated_twice",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!(
                "`{}` derives its value, and states what it depends on by reading it: \
                 `depends_on` states it again",
                d.name
            ),
            primary_span: p.span.clone(),
            related: Vec::new(),
            explanation: Some(
                "A materialization that declares its type reads what it depends on in \
                 its body, `query R(..)`, and the graph takes those reads as its edges, \
                 as a page's (ADR-0273)."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: "remove the `depends_on`, and read each in the body".to_string(),
                replacement: None,
            }],
        })
        .collect()
}

/// **An opaque type's own module reads its representation as `.value`, so it
/// declares no member of that name** (ADR-0214, ADR-0210's urgent defect 8).
/// `browser` declared `fn value<T>(snapshot: LayoutSnapshot<T>) -> T`, and
/// in `browser` `snapshot.value` named both; `fn value(p: PositiveInt) ->
/// Int { p.value }` would call itself for ever.
/// **The handle a type holds, if any** (ADR-0263): a session's, a user's or
/// an organization's, wherever it sits: the type itself, an argument, a
/// record's field, a case's payload, an opaque type's representation. A
/// table's row holds none (PW6901, `tables::check`).
pub(crate) fn handle_in(
    sigs: &Signatures,
    ty: &crate::resolved::ResolvedType,
    seen: &mut BTreeSet<crate::resolve::DefId>,
) -> Option<&'static str> {
    use crate::signatures::PrivacyQualifier as Q;
    match sigs.privacy_qualifier(ty) {
        Some(Q::Session) => return Some("a session's"),
        Some(Q::User) => return Some("a user's"),
        Some(Q::Organization) => return Some("an organization's"),
        _ => {}
    }
    if let Some(held) = ty.args().iter().find_map(|a| handle_in(sigs, a, seen)) {
        return Some(held);
    }
    let def = ty.def_id()?;
    if !seen.insert(def) {
        return None;
    }
    declared_parts(sigs, def)
        .into_iter()
        .find_map(|t| handle_in(sigs, t, seen))
}

/// **The parts of a declared type** (ADR-0264): its record's fields, its
/// opaque representation and its cases' payloads, each resolved.
fn declared_parts(
    sigs: &Signatures,
    def: crate::resolve::DefId,
) -> Vec<&crate::resolved::ResolvedType> {
    let Some(decl) = sigs.type_decl(def) else {
        return Vec::new();
    };
    decl.record
        .iter()
        .flatten()
        .map(|(_, t)| t)
        .chain(decl.representation.iter())
        .chain(decl.variants.iter().flatten().flat_map(|(_, ts)| ts.iter()))
        .filter_map(crate::resolved::TypeResolution::resolved)
        .collect()
}

/// **Whether a type holds a session's handle** (ADR-0264), wherever it sits.
fn session_in(
    sigs: &Signatures,
    ty: &crate::resolved::ResolvedType,
    seen: &mut BTreeSet<crate::resolve::DefId>,
) -> bool {
    if matches!(
        sigs.privacy_qualifier(ty),
        Some(crate::signatures::PrivacyQualifier::Session)
    ) {
        return true;
    }
    if ty.args().iter().any(|a| session_in(sigs, a, seen)) {
        return true;
    }
    let Some(def) = ty.def_id() else {
        return false;
    };
    seen.insert(def)
        && declared_parts(sigs, def)
            .into_iter()
            .any(|t| session_in(sigs, t, seen))
}

/// [`session_in`], of the type the typer gives an expression.
fn session_in_ty(
    sigs: &Signatures,
    ty: &crate::values::Ty,
    seen: &mut BTreeSet<crate::resolve::DefId>,
) -> bool {
    use crate::values::Ty;
    match ty {
        Ty::Nominal(def, args) => {
            matches!(
                sigs.privacy_kind(*def),
                Some(crate::signatures::PrivacyQualifier::Session)
            ) || args.iter().any(|a| session_in_ty(sigs, a, seen))
                || (seen.insert(*def)
                    && declared_parts(sigs, *def)
                        .into_iter()
                        .any(|t| session_in(sigs, t, seen)))
        }
        Ty::Builtin(_, args) => args.iter().any(|a| session_in_ty(sigs, a, seen)),
        _ => false,
    }
}

/// **A session's handle never reaches the browser** (ADR-0264). A session's
/// id is its cookie's value, and the cookie is `HttpOnly`, kept from the
/// page's scripts; a page that printed the handle gave it back to them, and
/// until ADR-0264 `<p>{mine.session}</p>` checked clean. What reaches the
/// browser: what markup prints, as text or an attribute's value; and an
/// answer, a query's or a subscription's that a page shows, speculates on
/// or computes from in the browser, or a command's. Each is PW5040 where
/// it holds one. A user's or an organization's id is a name, and the
/// browser holding it reads nothing.
fn sessions_stay_on_the_server(
    workspace: &crate::resolve::Workspace,
    sigs: &Signatures,
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let refused = |message: String, span: crate::hir::Span| Diagnostic {
        code: crate::codes::HANDLE_TO_BROWSER.id,
        invariant: crate::codes::HANDLE_TO_BROWSER.invariant,
        reason: "handle_to_browser",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message,
        primary_span: span,
        related: Vec::new(),
        explanation: Some(
            "A session's id is its cookie's value, and the cookie is `HttpOnly`: \
             the page's scripts never read it, so a script a page runs cannot \
             take the session away. A session's handle is that id, and what \
             reaches the browser a script can read. A query reads the reader's \
             data with `current_session()`; what it answers is that data, never \
             the handle."
                .to_string(),
        ),
        repairs: Vec::new(),
    };
    for (_, decl) in hir.all_decls() {
        if !matches!(
            decl.kind,
            DeclKind::Query | DeclKind::Subscription | DeclKind::Command
        ) {
            continue;
        }
        let Some(ret) = &decl.ret else {
            continue;
        };
        let holds = crate::resolved::resolve(
            workspace,
            unit,
            None,
            &decl.type_params,
            ret,
            decl.name_span.clone(),
        )
        .resolved()
        .is_some_and(|t| session_in(sigs, t, &mut BTreeSet::new()));
        if holds {
            out.push(refused(
                format!(
                    "`{}`'s answer holds a session's handle, which would reach the browser",
                    decl.name
                ),
                decl.name_span.clone(),
            ));
        }
    }
    let module = workspace
        .modules
        .iter()
        .find(|m| m.unit == unit)
        .map(|m| m.name.as_str());
    for (_, body, _) in hir.bodies.iter() {
        let decl = hir.decl(body.owner);
        // One typer for the body, built for its first printed value: one for
        // each value solved the whole body once a hole (the checker's
        // latency, 2026-10-13).
        let mut types: Option<crate::values::BodyTypes> = None;
        for (_, node, _) in body.nodes.iter() {
            // What the node prints, by where each of its expressions is
            // written (ADR-0345): a position this does not name is a compile
            // error here, never one it skips.
            let mut printed: Vec<ExprId> = node
                .exprs()
                .into_iter()
                .filter(|(position, _)| match position {
                    // Written as text, in the document or an attribute.
                    crate::hir::Position::Hole => true,
                    crate::hir::Position::Attribute => true,
                    // Run when a person acts; what a block decides by; the
                    // list the rows come from; and the key, which the
                    // document holds as a keyed digest, never as itself.
                    crate::hir::Position::Handler
                    | crate::hir::Position::Subject
                    | crate::hir::Position::Condition
                    | crate::hir::Position::List
                    | crate::hir::Position::Key => false,
                })
                .map(|(_, e)| e)
                .collect();
            // A string with holes prints each hole.
            printed = printed
                .into_iter()
                .flat_map(|e| match body.expr(e) {
                    Expr::Interpolated { parts, .. } => parts.clone(),
                    _ => vec![e],
                })
                .collect();
            for e in printed {
                let types = types.get_or_insert_with(|| {
                    crate::values::BodyTypes::new(sigs, workspace, unit, module, decl, body)
                });
                let (ty, _) = types.of(e);
                if session_in_ty(sigs, &ty, &mut BTreeSet::new()) {
                    out.push(refused(
                        "this prints a session's handle, which would reach the browser".to_string(),
                        body.expr_span(e),
                    ));
                }
            }
        }
    }
    out
}

/// **A session's, a user's or an organization's handle is the platform's
/// to make** (ADR-0263). A handle is what reads that one's data: a program
/// that could make one could read anyone's, and until ADR-0263 one could.
/// `Session("…")` checked clean, and so did a command whose session the
/// browser supplies. Three ways one was made, each refused where it is
/// written: constructed by a program at all, since the host makes each one
/// from the request it answers (PW5037),
/// answered by an operation that is not the platform's (PW5038), and
/// supplied by the browser, as a command's or a page's parameter or a
/// signal's value (PW5039).
fn handles_are_the_platforms(
    workspace: &crate::resolve::Workspace,
    sigs: &Signatures,
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    use crate::signatures::PrivacyQualifier as Q;
    let mut out = Vec::new();
    let diagnostic = |code: crate::codes::Code,
                      reason: &'static str,
                      message: String,
                      span: crate::hir::Span,
                      explanation: &str| Diagnostic {
        code: code.id,
        invariant: code.invariant,
        reason,
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message,
        primary_span: span,
        related: Vec::new(),
        explanation: Some(explanation.to_string()),
        repairs: Vec::new(),
    };
    // PW5037: made by a call to its type, anywhere a program is written.
    for (_, body, _) in hir.bodies.iter() {
        let Some(lexical) = crate::lexical::Lexical::build_in(hir, body.owner) else {
            continue;
        };
        for (_, e, span) in body.exprs.iter() {
            let Expr::Call { callee, .. } = e else {
                continue;
            };
            let crate::values::Named::Target(crate::values::Target::Opaque(def)) =
                crate::values::named_at(sigs, workspace, unit, body, &lexical, *callee)
            else {
                continue;
            };
            let whose = match sigs.privacy_kind(def) {
                Some(Q::Session) => "a session's",
                Some(Q::User) => "a user's",
                Some(Q::Organization) => "an organization's",
                _ => continue,
            };
            out.push(diagnostic(
                crate::codes::HANDLE_MADE,
                "handle_made",
                format!("{whose} handle is the platform's to make, and this makes one"),
                span.clone(),
                "A handle is what reads the data of the one it names: a session's \
                 cart, a user's notifications. Only the platform makes one, from the \
                 request it is answering (`current_session()`, `current_user()`), so a \
                 program reads its reader's data and no one else's. An id names someone \
                 and is the program's to make; a handle grants, and is not.",
            ));
        }
    }
    for (_, decl) in hir.all_decls() {
        let resolved = |declared: &crate::hir::DeclaredType, span: &crate::hir::Span| {
            crate::resolved::resolve(
                workspace,
                unit,
                None,
                &decl.type_params,
                declared,
                span.clone(),
            )
            .resolved()
            .and_then(|t| handle_in(sigs, t, &mut BTreeSet::new()))
        };
        // PW5038: answered by an operation outside the platform's.
        if decl.kind == DeclKind::Fn
            && let Some(op) = crate::backend::host_binding(decl)
            && !op.interface.starts_with("pw:")
            && let Some(ret) = &decl.ret
            && let Some(whose) = resolved(ret, &decl.name_span)
        {
            out.push(diagnostic(
                crate::codes::HANDLE_ANSWERED,
                "handle_answered",
                format!(
                    "`{}` is answered by `{}#{}`, which is not the platform's, and its \
                     answer holds {whose} handle",
                    decl.name, op.interface, op.name
                ),
                decl.name_span.clone(),
                "A data layer answers ids, never a handle: a handle it answered would \
                 read the data of whomever it named. The platform's own operations \
                 (`pw:host/…`) answer the reader's, from the request.",
            ));
        }
        // PW5039: a command's or a page's parameter, which the browser
        // supplies, in a command's body or a page's address.
        if matches!(decl.kind, DeclKind::Command | DeclKind::Page) {
            for p in &decl.params {
                let Some(whose) = p.ty.as_ref().and_then(|t| resolved(t, &p.span)) else {
                    continue;
                };
                out.push(diagnostic(
                    crate::codes::HANDLE_FROM_BROWSER,
                    "handle_from_browser",
                    format!(
                        "`{}`'s parameter `{}` holds {whose} handle, which the browser \
                         would supply",
                        decl.name, p.name
                    ),
                    p.span.clone(),
                    "What the browser supplies is whatever its sender writes: a handle \
                     there would read the data of whomever it named. A command or a page \
                     reads its reader's own with `current_session()` or `current_user()`.",
                ));
            }
        }
        // PW5039: a signal a module declares, `signal drawer: Bool`.
        if decl.kind == DeclKind::Signal
            && let Some(ret) = &decl.ret
            && let Some(whose) = resolved(ret, &decl.name_span)
        {
            out.push(diagnostic(
                crate::codes::HANDLE_FROM_BROWSER,
                "handle_from_browser",
                format!("the signal `{}` holds {whose} handle", decl.name),
                decl.name_span.clone(),
                "A signal is the browser's state, and what the browser holds it may \
                 write: a handle there would read the data of whomever it named.",
            ));
        }
    }
    // PW5039: a page's or a view's signal, `signal x: T = e`.
    for (_, body, _) in hir.bodies.iter() {
        let owner = hir.decl(body.owner);
        for at in &body.signals {
            let Expr::Let { ty: Some(t), .. } = body.expr(*at) else {
                continue;
            };
            let span = body.expr_span(*at);
            let Some(written) = crate::resolved::written_in_body(body, *t) else {
                continue;
            };
            let Some(whose) = crate::resolved::resolve(
                workspace,
                unit,
                None,
                &owner.type_params,
                &written,
                span.clone(),
            )
            .resolved()
            .and_then(|t| handle_in(sigs, t, &mut BTreeSet::new())) else {
                continue;
            };
            out.push(diagnostic(
                crate::codes::HANDLE_FROM_BROWSER,
                "handle_from_browser",
                format!("a signal of `{}` holds {whose} handle", owner.name),
                span,
                "A signal is the browser's state, and what the browser holds it may \
                 write: a handle there would read the data of whomever it named.",
            ));
        }
    }
    out
}

fn representation_unshadowed(sigs: &Signatures, unit: usize, hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        if decl.kind != DeclKind::Fn || decl.name != "value" {
            continue;
        }
        let Some(receiver) = sigs
            .by_def(crate::resolve::DefId { unit, decl: id.0 })
            .and_then(|s| s.params.first())
            .and_then(Option::as_ref)
            .and_then(crate::resolved::TypeResolution::resolved)
            .and_then(crate::resolved::ResolvedType::def_id)
        else {
            continue;
        };
        if receiver.unit != unit
            || hir
                .all_decls()
                .find(|(d, _)| d.0 == receiver.decl)
                .is_none_or(|(_, d)| d.kind != DeclKind::Opaque)
        {
            continue;
        }
        let ty = hir
            .all_decls()
            .find(|(d, _)| d.0 == receiver.decl)
            .map(|(_, d)| d.name.clone())
            .unwrap_or_default();
        out.push(Diagnostic {
            code: crate::codes::REPRESENTATION_SHADOWED.id,
            invariant: crate::codes::REPRESENTATION_SHADOWED.invariant,
            reason: "representation_shadowed",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!(
                "`value` is `{ty}`'s representation in this module, and a member of that \
                 name would be read in its place"
            ),
            primary_span: decl.name_span.clone(),
            related: Vec::new(),
            explanation: Some(format!(
                "In the module that declares an opaque type, `x.value` reads its \
                 representation (ADR-0048). A member `value` of `{ty}` would be read \
                 there instead, so the representation could not be reached, and a \
                 member that reads it would call itself."
            )),
            repairs: vec![Repair {
                description: format!("name the member for what it gives of a `{ty}`"),
                replacement: None,
            }],
        });
    }
    out
}

/// `a query` → `A query`.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What a declaration of `kind` is, for a message.
pub(crate) fn described(kind: DeclKind) -> &'static str {
    match kind {
        DeclKind::Query => "a query",
        DeclKind::Command => "a command",
        DeclKind::Subscription => "a subscription",
        DeclKind::Resource => "a resource",
        DeclKind::Materialize => "a materialization",
        DeclKind::View => "a view",
        DeclKind::Component => "a component",
        DeclKind::Page => "a page",
        DeclKind::Layout => "a layout",
        DeclKind::Task => "a task",
        DeclKind::Event => "an event",
        DeclKind::Source => "a data source",
        DeclKind::Table => "a table",
        DeclKind::Upload => "an upload",
        DeclKind::Predicate => "a predicate",
        DeclKind::Effect => "an effect",
        DeclKind::Prelude => "a prelude",
        DeclKind::Fn => "a function",
        DeclKind::Type | DeclKind::Opaque => "a type",
        DeclKind::Let => "a value",
        DeclKind::Signal => "a signal",
        DeclKind::Import => "an import",
        DeclKind::Other => "a declaration",
    }
}

/// **A listener binds its declaration's key** (ADR-0091).
///
/// `invalidates_on InventoryChanged(id, _)`: each argument is one of the
/// declaration's parameters, which an event's value at that position must
/// equal, or `_`, which any value does. The materializer compares them so.
/// Until 2026-09-26 the value was text: `InventoryChanged(id, item)`, with
/// `item` naming nothing, checked, and the materializer read an event's
/// values as a set, so an inventory change never reached its store's menu.
fn listener_keys(hir: &Hir, src: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        // An interface's listeners too (ADR-0240).
        let Some(body) = decl.terms_body().map(|b| hir.body(b)) else {
            continue;
        };
        for p in &decl.policies {
            if crate::policy::domain_of(&p.name) != Some(crate::policy::Domain::Listener) {
                continue;
            }
            for key in &p.keys {
                for arg in &key.args {
                    if let Expr::Name(n) = body.expr(arg.value)
                        && (n == "_" || decl.params.iter().any(|q| q.name == *n))
                    {
                        continue;
                    }
                    let written = src.get(body.expr_span(arg.value)).unwrap_or("?");
                    out.push(Diagnostic {
                        code: crate::codes::LISTENER_KEY.id,
                        invariant: crate::codes::LISTENER_KEY.invariant,
                        reason: "listener_key",
                        detector: Detector::ResourceGraph,
                        severity: Severity::Error,
                        message: format!(
                            "`{}` listens for `{}` with `{written}`, which is none of its parameters",
                            decl.name, key.name
                        ),
                        primary_span: body.expr_span(arg.value),
                        related: vec![Related::new(key.span.clone(), format!("what `{}` is listened for with", key.name))],
                        explanation: Some(format!(
                            "A listener's argument says which part of `{}`'s key an event's \
                             value must equal: one of its parameters, or `_` for any value. \
                             The materializer invalidates the entries whose key the event's \
                             values match, position by position, so an argument that is \
                             neither names nothing it can compare.",
                            decl.name
                        )),
                        repairs: vec![Repair {
                            description: "name a parameter, or write `_` for any value".to_string(),
                            replacement: None,
                        }],
                    });
                }
            }
        }
    }
    out
}

/// `a`, `b` or `c`, each in backticks, for a message.
pub(crate) fn listed<S: AsRef<str>>(words: &[S]) -> String {
    let words: Vec<String> = words.iter().map(|w| format!("`{}`", w.as_ref())).collect();
    match words.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// **A name is written once where it is declared** (ADR-0098).
///
/// A module declares a name once in a namespace (PW0024); inside a
/// declaration nothing held a name to that. A parameter taken twice, a
/// field or a case declared twice, a policy written twice and an attribute
/// given twice each checked until 2026-09-26. Every reader of a policy took
/// its first writing, so `cache private` then `cache shared` decided by its
/// order whether a session's data was refused a shared cache; and HTML keeps
/// an attribute's first value.
fn declared_once(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        let mut twice: Vec<(crate::hir::Span, String)> = Vec::new();
        let mut seen = BTreeSet::new();
        for p in &decl.params {
            if !seen.insert(p.name.as_str()) {
                twice.push((
                    p.span.clone(),
                    format!("`{}` takes `{}` twice", decl.name, p.name),
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for t in &decl.type_params {
            if !seen.insert(t.as_str()) {
                twice.push((
                    decl.name_span.clone(),
                    format!("`{}` takes `{t}` twice", decl.name),
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for f in decl.fields.iter().flatten() {
            if !seen.insert(f.name.as_str()) {
                twice.push((
                    f.span.clone(),
                    format!("`{}` declares `{}` twice", decl.name, f.name),
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for v in decl.variants.iter().flatten() {
            if !seen.insert(v.name.as_str()) {
                twice.push((
                    v.span.clone(),
                    format!("`{}` declares `{}` twice", decl.name, v.name),
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for p in &decl.policies {
            if !crate::policy::repeats(&p.name) && !seen.insert(p.name.as_str()) {
                twice.push((
                    p.span.clone(),
                    format!("`{}` declares `{}` twice", decl.name, p.name),
                ));
            }
        }
        if let Some(body_id) = decl.terms_body() {
            let body = hir.body(body_id);
            // A pattern, a lambda's parameters and a `let` bind a name once.
            let mut trees = vec![body.root];
            trees.extend(decl.term_roots().map(|(_, r)| r.root));
            for e in trees.iter().flat_map(|t| body.walk_from(*t)) {
                let groups: Vec<Vec<crate::hir::PatternId>> = match body.expr(e) {
                    Expr::Match { arms, .. } => arms.iter().map(|a| vec![a.pat]).collect(),
                    Expr::Lambda { params, .. } => vec![params.clone()],
                    Expr::Let { pat: Some(p), .. } | Expr::For { pat: Some(p), .. } => {
                        vec![vec![*p]]
                    }
                    _ => continue,
                };
                for group in groups {
                    let mut seen = BTreeSet::new();
                    for (name, span) in group.iter().flat_map(|p| bound_names(body, *p)) {
                        if !seen.insert(name.clone()) {
                            twice.push((span, format!("`{name}` is bound twice in one pattern")));
                        }
                    }
                }
            }
            let mut roots = Vec::new();
            for e in body.walk() {
                if let Expr::Template { roots: r, .. } = body.expr(e) {
                    roots.extend(r.iter().copied());
                }
            }
            for n in body.walk_markup(&roots) {
                let Node::Element { tag, attrs, .. } = body.node(n) else {
                    continue;
                };
                let mut seen = BTreeSet::new();
                for a in attrs {
                    // HTML reads an attribute's name in any case (ADR-0095).
                    if !seen.insert(a.name.to_ascii_lowercase()) {
                        twice.push((
                            a.span.clone(),
                            format!("`<{tag}>` is given `{}` twice", a.name),
                        ));
                    }
                }
            }
        }
        for (span, message) in twice {
            out.push(Diagnostic {
                code: crate::codes::DECLARED_TWICE.id,
                invariant: crate::codes::DECLARED_TWICE.invariant,
                reason: "declared_twice",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: span,
                related: vec![Related::new(
                    hir.decl_span(id),
                    format!("`{}` is declared here", decl.name),
                )],
                // Until 2026-09-26 each checked.
                explanation: Some(
                    "A name written twice where it is declared says two things, and one of \
                     them is dropped: a body sees one of two parameters, a field access one of \
                     two fields, every reader of a policy its first writing, and HTML an \
                     attribute's first value."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "write it once, or give the second its own name".to_string(),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// The names a pattern binds, with where. An or-pattern's alternatives bind
/// the same names, so its first alternative speaks for it.
fn bound_names(body: &Body, pat: crate::hir::PatternId) -> Vec<(String, crate::hir::Span)> {
    match body.pat(pat) {
        HPat::Bind { name, .. } => vec![(name.clone(), body.pat_span(pat))],
        HPat::Ctor { args, .. } => args.iter().flat_map(|a| bound_names(body, *a)).collect(),
        HPat::Or(alternatives) => alternatives
            .first()
            .map(|a| bound_names(body, *a))
            .unwrap_or_default(),
        HPat::Wild | HPat::Literal(_) | HPat::Error => Vec::new(),
    }
}

/// **A comment in markup is `<!-- -->`** (ADR-0167, PW5028). In markup,
/// `//` and `/*` begin no comment: what follows them is the page's text, as
/// in HTML and JSX. A line of text that begins with one reads as a comment
/// and would be shown, so it is refused, as `eslint-plugin-react`'s
/// `jsx-no-comment-textnodes` refuses it in JSX.
/// **Ids, and what names them** (ADR-0185, charter §8.2's "duplicate IDs"
/// and "invalid ARIA relationships"). Until 2026-10-04 the browser was left
/// to find these (ADR-0182's audit found none on the store, and would have
/// found them only on a page it read):
/// - PW5031: an id names one element of its page. Two elements with one id,
///   unless they are in two arms of one block, of which only one is shown;
///   and an id written as text inside an `{#each}`, which every row has.
/// - PW5032: an id reference names an element the page shows whenever the
///   referrer is shown: in the referrer's own arm, or in every arm of a
///   block around it.
/// - PW5033: an ARIA attribute, its value and a role are ones WAI-ARIA
///   defines. A browser ignores one it does not know, and says nothing.
///
/// Within the declaration that renders them, as PW5014 reads a label
/// (ADR-0143). An id or a reference the program computes is not read.
fn ids_and_aria(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (decl_id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let mut roots = Vec::new();
        for e in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(e) {
                roots.extend(r.iter().copied());
            }
        }
        if roots.is_empty() {
            continue;
        }
        let mut places = BTreeMap::new();
        markup_places(
            body,
            &roots,
            &mut Vec::new(),
            &mut vec![roots.clone()],
            false,
            &mut places,
        );
        let ids = Ids::of(body, places.keys());
        duplicate_ids(hir, decl_id, decl, body, &places, &ids, &mut out);
        references(hir, decl_id, decl, body, &places, &ids, &mut out);
        aria_values(hir, decl_id, decl, body, &places, &mut out);
    }
    out
}

/// Where an element is: the arms of the blocks around it, outermost first,
/// as (block, arm); the node lists of those arms, the root's first, whose
/// content is shown whenever the element is; and whether it is inside an
/// `{#each}`, so that every row has it.
struct Place {
    arms: Vec<(hir::NodeId, usize)>,
    scopes: Vec<Vec<hir::NodeId>>,
    repeated: bool,
}

fn markup_places(
    body: &Body,
    nodes: &[hir::NodeId],
    arms: &mut Vec<(hir::NodeId, usize)>,
    scopes: &mut Vec<Vec<hir::NodeId>>,
    repeated: bool,
    out: &mut BTreeMap<hir::NodeId, Place>,
) {
    for &n in nodes {
        match body.node(n) {
            Node::Element { tag, children, .. } => {
                out.insert(
                    n,
                    Place {
                        arms: arms.clone(),
                        scopes: scopes.clone(),
                        repeated,
                    },
                );
                if tag == "stream" {
                    for (i, arm) in stream_arms(body, children).into_iter().enumerate() {
                        arms.push((n, i));
                        scopes.push(arm.clone());
                        markup_places(body, &arm, arms, scopes, repeated, out);
                        scopes.pop();
                        arms.pop();
                    }
                } else {
                    markup_places(body, children, arms, scopes, repeated, out);
                }
            }
            Node::Block {
                directive,
                children,
                ..
            } => {
                let each = directive.trim_start().starts_with("{#each");
                for (i, arm) in block_arms(body, children).into_iter().enumerate() {
                    arms.push((n, i));
                    scopes.push(arm.clone());
                    markup_places(body, &arm, arms, scopes, repeated || each, out);
                    scopes.pop();
                    arms.pop();
                }
            }
            _ => {}
        }
    }
}

/// A block's arms: its children between its markers, `{:else}` and a
/// `{#match}`'s cases.
fn block_arms(body: &Body, children: &[hir::NodeId]) -> Vec<Vec<hir::NodeId>> {
    let mut arms = vec![Vec::new()];
    for &c in children {
        if matches!(body.node(c), Node::Branch { .. }) {
            arms.push(Vec::new());
        } else if let Some(arm) = arms.last_mut() {
            arm.push(c);
        }
    }
    arms
}

/// A `<stream>`'s arms: its placeholder's, its ready's and its failed's
/// content, of which one is shown (ADR-0148).
fn stream_arms(body: &Body, children: &[hir::NodeId]) -> Vec<Vec<hir::NodeId>> {
    children
        .iter()
        .filter_map(|c| match body.node(*c) {
            Node::Element { children, .. } => Some(children.clone()),
            _ => None,
        })
        .collect()
}

/// Whether an element with `id` is shown whenever `nodes` are: one of them
/// has it, outside any block; or a block among them has it in every arm,
/// and shows one of them whatever its subject is.
fn always_shows(body: &Body, nodes: &[hir::NodeId], id: &str) -> bool {
    nodes.iter().any(|&n| match body.node(n) {
        Node::Element { tag, attrs, children, .. } => {
            static_attr(attrs, "id") == Some(id)
                || if tag == "stream" {
                    let arms = stream_arms(body, children);
                    !arms.is_empty() && arms.iter().all(|a| always_shows(body, a, id))
                } else {
                    always_shows(body, children, id)
                }
        }
        Node::Block {
            directive,
            children,
            ..
        } => {
            let d = directive.trim_start();
            // An `{#if}` with an `{:else}` shows one of its arms, and a
            // `{#match}` covers every case (PW0305); a loop may show none.
            let complete = if d.starts_with("{#if") {
                children.iter().any(|c| {
                    matches!(body.node(*c), Node::Branch { marker, .. } if marker.trim() == "{:else}")
                })
            } else {
                d.starts_with("{#match")
            };
            complete
                && block_arms(body, children)
                    .iter()
                    .skip(usize::from(d.starts_with("{#match")))
                    .all(|a| always_shows(body, a, id))
        }
        _ => false,
    })
}

/// The ids a declaration writes: each written as text, by its elements; and
/// whether any is computed, `id={x}` or `id="row-{x}"`, which a reference
/// may name and this does not know.
struct Ids<'b> {
    written: BTreeMap<&'b str, Vec<hir::NodeId>>,
    computed: Vec<(&'b str, &'b str)>,
    anything: bool,
}

impl<'b> Ids<'b> {
    /// The ids `nodes` write, each an element of the declaration's markup.
    fn of<'n>(body: &'b Body, nodes: impl IntoIterator<Item = &'n hir::NodeId>) -> Ids<'b> {
        let mut ids = Ids {
            written: BTreeMap::new(),
            computed: Vec::new(),
            anything: false,
        };
        for &n in nodes {
            let Node::Element { attrs, .. } = body.node(n) else {
                continue;
            };
            let Some(a) = attrs.iter().find(|a| a.name == "id") else {
                continue;
            };
            match &a.value {
                AttrValue::Static(_) => {
                    let v = static_attr(attrs, "id").unwrap_or_default();
                    ids.written.entry(v).or_default().push(n);
                }
                // `id="line-{line.item_id}"`: a string with holes, whose text
                // before the first and after the last is fixed. Until
                // 2026-10-04 it was taken as an id that may be anything, and
                // a declaration with one let every reference to nothing
                // through (a correction to ADR-0185).
                AttrValue::Expr(e) => match body.expr(*e) {
                    Expr::Interpolated { text, .. } => {
                        let v = text.trim_matches(|c| c == '"' || c == '\'');
                        match (v.find('{'), v.rfind('}')) {
                            (Some(open), Some(close)) => {
                                ids.computed.push((&v[..open], &v[close + 1..]))
                            }
                            _ => ids.anything = true,
                        }
                    }
                    _ => ids.anything = true,
                },
                AttrValue::None => {}
            }
        }
        ids
    }

    /// Whether a computed id might be `id` when the page runs.
    fn may_be(&self, id: &str) -> bool {
        self.anything
            || self.computed.iter().any(|(before, after)| {
                id.len() > before.len() + after.len()
                    && id.starts_with(before)
                    && id.ends_with(after)
            })
    }
}

fn duplicate_ids(
    hir: &Hir,
    decl_id: hir::DeclId,
    decl: &Decl,
    body: &Body,
    places: &BTreeMap<hir::NodeId, Place>,
    ids: &Ids<'_>,
    out: &mut Vec<Diagnostic>,
) {
    let diagnostic = |n: hir::NodeId, message: String, related: Vec<Related>| Diagnostic {
        code: crate::codes::DUPLICATE_ID.id,
        invariant: crate::codes::DUPLICATE_ID.invariant,
        reason: "duplicate_id",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message,
        primary_span: body.node_span(n),
        related,
        explanation: Some(
            "An id is how a label, an ARIA relation, a fragment link and the page's \
             own runtime find one element. With two, each finds the first, and the \
             second is named by nothing: a screen reader is told about the wrong \
             element, and a link lands on the wrong one."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: "give each its own id; in a list, write the row's key into it: \
                          `id=\"line-{line.item_id}\"`"
                .to_string(),
            replacement: None,
        }],
    };
    for (id, nodes) in &ids.written {
        for (i, &n) in nodes.iter().enumerate() {
            let place = &places[&n];
            if place.repeated {
                out.push(diagnostic(
                    n,
                    format!("`{id}` names every row of a list in `{}`", decl.name),
                    vec![Related::new(
                        hir.decl_span(decl_id),
                        format!("`{}` renders this", decl.name),
                    )],
                ));
                continue;
            }
            // Two in two arms of one block are one element: one arm is shown.
            let together = nodes[..i].iter().find(|&&m| {
                let other = &places[&m];
                !other.repeated && {
                    let same = place
                        .arms
                        .iter()
                        .zip(&other.arms)
                        .take_while(|(a, b)| a == b)
                        .count();
                    !matches!(
                        (place.arms.get(same), other.arms.get(same)),
                        (Some((x, _)), Some((y, _))) if x == y
                    )
                }
            });
            if let Some(&first) = together {
                out.push(diagnostic(
                    n,
                    format!("`{id}` names two elements of `{}`", decl.name),
                    vec![Related::new(
                        body.node_span(first),
                        format!("`{id}` is this element's too"),
                    )],
                ));
            }
        }
    }
}

/// The attributes whose value is an id, or ids, that a reference names.
const ID_REFERENCES: &[&str] = &[
    "aria-activedescendant",
    "aria-controls",
    "aria-describedby",
    "aria-details",
    "aria-errormessage",
    "aria-flowto",
    "aria-labelledby",
    "aria-owns",
    "commandfor",
    "for",
    "form",
    "headers",
    "list",
    "popovertarget",
];

fn references(
    hir: &Hir,
    decl_id: hir::DeclId,
    decl: &Decl,
    body: &Body,
    places: &BTreeMap<hir::NodeId, Place>,
    ids: &Ids<'_>,
    out: &mut Vec<Diagnostic>,
) {
    for (&n, place) in places {
        let Node::Element { tag, attrs, .. } = body.node(n) else {
            continue;
        };
        for attr in ID_REFERENCES {
            let Some(value) = static_attr(attrs, attr) else {
                continue;
            };
            if value.contains('{') {
                continue;
            }
            for token in value.split_ascii_whitespace() {
                let written = ids.written.get(token);
                // An id every row has is PW5031's: the row is the defect.
                if written.is_some_and(|w| w.iter().all(|t| places[t].repeated)) {
                    continue;
                }
                let message = match written {
                    None if ids.may_be(token) => continue,
                    None => format!(
                        "`{attr}` names `{token}`, which no element of `{}` has",
                        decl.name
                    ),
                    Some(_) => {
                        if place.scopes.iter().any(|s| always_shows(body, s, token)) {
                            continue;
                        }
                        format!(
                            "`{attr}` names `{token}`, which `{}` does not always show with \
                             its `<{tag}>`",
                            decl.name
                        )
                    }
                };
                let attribute = attrs
                    .iter()
                    .find(|a| a.name == *attr)
                    .map(|a| a.span.clone())
                    .unwrap_or_else(|| body.node_span(n));
                out.push(Diagnostic {
                    code: crate::codes::REFERENCE_NAMES_NOTHING.id,
                    invariant: crate::codes::REFERENCE_NAMES_NOTHING.invariant,
                    reason: "reference_names_nothing",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message,
                    primary_span: attribute,
                    related: vec![Related::new(
                        hir.decl_span(decl_id),
                        format!("`{}` renders this", decl.name),
                    )],
                    explanation: Some(
                        "A browser drops a reference to an id the page does not have, and \
                         says nothing: a field loses its description, a section its name, a \
                         label its control. One to an element shown only some of the time \
                         names nothing the rest of it."
                            .to_string(),
                    ),
                    repairs: vec![Repair {
                        description: format!(
                            "give the element it means `id=\"{token}\"`, or name the id that \
                             element has; and write it where it is shown whenever this is"
                        ),
                        replacement: None,
                    }],
                });
            }
        }
    }
}

/// What an ARIA attribute's value may be (WAI-ARIA 1.2, §6.6 and §6.7).
enum AriaValue {
    Text,
    Ids,
    Tokens(&'static [&'static str]),
    TokenList(&'static [&'static str]),
    Integer,
    Number,
}

/// **WAI-ARIA's states and properties**: 1.2's, and 1.3's
/// `aria-description`, `aria-braille*` and `*indextext`, with what each
/// value may be.
const ARIA_ATTRIBUTES: &[(&str, AriaValue)] = {
    use AriaValue::*;
    const TRUE_FALSE: &[&str] = &["true", "false"];
    const OPTIONAL: &[&str] = &["true", "false", "undefined"];
    const TRISTATE: &[&str] = &["true", "false", "mixed", "undefined"];
    &[
        ("aria-activedescendant", Ids),
        ("aria-atomic", Tokens(TRUE_FALSE)),
        (
            "aria-autocomplete",
            Tokens(&["inline", "list", "both", "none"]),
        ),
        ("aria-braillelabel", Text),
        ("aria-brailleroledescription", Text),
        ("aria-busy", Tokens(TRUE_FALSE)),
        ("aria-checked", Tokens(TRISTATE)),
        ("aria-colcount", Integer),
        ("aria-colindex", Integer),
        ("aria-colindextext", Text),
        ("aria-colspan", Integer),
        ("aria-controls", Ids),
        (
            "aria-current",
            Tokens(&["page", "step", "location", "date", "time", "true", "false"]),
        ),
        ("aria-describedby", Ids),
        ("aria-description", Text),
        ("aria-details", Ids),
        ("aria-disabled", Tokens(TRUE_FALSE)),
        (
            "aria-dropeffect",
            TokenList(&["copy", "execute", "link", "move", "none", "popup"]),
        ),
        ("aria-errormessage", Ids),
        ("aria-expanded", Tokens(OPTIONAL)),
        ("aria-flowto", Ids),
        ("aria-grabbed", Tokens(OPTIONAL)),
        (
            "aria-haspopup",
            Tokens(&["false", "true", "menu", "listbox", "tree", "grid", "dialog"]),
        ),
        ("aria-hidden", Tokens(OPTIONAL)),
        (
            "aria-invalid",
            Tokens(&["grammar", "false", "spelling", "true"]),
        ),
        ("aria-keyshortcuts", Text),
        ("aria-label", Text),
        ("aria-labelledby", Ids),
        ("aria-level", Integer),
        ("aria-live", Tokens(&["assertive", "off", "polite"])),
        ("aria-modal", Tokens(TRUE_FALSE)),
        ("aria-multiline", Tokens(TRUE_FALSE)),
        ("aria-multiselectable", Tokens(TRUE_FALSE)),
        (
            "aria-orientation",
            Tokens(&["horizontal", "undefined", "vertical"]),
        ),
        ("aria-owns", Ids),
        ("aria-placeholder", Text),
        ("aria-posinset", Integer),
        ("aria-pressed", Tokens(TRISTATE)),
        ("aria-readonly", Tokens(TRUE_FALSE)),
        (
            "aria-relevant",
            TokenList(&["additions", "all", "removals", "text"]),
        ),
        ("aria-required", Tokens(TRUE_FALSE)),
        ("aria-roledescription", Text),
        ("aria-rowcount", Integer),
        ("aria-rowindex", Integer),
        ("aria-rowindextext", Text),
        ("aria-rowspan", Integer),
        ("aria-selected", Tokens(OPTIONAL)),
        ("aria-setsize", Integer),
        (
            "aria-sort",
            Tokens(&["ascending", "descending", "none", "other"]),
        ),
        ("aria-valuemax", Number),
        ("aria-valuemin", Number),
        ("aria-valuenow", Number),
        ("aria-valuetext", Text),
    ]
};

/// **WAI-ARIA's roles** that a page may write: 1.2's, but for the abstract
/// ones; 1.3's `comment`, `image`, `mark` and `suggestion`; and the roles of
/// the Graphics and DPUB modules.
const ARIA_ROLES: &[&str] = &[
    "alert",
    "alertdialog",
    "application",
    "article",
    "banner",
    "blockquote",
    "button",
    "caption",
    "cell",
    "checkbox",
    "code",
    "columnheader",
    "combobox",
    "comment",
    "complementary",
    "contentinfo",
    "definition",
    "deletion",
    "dialog",
    "directory",
    "document",
    "emphasis",
    "feed",
    "figure",
    "form",
    "generic",
    "grid",
    "gridcell",
    "group",
    "heading",
    "image",
    "img",
    "insertion",
    "link",
    "list",
    "listbox",
    "listitem",
    "log",
    "main",
    "mark",
    "marquee",
    "math",
    "menu",
    "menubar",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "meter",
    "navigation",
    "none",
    "note",
    "option",
    "paragraph",
    "presentation",
    "progressbar",
    "radio",
    "radiogroup",
    "region",
    "row",
    "rowgroup",
    "rowheader",
    "scrollbar",
    "search",
    "searchbox",
    "separator",
    "slider",
    "spinbutton",
    "status",
    "strong",
    "subscript",
    "suggestion",
    "superscript",
    "switch",
    "tab",
    "table",
    "tablist",
    "tabpanel",
    "term",
    "textbox",
    "time",
    "timer",
    "toolbar",
    "tooltip",
    "tree",
    "treegrid",
    "treeitem",
    "graphics-document",
    "graphics-object",
    "graphics-symbol",
    "doc-abstract",
    "doc-acknowledgments",
    "doc-afterword",
    "doc-appendix",
    "doc-backlink",
    "doc-biblioentry",
    "doc-bibliography",
    "doc-biblioref",
    "doc-chapter",
    "doc-colophon",
    "doc-conclusion",
    "doc-cover",
    "doc-credit",
    "doc-credits",
    "doc-dedication",
    "doc-endnote",
    "doc-endnotes",
    "doc-epigraph",
    "doc-epilogue",
    "doc-errata",
    "doc-example",
    "doc-footnote",
    "doc-foreword",
    "doc-glossary",
    "doc-glossref",
    "doc-index",
    "doc-introduction",
    "doc-noteref",
    "doc-notice",
    "doc-pagebreak",
    "doc-pagefooter",
    "doc-pageheader",
    "doc-pagelist",
    "doc-part",
    "doc-preface",
    "doc-prologue",
    "doc-pullquote",
    "doc-qna",
    "doc-subtitle",
    "doc-tip",
    "doc-toc",
];

fn aria_values(
    hir: &Hir,
    decl_id: hir::DeclId,
    decl: &Decl,
    body: &Body,
    places: &BTreeMap<hir::NodeId, Place>,
    out: &mut Vec<Diagnostic>,
) {
    let mut say = |span: crate::diagnostics::Span, message: String, repair: String| {
        out.push(Diagnostic {
            code: crate::codes::ARIA_UNKNOWN.id,
            invariant: crate::codes::ARIA_UNKNOWN.invariant,
            reason: "aria_unknown",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message,
            primary_span: span,
            related: vec![Related::new(
                hir.decl_span(decl_id),
                format!("`{}` renders this", decl.name),
            )],
            explanation: Some(
                "A browser ignores an ARIA attribute it does not know, a value its \
                 attribute does not take, and a role WAI-ARIA does not define, and says \
                 nothing: the element is read as if it were never written, and a \
                 one-letter slip, `aria-labeledby`, takes a section's name away."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: repair,
                replacement: None,
            }],
        });
    };
    for &n in places.keys() {
        let Node::Element { attrs, .. } = body.node(n) else {
            continue;
        };
        for a in attrs {
            if a.name == "role" {
                let Some(roles) = static_attr(attrs, "role") else {
                    continue;
                };
                if roles.contains('{') {
                    continue;
                }
                for role in roles.split_ascii_whitespace() {
                    if !ARIA_ROLES.contains(&role) {
                        say(
                            a.span.clone(),
                            format!("`role=\"{role}\"` is no ARIA role"),
                            "write a role WAI-ARIA defines, or the element HTML has for it: \
                             `<button>` rather than `role=\"button\"`"
                                .to_string(),
                        );
                    }
                }
                continue;
            }
            if !a.name.starts_with("aria-") {
                continue;
            }
            let Some((_, kind)) = ARIA_ATTRIBUTES.iter().find(|(name, _)| *name == a.name) else {
                let near = ARIA_ATTRIBUTES
                    .iter()
                    .map(|(name, _)| *name)
                    .min_by_key(|name| edit_distance(name, &a.name))
                    .unwrap_or("aria-label");
                say(
                    a.span.clone(),
                    format!("`{}` is no ARIA attribute", a.name),
                    format!("write one WAI-ARIA defines: `{near}`?"),
                );
                continue;
            };
            let value = match &a.value {
                AttrValue::Static(_) => static_attr(attrs, &a.name).unwrap_or_default(),
                AttrValue::None => "",
                AttrValue::Expr(_) => continue,
            };
            if value.contains('{') {
                continue;
            }
            let fits = match kind {
                AriaValue::Text | AriaValue::Ids => true,
                AriaValue::Tokens(allowed) => allowed.contains(&value),
                AriaValue::TokenList(allowed) => {
                    !value.trim().is_empty()
                        && value.split_ascii_whitespace().all(|t| allowed.contains(&t))
                }
                AriaValue::Integer => {
                    let v = value.strip_prefix('-').unwrap_or(value);
                    !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())
                }
                AriaValue::Number => value.trim().parse::<f64>().is_ok_and(f64::is_finite),
            };
            if !fits {
                let allowed = match kind {
                    AriaValue::Tokens(t) | AriaValue::TokenList(t) => t
                        .iter()
                        .map(|t| format!("`{t}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    AriaValue::Integer => "an integer".to_string(),
                    AriaValue::Number => "a number".to_string(),
                    AriaValue::Text | AriaValue::Ids => String::new(),
                };
                say(
                    a.span.clone(),
                    format!("`{}=\"{value}\"` is no value of `{}`", a.name, a.name),
                    format!("`{}` takes {allowed}", a.name),
                );
            }
        }
    }
}

/// How many single-character edits make `a` of `b` (Levenshtein): which
/// ARIA attribute a misspelt one meant.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(here + 1);
            previous = here;
        }
    }
    row[b.len()]
}

/// **A page's metadata** (ADR-0186): `<meta name="description" content="…">`
/// and `<meta property="og:title" content="…">` at the top of its view,
/// beside its title. What a page says of itself to what reads it without
/// showing it, a search engine's result or a link's preview. The host writes
/// it into the document's head, as the page is served.
///
/// PW5034 refuses:
/// - a `<meta>` in a view, which would describe every page that composes it;
/// - one inside an element or a block;
/// - the host's own: a charset, an `http-equiv` or the viewport (ADR-0182);
/// - one that names nothing it describes, names it by a computed value, or
///   by both `name` and `property`;
/// - one with any other attribute, `media` or `lang`, which the host does
///   not write: a page's metadata is its name and its content;
/// - one with no content, a blank one, or one computed in place;
/// - a second of a name HTML allows once: `description` and `color-scheme`
///   once a document, `application-name` once a language and `theme-color`
///   once a media query, which with neither written is once.
///
/// A `<meta itemprop>` is an item's property (microdata), not the page's
/// metadata, and is written where it is.
fn metadata(hir: &Hir) -> Vec<Diagnostic> {
    // Compared as HTML compares them, ignoring ASCII case.
    const ONCE: [&str; 4] = [
        "description",
        "color-scheme",
        "application-name",
        "theme-color",
    ];
    let mut out = Vec::new();
    for (decl_id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let mut roots = Vec::new();
        for e in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(e) {
                roots.extend(r.iter().copied());
            }
        }
        let page = decl.kind == DeclKind::Page;
        let mut named: BTreeSet<String> = BTreeSet::new();
        for n in body.walk_markup(&roots) {
            let Node::Element { tag, attrs, .. } = body.node(n) else {
                continue;
            };
            if tag != "meta" {
                continue;
            }
            let attr = |name: &str| attrs.iter().find(|a| a.name == name);
            let keys: Vec<(&str, &hir::Attr)> = ["name", "property"]
                .into_iter()
                .filter_map(|k| attr(k).map(|a| (k, a)))
                .collect();
            let host = if attr("charset").is_some() {
                Some("charset")
            } else if attr("http-equiv").is_some() {
                Some("http-equiv")
            } else if static_attr(attrs, "name").is_some_and(|v| v.eq_ignore_ascii_case("viewport"))
            {
                Some("name=\"viewport\"")
            } else if static_attr(attrs, "property")
                .is_some_and(|v| v.eq_ignore_ascii_case("og:url"))
            {
                // Open Graph's canonical address, which the host writes from
                // the page's (ADR-0311).
                Some("property=\"og:url\"")
            } else {
                None
            };
            let message = if attr("itemprop").is_some() {
                // An item's property, written where it is.
                if keys.is_empty() && host.is_none() {
                    continue;
                }
                Some(format!(
                    "`<meta itemprop>` in `{}` is an item's property and names the page's \
                     metadata too: HTML allows one of `name`, `http-equiv`, `charset` and \
                     `itemprop`",
                    decl.name
                ))
            } else if !page {
                let kind = match decl.kind {
                    DeclKind::View => "view",
                    DeclKind::Component => "component",
                    DeclKind::Layout => "layout",
                    _ => "declaration",
                };
                Some(format!(
                    "`<meta>` in the {kind} `{}` describes every page that composes it",
                    decl.name
                ))
            } else if !roots.contains(&n) {
                Some(format!(
                    "`<meta>` in `{}` is inside an element or a block, not at the top of its view",
                    decl.name
                ))
            } else if let Some(host) = host {
                Some(match host.starts_with("property") {
                    true => format!(
                        "`<meta {host}>` in `{}` is the host's: it writes Open Graph's address \
                         from the page's canonical address (ADR-0311)",
                        decl.name
                    ),
                    false => format!(
                        "`<meta {host}>` in `{}` is the host's: it writes the document's charset \
                         and viewport (ADR-0182)",
                        decl.name
                    ),
                })
            } else {
                match keys.as_slice() {
                    [] => Some(format!(
                        "`<meta>` in `{}` names nothing it describes",
                        decl.name
                    )),
                    [_, _] => Some(format!(
                        "`<meta>` in `{}` names what it describes twice, by `name` and by \
                         `property`: one `<meta>` names one",
                        decl.name
                    )),
                    [(k, a)] if !matches!(a.value, AttrValue::Static(_)) => Some(format!(
                        "`<meta {k}>` in `{}` is named by a value computed in place",
                        decl.name
                    )),
                    [(k, _)] => {
                        let v = static_attr(attrs, k).unwrap_or_default();
                        let other = attrs
                            .iter()
                            .find(|a| !["name", "property", "content"].contains(&a.name.as_str()));
                        match attr("content").map(|c| &c.value) {
                            _ if other.is_some() => Some(format!(
                                "`<meta {k}=\"{v}\">` in `{}` has `{}`, which the host does not \
                                 write: a page's metadata is its name and its content",
                                decl.name,
                                other.map(|a| a.name.as_str()).unwrap_or_default()
                            )),
                            None | Some(AttrValue::None) => Some(format!(
                                "`<meta {k}=\"{v}\">` in `{}` has no content",
                                decl.name
                            )),
                            Some(AttrValue::Static(_)) if !non_blank(attrs, "content") => Some(
                                format!("`<meta {k}=\"{v}\">` in `{}` has no content", decl.name),
                            ),
                            Some(AttrValue::Expr(e)) if !content_reads_values(body, *e) => {
                                Some(format!(
                                    "`<meta {k}=\"{v}\">` in `{}` holds a value computed in \
                                     place, and its content is text and values",
                                    decl.name
                                ))
                            }
                            _ if *k == "name"
                                && ONCE.iter().any(|o| o.eq_ignore_ascii_case(v))
                                && !named.insert(v.to_ascii_lowercase()) =>
                            {
                                Some(format!(
                                    "`{}` states `<meta name=\"{v}\">` twice, which HTML allows \
                                     once",
                                    decl.name
                                ))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                }
            };
            let Some(message) = message else { continue };
            out.push(Diagnostic {
                code: crate::codes::METADATA_MISPLACED.id,
                invariant: crate::codes::METADATA_MISPLACED.invariant,
                reason: "metadata_misplaced",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: body.node_span(n),
                related: vec![Related::new(
                    hir.decl_span(decl_id),
                    format!("`{}` renders this", decl.name),
                )],
                explanation: Some(
                    "A page's metadata is what it says of itself to what reads it without \
                     showing it: a search engine's result, a link's preview. The host writes \
                     it into the document's head from the page's values, beside the page's \
                     title, as its name and its content. One in a view would describe every \
                     page that composes it; one inside an element or a block, or a second \
                     description, would leave which is meant to chance; and the charset and \
                     the viewport are the host's, the same for every page. A `<meta itemprop>` \
                     is an item's property, and is written where it is."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "write it once at the top of the page's view, beside its \
                                  `<title>`: `<meta name=\"description\" \
                                  content={store.description} />`"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// **A `<link>` is written where HTML allows it** (ADR-0189): in the body,
/// where markup is written, a link whose every relation is body-ok
/// (`stylesheet`, `preload`, `modulepreload`, `prefetch`, `preconnect`,
/// `dns-prefetch`, `pingback`), or an item's property, `itemprop`. Every
/// other relation, `canonical`, `icon`, `manifest`, `alternate`, is the
/// head's: written into the body it is not conforming and nothing reads it.
///
/// PW5035 refuses a `<link>` with neither `rel` nor `itemprop`, or both; one
/// whose `rel` is computed in place; and one with a relation that is not
/// body-ok, compared as HTML compares keywords, ignoring ASCII case.
///
/// **But a page's canonical address** (ADR-0311): one `<link
/// rel="canonical">` at the top of a page's view is the page's, written
/// into the head by its host. It is its relation and its address alone;
/// what it addresses is PW5051's. One anywhere else, in a view, inside an
/// element or a block, or a second, is refused here, saying where it goes.
fn links(hir: &Hir) -> Vec<Diagnostic> {
    const BODY_OK: [&str; 7] = [
        "dns-prefetch",
        "modulepreload",
        "pingback",
        "preconnect",
        "prefetch",
        "preload",
        "stylesheet",
    ];
    let mut out = Vec::new();
    for (decl_id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let mut roots = Vec::new();
        for e in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(e) {
                roots.extend(r.iter().copied());
            }
        }
        let own = crate::canonical::page_link(decl, body);
        for n in body.walk_markup(&roots) {
            let Node::Element { tag, attrs, .. } = body.node(n) else {
                continue;
            };
            if tag != "link" {
                continue;
            }
            let attr = |name: &str| attrs.iter().find(|a| a.name == name);
            let canonical = crate::canonical::is_canonical_link(body, n);
            let message = match (attr("rel"), attr("itemprop")) {
                // A page's canonical address, where a page states it.
                (Some(_), None) if canonical && own == Some(n) => {
                    let written = static_attr(attrs, "rel").unwrap_or_default();
                    let other = attrs
                        .iter()
                        .find(|a| !["rel", "href"].contains(&a.name.as_str()));
                    if written.split_ascii_whitespace().count() > 1 {
                        Some(format!(
                            "`<link rel=\"{written}\">` in `{}` is the page's canonical address \
                             and more: it names `canonical` alone",
                            decl.name
                        ))
                    } else {
                        other.map(|a| {
                            format!(
                                "`<link rel=\"canonical\">` in `{}` has `{}`, which the host \
                                 does not write: a canonical link is its relation and its \
                                 address",
                                decl.name, a.name
                            )
                        })
                    }
                }
                // One that is not the page's: where it is says why.
                (Some(_), None) if canonical => Some(if decl.kind != DeclKind::Page {
                    format!(
                        "`<link rel=\"canonical\">` in the {} `{}` would name one address for \
                         every page that composes it: a page states its own",
                        match decl.kind {
                            DeclKind::View => "view",
                            DeclKind::Component => "component",
                            DeclKind::Layout => "layout",
                            _ => "declaration",
                        },
                        decl.name
                    )
                } else if !roots.contains(&n) {
                    format!(
                        "`<link rel=\"canonical\">` in `{}` is inside an element or a block, \
                         not at the top of its view, where HTML does not allow it and nothing \
                         reads it",
                        decl.name
                    )
                } else {
                    format!("`{}` states its canonical address twice", decl.name)
                }),
                (None, None) => Some(format!(
                    "`<link>` in `{}` names no relation and no item's property",
                    decl.name
                )),
                (Some(_), Some(_)) => Some(format!(
                    "`<link>` in `{}` is a relation and an item's property at once: HTML \
                     allows one of `rel` and `itemprop`",
                    decl.name
                )),
                (None, Some(_)) => None,
                (Some(rel), None) => match &rel.value {
                    AttrValue::Static(_) => {
                        let written = static_attr(attrs, "rel").unwrap_or_default();
                        let relations: Vec<&str> = written.split_ascii_whitespace().collect();
                        match relations
                            .iter()
                            .find(|r| !BODY_OK.iter().any(|ok| ok.eq_ignore_ascii_case(r)))
                        {
                            _ if relations.is_empty() => {
                                Some(format!("`<link>` in `{}` names no relation", decl.name))
                            }
                            Some(head) => Some(format!(
                                "`<link rel=\"{written}\">` in `{}` is written into the body, \
                                 where HTML does not allow `{head}` and nothing reads it: the \
                                 document's head holds the page's title and metadata, and the \
                                 host's links",
                                decl.name
                            )),
                            None => None,
                        }
                    }
                    _ => Some(format!(
                        "`<link rel>` in `{}` is a value computed in place, and which relation \
                         it names decides whether HTML allows it in the body",
                        decl.name
                    )),
                },
            };
            let Some(message) = message else { continue };
            out.push(Diagnostic {
                code: crate::codes::LINK_NOT_IN_BODY.id,
                invariant: crate::codes::LINK_NOT_IN_BODY.invariant,
                reason: "link_not_in_body",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: body.node_span(n),
                related: vec![Related::new(
                    hir.decl_span(decl_id),
                    format!("`{}` renders this", decl.name),
                )],
                explanation: Some(
                    "Markup is written into the document's body. HTML allows a `<link>` there \
                     only when each of its relations is body-ok: `stylesheet`, `preload`, \
                     `modulepreload`, `prefetch`, `preconnect`, `dns-prefetch` and \
                     `pingback`; or when it is an item's property. A `canonical`, an `icon`, \
                     a `manifest` or an `alternate` link belongs to the head, and in the body \
                     nothing reads it: no search engine takes the address, and no browser \
                     shows the icon. A page's canonical address is the one the host writes \
                     into the head for it: stated once, at the top of the page's view, as \
                     its relation and its address."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: match canonical {
                        true => "state the page's canonical address once at the top of its \
                                 view, beside its `<title>`: `<link rel=\"canonical\" \
                                 href={StorePage(id)} />` (ADR-0311)"
                            .to_string(),
                        false => "state what the page says of itself as its metadata, \
                                  `<meta name=\"description\" content={...} />` (ADR-0186); \
                                  the document's other links are the host's"
                            .to_string(),
                    },
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// **What a page's canonical link addresses** (ADR-0311): a page, by its
/// declaration, as `navigate` names one (ADR-0280), `href={StorePage(id)}`.
/// Its arguments are related to the page's parameters as a call's are
/// (PW0604, PW0605), which the value relations say.
///
/// PW5051 refuses one that states no address; one written as text, which
/// could name no page or one served elsewhere; one computed in place; one
/// that names no page, or a page served at no route, or at a route with a
/// query part; and an argument computed in place, where the host is given a
/// value the page reads, or text.
fn canonical_addresses(
    ws: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    sigs: &Signatures,
) -> Vec<Diagnostic> {
    let hir = hirs[unit];
    let mut out = Vec::new();
    for (decl_id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let Some(link) = crate::canonical::page_link(decl, body) else {
            continue;
        };
        let Node::Element { attrs, .. } = body.node(link) else {
            continue;
        };
        let href = attrs.iter().find(|a| a.name == "href");
        let mut refuse = |message: String, at: hir::Span| {
            out.push(Diagnostic {
                code: crate::codes::CANONICAL_ADDRESS.id,
                invariant: crate::codes::CANONICAL_ADDRESS.invariant,
                reason: "canonical_address",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: at,
                related: vec![Related::new(
                    hir.decl_span(decl_id),
                    format!("`{}`'s canonical address", decl.name),
                )],
                explanation: Some(
                    "A page's canonical address is the one a search engine takes for it \
                     and a link's preview names. It is a page's, named by its declaration \
                     and given its parameters as `navigate` gives them, so the address the \
                     host writes is one a page is served at, with values of the types its \
                     route gives. The host writes it absolute, with the deployment's origin."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "write the page and its arguments: `<link rel=\"canonical\" \
                                  href={StorePage(id)} />`"
                        .to_string(),
                    replacement: None,
                }],
            });
        };
        let call = match href.map(|a| &a.value) {
            None | Some(AttrValue::None) => {
                refuse(
                    format!(
                        "`<link rel=\"canonical\">` in `{}` states no address",
                        decl.name
                    ),
                    body.node_span(link),
                );
                continue;
            }
            Some(AttrValue::Static(_)) => {
                refuse(
                    format!(
                        "`<link rel=\"canonical\">` in `{}` states its address as text: a \
                         canonical address is a page's, which the compiler checks",
                        decl.name
                    ),
                    body.node_span(link),
                );
                continue;
            }
            Some(AttrValue::Expr(e)) => match body.expr(*e) {
                Expr::Address { page, args } => (*e, page.clone(), args),
                Expr::Interpolated { .. } | Expr::Literal(_) => {
                    refuse(
                        format!(
                            "`<link rel=\"canonical\">` in `{}` states its address as text: a \
                             canonical address is a page's, which the compiler checks",
                            decl.name
                        ),
                        body.expr_span(*e),
                    );
                    continue;
                }
                _ => {
                    refuse(
                        format!(
                            "`<link rel=\"canonical\">` in `{}` is an address computed in \
                             place: a canonical address is a page's",
                            decl.name
                        ),
                        body.expr_span(*e),
                    );
                    continue;
                }
            },
        };
        let (e, name, args) = call;
        let def = crate::canonical::page_named(ws, unit, &name);
        let kind = def.and_then(|d| sigs.kind_of(d)).or_else(|| {
            // What it names otherwise, said by its kind, as `navigate`'s is.
            match ws.resolve_in(unit, crate::resolve::Namespace::Term, &name) {
                crate::resolve::Resolution::Local(d)
                | crate::resolve::Resolution::Imported { def: d, .. } => sigs.kind_of(d),
                _ => None,
            }
        });
        if kind != Some(DeclKind::Page) {
            let what = match kind {
                Some(k) => format!("names {}", described(k)),
                None => "names nothing this module sees".to_string(),
            };
            refuse(
                format!(
                    "`<link rel=\"canonical\" href={{{name}(..)}}>` in `{}` {what}: a \
                     canonical address is a page's",
                    decl.name
                ),
                body.expr_span(e),
            );
            continue;
        }
        let Some(def) = def else { continue };
        // Its route, read where the page is declared.
        let route = crate::resolve::declaration(hirs, def)
            .and_then(|d| crate::routes::declared_route(hirs[def.unit], d));
        match route {
            None => refuse(
                format!(
                    "`<link rel=\"canonical\">` in `{}` names `{name}`, which is served at no \
                     route",
                    decl.name
                ),
                body.expr_span(e),
            ),
            Some(r) if r.contains("{?") => refuse(
                format!(
                    "`<link rel=\"canonical\">` in `{}` names `{name}`, whose route `{r}` has a \
                     query part: a canonical address with a query is not claimed",
                    decl.name
                ),
                body.expr_span(e),
            ),
            Some(_) => {}
        }
        for a in args {
            let text = matches!(body.expr(a.value), Expr::Literal(hir::Literal::Str(_)));
            if !text && crate::template_ir::value_path_of(body, a.value).is_none() {
                refuse(
                    format!(
                        "`<link rel=\"canonical\">` in `{}` gives `{name}` a value computed in \
                         place: each argument is a value the page reads, or text",
                        decl.name
                    ),
                    body.expr_span(a.value),
                );
            }
        }
    }
    out
}

/// Whether a `content` written as a value reads values: a path, or text with
/// holes that are each a path.
fn content_reads_values(body: &Body, e: ExprId) -> bool {
    match body.expr(e) {
        Expr::Interpolated { parts, .. } => parts
            .iter()
            .all(|p| crate::template_ir::value_path_of(body, *p).is_some()),
        _ => crate::template_ir::value_path_of(body, e).is_some(),
    }
}

/// **A page states its title** (ADR-0183): `<title>` at the top of its view,
/// beside `<main>`, as text and the values the page reads. The host writes
/// it into the document's head. Until 2026-10-04 a page could not say what
/// it is, and every store's page was titled "Store" by the host (WCAG 2.4.2,
/// failure F25; found by ADR-0182's audit).
///
/// PW5029 refuses a page served at a route that states no title. PW5030
/// refuses a `<title>` anywhere else: in a view, which would title every
/// page that composes it; inside an element or a block; a second one; or one
/// that holds more than text and values.
fn titles(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (decl_id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let mut roots = Vec::new();
        for e in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(e) {
                roots.extend(r.iter().copied());
            }
        }
        let page = decl.kind == DeclKind::Page;
        let mut stated = None;
        for n in body.walk_markup(&roots) {
            let Node::Element { tag, children, .. } = body.node(n) else {
                continue;
            };
            // `<title>` exactly: `<Title>` is a view (ADR-0072).
            if tag != "title" {
                continue;
            }
            // What the title holds besides text and the values it reads.
            let other = children.iter().find_map(|c| match body.node(*c) {
                Node::Text(_) => None,
                Node::Interpolation(e) => crate::template_ir::value_path_of(body, *e)
                    .is_none()
                    .then(|| "a value computed in place".to_string()),
                Node::Element { tag, .. } => Some(format!("`<{tag}>`")),
                Node::Block { .. } | Node::Branch { .. } => Some("a block".to_string()),
            });
            let misplaced = if !page {
                let kind = match decl.kind {
                    DeclKind::View => "view",
                    DeclKind::Component => "component",
                    DeclKind::Layout => "layout",
                    _ => "declaration",
                };
                Some(format!(
                    "`<title>` in the {kind} `{}` titles every page that composes it",
                    decl.name
                ))
            } else if !roots.contains(&n) {
                Some(format!(
                    "`<title>` in `{}` is inside an element or a block, not at the top of its view",
                    decl.name
                ))
            } else if stated.is_some() {
                Some(format!("`{}` states a second `<title>`", decl.name))
            } else if let Some(other) = other {
                Some(format!(
                    "`<title>` in `{}` holds {other}, and a title is text and values",
                    decl.name
                ))
            } else {
                stated = Some(n);
                None
            };
            let Some(message) = misplaced else { continue };
            out.push(Diagnostic {
                code: crate::codes::TITLE_MISPLACED.id,
                invariant: crate::codes::TITLE_MISPLACED.invariant,
                reason: "title_misplaced",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: body.node_span(n),
                related: vec![Related::new(
                    hir.decl_span(decl_id),
                    format!("`{}` renders this", decl.name),
                )],
                explanation: Some(
                    "A page's title is the page's statement of what it is, and the \
                     host writes it into the document's head from the page's values. \
                     One in a view would title every page that composes the view; one \
                     inside an element or a block, or a second, would leave which is \
                     the title to chance; and a title holds text, which a screen \
                     reader says first."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "write one `<title>` at the top of the page's view, beside \
                                  `<main>`, as text and values: `<title>{store.name}</title>`"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
        // A title with neither text nor a value states nothing.
        let blank = stated.is_some_and(|t| match body.node(t) {
            Node::Element { children, .. } => children
                .iter()
                .all(|c| matches!(body.node(*c), Node::Text(s) if s.trim().is_empty())),
            _ => false,
        });
        if !page || !(stated.is_none() || blank) {
            continue;
        }
        let Some(route) = crate::routes::declared_route(hir, decl) else {
            continue;
        };
        // Where the title goes: the page's view.
        let view = body
            .walk()
            .into_iter()
            .find(|e| matches!(body.expr(*e), Expr::Template { .. }))
            .map(|e| body.expr_span(e))
            .unwrap_or_else(|| hir.decl_span(decl_id));
        out.push(Diagnostic {
            code: crate::codes::TITLE_MISSING.id,
            invariant: crate::codes::TITLE_MISSING.invariant,
            reason: "title_missing",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!("`{}` is served at `{route}` and states no title", decl.name),
            primary_span: match stated {
                Some(t) => body.node_span(t),
                None => hir.decl_span(decl_id),
            },
            related: vec![Related::new(
                view,
                format!("`{}`'s view, at whose top its title is written", decl.name),
            )],
            explanation: Some(
                "A screen reader says a page's title first, and a person tells pages \
                 apart by it, in tabs, history and bookmarks. A page that states none is \
                 titled by its host, which knows only what kind of page it is: every \
                 store's page was \"Store\", which WCAG 2.4.2 names failure F25."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: format!(
                    "write `<title>` at the top of `{}`'s view, from what it shows: \
                     `<title>{{store.name}}</title>`",
                    decl.name
                ),
                replacement: None,
            }],
        });
    }
    out
}

fn comment_as_text(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (decl_id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let mut roots = Vec::new();
        for e in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(e) {
                roots.extend(r.iter().copied());
            }
        }
        // A stylesheet's or a script's text is its own language's, where
        // `/* */` and `//` are comments (PW5023 refuses a script's anyway).
        let mut raw = std::collections::HashSet::new();
        for n in body.walk_markup(&roots) {
            if let Node::Element { tag, children, .. } = body.node(n)
                && matches!(tag.to_ascii_lowercase().as_str(), "style" | "script")
            {
                raw.extend(body.walk_markup(children));
            }
        }
        for n in body.walk_markup(&roots) {
            let Node::Text(text) = body.node(n) else {
                continue;
            };
            if raw.contains(&n) {
                continue;
            }
            let at = body.node_span(n);
            let mut offset = 0;
            for line in text.split_inclusive('\n') {
                let written = line.trim_start();
                let start = offset + (line.len() - written.len());
                offset += line.len();
                if !(written.starts_with("//") || written.starts_with("/*")) {
                    continue;
                }
                let shown = written.trim_end();
                out.push(Diagnostic {
                    code: crate::codes::COMMENT_AS_TEXT.id,
                    invariant: crate::codes::COMMENT_AS_TEXT.invariant,
                    reason: "comment_as_text",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message: format!("`{shown}` reads as a comment, and the page would show it"),
                    primary_span: at.start + start..at.start + start + shown.len(),
                    related: vec![Related::new(
                        hir.decl_span(decl_id),
                        format!("`{}` renders this", decl.name),
                    )],
                    // Until 2026-10-03 such a line between two elements checked,
                    // and the page showed it.
                    explanation: Some(
                        "In markup, `//` and `/*` begin no comment: what follows is the \
                         page's text, as in HTML and JSX, and the page would show it \
                         (ADR-0167)."
                            .to_string(),
                    ),
                    repairs: vec![
                        Repair {
                            description: "write a comment as `<!-- -->`, which no page shows"
                                .to_string(),
                            replacement: None,
                        },
                        Repair {
                            description: "write text that begins so as a string: \
                                          `{\"// …\"}`"
                                .to_string(),
                            replacement: None,
                        },
                    ],
                });
            }
        }
    }
    out
}

/// **A template writes no code** (ADR-0094).
///
/// The renderer escapes a value by where it sits: text, an attribute, a URL,
/// a style, or raw HTML behind a capability. Four places are none of those.
/// A `<script>` element's text and an inline handler (`onclick`) are
/// JavaScript, `srcdoc` is a document whose scripts run on this page's
/// origin, and a `<style>` element's text is a stylesheet. Until 2026-09-26
/// a value in each was escaped as text or an attribute, which neutralizes
/// none of them. Code written there by hand is code the language never
/// checks, and is refused with it: handlers are bound with `on:`.
fn code_in_markup(hir: &Hir) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (decl_id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let mut roots = Vec::new();
        for e in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(e) {
                roots.extend(r.iter().copied());
            }
        }
        for n in body.walk_markup(&roots) {
            let Node::Element {
                tag,
                attrs,
                children,
                ..
            } = body.node(n)
            else {
                continue;
            };
            let mut found: Vec<(crate::hir::Span, String, &str)> = Vec::new();
            let tag = tag.to_ascii_lowercase();
            // ADR-0096: what moves a URL rather than writes code.
            let mut moved: Vec<(crate::hir::Span, String, &str)> = Vec::new();
            if tag == "base" {
                moved.push((
                    body.node_span(n),
                    "`<base>` moves every relative URL after it, the platform's runtime \
                     among them"
                        .to_string(),
                    "write the path whole; the document's base is the platform's",
                ));
            }
            if matches!(tag.as_str(), "animate" | "set")
                && let Some(target) = attrs
                    .iter()
                    .find(|a| a.name.eq_ignore_ascii_case("attributeName"))
            {
                let named = match &target.value {
                    AttrValue::Static(text) => Some(text.trim_matches('"').to_string()),
                    _ => None,
                };
                let runs = |name: &str| {
                    let local = name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase();
                    crate::template_ir::Context::of_attribute(name)
                        == crate::template_ir::Context::Url
                        || local.strip_prefix("on").is_some_and(|e| {
                            !e.is_empty() && e.chars().all(|c| c.is_ascii_alphabetic())
                        })
                };
                match named {
                    Some(name) if !runs(&name) => {}
                    Some(name) => moved.push((
                        target.span.clone(),
                        format!("`<{tag}>` sets `{name}`, past the check its own attribute gets"),
                        "write the link's URL on the link, and bind a handler with `on:`",
                    )),
                    None => moved.push((
                        target.span.clone(),
                        format!("`<{tag}>` sets an attribute a value names"),
                        "name the animated attribute as text",
                    )),
                }
            }
            for (span, message, repair) in moved {
                out.push(Diagnostic {
                    code: crate::codes::MOVES_A_URL.id,
                    invariant: crate::codes::MOVES_A_URL.invariant,
                    reason: "moves_a_url",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message,
                    primary_span: span,
                    related: vec![Related::new(
                        hir.decl_span(decl_id),
                        format!("`{}` renders this", decl.name),
                    )],
                    // Until 2026-09-26 both checked and built.
                    explanation: Some(
                        "A view renders into a page that loads the platform's runtime after \
                         the view's markup, by a relative URL; a `<base>` element anywhere in \
                         the document moves it, and every link after it. An animation sets \
                         the attribute it names, so `<animate attributeName=\"href\">` gives a \
                         link a URL that never passed the check a URL attribute gets."
                            .to_string(),
                    ),
                    repairs: vec![Repair {
                        description: repair.to_string(),
                        replacement: None,
                    }],
                });
            }
            if tag == "script" {
                found.push((
                    body.node_span(n),
                    "`<script>` is code the language never checks".to_string(),
                    "bind a handler with `on:`, or compute the value in the program",
                ));
            }
            if tag == "style"
                && children
                    .iter()
                    .any(|c| !matches!(body.node(*c), Node::Text(_)))
            {
                found.push((
                    body.node_span(n),
                    "`<style>` holds a value a program writes, inside a stylesheet".to_string(),
                    "give the value to a `style` attribute, which escapes it as a style",
                ));
            }
            for a in attrs {
                let name = a.name.to_ascii_lowercase();
                if let Some(event) = name.strip_prefix("on")
                    && !event.is_empty()
                    && event.chars().all(|c| c.is_ascii_alphabetic())
                {
                    found.push((
                        a.span.clone(),
                        format!("`{}` is code the language never checks", a.name),
                        "bind a handler with `on:`",
                    ));
                }
                // What the author wrote: static text, or a string with holes.
                let written = match &a.value {
                    AttrValue::Static(text) => Some(text.as_str()),
                    AttrValue::Expr(e) => match body.expr(*e) {
                        Expr::Interpolated { text, .. } => Some(text.as_str()),
                        _ => None,
                    },
                    AttrValue::None => None,
                };
                if let Some(text) = written
                    && crate::template_ir::Context::of_attribute(&a.name)
                        == crate::template_ir::Context::Url
                    && let Some(why) = executing_url(text.trim_matches(|c| c == '"' || c == '\''))
                {
                    found.push((
                        a.span.clone(),
                        format!("`{}` {why}", a.name),
                        "link to a location, and run code through a handler bound with `on:`",
                    ));
                }
                if name == "srcdoc" {
                    found.push((
                        a.span.clone(),
                        "`srcdoc` is a document written inline, and its scripts run on this \
                         page's origin"
                            .to_string(),
                        "load the document with `src`",
                    ));
                }
            }
            for (span, message, repair) in found {
                out.push(Diagnostic {
                    code: crate::codes::CODE_IN_MARKUP.id,
                    invariant: crate::codes::CODE_IN_MARKUP.invariant,
                    reason: "code_in_markup",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message,
                    primary_span: span,
                    related: vec![Related::new(
                        hir.decl_span(decl_id),
                        format!("`{}` renders this", decl.name),
                    )],
                    // Until 2026-09-26 `<script>{msg}</script>` and `onclick={msg}`
                    // checked and built, and ran `msg`.
                    explanation: Some(
                        "The renderer escapes a value by where it sits: text, an attribute, a \
                         URL, a style. A script, an inline handler, a `srcdoc` document and a \
                         stylesheet are none of those, and no escaping makes a value in them \
                         inert: `<script>{msg}</script>` and `onclick={msg}` would run `msg`."
                            .to_string(),
                    ),
                    repairs: vec![Repair {
                        description: repair.to_string(),
                        replacement: None,
                    }],
                });
            }
        }
    }
    out
}

/// **What is wrong with a URL an author wrote, where it runs code** (ADR-0094).
///
/// `javascript:go()` is a script, and a value written after it runs too: a
/// browser percent-decodes the URL before it runs, so an encoded `id` in
/// `javascript:go({id})` is code again. A scheme a value completes,
/// `jav{x}ascript:`, is one no author wrote. Whitespace and controls are
/// dropped first, as a browser's URL parser drops them. `data:` is left to
/// the author: an image can be one, and a document in one runs on an opaque
/// origin.
fn executing_url(text: &str) -> Option<String> {
    const HOLE: char = '\u{1}';
    let mut written = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        written.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            rest = &rest[open..];
            break;
        };
        written.push(HOLE);
        rest = &rest[open + close + 1..];
    }
    written.push_str(rest);
    let url: String = written
        .chars()
        .filter(|c| *c == HOLE || *c > ' ')
        .collect::<String>()
        .to_ascii_lowercase();
    let end =
        url.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-' | HOLE)))?;
    if !url[end..].starts_with(':') {
        return None;
    }
    let scheme = &url[..end];
    if scheme.contains(HOLE) {
        return Some("takes its scheme from a value".to_string());
    }
    ["javascript", "vbscript"]
        .contains(&scheme)
        .then(|| format!("begins with `{scheme}:`, a URL that runs code"))
}

/// **An element named with a capital letter** (ADR-0072).
///
/// HTML lowercases every tag it parses, so `<Money>` is never HTML. The
/// charter writes a view used in another view that way:
/// `<Money value={item.price} />` (§8.1). Until 2026-09-26 nothing read such a
/// tag. It built as an unknown element named `Money`: the view's markup was
/// never rendered, and its props were checked by nothing. A view used in
/// another view is not compiled yet, so it is refused, and so is a tag that
/// names no view.
fn view_elements(
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    use crate::resolve::Resolution;

    let mut out = Vec::new();
    for (decl_id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let mut roots = Vec::new();
        for e in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(e) {
                roots.extend(r.iter().copied());
            }
        }
        for n in body.walk_markup(&roots) {
            let Node::Element { tag, attrs, .. } = body.node(n) else {
                continue;
            };
            if !tag.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            let resolved = match workspace.resolve_in(unit, crate::resolve::Namespace::Ui, tag) {
                Resolution::Local(def) | Resolution::Imported { def, .. } => {
                    crate::resolve::declaration(hirs, def).map(|d| (def, d.kind))
                }
                Resolution::Unresolved | Resolution::Ambiguous(_) => None,
            };
            // Resolve rendered declarations in their namespace; a different
            // namespace is consulted only to preserve the diagnostic for a
            // tag that names no UI declaration.
            let named = resolved
                .map(|(_, k)| k)
                .or_else(|| match workspace.resolve(unit, tag) {
                    Resolution::Local(def) | Resolution::Imported { def, .. } => {
                        crate::resolve::declaration(hirs, def).map(|d| d.kind)
                    }
                    Resolution::Unresolved | Resolution::Ambiguous(_) => None,
                });
            // **A view composes** (ADR-0136): its markup is written into the
            // page where it is used, its parameters read as the props given.
            // What this slice composes: a view whose body is its markup alone,
            // that does not contain itself, given a value path for each prop.
            if let Some((def, DeclKind::View)) = resolved {
                let mut why = composes(workspace, hirs, def);
                if why.is_none() {
                    why = attrs.iter().find_map(|a| match &a.value {
                        AttrValue::Expr(e)
                            if crate::template_ir::value_path_of(body, *e).is_none() =>
                        {
                            Some(format!(
                                "`<{tag}>`'s prop `{}` is computed, and a prop is a value: a name, \
                                 or fields read from one (ADR-0073)",
                                a.name
                            ))
                        }
                        _ => None,
                    });
                }
                let Some(message) = why else {
                    continue;
                };
                out.push(Diagnostic {
                    code: crate::codes::VIEW_ELEMENT.id,
                    invariant: crate::codes::VIEW_ELEMENT.invariant,
                    reason: "view_element",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message,
                    primary_span: body.node_span(n),
                    related: vec![Related::new(
                        hir.decl_span(decl_id),
                        format!("`{}` renders this", decl.name),
                    )],
                    explanation: Some(
                        "A view used in another is composed when the page is built: its \
                         markup is written where it is used, and its parameters read as the \
                         props it is given (ADR-0136)."
                            .to_string(),
                    ),
                    repairs: vec![Repair {
                        description: format!("write `{tag}`'s markup here, or simplify `{tag}`"),
                        replacement: None,
                    }],
                });
                continue;
            }
            let (message, repair) = match named {
                // A page is shown in its layout (ADR-0303): the page names
                // it, and no markup uses it.
                Some(DeclKind::Layout) => (
                    format!(
                        "`<{tag}>` uses the layout `{tag}` as an element, and a page is shown \
                         in a layout by naming it"
                    ),
                    format!("write `layout {tag}` in the page"),
                ),
                Some(DeclKind::Component | DeclKind::Page) => (
                    format!(
                        "`<{tag}>` uses `{tag}` inside another view, and only a view is used \
                         as an element"
                    ),
                    format!("make `{tag}` a view, or write its markup here"),
                ),
                Some(_) => (
                    format!("`<{tag}>` names a declaration that is not a view"),
                    "an element is an HTML element, written in lowercase, or a view".to_string(),
                ),
                None => (
                    format!("`<{tag}>` names no view in scope"),
                    format!("import `{tag}`, declare it, or write an HTML element in lowercase"),
                ),
            };
            out.push(Diagnostic {
                code: crate::codes::VIEW_ELEMENT.id,
                invariant: crate::codes::VIEW_ELEMENT.invariant,
                reason: "view_element",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: body.node_span(n),
                related: vec![Related::new(
                    hir.decl_span(decl_id),
                    format!("`{}` renders this", decl.name),
                )],
                // Until 2026-09-26 one built as an unknown HTML element, and the
                // view's markup was never rendered.
                explanation: Some(
                    "HTML lowercases every tag it parses, so an element named with a capital \
                     letter is a view. Built as HTML, it would be an element no browser \
                     knows, and the view's markup would never render."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: repair,
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// **A data source, as the program states it** (ADR-0207): what it holds,
/// by the names the program's effects give them, and what it guarantees.
/// A clause left out guarantees nothing.
#[derive(Debug, Clone)]
pub(crate) struct Source {
    pub(crate) name: String,
    pub(crate) unit: usize,
    pub(crate) span: crate::hir::Span,
    pub(crate) holds: Vec<(String, crate::hir::Span)>,
    /// The isolation its transactions have, `none` for none.
    transactions: String,
    /// What its reads may promise.
    reads: BTreeSet<String>,
    /// Whether it tells what changed, once committed.
    feed: bool,
    /// **What one set read costs it** (4c), as its `sets` clause says:
    /// `one_call`, `chunked(k)` or `per_key`. `None` where it states none,
    /// which guarantees nothing: `per_key`.
    pub(crate) sets: Option<String>,
}

/// Every source the program declares.
pub(crate) fn declared_sources(hirs: &[&Hir]) -> Vec<Source> {
    let mut out = Vec::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if decl.kind != DeclKind::Source {
                continue;
            }
            let words = |head: &str| -> Vec<String> {
                decl.policy(head)
                    .map(|p| {
                        p.value
                            .split(',')
                            .map(|w| w.trim().to_string())
                            .filter(|w| !w.is_empty())
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let holds = decl
                .policy("holds")
                .map(|p| {
                    p.value
                        .split(',')
                        .map(str::trim)
                        .filter(|w| !w.is_empty())
                        .map(|w| (w.to_string(), p.span.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let reads: BTreeSet<String> = words("reads").into_iter().collect();
            out.push(Source {
                name: decl.name.clone(),
                unit,
                span: hir.decl_span(id),
                holds,
                transactions: words("transactions")
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| "none".to_string()),
                reads: if reads.is_empty() {
                    BTreeSet::from(["eventual".to_string()])
                } else {
                    reads
                },
                feed: words("changes").first().is_some_and(|w| w == "feed"),
                sets: decl.policy("sets").map(|p| p.value.trim().to_string()),
            });
        }
    }
    out
}

/// **A data source's clauses, as a host compares them with what its
/// database provides** (ADR-0207's "Not claimed", ADR-0246): what `pw build`
/// writes to `sources.json`. Read as the checker reads them: a clause left
/// out guarantees nothing, so `transactions` is `none`, `reads` is
/// `eventual`, `changes` is `none` and `sets` is `per_key` (4c).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceClauses {
    pub name: String,
    /// What it holds, as the program's effects name it: `Post` for
    /// `database.read<Post>`.
    pub holds: Vec<String>,
    pub transactions: String,
    pub reads: Vec<String>,
    /// `feed` or `none`.
    pub changes: String,
    /// **What one set read costs it** (4c): `one_call`, `chunked(k)` or
    /// `per_key`, the last where it states none. Defaulted where a build
    /// from before 4c is read.
    #[serde(default = "per_key")]
    pub sets: String,
}

fn per_key() -> String {
    "per_key".to_string()
}

/// Every source the program declares, as [`SourceClauses`].
pub fn source_clauses(hirs: &[&Hir]) -> Vec<SourceClauses> {
    declared_sources(hirs)
        .into_iter()
        .map(|s| SourceClauses {
            name: s.name,
            holds: s.holds.into_iter().map(|(h, _)| h).collect(),
            transactions: s.transactions,
            reads: s.reads.into_iter().collect(),
            changes: if s.feed { "feed" } else { "none" }.to_string(),
            sets: crate::bounded::Sets::of(s.sets.as_deref()).written(),
        })
        .collect()
}

/// How much a transaction's isolation prevents, as a command is written
/// against it: each level prevents what the one below does, and more
/// (Berenson et al., SIGMOD 1995; Jepsen's consistency models).
fn isolation(word: &str) -> u8 {
    match word {
        "serializable" => 3,
        "snapshot" => 2,
        "read_committed" => 1,
        _ => 0,
    }
}

/// Does a source whose reads give `given` give a query that asks `asked`?
/// `strong` gives every promise, and every source gives `eventual`.
fn reads_give(given: &BTreeSet<String>, asked: &str) -> bool {
    asked == "eventual" || given.contains("strong") || given.contains(asked)
}

/// **A data source states what it guarantees, and nothing asks it for more**
/// (ADR-0207, ADR-0195's ruling 11).
///
/// A resource no source holds is the host's own database's (ADR-0005,
/// ADR-0019): serializable, read strongly, a command's events committed
/// with its writes. A declared source may give less, and each query and
/// command is held to what it gives:
/// - a query's `consistency`, to the source's `reads`;
/// - a command's writes, to one source, one transaction;
/// - a command's `transaction`, to the source's `transactions`;
/// - a command's `emits`, to a transaction its events commit in, the
///   outbox, or a feed of the source's changes;
/// - a command's `idempotent_by`, to a transaction its record commits in.
fn sources_give_what_is_asked(
    inference: &crate::effects::Inference<'_>,
    sources: &[Source],
    visible: &BTreeSet<String>,
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let diagnostic = |code: crate::codes::Code,
                      reason: &'static str,
                      message: String,
                      span: crate::hir::Span,
                      source: &Source,
                      explanation: &str,
                      repair: String| Diagnostic {
        code: code.id,
        invariant: code.invariant,
        reason,
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message,
        primary_span: span,
        related: vec![Related::new(
            source.span.clone(),
            format!("`{}` states what it guarantees here", source.name),
        )],
        explanation: Some(explanation.to_string()),
        repairs: vec![Repair {
            description: repair,
            replacement: None,
        }],
    };
    // The sources this unit declares, held to what they name.
    for source in sources.iter().filter(|s| s.unit == unit) {
        if source.holds.is_empty() {
            out.push(diagnostic(
                crate::codes::SOURCE_MALFORMED,
                "source_malformed",
                format!("`{}` holds nothing", source.name),
                source.span.clone(),
                source,
                "A source states what a database the program's effects name guarantees, \
                 and holds the resources its effects name: `holds Carts, Orders` for \
                 `database.write<Carts>`.",
                format!("write `holds` and the resources `{}` holds", source.name),
            ));
        }
        for (held, span) in &source.holds {
            if !visible.contains(held) {
                out.push(diagnostic(
                    crate::codes::SOURCE_MALFORMED,
                    "source_malformed",
                    format!(
                        "`{}` holds `{held}`, which names no type or module visible here",
                        source.name
                    ),
                    span.clone(),
                    source,
                    "A source holds what an effect names: `database.read<Carts>` names \
                     `Carts`, a type or a module the program can see.",
                    format!("name what `database.read<..>` names, or import `{held}`"),
                ));
            }
            if let Some(other) = sources
                .iter()
                .find(|o| o.name != source.name && o.holds.iter().any(|(h, _)| h == held))
            {
                out.push(diagnostic(
                    crate::codes::SOURCE_MALFORMED,
                    "source_malformed",
                    format!(
                        "`{held}` is held by `{}` and by `{}`",
                        source.name, other.name
                    ),
                    span.clone(),
                    source,
                    "One resource is in one database: what a query of it may promise, and \
                     what a command's writes to it commit with, are that database's.",
                    format!("hold `{held}` in one of them"),
                ));
            }
        }
    }
    let held_by = |resource: &str| {
        sources
            .iter()
            .find(|s| s.holds.iter().any(|(h, _)| h == resource))
    };
    for (id, decl) in hir.all_decls() {
        let effects = inference.effective_effects(unit, hir, id);
        let span = hir.decl_span(id);
        match decl.kind {
            DeclKind::Query | DeclKind::Subscription | DeclKind::Resource => {
                let Some(asked) = decl.policy("consistency").map(|p| p.value.trim()) else {
                    continue;
                };
                let reads: BTreeSet<&str> = effects
                    .iter()
                    .filter_map(|e| crate::effects::database_domain(e, "read"))
                    .collect();
                for resource in reads {
                    let Some(source) = held_by(resource) else {
                        continue;
                    };
                    if reads_give(&source.reads, asked) {
                        continue;
                    }
                    let given: Vec<&str> = source.reads.iter().map(String::as_str).collect();
                    out.push(diagnostic(
                        crate::codes::READS_MORE_THAN_GIVEN,
                        "reads_more_than_given",
                        format!(
                            "`{}` asks `consistency {asked}` of `{resource}`, and `{}` reads `{}`",
                            decl.name,
                            source.name,
                            given.join(", ")
                        ),
                        span.clone(),
                        source,
                        "A query's `consistency` is a promise about what a reader sees, and it \
                         is the source's to keep: a source whose reads are eventual cannot \
                         show a reader its own write at once, however the query is written.",
                        format!(
                            "ask what `{}` gives, or read `{resource}` from a source that gives \
                             `{asked}`",
                            source.name
                        ),
                    ));
                }
            }
            DeclKind::Command => {
                let written: BTreeSet<&str> = effects
                    .iter()
                    .filter_map(|e| crate::effects::database_domain(e, "write"))
                    .collect();
                if written.is_empty() {
                    continue;
                }
                // Each resource's source: a declared one, or the host's own.
                let mut by_source: BTreeMap<String, Vec<&str>> = BTreeMap::new();
                for w in &written {
                    let name = held_by(w).map_or_else(
                        || "the host's database".to_string(),
                        |s| format!("`{}`", s.name),
                    );
                    by_source.entry(name).or_default().push(w);
                }
                if by_source.len() > 1 {
                    let spans: Vec<String> = by_source
                        .iter()
                        .map(|(source, rs)| {
                            let rs: Vec<String> = rs.iter().map(|r| format!("`{r}`")).collect();
                            format!("{} in {source}", rs.join(", "))
                        })
                        .collect();
                    // Two sources include a declared one, since every
                    // resource no source holds is the one host's database:
                    // there is always a source to point to.
                    let Some(first) = written.iter().find_map(|w| held_by(w)) else {
                        continue;
                    };
                    out.push(diagnostic(
                        crate::codes::WRITES_TWO_SOURCES,
                        "writes_two_sources",
                        format!(
                            "`{}` writes {}, and no one transaction spans two sources",
                            decl.name,
                            spans.join(" and ")
                        ),
                        span.clone(),
                        first,
                        "A command commits all of its writes or none. Two databases commit \
                         apart: one of them could keep a write the other lost, and nothing \
                         would undo it.",
                        "write one source in a command, and the other in a command it leads \
                         to"
                        .to_string(),
                    ));
                    continue;
                }
                let Some(source) = written.iter().find_map(|w| held_by(w)) else {
                    continue;
                };
                if let Some(asked) = decl.policy("transaction").map(|p| p.value.trim())
                    && isolation(asked) > isolation(&source.transactions)
                {
                    out.push(diagnostic(
                        crate::codes::ISOLATION_MORE_THAN_GIVEN,
                        "isolation_more_than_given",
                        format!(
                            "`{}` asks `transaction {asked}` of `{}`, whose transactions are \
                             `{}`",
                            decl.name, source.name, source.transactions
                        ),
                        span.clone(),
                        source,
                        "A command's `transaction` is the isolation it is written against, and \
                         its source's to give: an anomaly the command assumes away happens \
                         where the source does not prevent it.",
                        format!("ask what `{}` gives", source.name),
                    ));
                }
                let atomic = source.transactions != "none";
                if decl.policy("emits").is_some() && !atomic && !source.feed {
                    out.push(diagnostic(
                        crate::codes::EVENTS_WITHOUT_COMMIT,
                        "events_without_commit",
                        format!(
                            "`{}` emits events of its writes to `{}`, which commits them in no \
                             transaction and tells no change",
                            decl.name, source.name
                        ),
                        span.clone(),
                        source,
                        "An event is sent if and only if its writes commit: written in their \
                         transaction, the outbox, or read from the source's own feed of what \
                         changed. With neither, an event could tell of a write that never \
                         committed, or be lost for one that did.",
                        format!(
                            "give `{}` transactions or `changes feed`, or emit nothing",
                            source.name
                        ),
                    ));
                }
                if decl.policy("idempotent_by").is_some() && !atomic {
                    out.push(diagnostic(
                        crate::codes::IDEMPOTENT_WITHOUT_COMMIT,
                        "idempotent_without_commit",
                        format!(
                            "`{}` is idempotent by its interaction, and `{}` commits its writes \
                             in no transaction",
                            decl.name, source.name
                        ),
                        span.clone(),
                        source,
                        "A command sent again answers what it answered the first time, by a \
                         record of the interaction committed with its writes. Without a \
                         transaction, a write could commit and its record not, and the command \
                         would run twice.",
                        format!("give `{}` transactions", source.name),
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// **Why the view `def` cannot be composed yet**, if it cannot (ADR-0136):
/// it declares bindings of its own, or it contains itself, directly or
/// through the views it uses. A view it uses that cannot be composed is
/// reported where that one is used, not again at every use around it.
fn composes(
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    def: crate::resolve::DefId,
) -> Option<String> {
    let decl = crate::resolve::declaration(hirs, def)?;
    let body = hirs[def.unit].body(decl.body?);
    if !crate::template_ir::composable(body) {
        return Some(format!(
            "`<{}>` declares values of its own, and a view composed into another holds its \
             markup and its signals alone",
            decl.name
        ));
    }
    // A view that contains itself is an instance made at run time, where a
    // block, `{#if}`, `{#match}` or `{#each}`, is on the way back to it:
    // the data ends it (ADR-0203, ADR-0130's ruling 2). With none, no data
    // does.
    if crate::template_ir::contains_itself_endlessly(workspace, hirs, def) {
        return Some(format!(
            "`<{}>` contains itself with no `{{#if}}`, `{{#match}}` or `{{#each}}` on the way \
             back to it, so it would never end",
            decl.name
        ));
    }
    if crate::template_ir::contains_itself(workspace, hirs, def)
        && !(crate::page_values::signals_of(body).is_empty()
            && crate::template_ir::provides_of(hirs, workspace, def.unit, body).is_empty())
    {
        return Some(format!(
            "`<{}>` contains itself and holds a signal, and a view that contains itself holds \
             none yet",
            decl.name
        ));
    }
    // A host runs the streams its page's plan names, and an instance's are
    // its view's own template's (ADR-0148, ADR-0203).
    if crate::template_ir::contains_itself(workspace, hirs, def)
        && crate::template_ir::shows_a_stream(body)
    {
        return Some(format!(
            "`<{}>` contains itself and shows a query's state in a `<stream>`, and a view that \
             contains itself shows none yet",
            decl.name
        ));
    }
    None
}

/// **An optimistic transition performs nothing.**
///
/// Architect ruling, 2026-08-11 (ADR-0025):
///
/// > The desired shape is `Cart → Cart`, not `Cart → arbitrary client program
/// > with arbitrary effects`. […] That gives automatic rollback meaningful
/// > semantics. An arbitrary externally visible effect cannot generally be
/// > undone by restoring the resource value.
///
/// The transition is its own execution root, so its row is inferred
/// independently of the declaration's body — `infer_rooted`. Neither absorbs
/// the other's effects, which is what makes this checkable at all: a
/// transition merged into the command's row would be indistinguishable from
/// the command performing them itself.
fn named_roots_respect_their_context(
    inference: &crate::effects::Inference<'_>,
    unit: usize,
    hir: &Hir,
    out: &mut Vec<Diagnostic>,
) {
    for (_, decl) in hir.all_decls() {
        let Some(body_id) = decl.terms_body() else {
            continue;
        };
        let body = hir.body(body_id);
        // **Every root, asked its own context's question.** Not a rule keyed on
        // the policy's spelling: the context says whether effects are permitted
        // there, and `optimistic` is currently the only one that says no.
        for (policy, root) in decl.term_roots() {
            if root.context.permits_effects() {
                continue;
            }
            let inferred = inference.infer_rooted(unit, body, root.root);
            if inferred.effects.is_empty() {
                continue;
            }
            let performed: Vec<String> = inferred.effects.iter().cloned().collect();
            out.push(Diagnostic {
                code: crate::codes::OPTIMISTIC_NOT_PURE.id,
                invariant: crate::codes::OPTIMISTIC_NOT_PURE.invariant,
                reason: "optimistic_transition_performs_effects",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!(
                    "`{}`'s optimistic transition performs {}",
                    decl.name,
                    performed.join(", ")
                ),
                primary_span: body.expr_span(root.root),
                related: vec![Related::new(
                    policy.span.clone(),
                    "declared here".to_string(),
                )],
                explanation: Some(
                    "An optimistic transition is a pure function from a resource's current \
                     value to its speculative one. It runs on the client, before the round \
                     trip, and is discarded if the command fails — and the platform discards \
                     it by restoring the value it held. An externally visible effect cannot \
                     be undone that way, so a transition that performs one has no defined \
                     behaviour on rejection."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "move the effect into the command's body, which runs once and \
                                  authoritatively"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
    }
}

/// **A `derived` value performs no effect** (ADR-0082).
///
/// The charter's `derived` is "a pure value computed from other values"
/// (§7.5), recomputed whenever they change. Until 2026-09-26 nothing held it
/// to that: `let t = derived clock.now()` checked, in a declaration whose row
/// allows the clock, and ADR-0047 recorded the gap. Everything the value
/// performs counts: its calls, the members it reads, and the functions it
/// names (ADR-0078).
fn derived_values_are_pure(
    inference: &crate::effects::Inference<'_>,
    sigs: &Signatures,
    unit: usize,
    hir: &Hir,
    out: &mut Vec<Diagnostic>,
) {
    for (id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let derived: Vec<ExprId> = body
            .walk()
            .into_iter()
            .filter_map(|e| match body.expr(e) {
                Expr::Keyword { keyword, args, .. } if keyword == "derived" => {
                    args.first().copied()
                }
                _ => None,
            })
            .collect();
        if derived.is_empty() {
            continue;
        }
        let types = crate::infer::Types::of_decl(sigs, hir, id, body);
        let found = inference.infer_in_at(unit, body, &types);
        for value in derived {
            let region = body.expr_span(value);
            let Some(source) = found
                .sources
                .iter()
                .find(|s| s.span.start >= region.start && s.span.end <= region.end)
            else {
                continue;
            };
            out.push(Diagnostic {
                code: crate::codes::DERIVED_NOT_PURE.id,
                invariant: crate::codes::DERIVED_NOT_PURE.invariant,
                reason: "derived_value_performs_effects",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!(
                    "`{}`'s `derived` value performs `{}`",
                    decl.name, source.effect
                ),
                primary_span: source.span.clone(),
                related: vec![Related::new(region, "this value is `derived`".to_string())],
                explanation: Some(format!(
                    "A `derived` value is computed from other values, and recomputed when \
                     they change, so it performs nothing (charter §7.5). {}.",
                    source.via.describe()
                )),
                repairs: vec![Repair {
                    description: "compute it with a plain `let`, which runs once where the \
                                  declaration's row allows the effect"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
    }
}

/// **An optimistic clause's target is a resource's entry** (PW0331): a
/// resource this file can see, applied to a key.
///
/// Architect ruling, 2026-08-11, treating this as blocking before codegen:
///
/// ```text
/// target              ResourceEntry<Cart>
/// binder `cart`       Cart
/// transition result   Cart
/// ```
///
/// # Target selection and state transformation are different computations
///
/// The ruling is explicit that these must not be conflated:
///
/// > The purity requirement belongs to the transformation. The target selector
/// > may legitimately need context to identify the entry — `current_session()`
/// > is the obvious example — without making the actual state transformation
/// > effectful. So don't accidentally implement
/// > `effects(target) ∪ effects(transition) must be {}`.
///
/// So `optimistic_transitions_are_pure` infers from `t.body` alone, and this
/// function does not look at the target's effects at all. It asks the target
/// one question: which resource. The transition's result, against the
/// resource's value, is the value typer's (`values.rs`, ADR-0241). It types
/// every expression, where the reading this function made had no answer for
/// a literal, and `=> "no"` checked over an entry holding an `Int`.
fn optimistic_targets_are_resource_entries(
    sigs: &Signatures,
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    out: &mut Vec<Diagnostic>,
) {
    let hir = hirs[unit];
    for (_, decl) in hir.all_decls() {
        let Some(body_id) = decl.terms_body() else {
            continue;
        };
        let body = hir.body(body_id);
        for (policy, target, _) in decl.optimistic_clauses() {
            // The target names a resource. `Cart(current_session())` — the
            // callee, not the argument.
            let Expr::Call { callee, .. } = body.expr(target.root) else {
                out.push(target_is_not_a_resource_entry(
                    decl,
                    body,
                    target,
                    policy,
                    "it is not a resource applied to a key",
                ));
                continue;
            };
            let path = path_of(body, *callee);
            if resource_value_type(sigs, workspace, hirs, unit, &path).is_none() {
                out.push(target_is_not_a_resource_entry(
                    decl,
                    body,
                    target,
                    policy,
                    &format!("`{path}` is not a resource this file can see"),
                ));
            }
        }
    }
}

/// The VALUE a resource holds: `Cart` for `query Cart(..) -> Result<Cart, E>`.
///
/// The `Ok` side, because that is what the client displays and what an
/// optimistic transition replaces. A `Result` in the entry would make the
/// speculative value a different shape from the held one.
fn resource_value_type(
    sigs: &Signatures,
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    path: &str,
) -> Option<crate::resolved::ResolvedType> {
    use crate::resolve::{Namespace, Resolution};
    // **A bare name resolves in the TERM namespace**, not in whichever
    // namespace answers first.
    //
    // A resource is a data operation and lives there. `examples/store/app.pw`
    // declares `session query Cart(..)` and also imports `domain.{ Cart }` —
    // the type — so a namespace-agnostic lookup finds the type and reports the
    // program's own resource as one the file cannot see. Two declarations, one
    // spelling, two namespaces: exactly what `Namespace` exists for.
    //
    // A QUALIFIED path keeps its qualifier. Taking the last segment here would
    // have been `docs/RISK_QUEUE.md` 34 in a new place: `Resources.Cart` and
    // `store.page.Cart` are two resources, and an optimistic clause targeting
    // one must not be typechecked against the other.
    let resolution = match path.contains('.') {
        true => workspace.resolve_path(unit, path),
        false => workspace.resolve_in(unit, Namespace::Term, path),
    };
    let def = match resolution {
        Resolution::Local(def) | Resolution::Imported { def, .. } => def,
        _ => return None,
    };
    // The WHOLE program. `A-005` writes `optimistic Cart(..)` and imports
    // `Resources.{ Cart }`, so a lookup restricted to this unit would report
    // every cross-module resource as one the file cannot see — which is the
    // shape of half the defects this milestone found.
    let decl = crate::resolve::declaration(hirs, def)?;
    if !matches!(
        decl.kind,
        DeclKind::Query | DeclKind::Resource | DeclKind::Subscription
    ) {
        return None;
    }
    let ty = sigs.by_def(def)?.result()?;
    match ty.as_builtin() {
        Some(crate::resolved::Builtin::Result) => ty.args().first().cloned(),
        _ => Some(ty.clone()),
    }
}

fn target_is_not_a_resource_entry(
    decl: &Decl,
    body: &Body,
    target: &crate::hir::TermRoot,
    policy: &crate::hir::Policy,
    why: &str,
) -> Diagnostic {
    Diagnostic {
        code: crate::codes::OPTIMISTIC_TARGET_MISMATCH.id,
        invariant: crate::codes::OPTIMISTIC_TARGET_MISMATCH.invariant,
        reason: "optimistic_target_is_not_a_resource_entry",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: format!(
            "`{}`'s optimistic clause targets no resource: {why}",
            decl.name
        ),
        primary_span: body.expr_span(target.root),
        related: vec![Related::new(
            policy.span.clone(),
            "declared here".to_string(),
        )],
        explanation: Some(
            "An optimistic clause names the resource ENTRY it speculatively updates — \
             `Cart(current_session())`, not `Cart`. Two entries can have the same type, so a \
             clause that named only a type would not say which one it changed."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: "name a declared resource and the key that selects its entry".to_string(),
            replacement: None,
        }],
    }
}

/// **The keys an optimistic clause's target leaves unnamed** (ADR-0222): each
/// argument of its call written `_`. Anywhere else `_` is a name, and
/// resolves to nothing.
pub(crate) fn target_wildcards(body: &Body, root: &crate::hir::TermRoot) -> Vec<ExprId> {
    if root.context != crate::hir::ExecutionContext::TargetSelection {
        return Vec::new();
    }
    let Expr::Call { args, .. } = body.expr(root.root) else {
        return Vec::new();
    };
    args.iter()
        .map(|a| a.value)
        .filter(|e| matches!(body.expr(*e), Expr::Name(n) if n == "_"))
        .collect()
}

/// **Whether `e` is written `_`** (ADR-0256): an `invalidates` key's
/// argument so is every value at its position. Anywhere else `_` is a name,
/// and resolves to nothing.
pub(crate) fn every_value(body: &Body, e: ExprId) -> bool {
    matches!(body.expr(e), Expr::Name(n) if n == "_")
}

/// **An `emits` key's argument written `_`** (ADR-0256): no value, where a
/// listener matches the event by each it carries.
fn emitted_no_value(
    hirs: &[&Hir],
    workspace: &crate::resolve::Workspace,
    unit: usize,
    hir: &Hir,
    decl: &Decl,
    body: &Body,
    e: ExprId,
) -> Diagnostic {
    // The key it is an argument of, and the parameter it is given at: by its
    // name where it is named, and otherwise in order.
    let found = decl
        .policies
        .iter()
        .filter(|p| p.name == "emits")
        .flat_map(|p| &p.keys)
        .find_map(|key| {
            let mut next = 0;
            for a in &key.args {
                let at = match &a.name {
                    Some(name) => Err(name.clone()),
                    None => {
                        next += 1;
                        Ok(next - 1)
                    }
                };
                if a.value == e {
                    return Some((key, at));
                }
            }
            None
        });
    let (event, param) = match found {
        Some((key, at)) => {
            let param = match at {
                Err(name) => Some(name),
                Ok(i) => crate::backend::emitted_event(workspace, unit, key)
                    .and_then(|def| crate::resolve::declaration(hirs, def))
                    .and_then(|event| event.params.get(i))
                    .map(|p| p.name.clone()),
            };
            (key.name.clone(), param)
        }
        None => (String::new(), None),
    };
    let at = param.map(|p| format!(" at `{p}`")).unwrap_or_default();
    Diagnostic {
        code: crate::codes::UNRESOLVED_NAME.id,
        invariant: crate::codes::UNRESOLVED_NAME.invariant,
        reason: "emitted_event_given_no_value",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: format!("`emits {event}` is given no value{at}: `_` is none"),
        primary_span: body.expr_span(e),
        related: vec![Related::new(
            hir.decl_span(decl_id_of(hir, decl)),
            format!("written in `emits` on `{}`", decl.name),
        )],
        explanation: Some(
            "A listener matches an event by the values it carries, so `emits` gives it \
             one at each of its parameters, as the command computes it. `_` is a value \
             nowhere: in a listener it matches any, and in an `invalidates` key it is \
             every entry at the rest (ADR-0256)."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: "give the event the value it carries here".to_string(),
            replacement: None,
        }],
    }
}

/// **A bare entry names none** (ADR-0256, ADR-0195's ruling 10): `invalidates
/// Cart` passes no argument, and is refused as a call passing none is
/// (PW0604). Its repair says how every entry is written: `Cart(_)`.
fn every_entry_repair(
    hir: &Hir,
    sigs: &Signatures,
    ws: &crate::resolve::Workspace,
    at: usize,
    found: &mut [Diagnostic],
) {
    for (_, decl) in hir.all_decls() {
        for key in decl
            .policies
            .iter()
            .filter(|p| p.name == "invalidates")
            .flat_map(|p| &p.keys)
            .filter(|k| k.args.is_empty())
        {
            let Some(sig) =
                crate::backend::invalidated_query(ws, sigs, at, key).and_then(|d| sigs.by_def(d))
            else {
                continue;
            };
            if sig.params.is_empty() {
                continue;
            }
            let every = format!("{}({})", key.name, vec!["_"; sig.params.len()].join(", "));
            for d in found
                .iter_mut()
                .filter(|d| d.code == crate::codes::CALL_ARITY.id && d.primary_span == key.span)
            {
                d.repairs.insert(
                    0,
                    Repair {
                        description: format!(
                            "write `{every}` to drop every entry, or the key of the one the \
                             command changes"
                        ),
                        replacement: None,
                    },
                );
            }
        }
    }
}

/// A name inside an `optimistic` or `rollback` term that nothing binds.
fn unresolved_in_policy_term(
    hir: &Hir,
    decl: &Decl,
    body: &Body,
    id: ExprId,
    name: &str,
    policy: &str,
) -> Diagnostic {
    Diagnostic {
        code: crate::codes::UNRESOLVED_NAME.id,
        invariant: crate::codes::UNRESOLVED_NAME.invariant,
        reason: "unresolved_in_policy_term",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: format!("`{name}` does not resolve"),
        primary_span: body.expr_span(id),
        related: vec![Related::new(
            hir.decl_span(decl_id_of(hir, decl)),
            format!("written in `{policy}` on `{}`", decl.name),
        )],
        // This value was kept as unparsed text until 2026-08-10, so nothing
        // had ever asked.
        explanation: Some(format!(
            "An `{policy}` value is executable code: it runs on the client, and \
             its names come from lexical scope like any other term's. `{name}` \
             is not a parameter, not a local, and not a declaration this file \
             can see.\n\nThe value is checked as any term is: no ambient binding \
             is supplied because the policy is spelled `{policy}`."
        )),
        repairs: vec![Repair {
            description: format!(
                "bind `{name}` explicitly, or write a value whose names are in scope"
            ),
            replacement: None,
        }],
    }
}

/// Does this bare call name something nothing in the language owns?
///
/// The owners, in the order `tests/semantic_ownership.rs` establishes them:
/// lexical scope, a declaration the unit can see, language syntax, a policy
/// head or operator, a value domain. A name none of those claims is a name the
/// author wrote and the compiler never gave meaning to.
/// **A bare call whose name two whole modules declare** (ADR-0326):
/// `import List` and `import domain` both have `sum`, and the file names
/// neither by name. Refused where it is written, naming each module.
fn ambiguous_bare_call(
    workspace: &crate::resolve::Workspace,
    hir: &Hir,
    decl: &crate::hir::Decl,
    body: &crate::hir::Body,
    id: crate::hir::ExprId,
    name: &str,
    defs: &[crate::resolve::DefId],
) -> Diagnostic {
    let modules: Vec<String> = defs
        .iter()
        .filter_map(|d| workspace.module_of(d.unit).map(|m| m.name.clone()))
        .collect();
    let named = modules
        .iter()
        .map(|m| format!("`{m}`"))
        .collect::<Vec<_>>()
        .join(" and ");
    let qualified = modules
        .iter()
        .map(|m| format!("`{m}.{name}`"))
        .collect::<Vec<_>>()
        .join(" or ");
    Diagnostic {
        code: crate::codes::AMBIGUOUS_NAME.id,
        invariant: crate::codes::AMBIGUOUS_NAME.invariant,
        reason: "ambiguous_name",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: format!(
            "`{name}` is declared by {named}, each imported whole, and the file names neither"
        ),
        primary_span: body.expr_span(id),
        related: vec![Related::new(
            hir.decl_span(decl_id_of(hir, decl)),
            format!("used inside `{}`", decl.name),
        )],
        explanation: None,
        repairs: vec![Repair {
            description: format!(
                "write {qualified}, or import `{name}` by name from the module meant"
            ),
            replacement: None,
        }],
    }
}

fn bare_call_is_unowned(
    workspace: &crate::resolve::Workspace,
    unit: usize,
    path: &str,
    in_scope: &std::collections::BTreeSet<String>,
) -> bool {
    use crate::resolve::Resolution;

    if in_scope.contains(path)
        || crate::resolve::INTRINSIC_CALLS.contains(&path)
        || crate::resolve::VALUE_DOMAIN_CALLS.contains(&path)
    {
        return false;
    }
    // A policy head, or an operator under one. `merge_by_field(..)` and
    // `content_address(..)` lower into the body as ordinary calls today; the
    // `PolicyExpr` split is what will stop them doing so, and until it lands
    // reporting them would be reporting a policy value as a missing function.
    if crate::policy::domain_of(path).is_some()
        || ["retry", "reconnect", "conflict", "identity"]
            .iter()
            .any(|h| crate::policy::operator(h, path).is_some())
    {
        return false;
    }
    // A privacy label constructor: `privacy User(consumer)`.
    if matches!(path, "Session" | "User" | "Organization" | "Device") {
        return false;
    }
    matches!(workspace.resolve(unit, path), Resolution::Unresolved)
}

fn unresolved_bare_call(hir: &Hir, decl: &Decl, body: &Body, id: ExprId, path: &str) -> Diagnostic {
    Diagnostic {
        code: crate::codes::UNRESOLVED_NAME.id,
        invariant: crate::codes::UNRESOLVED_NAME.invariant,
        reason: "unresolved_bare_call",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: format!("`{path}` does not resolve"),
        primary_span: body.expr_span(id),
        related: vec![Related::new(
            hir.decl_span(decl_id_of(hir, decl)),
            format!("called inside `{}`", decl.name),
        )],
        explanation: Some(format!(
            "`{path}` names nothing this file can see. Assumption A-009: term \
             names are not ambient — only the Effect namespace is — so a \
             function must be declared here or brought in by an explicit \
             import.\n\nA call that resolves to nothing is not caught by \
             anything downstream. Inference contributes no effects for it, \
             privacy no label, placement no constraint and the contract no \
             capability, and each of those answers is what a harmless call \
             produces too."
        )),
        repairs: vec![Repair {
            description: format!("import the module that declares `{path}`, or declare it here"),
            replacement: None,
        }],
    }
}

/// A resolution failure, as a diagnostic.
fn resolve_diagnostic(e: &crate::resolve::ResolveError) -> Diagnostic {
    use crate::codes;
    use crate::resolve::ResolveErrorKind as K;

    let (code, repair) = match &e.kind {
        K::UnresolvedModule { module } => (
            codes::UNRESOLVED_MODULE,
            format!("declare `module {module}`, or import a module that exists"),
        ),
        K::UnresolvedName { module, name } => (
            codes::UNRESOLVED_NAME,
            format!("declare `{name}` in `{module}`, or import a name it has"),
        ),
        K::AmbiguousName { name, .. } => (
            codes::AMBIGUOUS_NAME,
            format!("import `{name}` from one module, or qualify each use"),
        ),
        K::DuplicateDeclaration { name } => (
            codes::DUPLICATE_DECLARATION,
            format!("rename one of them; `{name}` can name one thing per namespace"),
        ),
        K::PrivateAccess { module, name } => (
            codes::PRIVATE_ACCESS,
            // Not "make it public": `public` says whose data a declaration
            // holds, and a user's query made public would be everyone's
            // (ADR-0324).
            format!("drop `private` from `{name}` in `{module}`, or stop importing it"),
        ),
        K::PrivateData { .. } => (
            codes::PRIVATE_DATA,
            "write `user` for the user's, `session` for the session's, or `public` for \
             everyone's; each is importable"
                .to_string(),
        ),
        K::ImportCycle { .. } => (
            codes::IMPORT_CYCLE,
            "extract the shared declarations into a third module".to_string(),
        ),
    };

    Diagnostic {
        code: code.id,
        invariant: code.invariant,
        reason: "name_resolution",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: e.kind.message(),
        primary_span: e.span.clone(),
        related: vec![Related::new(
            e.span.clone(),
            "resolved against the workspace module graph".to_string(),
        )],
        explanation: Some(
            "Names are visible through lexical scope, module membership, or an \
             explicit import — and through nothing else. External implementation \
             is allowed; a missing declaration is not."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: repair,
            replacement: None,
        }],
    }
}

pub fn check_unit(unit: &Unit) -> Vec<Diagnostic> {
    let sigs = Signatures::default();
    // One unit's own workspace, so a call to a sibling in the same file still
    // resolves. Narrower than `check_units` by construction — see below.
    let own = crate::resolve::Workspace::build(&[&unit.hir]);
    let inference = crate::effects::Inference::new(&sigs, &own);
    let manifest = crate::boundary::TypeFacts::default();
    let routes = crate::routes::Table::default();
    // One unit's own graph. Enough for a single-file caller, and honestly
    // narrower than the whole-program one: an edge to another file's query is
    // dangling here, which is why `check_units` is the entry point every real
    // caller uses.
    let ws = crate::resolve::Workspace::build(&[&unit.hir]);
    let graph = crate::graph::Graph::build(&[&unit.hir], &ws);
    check_unit_with(
        &BTreeMap::new(),
        &Reads::default(),
        // No program-wide pass over one unit: each callee is read by its
        // signature, as before ADR-0129.
        &BTreeMap::new(),
        // And no view is resolved across files, so none is composed.
        &BTreeMap::new(),
        &crate::signals::Provision::default(),
        &sigs,
        &inference,
        &manifest,
        &routes,
        &graph,
        &BTreeMap::new(),
        &crate::ontology::Ontology::build_with(&[&unit.hir], &ws),
        &ws,
        // This entry point builds a workspace from ONE unit, so the unit it
        // resolves in is 0. `check_units` is what real callers use.
        0,
        &crate::ontology::argument_names_visible_from(&ws, 0, &[&unit.hir]),
        unit,
    )
}

#[allow(clippy::too_many_arguments)]
fn check_unit_with(
    labels: &BTreeMap<crate::resolve::DefId, Label>,
    // What each declaration reads through what it calls (ADR-0118).
    reads: &Reads,
    // What calling each declaration makes from its body (ADR-0129).
    summaries: &BTreeMap<crate::resolve::DefId, Label>,
    // Which parameters each view's handlers capture (ADR-0136).
    captured: &BTreeMap<crate::resolve::DefId, std::collections::BTreeSet<String>>,
    // What each view needs provided, and which hold signals (ADR-0144).
    provision: &crate::signals::Provision,
    sigs: &Signatures,
    inference: &crate::effects::Inference<'_>,
    manifest: &crate::boundary::TypeFacts,
    routes: &crate::routes::Table,
    graph: &crate::graph::Graph,
    // Each resource's value label, by path, where it names a reader
    // (ADR-0118).
    resources: &BTreeMap<String, Label>,
    // What each effect DECLARES about itself, and the graph its arguments
    // resolve through. Phase legality reads an effect's semantic facets, which
    // only its declaration knows.
    ontology: &crate::ontology::Ontology,
    ws: &crate::resolve::Workspace,
    // Which unit this is, so a call to a SIBLING resolves rather than being
    // matched by spelling — `docs/RISK_QUEUE.md` 34.
    at: usize,
    // What a capability argument may name FROM THIS UNIT: the types and
    // modules it can see. Per-unit since A-017 was retired — the ambient
    // whole-program set is gone.
    types: &std::collections::BTreeSet<String>,
    unit: &Unit,
) -> Vec<Diagnostic> {
    // What the compiler cannot read, first: what lowering parsed and could
    // not read (ADR-0237), then what it read and has no meaning for
    // (ADR-0200).
    let mut out: Vec<Diagnostic> = unit.hir.syntax.iter().cloned().map(syntax_error).collect();
    out.extend(unread(&unit.hir, &unit.src));

    // Placement is inherited: a `fn` inside a `component placement browser`
    // runs in the browser too. Checking each declaration in isolation misses
    // exactly the case the charter opens with — a database read inside a
    // browser-placed component — because the effect and the placement are on
    // different declarations.
    let mut inherited: BTreeMap<u32, World> = BTreeMap::new();
    for (id, decl) in unit.hir.all_decls() {
        if let Some(w) = declared_world(&unit.hir, decl) {
            for child in &decl.children {
                inherited.insert(child.0, w);
            }
            let _ = id;
        }
    }

    // Charter §7.5A relations: an ordering inside one frame, an observation
    // that feeds itself, a property the compositor cannot animate, a subtree
    // that is not as independent as it claims. None is an effect-row
    // violation, so none belongs in `effect_rows`.
    crate::layout::check(&unit.hir, sigs, &mut out);

    // A type with no finite value (ADR-0194): one that contains itself with
    // no way to be built without itself.
    crate::recursion::check(&unit.hir, sigs, at, &mut out);

    // Charter §7.1, §7.10, §8.2: three questions about what a value is, each
    // answered from a type the author wrote down.
    crate::annotations::check(&unit.hir, sigs, &mut out);

    // Affine resources: consumed exactly once, in the scope that acquired it.
    crate::affine::check(&unit.hir, sigs, &mut out);

    // Charter §8.5: the resume manifest ships with the document.
    crate::resume::check(&unit.hir, sigs, summaries, manifest, captured, at, &mut out);
    // Where UI state is read and written (ADR-0130).
    crate::signals::check(&unit.hir, sigs, at, captured, provision, &mut out);

    // E7 generator: the resume manifest and the handler artifact, derived by
    // two different walks and compared. A disagreement within one build is a
    // build error; disagreement ACROSS builds is `runtime/pw-resume`'s job and
    // is not a source diagnostic at all.
    crate::resume_artifacts::check(
        &unit.src,
        &unit.hir,
        sigs,
        crate::resume_artifacts::BUILD,
        &mut out,
    );

    // Charter §8.2: an internal link names a route the program declares.
    crate::routes::check(&unit.hir, routes, &mut out);
    crate::graph::check(&unit.hir, graph, resources, &mut out);

    // E9-V: every place a value meets a declared type — call arity and
    // argument types, constructed fields, declared results, annotated
    // bindings, and written types that resolve to nothing. The diagnostics
    // are a projection of `values::relations`, which an audit can query.
    let mut values =
        crate::values::diagnostics(&crate::values::relations(&unit.hir, sigs, ws, at), at);
    every_entry_repair(&unit.hir, sigs, ws, at, &mut values);
    out.extend(values);

    for (id, decl) in unit.hir.all_decls() {
        // The layout a page is shown in (ADR-0303), whose values its
        // document holds.
        let layout = (decl.kind == DeclKind::Page)
            .then(|| crate::layouts::written(&unit.hir, decl))
            .flatten()
            .and_then(|(name, _)| {
                crate::layouts::named(ws, at, &name)
                    .filter(|d| sigs.kind_of(*d) == Some(DeclKind::Layout))
                    .map(|d| (d, name))
            });
        privacy_and_placement(
            &unit.hir,
            &unit.src,
            labels,
            reads,
            inference,
            // Where each effect is meaningful, as the program declares it.
            // There was a hard-coded family→world table in `placement.rs`
            // answering this; the declaration is now the only source.
            ontology,
            at,
            id,
            decl,
            inherited.get(&id.0).copied(),
            layout.as_ref().map(|(d, n)| (*d, n.as_str())),
            &mut out,
        );
        privacy_flow(
            &unit.hir, sigs, labels, reads, summaries, inference, at, id, decl, &mut out,
        );
        privacy_sinks(
            &unit.hir, sigs, summaries, inference, at, id, decl, &mut out,
        );
        effect_rows(
            &unit.hir, sigs, inference, ontology, ws, at, id, decl, &mut out,
        );
        crate::capability::capability_arguments(&unit.hir, decl, types, &mut out);
        markup_rules(&unit.hir, decl, &mut out);
        loop_keys(&unit.hir, id, decl, &mut out);
        template_blocks(&unit.hir, sigs, id, decl, &mut out);
        let Some(body_id) = decl.body else { continue };
        let body = unit.hir.body(body_id);

        // Local type facts: a parameter's declared type, and any `let x: T`.
        // The DECLARED type, not a rendering of it — `List<MenuItem>` written
        // back into a string is a type a consumer has to re-parse, and
        // re-parsing is where arguments get dropped.
        let mut locals: BTreeMap<String, hir::DeclaredType> = BTreeMap::new();
        for p in &decl.params {
            if let Some(t) = &p.ty {
                locals.insert(p.name.clone(), t.clone());
            }
        }

        let site = MatchSite {
            ws,
            sigs,
            at,
            module: unit.hir.module_of(id),
            decl,
            body,
            locals: &locals,
        };
        for match_id in body.walk() {
            if let Expr::Match { scrutinee, arms } = body.expr(match_id) {
                exhaustiveness(&site, match_id, *scrutinee, arms, &mut out);
            }
        }
        scopes(decl, body, &mut out);
    }
    // **One finding, one diagnostic.** Two rules can reach one defect: a field
    // read through an `Option` bound by `let` is PW0600 to `annotations` and to
    // the member relation (ADR-0048). The first reported is kept; the order
    // above puts the more specific rule first.
    let mut seen = BTreeSet::new();
    out.retain(|d| seen.insert((d.code, d.primary_span.start, d.primary_span.end)));
    out.sort_by_key(|d| d.primary_span.start);
    out
}

/// **What the exhaustiveness analysis concluded about one `match`.**
///
/// Architect ruling, 2026-08-10:
///
/// > I would make diagnostics a **projection of an analysis result**, not the
/// > only observable result. […] Now the audit can assert **positive proof**
/// > rather than infer it from silence.
///
/// The hole this closes, found by `tests/evidence_reachability.rs`: A-002 and
/// A-012 are exhaustiveness fixtures, and the only thing observable about them
/// from outside was the *absence* of a diagnostic — which is also what a
/// checker that never ran produces. Both were classified `Unqueryable`.
///
/// `Blocked` is the variant that makes the difference. It is not a violation
/// and emphatically not a proof: it says the analysis produced no answer, and
/// it is what every early return in `analyse_match` became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchOutcome {
    /// The analysis ran and every value is covered.
    Proven,
    /// The analysis ran and these values are not.
    NonExhaustive { missing: Vec<String> },
    /// The analysis did not run. Never read this as coverage.
    Blocked { reason: String },
}

/// One `match`, with what the analysis concluded and where it is.
#[derive(Debug, Clone)]
pub struct MatchAnalysis {
    /// The declaration the match is written in.
    pub declaration: String,
    /// The scrutinee's type as the program declares it, when it has one.
    pub scrutinee_type: Option<String>,
    pub span: crate::hir::Span,
    pub outcome: MatchOutcome,
    /// Each arm no value reaches, by its pattern: the arms before it take
    /// every value it matches (ADR-0076). Empty where the analysis did not
    /// run.
    pub unreachable: Vec<crate::hir::Span>,
}

/// **Every `match` in the program, and what was concluded about it.**
///
/// The same walk `check_unit` does, so a `match` that reaches the checker
/// reaches this and vice versa. A separate walk here would let the two disagree
/// about which matches exist, and the disagreement would be invisible: an
/// audit would report a proof for a match no rule examined.
pub fn match_analysis(units: &[Unit]) -> Vec<MatchAnalysis> {
    // One workspace, because a scrutinee's type must be RESOLVED before the
    // environment is consulted — `Status` alone does not say whose.
    let hirs: Vec<&Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = crate::resolve::Workspace::build(&hirs);
    // The value relations type a scrutinee no annotation types.
    let sigs = crate::signatures::Signatures::build(&ws, &hirs);
    let mut out = Vec::new();
    for (at, unit) in units.iter().enumerate() {
        for (decl_id, decl) in unit.hir.all_decls() {
            let Some(body_id) = decl.body else { continue };
            let body = unit.hir.body(body_id);
            let mut locals: BTreeMap<String, hir::DeclaredType> = BTreeMap::new();
            for p in &decl.params {
                if let Some(t) = &p.ty {
                    locals.insert(p.name.clone(), t.clone());
                }
            }
            let site = MatchSite {
                ws: &ws,
                sigs: &sigs,
                at,
                module: unit.hir.module_of(decl_id),
                decl,
                body,
                locals: &locals,
            };
            for id in body.walk() {
                if let Expr::Match { scrutinee, arms } = body.expr(id) {
                    out.push(analyse_match(&site, &decl.name, id, *scrutinee, arms).0);
                }
            }
        }
    }
    out
}

/// **What a match is over**: its type as the analysis reads it, the
/// program that type's constructors live in, and its name, found once and
/// read by both the analysis and its diagnostic.
struct Subject {
    ty: Type,
    /// The analysis's types for this match (ADR-0060): each instance a
    /// pattern can take apart, as an ADT of its own.
    program: Program,
    /// Which declared sum type each of `program`'s ADTs is an instance of,
    /// for a pattern's qualifier.
    defs: BTreeMap<crate::types::AdtId, crate::resolve::DefId>,
    /// The opaque types that stand for a type the program does not state: a
    /// pattern against one is not read, where against a known type no
    /// pattern takes apart it is an error.
    unknown: BTreeSet<crate::types::OpaqueId>,
    /// The type as written or inferred, for messages.
    name: String,
}

/// Where a match is, and what it can read: the context both callers share.
struct MatchSite<'a> {
    ws: &'a crate::resolve::Workspace,
    sigs: &'a crate::signatures::Signatures,
    at: usize,
    module: Option<&'a str>,
    decl: &'a Decl,
    body: &'a Body,
    locals: &'a BTreeMap<String, hir::DeclaredType>,
}

/// **The type a match is over**, or why the analysis cannot say.
///
/// A bare name with a declared type reads the declaration. Anything else (a
/// call: `match Entries.get(word) { .. }`) is typed by the value relations,
/// which type calls already. Until 2026-09-25 a scrutinee that was not a bare
/// declared name, and every match over `Option` or `Result`, was Blocked, and
/// a match with its `None` arm missing passed `pw check`. The kiokun slice's
/// backend refused one, which is how it was found.
///
/// A parameter's declared type is read only where the name means the
/// parameter. A body that binds the name again (`Some(x) => match x { .. }`
/// in a function with a parameter `x`) is left to the value relations, which
/// read a name bound at two sites as unknown rather than guess which one is
/// meant. Until 2026-09-25 the parameter's type was read, and a match over the
/// arm's value was proven against the parameter's constructors.
fn subject(site: &MatchSite<'_>, scrutinee: ExprId) -> Result<Subject, (String, Option<String>)> {
    let MatchSite {
        ws,
        sigs,
        at,
        module,
        decl,
        body,
        locals,
    } = site;
    let span = body.expr_span(scrutinee);
    if let Expr::Name(n) = body.expr(scrutinee)
        && let Some(declared) = locals.get(n)
        && !rebinds(body, n)
    {
        // **Resolve, then look up.** `Status` alone does not say whose, and
        // two modules may declare one. The environment is handed an identity;
        // it never discovers one.
        let written = declared.written();
        let resolution = crate::resolved::resolve(ws, *at, None, &[], declared, span);
        let Some(t) = resolution.resolved() else {
            return Err((
                "the scrutinee's type does not resolve to a declaration".into(),
                Some(written),
            ));
        };
        return typed_subject(sigs, &crate::values::Ty::of(t), written);
    }
    match crate::values::type_of(sigs, ws, *at, *module, decl, body, scrutinee) {
        (crate::values::Ty::Unknown | crate::values::Ty::Var(_) | crate::values::Ty::Any, _) => {
            Err(("the scrutinee's type is unknown here".into(), None))
        }
        (ty, name) => typed_subject(sigs, &ty, name),
    }
}

/// A subject of a type a match takes apart: a sum type, the language's
/// `Option` or `Result`, a `Bool`, or an `Int` or a `String` by its
/// literals (ADR-0060).
fn typed_subject(
    sigs: &Signatures,
    ty: &crate::values::Ty,
    name: String,
) -> Result<Subject, (String, Option<String>)> {
    let mut instances = Instances {
        sigs,
        program: Program::new(),
        memo: BTreeMap::new(),
        defs: BTreeMap::new(),
        unknown: BTreeSet::new(),
    };
    let t = instances.of(ty);
    match &t {
        Type::Adt(id) if instances.program.adt(*id).ctors.is_empty() => Err((
            "the scrutinee's type declares no constructors".into(),
            Some(name),
        )),
        Type::Opaque(id) if instances.unknown.contains(id) => {
            Err(("the scrutinee's type is unknown here".into(), Some(name)))
        }
        // A record, a list, a `Float`: known, and taken apart by no pattern,
        // so only `_` or a name matches one, and anything else is an error.
        _ => Ok(Subject {
            ty: t,
            program: instances.program,
            defs: instances.defs,
            unknown: instances.unknown,
            name,
        }),
    }
}

/// **The analysis's types for one match** (ADR-0060): each type a pattern
/// can take apart, declared as an ADT of its own instance, every
/// constructor's fields typed. `Option<Option<Int>>` and `Option<Int>` are
/// two ADTs, and a declared case's fields are its type's, under the
/// arguments it is applied to. Until 2026-09-26 the payloads of `Some`, `Ok`
/// and `Err` were one opaque type and a declared case's fields were read by
/// their spelling, so a pattern nested under either blocked the analysis.
struct Instances<'a> {
    sigs: &'a Signatures,
    program: Program,
    memo: BTreeMap<String, Type>,
    defs: BTreeMap<crate::types::AdtId, crate::resolve::DefId>,
    /// The opaque types standing for a type the program does not state.
    unknown: BTreeSet<crate::types::OpaqueId>,
}

impl Instances<'_> {
    fn of(&mut self, ty: &crate::values::Ty) -> Type {
        use crate::resolved::{Builtin, Primitive};
        use crate::values::Ty;
        match ty {
            Ty::Primitive(Primitive::Int) => Type::Int,
            Ty::Primitive(Primitive::Str) => Type::Str,
            Ty::Primitive(Primitive::Bool) => Type::Bool,
            Ty::Builtin(Builtin::Option, a) if a.len() == 1 => {
                let inner = a[0].clone();
                self.adt(ty, None, move |me| {
                    vec![
                        ("Some".to_string(), vec![me.of(&inner)]),
                        ("None".to_string(), Vec::new()),
                    ]
                })
            }
            Ty::Builtin(Builtin::Result, a) if a.len() == 2 => {
                let (ok, err) = (a[0].clone(), a[1].clone());
                self.adt(ty, None, move |me| {
                    vec![
                        ("Ok".to_string(), vec![me.of(&ok)]),
                        ("Err".to_string(), vec![me.of(&err)]),
                    ]
                })
            }
            Ty::Nominal(def, args)
                if self
                    .sigs
                    .type_decl(*def)
                    .is_some_and(|t| t.variants.is_some()) =>
            {
                let cases = self
                    .sigs
                    .type_decl(*def)
                    .and_then(|t| t.variants.clone())
                    .unwrap_or_default();
                let (def, args) = (*def, args.clone());
                self.adt(ty, Some(def), move |me| {
                    cases
                        .iter()
                        .map(|(name, fields)| {
                            let fields = fields
                                .iter()
                                .map(|f| match f.resolved() {
                                    Some(t) => {
                                        me.of(&crate::values::substituted(&Ty::of(t), def, &args))
                                    }
                                    None => me.opaque(&Ty::Unknown),
                                })
                                .collect();
                            (name.clone(), fields)
                        })
                        .collect()
                })
            }
            other => self.opaque(other),
        }
    }

    /// An instance's ADT, declared once: before its constructors are
    /// computed, so a type that contains itself finds its own.
    fn adt(
        &mut self,
        ty: &crate::values::Ty,
        def: Option<crate::resolve::DefId>,
        ctors: impl FnOnce(&mut Self) -> Vec<(String, Vec<Type>)>,
    ) -> Type {
        let key = format!("{ty:?}");
        if let Some(t) = self.memo.get(&key) {
            return t.clone();
        }
        let id = self.program.declare_adt(&self.name(ty), Vec::new());
        if let Some(d) = def {
            self.defs.insert(id, d);
        }
        self.memo.insert(key, Type::Adt(id));
        let ctors = ctors(self);
        self.program.adts[id].ctors = ctors
            .into_iter()
            .map(|(name, fields)| Ctor { name, fields })
            .collect();
        Type::Adt(id)
    }

    /// A type no pattern takes apart: a record, an opaque type, a `Float`,
    /// a list, a type the program does not state.
    fn opaque(&mut self, ty: &crate::values::Ty) -> Type {
        let key = format!("{ty:?}");
        if let Some(t) = self.memo.get(&key) {
            return t.clone();
        }
        let id = self.program.declare_opaque(&self.name(ty), Type::Str);
        if matches!(
            ty,
            crate::values::Ty::Unknown | crate::values::Ty::Var(_) | crate::values::Ty::Any
        ) {
            self.unknown.insert(id);
        }
        let t = Type::Opaque(id);
        self.memo.insert(key, t.clone());
        t
    }

    /// A type as a witness names it: `Option<Int>`, `Shape`, `Maybe<String>`.
    fn name(&self, ty: &crate::values::Ty) -> String {
        use crate::values::Ty;
        let args = |xs: &[Ty]| match xs.is_empty() {
            true => String::new(),
            false => format!(
                "<{}>",
                xs.iter()
                    .map(|x| self.name(x))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        match ty {
            Ty::Primitive(p) => p.name().to_string(),
            Ty::Builtin(b, xs) => format!("{}{}", b.name(), args(xs)),
            Ty::Nominal(def, xs) => {
                let path = self.sigs.path_of(*def).unwrap_or("?");
                let bare = path.rsplit('.').next().unwrap_or(path);
                format!("{bare}{}", args(xs))
            }
            Ty::Parameter { .. } | Ty::Var(_) | Ty::Unknown | Ty::Any => "_".to_string(),
        }
    }
}

/// Whether a pattern in this body binds `name`, as a `let` or an arm does.
fn rebinds(body: &Body, name: &str) -> bool {
    body.pats
        .iter()
        .any(|(_, p, _)| matches!(p, HPat::Bind { name: n, .. } if n == name))
}

/// A constructor pattern naming a constructor its type does not have.
struct Foreign {
    constructor: String,
    /// The type the pattern is read against, and that type's constructors.
    ty: String,
    ctors: Vec<String>,
    /// What a pattern against a type without constructors may be, said
    /// instead of the list (ADR-0060).
    note: Option<String>,
    span: hir::Span,
}

/// A constructor pattern binding a different number of fields than its
/// constructor declares.
struct Arity {
    ctor: String,
    written: usize,
    declared: usize,
    span: hir::Span,
}

/// What a match's diagnostics are read from, beside its verdict.
enum Found {
    /// No subject. The audit says why, and there is nothing to report.
    Nothing,
    /// The subject, and each arm as the analysis read it, or why it could
    /// not. Kept per arm, so one arm's fault never hides another's.
    Arms(Box<Subject>, Vec<Result<Arm, PatternFault>>),
}

/// The analysis, with no diagnostics in it, and what they are read from.
fn analyse_match(
    site: &MatchSite<'_>,
    declaration: &str,
    match_id: ExprId,
    scrutinee: ExprId,
    arms: &[hir::MatchArm],
) -> (MatchAnalysis, Found) {
    let body = site.body;
    let span = body.expr_span(match_id);
    let blocked = |reason: &str, ty: Option<String>| MatchAnalysis {
        declaration: declaration.to_string(),
        scrutinee_type: ty,
        span: span.clone(),
        outcome: MatchOutcome::Blocked {
            reason: reason.to_string(),
        },
        unreachable: Vec::new(),
    };
    let subject = match subject(site, scrutinee) {
        Ok(s) => s,
        Err((reason, ty)) => return (blocked(&reason, ty), Found::Nothing),
    };
    // `Shape.Circle(r)`: the type a qualified pattern names, resolved where
    // the match is written (ADR-0059).
    let qualifier = |q: &str| {
        use crate::resolve::{Namespace, Resolution};
        let found = match q.contains('.') {
            true => site.ws.resolve_path_in(site.at, Namespace::Type, q),
            false => site.ws.resolve_in(site.at, Namespace::Type, q),
        };
        match found {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => Some(d),
            _ => None,
        }
    };
    // Each literal the match's patterns write, as a constructor of its
    // type's own (ADR-0060): `1` and `2` are two, and no list of them is
    // every `Int`.
    let literals = std::cell::RefCell::new(BTreeMap::new());
    let read: Vec<Result<Arm, PatternFault>> = arms
        .iter()
        .map(|a| {
            to_exhaust_pattern(
                &subject,
                body,
                &qualifier,
                &literals,
                a.pat,
                &subject.ty,
                &subject.name,
            )
            .map(|pattern| Arm {
                pattern,
                span: body.pat_span(a.pat),
            })
        })
        .collect();
    // The audit's reason is the first fault, in the order an author would
    // repair them: a constructor of another type, then a field count, then
    // what the analysis does not read.
    let fault = read
        .iter()
        .filter_map(|r| r.as_ref().err())
        .min_by_key(|f| match f {
            PatternFault::Foreign(_) => 0,
            PatternFault::Arity(_) => 1,
            PatternFault::Unread(_) => 2,
        });
    if let Some(f) = fault {
        let why = match f {
            PatternFault::Foreign(f) => {
                format!("`{}` is not a constructor of `{}`", f.constructor, f.ty)
            }
            PatternFault::Arity(a) => format!(
                "`{}` binds {} field(s) but declares {}",
                a.ctor, a.written, a.declared
            ),
            PatternFault::Unread(why) => why.clone(),
        };
        let name = subject.name.clone();
        return (
            blocked(&why, Some(name)),
            Found::Arms(Box::new(subject), read),
        );
    }
    let lowered: Vec<Arm> = read.into_iter().filter_map(Result::ok).collect();

    let report = exhaust::check_match(&subject.program, &subject.ty, &lowered);
    // Which arms no value reaches, where the analysis ran. `exhaust.rs` found
    // them all along; until 2026-09-26 nothing read them (ADR-0076).
    let unreachable: Vec<crate::hir::Span> = match report.blocked.is_empty() {
        true => report
            .unreachable
            .iter()
            .filter_map(|&i| lowered.get(i).map(|a| a.span.clone()))
            .collect(),
        false => Vec::new(),
    };
    let outcome = match report.outcome() {
        crate::outcome::Outcome::Proven(_) => MatchOutcome::Proven,
        crate::outcome::Outcome::Blocked(bs) => MatchOutcome::Blocked {
            reason: format!("{bs:?}"),
        },
        crate::outcome::Outcome::Violation(_) => MatchOutcome::NonExhaustive {
            missing: report
                .missing
                .iter()
                .map(|w| exhaust::render_witness(&subject.program, &subject.ty, w))
                .collect(),
        },
    };
    (
        MatchAnalysis {
            declaration: declaration.to_string(),
            scrutinee_type: Some(subject.name.clone()),
            span,
            outcome,
            unreachable,
        },
        Found::Arms(Box::new(subject), lowered.into_iter().map(Ok).collect()),
    )
}

fn exhaustiveness(
    site: &MatchSite<'_>,
    match_id: ExprId,
    scrutinee: ExprId,
    arms: &[hir::MatchArm],
    out: &mut Vec<Diagnostic>,
) {
    // **The verdict and its subject come from `analyse_match`.**
    //
    // Architect ruling, 2026-08-10: a diagnostic is a projection of an analysis
    // result. Deriving it twice would let the report an audit reads and the
    // error a developer reads disagree, and the disagreement would be silent —
    // which is `docs/RISK_QUEUE.md`'s most common shape. Until 2026-09-25 this
    // function re-derived the scrutinee's type for its messages; it now reads
    // the one `analyse_match` found.
    let body = site.body;
    let (analysis, found) = analyse_match(site, "", match_id, scrutinee, arms);
    let Found::Arms(subject, read) = found else {
        return;
    };
    let ty_name = &subject.name;

    // Each arm's own defect, where it is. A pattern whose arity disagrees with
    // its constructor, or that names a constructor its type lacks, is a
    // defect in its own right, and reporting it means the author is told what
    // is actually wrong, rather than being told about a missing variant that
    // follows from it. Both are found at any depth, and are never a reason to
    // stop reading the other arms.
    //
    // `exhaust.rs` defends against an arity mismatch anyway. Two layers,
    // because "upstream validated it" is the assumption that produced the
    // panic.
    for fault in read.into_iter().filter_map(Result::err) {
        match fault {
            PatternFault::Foreign(f) => out.push(foreign_constructor(body, match_id, &subject, f)),
            PatternFault::Arity(a) => out.push(constructor_arity(body, match_id, &subject, a)),
            PatternFault::Unread(_) => {}
        }
    }

    // **An arm no value reaches** (ADR-0076): the arms before it take every
    // value it matches, so the code in it is dead and the value it was
    // written for goes to an earlier arm. Until 2026-09-26 the analysis found
    // these and nothing reported them: `_ => 0` before `Circle(r) => r`
    // checked, the backend refused a case matched twice, and a literal
    // matched twice built with its second arm dead.
    for arm in &analysis.unreachable {
        out.push(Diagnostic {
            code: crate::codes::UNREACHABLE_ARM.id,
            invariant: crate::codes::UNREACHABLE_ARM.invariant,
            reason: "unreachable_arm",
            detector: Detector::PatternMatrix,
            severity: Severity::Error,
            message: format!(
                "no `{ty_name}` reaches this arm: the arms before it take every value it matches"
            ),
            primary_span: arm.clone(),
            related: vec![Related::new(
                body.expr_span(scrutinee),
                format!("this has type `{ty_name}`"),
            )],
            explanation: Some(
                "A match tries its arms in order, so an arm whose values the arms before it \
                 already take never runs."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: "remove the arm, or move it before the arm that takes its values"
                    .to_string(),
                replacement: None,
            }],
        });
    }

    // A `Blocked` analysis produced NO ANSWER. The reason is already reported
    // by whichever phase found it — `PW0603` above, for the arity case — and
    // saying it twice would turn one defect into two. What must not happen is
    // treating it as a proof of exhaustiveness, which is what `is_exhaustive()`
    // did before `Outcome` existed.
    let MatchOutcome::NonExhaustive { missing } = &analysis.outcome else {
        return;
    };

    let span = body.expr_span(match_id);
    out.push(Diagnostic {
        code: "PW0305",
        invariant: "a match must cover every value its scrutinee can take",
        reason: "non_exhaustive_match",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!("match on `{ty_name}` is not exhaustive"),
        primary_span: span.clone(),
        related: vec![Related::new(
            body.expr_span(scrutinee),
            format!("this has type `{ty_name}`"),
        )],
        explanation: Some(match subject.program.ctors_of(&subject.ty) {
            Some(ctors) if !subject.ty.is_infinite() => format!(
                "`{ty_name}` has {} constructor(s), and {} value(s) no arm takes. \
                 Adding a variant to a type must break every match on it at compile \
                 time, which is why a wildcard arm is not the default repair.",
                ctors.len(),
                missing.len(),
            ),
            // `Int` and `String`: no list of literals is every value (ADR-0060).
            _ => format!(
                "A `{ty_name}` has more values than any list of literals names, so \
                 a match over one needs an arm that takes the rest."
            ),
        }),
        repairs: match subject.ty.is_infinite() {
            // No list of literals is every `Int`: the rest is one arm.
            true => vec![Repair {
                description: "add an arm `_ =>` for the values no literal names".to_string(),
                replacement: None,
            }],
            false => missing
                .iter()
                .map(|m| Repair {
                    description: format!("add an arm for `{m}`"),
                    replacement: None,
                })
                .chain(std::iter::once(Repair {
                    description: "or add `_ =>` if the remaining cases are genuinely \
                                  interchangeable — this silences future variants too"
                        .to_string(),
                    replacement: None,
                }))
                .collect(),
        },
    });
}

/// **PW0603**: a constructor pattern binding a different number of fields
/// than its constructor declares.
fn constructor_arity(body: &Body, match_id: ExprId, subject: &Subject, a: Arity) -> Diagnostic {
    Diagnostic {
        code: crate::codes::CONSTRUCTOR_ARITY.id,
        invariant: crate::codes::CONSTRUCTOR_ARITY.invariant,
        reason: "constructor_pattern_arity_mismatch",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!(
            "`{}` binds {} field(s) but declares {}",
            a.ctor, a.written, a.declared
        ),
        primary_span: a.span,
        related: vec![Related::new(
            body.expr_span(match_id),
            format!("matching on `{}`", subject.name),
        )],
        explanation: Some(format!(
            "A constructor pattern binds its constructor's fields, so the count \
             is not a style choice — `{}` carries {} of them. Writing a different \
             number leaves the compiler with a pattern that does not describe any \
             value of this type.",
            a.ctor, a.declared
        )),
        repairs: vec![Repair {
            description: if a.written < a.declared {
                format!(
                    "bind the remaining field(s), or write `{}(_)`-style wildcards",
                    a.ctor
                )
            } else {
                format!("`{}` takes {}", a.ctor, a.declared)
            },
            replacement: None,
        }],
    }
}

/// **PW0608**: a constructor pattern whose constructor its type does not have.
fn foreign_constructor(body: &Body, match_id: ExprId, subject: &Subject, f: Foreign) -> Diagnostic {
    Diagnostic {
        code: crate::codes::PATTERN_CONSTRUCTOR.id,
        invariant: crate::codes::PATTERN_CONSTRUCTOR.invariant,
        reason: "pattern_names_a_constructor_its_type_lacks",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!("`{}` is not a constructor of `{}`", f.constructor, f.ty),
        primary_span: f.span,
        related: vec![Related::new(
            body.expr_span(match_id),
            format!("matching on `{}`", subject.name),
        )],
        explanation: Some(format!(
            "A constructor pattern matches the values its constructor builds, and \
             no value of `{}` is built by `{}`. The arm can never match. Read as \
             a binding, it would match everything and hide the cases it misses.",
            f.ty, f.constructor
        )),
        repairs: vec![Repair {
            description: match f.note {
                // A type without constructors (ADR-0060).
                Some(note) => note,
                None => format!(
                    "`{}`'s constructors are {}",
                    f.ty,
                    f.ctors
                        .iter()
                        .map(|c| format!("`{c}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            replacement: None,
        }],
    }
}

/// Why a pattern has no reading in the analysis.
enum PatternFault {
    /// A type error, reported as PW0608.
    Foreign(Foreign),
    /// A field count its constructor does not declare, reported as PW0603.
    Arity(Arity),
    /// Not an error, and not analysed. The analysis is Blocked, with this
    /// reason.
    Unread(String),
}

/// HIR pattern → the usefulness algorithm's pattern.
///
/// The load-bearing case: a nullary constructor has no parentheses, so the
/// grammar cannot tell `Draft` from a binding and emits `Pattern::Bind`. Read
/// as a binding it matches everything, which makes the first arm cover the
/// scrutinee and reports a non-exhaustive match as exhaustive — the failure is
/// silent and favourable, which is the shape `docs/RISK_QUEUE.md` tracks.
///
/// **Each pattern is read against its own type.** Until 2026-09-25 every
/// pattern, at any depth, was read against the constructors of the
/// scrutinee's type, and a constructor that type does not have read as a
/// wildcard. So `Ok(x)` and `Err(e)` arms proved a match over an `Option`
/// exhaustive, `Circle(Draft)` read as `Circle(_)`, and a literal covered
/// every value: each a proof of something false, of the same favourable shape
/// as above. Now a nested pattern is read against its field's type, a
/// constructor its type does not have is PW0608, and what the analysis cannot
/// read blocks it, with the reason: a literal, or a constructor pattern
/// against a type whose constructors it does not know, such as `Some`'s
/// payload.
#[allow(clippy::too_many_arguments)]
fn to_exhaust_pattern(
    subject: &Subject,
    body: &Body,
    qualifier: &dyn Fn(&str) -> Option<crate::resolve::DefId>,
    literals: &std::cell::RefCell<BTreeMap<String, usize>>,
    id: hir::PatternId,
    ty: &Type,
    ty_name: &str,
) -> Result<EPat, PatternFault> {
    let program = &subject.program;
    let ctors = program.ctors_of(ty);
    let unread = || {
        PatternFault::Unread(format!(
            "a pattern against `{ty_name}` is not analysed: the analysis does not know \
             that type's constructors"
        ))
    };
    let foreign = |name: &str, cs: &[Ctor]| {
        PatternFault::Foreign(Foreign {
            constructor: name.to_string(),
            ty: ty_name.to_string(),
            ctors: cs.iter().map(|c| c.name.clone()).collect(),
            note: None,
            span: body.pat_span(id),
        })
    };
    // A type without constructors: an `Int` or a `String` is matched by its
    // literals, and a record, a list or a `Float` by no pattern at all. A
    // type the program does not state is not read (ADR-0060).
    let untakeable = |name: &str| match ty {
        Type::Opaque(o) if subject.unknown.contains(o) => unread(),
        _ => PatternFault::Foreign(Foreign {
            constructor: name.to_string(),
            ty: ty_name.to_string(),
            ctors: Vec::new(),
            note: Some(match ty {
                Type::Int | Type::Str => {
                    format!("a pattern against `{ty_name}` is one of its literals, or `_`")
                }
                _ => format!(
                    "a `{ty_name}` is taken apart by no pattern: match it with `_` or a name"
                ),
            }),
            span: body.pat_span(id),
        }),
    };
    match body.pat(id) {
        HPat::Wild | HPat::Error => Ok(EPat::Wildcard),
        // A literal is a constructor of its type's own (ADR-0060). An `Int`
        // or a `String` has more of them than any match writes, so only an
        // arm taking the rest covers one; the analysis says which values
        // the literals leave, as `_`.
        HPat::Literal(l) => {
            let key = match (ty, l) {
                (Type::Int, hir::Literal::Int(n)) => n
                    .replace('_', "")
                    .parse::<i64>()
                    .ok()
                    .map(|n| format!("i:{n}")),
                (Type::Str, hir::Literal::Str(_)) => l.string_value().map(|v| format!("s:{v}")),
                _ => None,
            };
            match key {
                Some(k) => {
                    let mut seen = literals.borrow_mut();
                    let next = seen.len();
                    Ok(EPat::unit(*seen.entry(k).or_insert(next)))
                }
                // A `Float` literal: equality on floats decides no case.
                None if matches!(l, hir::Literal::Float(_)) && ty_name == "Float" => Err(
                    PatternFault::Unread("a `Float` literal pattern is not analysed".to_string()),
                ),
                None => {
                    let written = match l {
                        hir::Literal::Int(t)
                        | hir::Literal::Float(t)
                        | hir::Literal::Str(t)
                        | hir::Literal::UnterminatedStr(t) => t.clone(),
                        hir::Literal::Unit => "()".to_string(),
                    };
                    Err(match &ctors {
                        Some(cs) => foreign(&written, cs),
                        None => untakeable(&written),
                    })
                }
            }
        }
        // A binding, always: a pattern's name is a case by its capital, and
        // the parser made each such name a constructor pattern (ADR-0195,
        // ruling 2). Until ADR-0197 a bare name was a case when some type
        // anywhere had a case of that name, and a binding otherwise, so a
        // misspelt `Circel` matched everything.
        HPat::Bind { .. } => Ok(EPat::Wildcard),
        HPat::Ctor { path, args } => {
            // `DecodeError.Invalid` and `Invalid` name the same constructor,
            // where the qualifier names the type matched (ADR-0059). Read by
            // its last segment alone, `Other.Invalid` matched a `DecodeError`.
            let short = path.rsplit('.').next().unwrap_or(path);
            let Some(cs) = ctors else {
                return Err(untakeable(path));
            };
            if let Some((q, _)) = path.rsplit_once('.')
                && !matches!(ty, Type::Adt(t)
                    if qualifier(q).is_some_and(|d| subject.defs.get(t) == Some(&d)))
            {
                return Err(foreign(path, &cs));
            }
            let Some(i) = cs.iter().position(|c| c.name == short) else {
                return Err(foreign(short, &cs));
            };
            let fields = &cs[i].fields;
            if args.len() != fields.len() {
                return Err(PatternFault::Arity(Arity {
                    ctor: short.to_string(),
                    written: args.len(),
                    declared: fields.len(),
                    span: body.pat_span(id),
                }));
            }
            let args = args
                .iter()
                .zip(fields)
                .map(|(a, t)| {
                    to_exhaust_pattern(
                        subject,
                        body,
                        qualifier,
                        literals,
                        *a,
                        t,
                        &program.type_name(t),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(EPat::ctor(i, args))
        }
        HPat::Or(ps) => ps
            .iter()
            .map(|p| to_exhaust_pattern(subject, body, qualifier, literals, *p, ty, ty_name))
            .collect::<Result<Vec<_>, _>>()
            .map(EPat::Or),
    }
}

/// Parse, lower and check a set of files together.
///
/// As `pw check` does (ADR-0200): a file's syntax errors are its diagnostics,
/// and a file that does not parse is kept out of the program, since what
/// recovery makes of it would put declarations no one wrote in every other
/// file's environment (E0 finding F-4). Until ADR-0200 every file was lowered
/// whatever the parser said, and its syntax errors were not reported: a
/// test's program with a typo checked clean.
/// Semantic-only source checks, including partial/library programs.
/// Use `check_program_sources` for the executable-program promise of CLI check.
pub fn check_sources(files: &[(String, String)]) -> Vec<(String, Vec<Diagnostic>)> {
    check_sources_with(files, None)
}

/// Semantic-only source checks held to the released schema (PW0366).
/// Complete executable programs use check_program_sources_with instead.
pub fn check_sources_with(
    files: &[(String, String)],
    released: Option<&crate::tables::Schema>,
) -> Vec<(String, Vec<Diagnostic>)> {
    check_parsed_sources(files, |units| {
        let mut held = check_units(units);
        if released.is_some() {
            let hirs: Vec<&Hir> = units.iter().map(|u| &u.hir).collect();
            let ws = crate::resolve::Workspace::build(&hirs);
            let sigs = Signatures::build(&ws, &hirs);
            for (unit, diagnostic) in crate::tables::held_to(&hirs, &ws, &sigs, released).1 {
                if let Some((_, found)) = held.get_mut(unit) {
                    found.push(diagnostic);
                    found.sort_by_key(|d| d.primary_span.start);
                }
            }
        }
        held
    })
}

/// Full executable-program validation shared by CLI check and build.
pub fn check_program_sources(files: &[(String, String)]) -> Vec<(String, Vec<Diagnostic>)> {
    check_program_sources_with(files, None)
}

pub fn check_program_sources_with(
    files: &[(String, String)],
    released: Option<&crate::tables::Schema>,
) -> Vec<(String, Vec<Diagnostic>)> {
    crate::program::validate_sources_with(files, released).diagnostics
}

fn check_parsed_sources(
    files: &[(String, String)],
    check: impl FnOnce(&[Unit]) -> Vec<(String, Vec<Diagnostic>)>,
) -> Vec<(String, Vec<Diagnostic>)> {
    let parsed: Vec<pw_syntax::Parse> = files
        .iter()
        .map(|(_, src)| pw_syntax::parse_tree(src))
        .collect();
    let units: Vec<Unit> = files
        .iter()
        .zip(&parsed)
        .filter(|(_, p)| p.ok())
        .map(|((path, src), p)| Unit {
            path: path.clone(),
            src: src.clone(),
            hir: crate::lower::lower_file(src, &p.green),
        })
        .collect();
    let mut checked = check(&units).into_iter();
    files
        .iter()
        .zip(parsed)
        .filter_map(|((path, _), p)| match p.ok() {
            true => checked.next(),
            false => Some((
                path.clone(),
                p.errors.into_iter().map(syntax_error).collect(),
            )),
        })
        .collect()
}

/// **A syntax error, as a diagnostic**: the parser's code, message and help,
/// and the registry's invariant for the code.
pub(crate) fn syntax_error(e: pw_syntax::SyntaxError) -> Diagnostic {
    Diagnostic {
        code: e.code,
        invariant: crate::codes::lookup(e.code).map_or("a program must parse", |c| c.invariant),
        reason: "syntax_error",
        detector: Detector::Parser,
        severity: Severity::Error,
        message: e.message,
        primary_span: e.span,
        related: Vec::new(),
        explanation: None,
        // The parser's help says what to write, as a repair does, and is
        // rendered as `pw check` renders a file's parse errors' (ADR-0237).
        repairs: e
            .help
            .map(|description| Repair {
                description,
                replacement: None,
            })
            .into_iter()
            .collect(),
    }
}

/// **An expression or a pattern the compiler cannot read** (ADR-0200).
///
/// Lowering is total: what it has no meaning for becomes an error node, and
/// the checker types one as anything, since the parser's error says what is
/// wrong. Every file the checker is given parses (`check_sources`, `pw check`
/// and the backend's `Checked::of` keep out or refuse one that does not), so
/// an error node here is text the parser accepted and nothing reported.
/// `()` was one until ADR-0200, and `fn f() -> Int !{} { () }` checked.
///
/// Except where lowering parses text itself (ADR-0237): `{#if flag ==}` is an
/// error node, and the parse's own error, reported, says what is wrong. Such
/// a node is that error, and is not reported twice.
fn unread(hir: &Hir, src: &str) -> Vec<Diagnostic> {
    let reported = |span: &hir::Span| {
        hir.syntax
            .iter()
            .any(|e| span.start <= e.span.start && e.span.start <= span.end)
    };
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        let Some(b) = decl.body else { continue };
        let body = hir.body(b);
        let exprs = body
            .exprs()
            .filter(|(_, e, _)| matches!(e, Expr::Error))
            .map(|(_, _, span)| ("an expression", span.clone()));
        let patterns = body
            .pats
            .iter()
            .filter(|(_, p, _)| matches!(p, HPat::Error))
            .map(|(_, _, span)| ("a pattern", span.clone()));
        for (what, span) in exprs.chain(patterns).filter(|(_, span)| !reported(span)) {
            let text = src.get(span.clone()).unwrap_or_default().trim();
            out.push(Diagnostic {
                code: crate::codes::UNREAD.id,
                invariant: crate::codes::UNREAD.invariant,
                reason: "unread",
                detector: Detector::Parser,
                severity: Severity::Error,
                message: format!("`{text}` is {what} this compiler does not read"),
                primary_span: span,
                related: vec![Related::new(
                    hir.decl_span(id),
                    format!("in `{}`", decl.name),
                )],
                explanation: Some(
                    "The parser accepted this text, and the compiler has no meaning for it. \
                     Until ADR-0200 such an expression was typed as any type at all, and \
                     the build refused it later."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "write it another way the language reads".to_string(),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// Declarations that define a type, for callers that need the environment.
pub fn declares_a_type(kind: DeclKind) -> bool {
    matches!(kind, DeclKind::Type | DeclKind::Opaque)
}

// --- structured concurrency (E2A-S) --------------------------------------

/// The ambient scope a declaration's body runs in.
///
/// A `view` and a `component` live for as long as their component; a `page` for
/// a route; a `query` or `command` for one request. Getting this wrong in the
/// permissive direction would silently accept an escape, so anything
/// unrecognised is treated as the *narrowest* scope.
fn ambient_scope(decl: &Decl) -> ScopeKind {
    match decl.kind {
        DeclKind::View | DeclKind::Component => ScopeKind::Component,
        // A layout lives for as long as the pages shown in it (ADR-0303).
        DeclKind::Page | DeclKind::Layout => ScopeKind::Route,
        DeclKind::Query | DeclKind::Command => ScopeKind::Request,
        DeclKind::Subscription => ScopeKind::Session,
        _ => ScopeKind::Block,
    }
}

fn named_scope(name: &str) -> Option<ScopeKind> {
    Some(match name {
        "application" => ScopeKind::Application,
        "session" => ScopeKind::Session,
        "route" => ScopeKind::Route,
        "request" => ScopeKind::Request,
        "component" => ScopeKind::Component,
        _ => return None,
    })
}

/// Feed `crate::scope`'s graph from a real body (`docs/NEXT.md` item 7).
///
/// The graph and its rules were written and tested against hand-built inputs in
/// E2A-S. Nothing here re-implements them: this only builds the graph and
/// converts what comes back.
fn scopes(decl: &Decl, body: &Body, out: &mut Vec<Diagnostic>) {
    let mut g = ScopeGraph::new();
    // The lifetime hierarchy every declaration sits inside. Built in full each
    // time so `outlives` has real parent links to walk.
    let app = g.scope("application", ScopeKind::Application, None);
    let session = g.scope("session", ScopeKind::Session, Some(app));
    let route = g.scope("route", ScopeKind::Route, Some(session));
    let request = g.scope("request", ScopeKind::Request, Some(route));
    let component = g.scope("component", ScopeKind::Component, Some(route));
    let here = match ambient_scope(decl) {
        ScopeKind::Application => app,
        ScopeKind::Session => session,
        ScopeKind::Route => route,
        ScopeKind::Request => request,
        ScopeKind::Component | ScopeKind::Block => component,
    };
    let by_kind = move |k: ScopeKind| match k {
        ScopeKind::Application => app,
        ScopeKind::Session => session,
        ScopeKind::Route => route,
        ScopeKind::Request => request,
        ScopeKind::Component | ScopeKind::Block => component,
    };

    let mut declared: Vec<(usize, usize, crate::hir::Span, String)> = Vec::new();

    for id in body.walk() {
        match body.expr(id) {
            // `task.spawn(detached) { .. }` — a detach with no durable
            // capability. `durable.spawn` is the legal path and is not this.
            Expr::Call { callee, args } => {
                let path = path_of(body, *callee);
                if path != "task.spawn" {
                    continue;
                }
                // The `detached` argument is the violation; the call is where
                // the handle came from. Charter §16.3 wants both, and they must
                // be different spans or the second underline says nothing.
                let Some(at) = args
                    .iter()
                    .find(|a| matches!(body.expr(a.value), Expr::Name(n) if n == "detached"))
                    .map(|a| body.expr_span(a.value))
                else {
                    continue;
                };
                let h = g.spawn("task.spawn", HandleKind::Task, here, body.expr_span(id));
                g.op(Op::Detach {
                    handle: h,
                    span: at,
                });
            }

            // `observe intersection(self) -> Bool { scope application }` — a
            // subscription declaring a scope that outlives the one it is
            // created in.
            Expr::Keyword {
                keyword,
                modifiers,
                block,
                ..
            } if keyword == "observe" || keyword == "subscribe" => {
                let Some(block) = block else { continue };
                let Some((scope_name, at)) = block_scope(body, *block) else {
                    continue;
                };
                let Some(kind) = named_scope(&scope_name) else {
                    continue;
                };
                let label = match modifiers.first() {
                    Some(m) => format!("{keyword}.{m}"),
                    None => keyword.clone(),
                };
                let h = g.spawn(&label, HandleKind::Subscription, here, body.expr_span(id));
                declared.push((h, by_kind(kind), at.clone(), format!("scope {scope_name}")));
            }

            _ => {}
        }
    }

    let mut found = g.check();
    let mut clauses: Vec<Option<String>> = vec![None; found.len()];
    for (h, scope, span, clause) in declared {
        if let Some(v) = g.check_declared_scope(h, scope, span) {
            found.push(v);
            // The clause as written, so the diagnostic can quote `scope
            // application` rather than paraphrasing it.
            clauses.push(Some(clause));
        }
    }
    out.extend(
        found
            .into_iter()
            .zip(clauses)
            .map(|(v, c)| to_diagnostic(v, c)),
    );
}

/// `scope application` inside a block: the scope's name, and the span covering
/// the pair so a diagnostic can underline the clause rather than the statement.
fn block_scope(body: &Body, block: ExprId) -> Option<(String, crate::hir::Span)> {
    name_pair(body, block, "scope")
}

/// A callee's dotted path as written: `task.spawn`, `Analytics.record_view`.
fn path_of(body: &Body, id: ExprId) -> String {
    match body.expr(id) {
        Expr::Name(n) => n.clone(),
        Expr::Field { base, name } => format!("{}.{}", path_of(body, *base), name),
        _ => String::new(),
    }
}

fn to_diagnostic(v: ScopeViolation, clause: Option<String>) -> Diagnostic {
    Diagnostic {
        code: match v.code {
            "PW2001" => "PW2001",
            "PW2002" => "PW2002",
            "PW2003" => "PW2003",
            _ => "PW2004",
        },
        invariant: match v.code {
            "PW2001" => "a handle cannot outlive the scope that owns it",
            "PW2002" => "an ordinary task cannot be detached from its scope",
            "PW2003" => "a handle cannot be used after its owning scope has exited",
            _ => "a subscription cannot declare a scope that outlives its owner",
        },
        reason: match v.code {
            "PW2001" => "handle_escape",
            "PW2002" => "ordinary_task_detached",
            "PW2003" => "use_after_scope",
            _ => "declared_scope_outlives_owner",
        },
        detector: Detector::ScopeGraph,
        severity: Severity::Error,
        message: v.message,
        primary_span: v.span,
        related: vec![Related::new(
            v.origin_span,
            "the handle is created here".to_string(),
        )],
        // The explanation says WHY the invariant exists; the repair says what
        // to do instead. Emitting the same sentence twice reads as a bug.
        explanation: Some(match v.code {
            "PW2001" => "A handle is cancelled when its owning scope ends. One that has \
                         escaped is a reference to work that may already be gone."
                .to_string(),
            "PW2002" => "Structured concurrency has no fire-and-forget: every ordinary task \
                         belongs to a scope and is cancelled with it. Work that must survive \
                         the scope is a different kind of work, and says so."
                .to_string(),
            "PW2003" => "A result arriving after its owning scope has exited must not be \
                         committed — there is nothing left to commit it to."
                .to_string(),
            _ => format!(
                "A subscription keeps pushing updates for as long as its declared \
                 scope lives. `{}` outlives the component that created it, so the \
                 subscription would push into something that no longer exists.",
                clause.as_deref().unwrap_or("the declared scope")
            ),
        }),
        repairs: vec![Repair {
            description: v.help,
            replacement: None,
        }],
    }
}

// --- privacy, cache safety and placement (E5) ------------------------------

/// A declaration's privacy label, from its visibility keyword.
///
/// Conservative on purpose: an unlabelled declaration is treated as public, so
/// the checker can only *under*-restrict a value it was never told about. It
/// cannot invent a restriction and reject a legal program.
pub(crate) fn label_of(decl: &Decl) -> Label {
    // The parameters are the charter §7.8 type names — `Session<SessionId>`,
    // not `Session<Cart>`. A label names *what* a value is scoped to, not which
    // declaration produced it, and the corpus declares the former as the text
    // the developer must be shown.
    match decl.visibility.as_deref() {
        Some("session") => Label::session("SessionId"),
        // `user`, the user's (track `store-accounts`). `private` says only
        // that a declaration is not importable (ADR-0324).
        Some("user") => Label::user("UserId"),
        _ => Label::public(),
    }
}

/// The world a declaration pins, from either place it can be written.
///
/// A `query` puts `placement origin` in its policy block, before the brace. A
/// `component` writes `placement browser` inside its body. Reading only the
/// policy block missed every component — which is the charter's opening
/// example, a database read inside a browser-placed component.
pub(crate) fn declared_world(hir: &Hir, decl: &Decl) -> Option<World> {
    if let Some(p) = decl.policy("placement") {
        let v = p.value.trim();
        if let Some(w) = ALL_WORLDS.iter().copied().find(|w| w.name() == v) {
            return Some(w);
        }
    }
    let body = hir.body(decl.body?);
    let (name, _) = name_pair(body, body.root, "placement")?;
    ALL_WORLDS.iter().copied().find(|w| w.name() == name)
}

/// `<keyword> <value>` as two adjacent name statements in a block, with the
/// span covering the pair.
fn name_pair(body: &Body, block: ExprId, keyword: &str) -> Option<(String, crate::hir::Span)> {
    let Expr::Block { stmts } = body.expr(block) else {
        return None;
    };
    let mut it = stmts.iter().peekable();
    while let Some(s) = it.next() {
        if !matches!(body.expr(*s), Expr::Name(n) if n == keyword) {
            continue;
        }
        let Some(next) = it.peek() else { continue };
        if let Expr::Name(v) = body.expr(**next) {
            let span = body.expr_span(*s).start..body.expr_span(**next).end;
            return Some((v.clone(), span));
        }
    }
    None
}

/// E5's checks over one declaration.
///
/// Each is a *flow* question answered by the algebra in `crate::privacy` and
/// `crate::placement`, not a pattern match on syntax. That is what lets a
/// diagnostic name the value and the boundary rather than reporting that a
/// keyword was in the wrong place (charter §14 M5 gate).
#[allow(clippy::too_many_arguments)]
fn privacy_and_placement(
    hir: &Hir,
    src: &str,
    labels: &BTreeMap<crate::resolve::DefId, Label>,
    reads: &Reads,
    inference: &crate::effects::Inference<'_>,
    declared: &dyn crate::placement::Placements,
    at: usize,
    id: crate::hir::DeclId,
    decl: &Decl,
    inherited: Option<World>,
    // The layout a page is shown in (ADR-0303), and its name.
    layout: Option<(crate::resolve::DefId, &str)>,
    out: &mut Vec<Diagnostic>,
) {
    // The declaration's own visibility, joined with everything its body reads.
    // A `page` that is itself unlabelled but renders a `session query` is a
    // session materialization — which is the corpus's canonical case, and is
    // invisible to a rule that only reads the header.
    let (read, read_from) = reads_label_with_source(hir, labels, inference, at, decl);
    // **And everything its layout observes** (ADR-0303): the layout's markup
    // is rendered into the page's document, so an unlabelled layout that
    // reads a session's cart makes the page a session's, as a read of its
    // own would.
    let shown = layout.map_or_else(Label::public, |(l, _)| reads.observed(l));
    let label = label_of(decl).join(&read).join(&shown);

    // 1. A reader's value in a shared cache. Charter §7.8's canonical case.
    //
    // The value's whole label (ADR-0128): what the declaration observes,
    // its own parameters included, and not only what its keyword and its
    // callees' keywords declare. `public query Cart(session:
    // Session<SessionId>) cache shared` held each session's cart in the
    // shared cache and passed until 2026-10-02 (T12).
    let def = crate::resolve::DefId {
        unit: at,
        decl: id.0,
    };
    let held = label.join(&reads.observed(def));
    if let Some((cache_value, cache_span)) =
        declared_cache(hir, decl).filter(|(v, _)| v == "shared" && !held.safe_in_shared_cache())
    {
        let label = &held;
        // The parameter, where one is the source, and the label it states:
        // the related span is where it is declared, not the whole
        // declaration, and what is said of it is its own label, not all the
        // declaration holds.
        let param = read_from
            .is_none()
            .then(|| reads.given_by(def))
            .flatten()
            .and_then(|(name, own)| {
                decl.params
                    .iter()
                    .find(|p| p.name == name)
                    .map(|p| (p, own))
            });
        // The label of what the cause chain names as the source.
        let source = param.map_or(label, |(_, own)| own);
        let read_from = read_from
            .map(|v| format!("`{v}`"))
            .or_else(|| param.map(|(p, _)| format!("its parameter `{}`", p.name)))
            .or_else(|| {
                layout
                    .filter(|_| !shown.is_public())
                    .map(|(_, name)| format!("what its layout `{name}` shows"))
            })
            .or_else(|| observed_from(hir, reads, inference, at, decl).map(|v| format!("`{v}`")));
        {
            let cache = crate::hir::Policy {
                name: "cache".to_string(),
                value: cache_value,
                roots: Vec::new(),
                keys: Vec::new(),
                span: cache_span,
            };
            out.push(Diagnostic {
                code: "PW5001",
                invariant: "a value that is not public cannot live in a shared cache",
                reason: "private_value_in_shared_cache",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!("`{}` is {label} and declares a shared cache", decl.name),
                primary_span: cache.span.clone(),
                related: vec![match param {
                    Some((p, own)) => Related::new(
                        p.span.clone(),
                        format!(
                            "`{}` is {own}, so what `{}` makes from it is too",
                            p.name, decl.name
                        ),
                    ),
                    None => Related::new(
                        hir.decl_span(decl_id_of(hir, decl)),
                        format!("`{}` is labelled {label} here", decl.name),
                    ),
                }],
                explanation: Some(format!(
                    "A shared cache may contain only `Public` values, because every \
                     user reads it. The cause chain: `{}` materializes {}, which is \
                     labeled `{source}`, and `{source}` does not flow into `Public` — \
                     so one user's value would be served to another. A key naming \
                     the reader does not make it shareable: an entry only that \
                     reader can hit gains nothing from a shared cache, and puts \
                     their value where every reader's is held (ADR-0128).",
                    decl.name,
                    read_from.as_deref().unwrap_or("a non-public value"),
                )),
                repairs: if label.holds_a_secret() {
                    vec![Repair {
                        description: "do not cache a value holding a secret: cache what \
                                      the secret fetched, or nothing"
                            .to_string(),
                        replacement: None,
                    }]
                } else {
                    vec![Repair {
                        description: "make the cache private, which stores per session".to_string(),
                        replacement: Some((cache.span.clone(), "cache private".to_string())),
                    }]
                },
            });
        }
    }

    // 2. Placement. The demand is what this definition EFFECTIVELY needs, plus
    // the label; the solver answers which worlds can satisfy it.
    //
    // `effective_effects`, not the declared row. Architect ruling, 2026-08-07:
    //
    // > Placement and E8 contract generation should use the same central
    // > effective-effects function. No caller should independently decide
    // > whether to look at declarations or inference.
    //
    // Two answers to "what does this need" is how one semantic declaration ends
    // up with two answers about where it can run — the pattern this project has
    // spent its life deleting. It also means an over-declared row no longer
    // narrows placement: `fn f() !{ database.read, trace } { trace("hi") }`
    // needs `trace`, and a host must not be asked for database access because
    // an annotation permitted it.
    let effects: Vec<String> = inference.effective_effects(at, hir, id);
    // **What it holds where it runs** (ADR-0282): the label the cache rules
    // read, a secret it only uses left out and one it answers or reads kept.
    // Until 2026-10-08 placement read the keywords of what a declaration
    // reads alone: a page reading the session through a query of its own
    // was public to placement and placed at build, and a fragment of a
    // secret at the edge.
    let label = label.join(&reads.holds(def));
    if effects.is_empty() && label.is_public() {
        return;
    }

    let world = declared_world(hir, decl).or(inherited);

    // An effect the row does NOT cover, at a world the author declared, is
    // `DECLARED_PLACEMENT_CANNOT_GRANT`'s to report — against the call that
    // performs it, naming the worlds that could. Reporting it here as well
    // would deliver one defect twice in two vocabularies, which is what
    // `every_rejected_fixture_emits_only_its_own_defect` exists to prevent.
    //
    // Covers, not equals: R-026 declares `secret<Payments>` and its body
    // performs `secret.read`. The row covers it, so this rule owns it.
    let row: Vec<String> = decl
        .declared_effects
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|e| e.path.clone())
        .collect();
    if let Some(w) = world
        && effects.iter().any(|e| {
            !crate::effects::row_covers(&row, e)
                && w.grants(e, declared) == crate::placement::Grant::No
        })
    {
        return;
    }
    // And an effect the row declares that the declared world cannot grant is
    // `rules::check_effects_against_placement`'s, at the effect, with its
    // reason (ADR-0216): R-026's `secret<Payments>` at `edge`. Its placement
    // was read by nothing until ADR-0216, so only this rule spoke.
    let pinned = decl.policy("placement").map(|p| p.value.trim().to_string());
    if let Some(target) = pinned.as_deref().filter(|t| !t.contains(','))
        && decl
            .declared_effects
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|e| crate::rules::placement_conflict(target, &e.path).is_some())
    {
        return;
    }

    let demand = Demand {
        effects: effects.clone(),
        label: label.clone(),
        declared: world,
    };
    let solution = solve(&demand, declared);
    if solution.is_satisfiable() {
        return;
    }
    // An effect that names nothing has no placement, and this rule has nothing
    // to say about it. `check_effect_rows` reports the row — with the family it
    // could not find and the nearest spelling — and reporting "nowhere to run"
    // here as well would name the declaration's placement as the problem while
    // the problem is a word in its row.
    if solution.is_blocked() {
        return;
    }

    // The effect AS WRITTEN, type arguments included. `database.read` and
    // `database.read<Stores>` are the same capability but not the same text,
    // and the corpus declares which one the developer must be shown.
    //
    // From the ROW's source span where a row exists, and from the effective set
    // otherwise. The row is what the author wrote and what the corpus declares
    // it must be shown: R-026 writes `secret<Payments>` and performs
    // `secret.read`, and being told about `secret.read` names something that
    // appears nowhere in the file.
    let mut written: Vec<String> = decl
        .declared_effects
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|e| {
            src.get(e.span.clone())
                .unwrap_or(&e.path)
                .trim()
                .to_string()
        })
        .collect();
    if written.is_empty() {
        written = effects.clone();
    }

    // Nowhere can run this. Name every reason, for every world — that is the
    // cause chain charter §14 M5 task 5 asks for.
    let mut chain: Vec<String> = Vec::new();
    for &w in ALL_WORLDS {
        let reasons: Vec<String> = solution
            .why_not(w)
            .iter()
            .map(|r| r.reason.to_string())
            .collect();
        if !reasons.is_empty() {
            chain.push(format!("  {w}: {}", reasons.join("; ")));
        }
    }

    // Point at the effect that cannot be granted, which is what the author has
    // to change. The declaration's own span is the boundary, and goes in
    // `related` — charter §16.3 wants both.
    let span = decl
        .declared_effects
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|e| {
            effects.iter().any(|f| f == &e.path || f == &e.written)
                && world.is_some_and(|w| w.grants(&e.path, declared) == crate::placement::Grant::No)
        })
        .map(|e| e.span.clone())
        .or_else(|| decl.policy("placement").map(|p| p.span.clone()))
        .unwrap_or_else(|| hir.decl_span(decl_id_of(hir, decl)));

    // **What rules each world out, as the solver found it** (ADR-0282): an
    // effect a world cannot grant, as written, and a label a world may not
    // hold. Until 2026-10-08 the message joined the effects alone, so a
    // session's page placed at build said "it requires" and nothing after
    // it, and a build page reading the session said it required
    // `session.read`, which build grants.
    let refused_an_effect = solution.ruled_out.iter().any(|r| {
        matches!(
            r.reason,
            crate::placement::RuledOut::MissingCapability { .. }
        )
    });
    let held: BTreeSet<String> = solution
        .ruled_out
        .iter()
        .filter_map(|r| match &r.reason {
            crate::placement::RuledOut::LabelNotPermitted { restriction } => {
                Some(Label::of(restriction.clone()).to_string())
            }
            _ => None,
        })
        .collect();
    let held: Vec<String> = held.into_iter().collect();
    let because = match (refused_an_effect && !written.is_empty(), held.is_empty()) {
        (true, false) => format!(
            "it requires {}, and holds {}",
            written.join(", "),
            held.join(", ")
        ),
        (false, false) => format!("it holds {}", held.join(", ")),
        (_, true) => format!("it requires {}", written.join(", ")),
    };
    out.push(Diagnostic {
        code: "PW5002",
        invariant: "every declaration must have somewhere it can run",
        reason: "no_feasible_placement",
        detector: Detector::CapabilityAudit,
        severity: Severity::Error,
        message: format!("`{}` cannot run in any world: {because}", decl.name),
        primary_span: span,
        related: vec![Related::new(
            hir.decl_span(decl_id_of(hir, decl)),
            because.trim_start_matches("it ").to_string(),
        )],
        explanation: Some(format!(
            "Placement is derived from what a body does, not chosen. Each world \
             was ruled out:\n{}",
            chain.join("\n")
        )),
        repairs: vec![Repair {
            description: "split the work: keep the privileged effect where it is \
                          granted and pass a value across the boundary"
                .to_string(),
            replacement: None,
        }],
    });
}

/// The id of a declaration, by identity of its span.
fn decl_id_of(hir: &Hir, decl: &Decl) -> crate::hir::DeclId {
    hir.all_decls()
        .find(|(_, d)| std::ptr::eq(*d, decl))
        .map(|(id, _)| id)
        .expect("the declaration came from this Hir")
}

/// Every restriction a declaration picks up by *calling* another declaration.
///
/// A `page` that renders a `session query` holds session data, whether or not
/// the page says so. Reading it from the program — rather than from the table
/// of library accessors below — is real label propagation: it follows the
/// author's own declarations, and it improves as the program does.
/// The same, plus the binding that carried the restriction.
///
/// A diagnostic that says "this page is Session" is true and unhelpful; the
/// author needs to know it was `cart`. The corpus declares that name as text it
/// expects to see, which is how the gap was found.
/// **The label a DECLARATION carries: its own, joined with everything its body
/// reads.**
///
/// Distinct from `body_label` below, which is a resume-manifest question about
/// one body's captures.
///
/// One derivation, two readers. `check.rs` uses it to refuse a non-public value
/// in a shared cache; `contract.rs` uses it to build the placement demand. It
/// passed `Label::public()` there until 2026-08-11 — see E10-P in
/// `docs/EVIDENCE_LEDGER.md` — four lines under a comment saying the solver
/// weighs privacy labels.
///
/// The deferral was deliberate and is now discharged: the label is the join of
/// what a body READS, and until policy values left the executable body tree a
/// body walk could not tell one from a term. Wiring it in first would have
/// given the contract's placement a second channel from policy values.
pub(crate) fn declaration_label(
    hir: &Hir,
    labels: &BTreeMap<crate::resolve::DefId, Label>,
    inference: &crate::effects::Inference<'_>,
    at: usize,
    decl: &Decl,
) -> Label {
    let (read, _) = reads_label_with_source(hir, labels, inference, at, decl);
    label_of(decl).join(&read)
}

fn reads_label_with_source(
    hir: &Hir,
    labels: &BTreeMap<crate::resolve::DefId, Label>,
    inference: &crate::effects::Inference<'_>,
    at: usize,
    decl: &Decl,
) -> (Label, Option<String>) {
    let Some(body_id) = decl.body else {
        return (Label::public(), None);
    };
    let body = hir.body(body_id);
    let mut label = Label::public();
    let mut source: Option<String> = None;

    // Bindings, so a labelled call can be attributed to the name it was bound
    // to rather than to the call itself.
    let mut bound_at: BTreeMap<usize, String> = BTreeMap::new();
    for id in body.walk() {
        let Expr::Let { pat, .. } = body.expr(id) else {
            continue;
        };
        if let Some(HPat::Bind { name, .. }) = pat.map(|p| body.pat(p)) {
            bound_at.insert(body.expr_span(id).start, name.clone());
        }
    }

    for id in body.walk() {
        // `query Cart(session)` parses as a keyword statement; `Cart(session)`
        // as a call. Both name a declaration.
        let called = match body.expr(id) {
            // A call through a local is the local's, never a declaration
            // of its spelling's (ADR-0063).
            Expr::Call { callee, .. }
                if matches!(body.expr(*callee), Expr::Name(_))
                    && inference.local(at, body, *callee) =>
            {
                continue;
            }
            Expr::Call { callee, .. } => path_of(body, *callee),
            Expr::Keyword {
                keyword, modifiers, ..
            } if keyword == "query" || keyword == "subscribe" => {
                modifiers.first().cloned().unwrap_or_default()
            }
            _ => continue,
        };
        if called.is_empty() {
            continue;
        }
        // RESOLVED. `Cart` in one module and `Cart` in another are two
        // declarations with two labels, and taking the join of both made a
        // page's privacy depend on what an unrelated module happened to call
        // its own query.
        //
        // A name that resolves to nothing contributes nothing — which is the
        // honest answer, and narrower than the join it replaces. A page reading
        // a query it never imported is a resolution error, reported as one.
        let Some(def) = inference.called_from(at, &called) else {
            continue;
        };
        let Some(found) = labels.get(&def).cloned() else {
            continue;
        };
        if found.is_public() {
            continue;
        }
        let short = called.rsplit('.').next().unwrap_or(&called);
        label = label.join(&found);
        if source.is_none() {
            // The nearest enclosing binding, by start offset.
            let at = body.expr_span(id).start;
            source = bound_at
                .range(..=at)
                .next_back()
                .map(|(_, n)| n.clone())
                .or_else(|| Some(short.to_string()));
        }
    }
    (label, source)
}

/// **What a declaration reads, it reads through what it calls** (ADR-0118).
///
/// A declaration reads the calls in its body and the queries it names,
/// resolved as its own unit resolves them. Two labels are joined over those
/// reads to a fixed point:
/// - `declared`: each declaration's own visibility (`label_of`), joined with
///   what it reads. PW5001 and the contract's placement read a value by it;
/// - `returned`: the labels the results of what it reads carry, by their
///   declared types. PW5004 reads a value by it.
///
/// Each read one call deep until 2026-09-26. A query reading the session
/// through a helper, or through another query, was public to both and to
/// PW5101.
///
/// What a command reads is not its caller's: a call to one is a request, and
/// the command is its own component (ADR-0113).
#[derive(Default)]
pub(crate) struct Reads {
    declared: BTreeMap<crate::resolve::DefId, Label>,
    returned: BTreeMap<crate::resolve::DefId, Label>,
    /// The label each declaration's own result type carries.
    results: BTreeMap<crate::resolve::DefId, Label>,
    /// **What each declaration is given** (ADR-0128): the labels its own
    /// parameters state, a secret left out, and the first parameter stating
    /// one. A query is a function of its arguments, so a `public query
    /// Cart(session: Session<SessionId>)` holds a session's value whatever
    /// its keyword says. Until 2026-10-02 nothing read a declaration's own
    /// parameters, and that query passed `cache shared` (T12).
    given: BTreeMap<crate::resolve::DefId, Given>,
    /// **What each declaration reads** (ADR-0282): the declarations its body
    /// calls and the queries it names, and a materialization's `depends_on`.
    reading: BTreeMap<crate::resolve::DefId, Vec<crate::resolve::DefId>>,
    /// **The secrets each declaration answers** (ADR-0282): its result
    /// type's, and those its body's value carries (ADR-0129's summary). A
    /// secret it only uses, as a key to fetch something public, is not one.
    answered: BTreeMap<crate::resolve::DefId, Label>,
}

impl Reads {
    pub(crate) fn of(
        hirs: &[&Hir],
        sigs: &Signatures,
        inference: &crate::effects::Inference<'_>,
    ) -> Reads {
        let mut me = Reads::default();
        let mut reading = Vec::new();
        for (unit, hir) in hirs.iter().enumerate() {
            for (id, decl) in hir.all_decls() {
                let def = crate::resolve::DefId { unit, decl: id.0 };
                me.declared.insert(def, label_of(decl));
                if let Some(sig) = sigs.by_def(def) {
                    me.results.insert(def, sig.label.clone());
                    if let Some(given) = given_label(sigs, sig) {
                        me.returned.insert(def, given.label.clone());
                        me.given.insert(def, given);
                    }
                }
                if decl.kind == DeclKind::Command {
                    continue;
                }
                let mut read = match decl.body {
                    Some(b) => reads_of(hir.body(b), inference, unit),
                    None => Vec::new(),
                };
                // **A materialization reads what it depends on** (ADR-0282):
                // its value is made from them. Until 2026-10-08 its
                // `depends_on` fed the graph alone, so a fragment of a
                // session's cart placed at build, or of a secret placed at
                // the edge, held nothing as placement read it.
                if decl.kind == DeclKind::Materialize {
                    read.extend(depends_on(decl, inference, unit));
                }
                if decl.body.is_some() || !read.is_empty() {
                    me.reading.insert(def, read.clone());
                    reading.push((def, read));
                }
            }
        }
        // Joins only add, so this reaches a fixed point, recursion included.
        loop {
            let mut changed = false;
            for (def, read) in &reading {
                let mut declared = me.declared(*def);
                let mut returned = me.returned.get(def).cloned().unwrap_or_default();
                for r in read {
                    declared = declared.join(&me.declared(*r));
                    returned = returned.join(&me.through(*r));
                }
                if declared != me.declared(*def) {
                    me.declared.insert(*def, declared);
                    changed = true;
                }
                if me.returned.get(def) != Some(&returned) {
                    me.returned.insert(*def, returned);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        me
    }

    fn declared(&self, def: crate::resolve::DefId) -> Label {
        self.declared.get(&def).cloned().unwrap_or_default()
    }

    /// **The secrets each declaration answers** (ADR-0282), from its result
    /// type and from what its body's value carries, `summaries` being
    /// ADR-0129's. Called once the summaries are made.
    pub(crate) fn answering(mut self, summaries: &BTreeMap<crate::resolve::DefId, Label>) -> Reads {
        let mut answered: BTreeMap<crate::resolve::DefId, Label> = BTreeMap::new();
        for (def, l) in self.results.iter().chain(summaries.iter()) {
            let secret = only_secrets(l);
            if !secret.is_public() {
                let held = answered.entry(*def).or_default();
                *held = held.join(&secret);
            }
        }
        self.answered = answered;
        self
    }

    /// **What `def`'s value holds** (ADR-0282): everything it observes but a
    /// secret, and each secret it answers. A secret it only uses, as a key
    /// to fetch something public, is not in it (ADR-0085). What keeps its
    /// value reads it (PW5101).
    pub(crate) fn answers(&self, def: crate::resolve::DefId) -> Label {
        let answered = self.answered.get(&def).cloned().unwrap_or_default();
        without_secrets(&self.observed(def)).join(&answered)
    }

    /// **What `def` holds where it runs** (ADR-0282): what its value holds,
    /// and each secret what it reads answers it. Placement reads it, in the
    /// checker and in the contract, as the cache rules read `observed`.
    pub(crate) fn holds(&self, def: crate::resolve::DefId) -> Label {
        let mut held = self.answers(def);
        for r in self.reading.get(&def).into_iter().flatten() {
            if let Some(a) = self.answered.get(r) {
                held = held.join(a);
            }
        }
        held
    }

    /// What reading `def` gives: its result's label, and what it reads to
    /// make it.
    fn through(&self, def: crate::resolve::DefId) -> Label {
        let result = self.results.get(&def).cloned().unwrap_or_default();
        match self.returned.get(&def) {
            Some(r) => result.join(r),
            None => result,
        }
    }

    /// **Everything `def` observes** (ADR-0128): its visibility, what it
    /// reads through what it calls, its own result, and what it is given.
    /// What a cache rule reads, so neither its keyword nor its spelling can
    /// lower the label of the value it holds.
    pub(crate) fn observed(&self, def: crate::resolve::DefId) -> Label {
        self.declared(def).join(&self.through(def))
    }

    /// The parameter that makes `def` hold a reader's value, if one does,
    /// and the label it states itself: one of what `def` is given, where
    /// `def` is given more than one.
    pub(crate) fn given_by(&self, def: crate::resolve::DefId) -> Option<(&str, &Label)> {
        self.given
            .get(&def)
            .map(|g| (g.first.as_str(), &g.first_label))
    }

    /// Each declaration's `declared` label, where it is not public: what
    /// `reads_label_with_source` reads a callee's label from.
    pub(crate) fn labels(&self) -> BTreeMap<crate::resolve::DefId, Label> {
        self.declared
            .iter()
            .filter(|(_, l)| !l.is_public())
            .map(|(d, l)| (*d, l.clone()))
            .collect()
    }

    /// Each resource's value label, by path, where it names a reader: its
    /// declared visibility and result, and what it reads (PW5101). A secret
    /// it only uses is left out: a query that uses one to fetch public data
    /// makes a public value (ADR-0085). A secret it answers is kept
    /// (ADR-0282): until 2026-10-08 every secret was left out, and a `public
    /// query` answering `Secret<Payments>` was kept in a shared fragment.
    pub(crate) fn of_resources(&self, hirs: &[&Hir]) -> BTreeMap<String, Label> {
        let mut out = BTreeMap::new();
        for (unit, hir) in hirs.iter().enumerate() {
            for (id, decl) in hir.all_decls() {
                if !matches!(
                    decl.kind,
                    DeclKind::Query | DeclKind::Subscription | DeclKind::Resource
                ) {
                    continue;
                }
                let def = crate::resolve::DefId { unit, decl: id.0 };
                let label = self.answers(def);
                if !label.is_public() {
                    out.insert(crate::graph::path_of(hir, id), label);
                }
            }
        }
        out
    }
}

/// A label's secrets alone (ADR-0282).
fn only_secrets(l: &Label) -> Label {
    l.restrictions()
        .filter(|r| matches!(r, Restriction::Secret(_)))
        .fold(Label::public(), |l, r| l.join(&Label::of(r.clone())))
}

/// A label without its secrets (ADR-0085, ADR-0282).
fn without_secrets(l: &Label) -> Label {
    l.restrictions()
        .filter(|r| !matches!(r, Restriction::Secret(_)))
        .fold(Label::public(), |l, r| l.join(&Label::of(r.clone())))
}

/// **What a materialization depends on** (ADR-0282): each `depends_on` key,
/// resolved as its unit resolves a call.
fn depends_on(
    decl: &Decl,
    inference: &crate::effects::Inference<'_>,
    at: usize,
) -> Vec<crate::resolve::DefId> {
    decl.policy("depends_on")
        .into_iter()
        .flat_map(|p| p.keys.iter())
        .filter_map(|k| inference.called_from(at, &k.name))
        .collect()
}

/// The declarations a body reads: what it calls, and the queries it names.
fn reads_of(
    body: &Body,
    inference: &crate::effects::Inference<'_>,
    at: usize,
) -> Vec<crate::resolve::DefId> {
    let mut out = Vec::new();
    for id in body.walk() {
        let read = match body.expr(id) {
            // A call through a local is the local's, never a declaration
            // of its spelling's (ADR-0063).
            Expr::Call { callee, .. }
                if matches!(body.expr(*callee), Expr::Name(_))
                    && inference.local(at, body, *callee) =>
            {
                continue;
            }
            Expr::Call { callee, .. } => path_of(body, *callee),
            Expr::Keyword {
                keyword, modifiers, ..
            } if keyword == "query" || keyword == "subscribe" => {
                modifiers.first().cloned().unwrap_or_default()
            }
            _ => continue,
        };
        if read.is_empty() {
            continue;
        }
        if let Some(def) = inference.called_from(at, &read) {
            out.push(def);
        }
    }
    out
}

/// What a declaration reads that makes its value a reader's, by name: the
/// first declaration it reads whose result, through what that reads, is not
/// public (ADR-0128). For a cause chain that names a source.
fn observed_from(
    hir: &Hir,
    reads: &Reads,
    inference: &crate::effects::Inference<'_>,
    at: usize,
    decl: &Decl,
) -> Option<String> {
    let body = hir.body(decl.body?);
    for id in body.walk() {
        let called = match body.expr(id) {
            // A call through a local is the local's, never a declaration
            // of its spelling's (ADR-0063).
            Expr::Call { callee, .. }
                if matches!(body.expr(*callee), Expr::Name(_))
                    && inference.local(at, body, *callee) =>
            {
                continue;
            }
            Expr::Call { callee, .. } => path_of(body, *callee),
            Expr::Keyword {
                keyword, modifiers, ..
            } if keyword == "query" || keyword == "subscribe" => {
                modifiers.first().cloned().unwrap_or_default()
            }
            _ => continue,
        };
        let Some(def) = inference.called_from(at, &called) else {
            continue;
        };
        if !reads.through(def).is_public() {
            return Some(called.rsplit('.').next().unwrap_or(&called).to_string());
        }
    }
    None
}

/// **What calling each declaration makes, from its body** (ADR-0129).
///
/// The label of its body's value, and of each failure a `?` in it may
/// return, with what each call in it makes, to a fixed point. Joins only add,
/// so recursion terminates. A declaration with no body, a host binding, has
/// none: its signature is its contract, and the platform answers for it, as
/// for `Payments.capture`'s receipt.
///
/// Until 2026-10-02 a call's value was labelled by its callee's signature
/// alone, so a helper returning a secret it read, in a `String`, returned a
/// public value. ADR-0118 followed reads for the coarse label the cache rules
/// read; this follows them for the value a sink reads.
pub(crate) fn summaries(
    hirs: &[&Hir],
    sigs: &Signatures,
) -> BTreeMap<crate::resolve::DefId, Label> {
    let imports: Vec<Vec<String>> = hirs
        .iter()
        .map(|h| crate::labels::imported_modules(h))
        .collect();
    let mut bodies = Vec::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            let callable = matches!(
                decl.kind,
                DeclKind::Fn
                    | DeclKind::Query
                    | DeclKind::Command
                    | DeclKind::Subscription
                    | DeclKind::Resource
            );
            if callable && decl.body.is_some() {
                bodies.push((unit, id));
            }
        }
    }
    let mut out: BTreeMap<crate::resolve::DefId, Label> = BTreeMap::new();
    loop {
        let mut next = out.clone();
        for &(unit, id) in &bodies {
            let hir = hirs[unit];
            let Some(b) = hir.decl(id).body else {
                continue;
            };
            let body = hir.body(b);
            let value =
                crate::labels::Labels::of_decl(sigs, hir, id, body, &imports[unit], Some(&out))
                    .value_of_body(body);
            if !value.is_public() {
                let def = crate::resolve::DefId { unit, decl: id.0 };
                let held = next.entry(def).or_default();
                *held = held.join(&value);
            }
        }
        if next == out {
            return out;
        }
        out = next;
    }
}

/// **What a declaration's parameters give it** (ADR-0128): the labels they
/// state, joined, and the first parameter stating one, with that label.
struct Given {
    label: Label,
    first: String,
    /// The first's own label. A query given a session and an organization
    /// holds both, and its `session` states only the first: PW5001 said
    /// "`session` is Session<SessionId> ⊔ Organization<OrganizationId>"
    /// until 2026-11-06.
    first_label: Label,
}

/// The labels a declaration's parameters state, a secret left out, with the
/// first parameter stating one (ADR-0128). A secret a parameter states is a
/// key the declaration uses, not a value it holds (ADR-0118 §5).
fn given_label(sigs: &Signatures, sig: &crate::signatures::Signature) -> Option<Given> {
    let mut label = Label::public();
    let mut first = None;
    for (i, param) in sig.params.iter().enumerate() {
        let Some(ty) = param
            .as_ref()
            .and_then(crate::resolved::TypeResolution::resolved)
        else {
            continue;
        };
        let held = sigs
            .label(ty)
            .restrictions()
            .filter(|r| !matches!(r, Restriction::Secret(_)))
            .fold(Label::public(), |l, r| l.join(&Label::of(r.clone())));
        if held.is_public() {
            continue;
        }
        label = label.join(&held);
        if first.is_none() {
            let name = sig.names.get(i).cloned().unwrap_or_else(|| format!("#{i}"));
            first = Some((name, held));
        }
    }
    first.map(|(first, first_label)| Given {
        label,
        first,
        first_label,
    })
}

/// **The signature a call names, as the unit sees it** (ADR-0112).
///
/// Resolved through the unit's own declarations and imports first, then by
/// a whole path. `secrets.payments()` and `payments()` after `import
/// secrets.{ payments }` are one declaration. The privacy rules read a
/// callee's label and its sink level by its fully qualified spelling alone
/// until 2026-09-26, so the second spelling carried no label: a secret
/// reached markup, and a public log, and `pw check` passed.
fn callee_signature<'s>(
    sigs: &'s Signatures,
    inference: &crate::effects::Inference<'_>,
    at: usize,
    body: &Body,
    callee: ExprId,
) -> Option<&'s crate::signatures::Signature> {
    let path = path_of(body, callee);
    inference
        .called_from(at, &path)
        .and_then(|def| sigs.by_def(def))
        .or_else(|| sigs.by_path(&path))
}

/// E5 rules that need the body's label, not only the declaration header.
/// The cache partition a declaration asks for, from either place it can be
/// written: a `query`'s policy block, or a `page`'s body.
/// The cache policy a declaration names, from either place it can be written.
///
/// A `query` puts `cache private` in its policy block, before the brace; a
/// `page` writes it inside its body. `pub(crate)` because `resume.rs` needs the
/// same answer to know what scope a manifest lands in, and reading it a second
/// way there is how the two would come to disagree about what a private page is.
pub(crate) fn declared_cache(hir: &Hir, decl: &Decl) -> Option<(String, crate::hir::Span)> {
    if let Some(p) = decl.policy("cache") {
        return Some((p.value.trim().to_string(), p.span.clone()));
    }
    let body = hir.body(decl.body?);
    name_pair(body, body.root, "cache")
}

/// Charter §7.8: a sink accepts only what its declared privacy level admits.
///
/// The level is read from the signature, not from the function's name.
/// `log.public` carries `!{ log<Public> }`, and it is the type argument that
/// makes the sink checkable — a rule that recognised the identifier `public`
/// would be inventing the vocabulary it then enforces, and would miss the next
/// sink somebody declares.
///
/// Deliberately outside `privacy_flow`, which returns early when the enclosing
/// declaration's own label is public. R-006's `trace_capture` returns `()`, so
/// its body label IS public; the defect is in what it passes along, not in what
/// it returns.
#[allow(clippy::too_many_arguments)]
fn privacy_sinks(
    hir: &Hir,
    sigs: &Signatures,
    summaries: &BTreeMap<crate::resolve::DefId, Label>,
    inference: &crate::effects::Inference<'_>,
    at: usize,
    id: crate::hir::DeclId,
    decl: &Decl,
    out: &mut Vec<Diagnostic>,
) {
    let Some(body_id) = decl.body else { return };
    let body = hir.body(body_id);
    let imports = crate::labels::imported_modules(hir);
    // Labels belong to VALUES. `labels.rs` explains why this is not a map from
    // binding name to restriction any more.
    let labels = crate::labels::Labels::of_decl(sigs, hir, id, body, &imports, Some(summaries));

    for id in body.walk() {
        let Expr::Call { callee, args } = body.expr(id) else {
            continue;
        };
        let Some(level) = sink_level(sigs, inference, at, body, *callee) else {
            continue;
        };
        // Only `Public` is checked, because only `Public` is what the corpus
        // specifies. A rule for the other levels would be a rule nothing has
        // ever exercised.
        if level.0 != "Public" {
            continue;
        }
        // **A public log takes only a public value** (ADR-0129): a public
        // log's contents "may be read by anyone" (`log.pw`), so a session's
        // or a user's value is as much a leak there as a secret. Until
        // 2026-10-02 only a secret was refused.
        let mut refused = false;
        for arg in args {
            // The argument's own label, whatever it is spelled as: a name, a
            // field of one, a branch that returns one, a string with one in a
            // hole. Not a search for names the body happened to bind.
            let label = labels.label(body, arg.value);
            if label.is_public() {
                continue;
            }
            refused = true;
            let (name, span, origin) = blame(body, &labels, arg.value);
            let origin = origin.unwrap_or_else(|| span.clone());
            let explanation = match label.secret_capabilities().next() {
                Some(cap) => format!(
                    "`log<Public>` accepts only `Public` values, and a \
                     `Secret<{cap}>` is not one. Logging is an effect \
                     parameterized by privacy level precisely so that this is a \
                     compile error rather than an incident found in a log \
                     archive months later. The row already says \
                     `secret<{cap}>`, which permits the value to EXIST here — \
                     it does not permit it to leave."
                ),
                None => format!(
                    "`log<Public>` accepts only `Public` values: its contents may \
                     be read by anyone. A value labelled `{label}` belongs to \
                     one reader, and reading it from a log is reading it as \
                     someone else (ADR-0129)."
                ),
            };
            out.push(Diagnostic {
                code: crate::codes::VALUE_EXCEEDS_SINK_LEVEL.id,
                invariant: crate::codes::VALUE_EXCEEDS_SINK_LEVEL.invariant,
                reason: "value_exceeds_sink_privacy_level",
                detector: Detector::PatternMatrix,
                severity: Severity::Error,
                message: format!("cannot log a `{label}` value at privacy level `Public`"),
                primary_span: span,
                related: vec![
                    Related::new(origin, format!("`{name}` becomes {label} here")),
                    Related::new(level.1.clone(), format!("`{}` is declared here", level.0)),
                ],
                explanation: Some(explanation),
                repairs: vec![Repair {
                    description: "log a non-secret correlate — an identifier or a \
                                  hash — or use a sink whose level admits the value"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
        // **Reaching a public log tells what decided it was reached**
        // (ADR-0129): `if secret { log.public("yes") }` logs the secret's
        // answer. One defect, one diagnostic: an argument already refused
        // is not asked again about its branch.
        if refused {
            continue;
        }
        let Some((condition, cause)) = labels.condition(id) else {
            continue;
        };
        out.push(Diagnostic {
            code: crate::codes::VALUE_EXCEEDS_SINK_LEVEL.id,
            invariant: crate::codes::VALUE_EXCEEDS_SINK_LEVEL.invariant,
            reason: "value_exceeds_sink_privacy_level",
            detector: Detector::PatternMatrix,
            severity: Severity::Error,
            message: format!(
                "cannot log at privacy level `Public` where a `{condition}` value decides it"
            ),
            primary_span: body.expr_span(*callee),
            related: vec![
                Related::new(
                    body.expr_span(*cause),
                    format!("this is {condition}, and whether the line is logged depends on it"),
                ),
                Related::new(level.1.clone(), format!("`{}` is declared here", level.0)),
            ],
            explanation: Some(format!(
                "A public log's contents may be read by anyone, and so may the \
                 fact that a line was logged. This one is reached only where a \
                 value labelled `{condition}` says so: a branch on it, a loop \
                 over it, or a `?` on it that did not fail. Its reader learns \
                 what that value was (ADR-0129)."
            )),
            repairs: vec![Repair {
                description: "log at a level that admits the condition, or decide \
                              on a public value"
                    .to_string(),
                replacement: None,
            }],
        });
    }
}

/// What to blame in the diagnostic: the innermost labelled name inside the
/// argument, its span, and where it acquired the label.
fn blame(
    body: &Body,
    labels: &crate::labels::Labels<'_>,
    value: ExprId,
) -> (String, crate::hir::Span, Option<crate::hir::Span>) {
    for id in body.walk_from(value) {
        if let Expr::Name(n) = body.expr(id)
            && let Some((_, origin)) = labels.origin(id)
        {
            return (n.clone(), body.expr_span(id), Some(origin.clone()));
        }
    }
    ("this value".to_string(), body.expr_span(value), None)
}

/// The privacy level a call's sink declares, with the span of the effect that
/// declares it.
fn sink_level(
    sigs: &Signatures,
    inference: &crate::effects::Inference<'_>,
    at: usize,
    body: &Body,
    callee: ExprId,
) -> Option<(String, crate::hir::Span)> {
    // As the unit resolves it (ADR-0112): a bare `public` is the `log.public`
    // it imports, or its own `public`, and never "the one declaration
    // spelled `public`" somewhere else.
    let sig = callee_signature(sigs, inference, at, body, callee)?;
    sig.effects.iter().find_map(|e| {
        let (_, rest) = e.split_once('<')?;
        let level = rest.strip_suffix('>')?;
        (!level.is_empty()).then(|| (level.to_string(), body.expr_span(callee)))
    })
}

#[allow(clippy::too_many_arguments)]
fn privacy_flow(
    hir: &Hir,
    sigs: &Signatures,
    declared_labels: &BTreeMap<crate::resolve::DefId, Label>,
    reads: &Reads,
    summaries: &BTreeMap<crate::resolve::DefId, Label>,
    inference: &crate::effects::Inference<'_>,
    at: usize,
    id: crate::hir::DeclId,
    decl: &Decl,
    out: &mut Vec<Diagnostic>,
) {
    let Some(body_id) = decl.body else { return };
    let body = hir.body(body_id);
    let imports = crate::labels::imported_modules(hir);
    let def = crate::resolve::DefId {
        unit: at,
        decl: id.0,
    };

    // 1. A secret reaching markup. Markup renders in the browser, and a secret
    //    never leaves the origin (charter §7.8, corpus R-003).
    //
    // The VALUE's label, via `labels.rs`. This used to read a map from binding
    // name to restriction, so `let shown = if dry_run { "none" } else { key }`
    // rendered a secret that the rule could not see — the same narrowness the
    // sink rule had, in the one place that had not been migrated with it.
    //
    // Every restriction, not the first (ADR-0129): a value that is a
    // session's and a secret is ordered `Session` first, and until
    // 2026-10-02 only that one was read, so it rendered with nothing
    // reported. Run whenever there is markup: a parameter can bring a secret
    // that nothing the body calls declares.
    let has_markup = body
        .walk()
        .into_iter()
        .any(|e| matches!(body.expr(e), Expr::Template { .. }));
    let labels = has_markup
        .then(|| crate::labels::Labels::of_decl(sigs, hir, id, body, &imports, Some(summaries)));
    for id in body.walk() {
        let Some(labels) = labels.as_ref() else {
            break;
        };
        let Expr::Template { parts, .. } = body.expr(id) else {
            continue;
        };
        for part in parts {
            let value_label = labels.label(body, *part);
            let Some(cap) = value_label.secret_capabilities().next() else {
                continue;
            };
            // The name that brings the secret, not the first labelled one: in
            // `"{s}{key}"` a session's value comes first.
            let (name, origin) = body
                .walk_from(*part)
                .into_iter()
                .find_map(|e| match body.expr(e) {
                    Expr::Name(n) => labels
                        .origin(e)
                        .filter(|(l, _)| l.secret_capabilities().any(|c| c == cap))
                        .map(|(_, o)| (n.clone(), o.clone())),
                    _ => None,
                })
                .unwrap_or_else(|| ("this value".to_string(), body.expr_span(*part)));
            let origin = &origin;
            out.push(Diagnostic {
                code: "PW5003",
                invariant: "a secret cannot be rendered to the browser",
                reason: "secret_crosses_to_browser",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!("`{name}` is Secret<{cap}> and is rendered into markup"),
                primary_span: body.expr_span(*part),
                related: vec![Related::new(
                    origin.clone(),
                    format!("`{name}` becomes Secret<{cap}> here"),
                )],
                explanation: Some(format!(
                    "Markup is sent to the browser. Secret<{cap}> may exist only in \
                     the origin world, so rendering it moves it across a boundary \
                     it may not cross — the value is in the HTML whether or not \
                     anything reads it."
                )),
                repairs: vec![Repair {
                    description: "perform the privileged operation on the origin and \
                                  render only its result"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
    }

    // 2. A shared cache keyed without every partition the value needs
    //    (charter §14 M5 task 4, corpus R-005).
    let shared = decl
        .policy("cache")
        .is_some_and(|c| c.value.trim() == "shared");
    // What the body reads through each call it makes, its result's label
    // among it, is `Reads::observed`'s (ADR-0118, ADR-0128). Until
    // 2026-10-07 a second derivation of it from the body's calls was joined
    // here too, and changed nothing.
    if !shared || reads.observed(def).is_public() {
        return;
    }
    // One defect, one diagnostic (ADR-0112). A value PW5001 already refuses
    // a shared cache is not asked again which partitions its key lacks. What
    // is left is a tenant's value: the one partition a shared cache can carry
    // by its key (ADR-0128).
    let (read, _) = reads_label_with_source(hir, declared_labels, inference, at, decl);
    let label = label_of(decl).join(&read).join(&reads.observed(def));
    if !label.safe_in_shared_cache() {
        return;
    }
    let key_policy = decl.policy("key");
    let key_text = key_policy.map(|k| k.value.clone()).unwrap_or_default();
    // Each of the key's items, by name. A substring matched a parameter
    // called `username` as the user's partition until 2026-09-26 (ADR-0089).
    let items: Vec<&str> = key_text.split(',').map(str::trim).collect();
    // An item names a partition, or a parameter whose declared type carries
    // one: `key store, org` with `org: Organization<OrganizationId>` keys by
    // the tenant as surely as `key store, organization` (ADR-0128).
    let mut covered: Vec<String> = items.iter().map(|i| i.to_string()).collect();
    if let Some(sig) = sigs.by_def(def) {
        for (name, param) in sig.names.iter().zip(&sig.params) {
            let Some(ty) = param
                .as_ref()
                .and_then(crate::resolved::TypeResolution::resolved)
            else {
                continue;
            };
            if items.contains(&name.as_str()) {
                covered.extend(sigs.label(ty).required_cache_partitions());
            }
        }
    }
    let missing: Vec<String> = label
        .required_cache_partitions()
        .into_iter()
        .filter(|p| !covered.contains(p))
        .collect();
    if missing.is_empty() {
        return;
    }

    let span = key_policy
        .map(|k| k.span.clone())
        .or_else(|| decl.policy("cache").map(|c| c.span.clone()))
        .unwrap_or_else(|| hir.decl_span(decl_id_of(hir, decl)));

    out.push(Diagnostic {
        code: "PW5004",
        invariant: "a shared cache key must carry every partition its value depends on",
        reason: "cache_key_omits_partition",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: format!(
            "`{}` is {label} but its shared cache key omits {}",
            decl.name,
            missing.join(" and ")
        ),
        primary_span: span,
        related: vec![Related::new(
            hir.decl_span(decl_id_of(hir, decl)),
            format!("the result depends on {label}"),
        )],
        explanation: Some(format!(
            "Two callers with different {} produce different results, and this key \
             cannot tell them apart — so the first caller's value is served to the \
             second. That is a cross-tenant leak, not a stale read.",
            missing.join(" and ")
        )),
        repairs: vec![Repair {
            description: format!(
                "add {} to the key, or make the cache private",
                missing.join(" and ")
            ),
            replacement: None,
        }],
    });
}

// --- markup rules (E3/E5, no inference required) ---------------------------

/// Elements whose children HTML constrains. Charter §8.2 asks for semantic
/// HTML; a `<div>` inside a `<ul>` is invalid markup that browsers silently
/// reparse, so the DOM stops matching the source.
fn permitted_children(tag: &str) -> Option<&'static [&'static str]> {
    Some(match tag {
        "ul" | "ol" => &["li", "script", "template"],
        "dl" => &["dt", "dd", "div", "script", "template"],
        "table" => &["caption", "colgroup", "thead", "tbody", "tfoot", "tr"],
        "thead" | "tbody" | "tfoot" => &["tr"],
        "tr" => &["td", "th"],
        "select" => &["option", "optgroup"],
        _ => return None,
    })
}

/// **An element holds only the children HTML permits, as the page holds
/// them** (ADR-0204). A block's rows and branches are the element's
/// children, and a view used there is the elements it renders at its top.
/// Until 2026-10-05 only the elements written directly inside were read: a
/// `<div>` in an `{#each}` inside a `<ul>` passed, and `<ul><Item /></ul>`
/// was refused by the view's name though `Item` renders an `<li>`.
fn nesting(
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    hir: &Hir,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        for id in body.walk_markup(&template_roots(body)) {
            let Node::Element { tag, children, .. } = body.node(id) else {
                continue;
            };
            let Some(allowed) = permitted_children(tag) else {
                continue;
            };
            let mut held = Vec::new();
            for &c in children {
                held_at(body, c, &mut held);
            }
            for c in held {
                let Node::Element { tag: child, .. } = body.node(c) else {
                    continue;
                };
                let (rendered, message) = if is_view_tag(child) {
                    let mut tops = Vec::new();
                    view_tops(
                        workspace,
                        hirs,
                        unit,
                        child,
                        &mut BTreeSet::new(),
                        &mut tops,
                    );
                    let Some(top) = tops.into_iter().find(|t| !allowed.contains(&t.as_str()))
                    else {
                        continue;
                    };
                    let message = format!(
                        "`<{child}>` renders `<{top}>`, which is not permitted as a child of \
                         `<{tag}>`"
                    );
                    (top, message)
                } else if allowed.contains(&child.as_str()) {
                    continue;
                } else {
                    let message = format!("`<{child}>` is not permitted as a child of `<{tag}>`");
                    (child.clone(), message)
                };
                out.push(Diagnostic {
                    code: crate::codes::INVALID_NESTING.id,
                    invariant: crate::codes::INVALID_NESTING.invariant,
                    reason: "invalid_nesting",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message,
                    primary_span: body.node_span(c),
                    related: vec![Related::new(
                        body.node_span(id),
                        format!("`<{tag}>` starts here"),
                    )],
                    explanation: Some(format!(
                        "`<{tag}>` accepts only {}: HTML's content model, which validators and \
                         assistive technology read a page by. A block's rows are the element's \
                         children, and a view's are the elements it renders. A browser's parser \
                         moves some of what HTML does not permit, as a table's stray content is \
                         placed before the table, so the DOM stops matching the source.",
                        allowed
                            .iter()
                            .map(|t| format!("`<{t}>`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    repairs: vec![Repair {
                        description: format!(
                            "wrap `<{rendered}>` in `<{}>`, or move it outside `<{tag}>`",
                            allowed[0]
                        ),
                        replacement: None,
                    }],
                });
            }
        }
    }
    out
}

/// A declaration's markup roots.
fn template_roots(body: &crate::hir::Body) -> Vec<crate::hir::NodeId> {
    let mut roots = Vec::new();
    for e in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(e) {
            roots.extend(r.iter().copied());
        }
    }
    roots
}

/// An element named with a capital letter is a view (ADR-0072).
fn is_view_tag(tag: &str) -> bool {
    tag.starts_with(|c: char| c.is_ascii_uppercase())
}

/// The elements `node` puts where it is written: itself, or a block's rows
/// and branches, through the blocks inside.
fn held_at(body: &crate::hir::Body, node: crate::hir::NodeId, out: &mut Vec<crate::hir::NodeId>) {
    match body.node(node) {
        Node::Element { .. } => out.push(node),
        Node::Block { children, .. } => {
            for &c in children {
                held_at(body, c, out);
            }
        }
        Node::Text(_) | Node::Interpolation(_) | Node::Branch { .. } => {}
    }
}

/// The elements the view `tag` renders at its top, resolved from `unit`:
/// through its blocks, and through the views it uses there. A view met
/// again on the way is not read twice, so one that contains itself ends.
fn view_tops(
    workspace: &crate::resolve::Workspace,
    hirs: &[&Hir],
    unit: usize,
    tag: &str,
    seen: &mut BTreeSet<crate::resolve::DefId>,
    out: &mut Vec<String>,
) {
    use crate::resolve::Resolution;
    let (Resolution::Local(def) | Resolution::Imported { def, .. }) = workspace.resolve(unit, tag)
    else {
        return;
    };
    let Some(decl) = crate::resolve::declaration(hirs, def) else {
        return;
    };
    if decl.kind != DeclKind::View || !seen.insert(def) {
        return;
    }
    let Some(body_id) = decl.body else { return };
    let body = hirs[def.unit].body(body_id);
    let mut held = Vec::new();
    for r in template_roots(body) {
        held_at(body, r, &mut held);
    }
    for n in held {
        let Node::Element { tag, .. } = body.node(n) else {
            continue;
        };
        if is_view_tag(tag) {
            view_tops(workspace, hirs, def.unit, tag, seen, out);
        } else if !out.contains(tag) {
            out.push(tag.clone());
        }
    }
}

/// Structural rules over a declaration's markup.
///
/// None of these needs effect inference: they are properties of the tree the
/// author wrote. They are grouped because they share the walk, not because they
/// share an invariant — each pushes its own code.
/// **A template block's markers, read** (ADR-0042).
///
/// Until 2026-09-25 nothing read them. `{:else}` was dropped, so both of an
/// `if`'s branches rendered together; `{#if a} .. {/each}` closed the `if`;
/// and an unknown directive passed `pw check`, refused only when rendered.
/// **A loop's key is its element, or a field read from it** (ADR-0073).
///
/// `{#each rs as x (x.id)}` keys each instance on its element's `id`. Until
/// 2026-09-26 the key's head was not read: the template IR kept the key's last
/// segment and keyed on that field of the element. So `(item.id)` in a loop
/// over `x` keyed on `x.id`, and `(k.r.id)` on `k.id`, silently.
fn loop_keys(hir: &Hir, id: crate::hir::DeclId, decl: &Decl, out: &mut Vec<Diagnostic>) {
    let Some(body_id) = decl.body else { return };
    let body = hir.body(body_id);
    let mut roots = Vec::new();
    for e in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(e) {
            roots.extend(r.iter().copied());
        }
    }
    for n in body.walk_markup(&roots) {
        let Node::Block {
            each: Some(each), ..
        } = body.node(n)
        else {
            continue;
        };
        let (binding, Some(read)) = (&each.binder, each.key) else {
            continue;
        };
        // A path from the row: in the key's scope, the row's name is the
        // innermost binding of it (ADR-0345).
        let from_row = crate::lexical::path_root(body, read)
            .is_some_and(|root| matches!(body.expr(root), Expr::Name(n) if n == binding));
        if from_row {
            continue;
        }
        let key = each.written.key.clone().unwrap_or_default();
        let key_span = body.expr_span(read);
        out.push(Diagnostic {
            code: crate::codes::LOOP_KEY.id,
            invariant: crate::codes::LOOP_KEY.invariant,
            reason: "loop_key",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!(
                "a loop's key is `{binding}` or a field read from it, and this is `{key}`"
            ),
            // The key, where it is written (ADR-0242).
            primary_span: key_span.clone(),
            related: vec![Related::new(
                hir.decl_span(id),
                format!("`{}` renders this", decl.name),
            )],
            // Until 2026-09-26 a key whose head was another name keyed on a field
            // of the element, silently.
            explanation: Some(
                "A key identifies each element of the list, so it is read from the element."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: format!("key the loop on `{binding}`, or on a field read from it"),
                replacement: None,
            }],
        });
    }
}

/// **A condition that tests a value's case** (ADR-0153): `x == C` or
/// `x != C`, either way round, where `C` is a case of `x`'s type, an
/// `Option`'s, a `Result`'s or a declared sum type's. The value as written,
/// and the case.
fn case_test(
    body: &hir::Body,
    sigs: &Signatures,
    types: &crate::infer::Types,
    unit: Option<usize>,
    c: hir::ExprId,
) -> Option<(String, String)> {
    use crate::resolved::Builtin;
    let Expr::Binary {
        op: hir::BinOp::Cmp(op),
        lhs,
        rhs,
    } = body.expr(c)
    else {
        return None;
    };
    if op != "==" && op != "!=" {
        return None;
    }
    let cases_of = |e: hir::ExprId| -> Vec<String> {
        let Some(ty) = types.of(body, e) else {
            return Vec::new();
        };
        match ty.as_builtin() {
            Some(Builtin::Option) => vec!["Some".to_string(), "None".to_string()],
            Some(Builtin::Result) => vec!["Ok".to_string(), "Err".to_string()],
            _ => ty
                .def_id()
                .and_then(|d| sigs.type_decl(d)?.variants.as_ref().cloned())
                .map(|vs| vs.into_iter().map(|(name, _)| name).collect())
                .unwrap_or_default(),
        }
    };
    // A case as written, read from the expression's shape: `Placed`, a
    // qualified `Status.Placed`, or a case's call, `Some(x)`. Never from a
    // path's spelling (RISK_QUEUE 34): a name spelled like a case of another
    // type is that type's, which `same` below tells apart.
    let case_named = |e: hir::ExprId| -> Option<String> {
        let at = match body.expr(e) {
            Expr::Call { callee, .. } => *callee,
            _ => e,
        };
        match body.expr(at) {
            Expr::Name(n) => Some(n.clone()),
            Expr::Field { name, .. } => Some(name.clone()),
            _ => None,
        }
    };
    // The same type, by identity: the language's `Option` or `Result`, or the
    // same declared sum type. A bare case the typer leaves untyped is looked
    // up among the cases of the value's own type, a lookup scoped by that
    // type's identity; a qualified one must be typed as the value's type.
    let same = |a: hir::ExprId, b: hir::ExprId| match (types.of(body, a), types.of(body, b)) {
        (Some(x), Some(y)) => match (x.as_builtin(), y.as_builtin()) {
            (Some(p), Some(q)) => p == q,
            (None, None) => x.def_id().is_some() && x.def_id() == y.def_id(),
            _ => false,
        },
        (Some(x), None) => match body.expr(b) {
            Expr::Name(_) => true,
            // `Status.Placed`: its qualifier resolved, by identity, to the
            // value's type, as a `{#match}` arm's is.
            Expr::Field { base, .. } => {
                use crate::resolve::{Namespace, Resolution};
                let q = crate::infer::path_of(body, *base);
                let named = unit.map(|u| match q.contains('.') {
                    true => sigs.workspace().resolve_path_in(u, Namespace::Type, &q),
                    false => sigs.workspace().resolve_in(u, Namespace::Type, &q),
                });
                matches!(
                    named,
                    Some(Resolution::Local(d) | Resolution::Imported { def: d, .. })
                        if Some(d) == x.def_id()
                )
            }
            _ => false,
        },
        _ => false,
    };
    for (value, other) in [(*lhs, *rhs), (*rhs, *lhs)] {
        if same(value, other)
            && let Some(case) = case_named(other)
            && cases_of(value).contains(&case)
        {
            return Some((crate::infer::path_of(body, value), case));
        }
    }
    None
}

fn case_tested_by_if(
    body: &hir::Body,
    at: hir::ExprId,
    value: &str,
    case: &str,
    related: Vec<Related>,
) -> Diagnostic {
    let code = crate::codes::CASE_TESTED_BY_IF;
    Diagnostic {
        code: code.id,
        invariant: code.invariant,
        reason: "case_tested_by_if",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!(
            "`{{#if}}` tests whether `{value}` is `{case}`; `{{#match {value}}}` names each case"
        ),
        primary_span: body.expr_span(at),
        related,
        explanation: Some(
            "A `{#match}` is held to cover every case of its value (PW0305), so a case \
             added later is a page that does not check until it shows it. `{#if}` \
             blocks testing cases are held to none: one that forgets a case shows \
             nothing for it (ADR-0153)."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: format!(
                "write `{{#match {value}}}` with an arm for each case, an empty one where \
                 nothing shows"
            ),
            replacement: None,
        }],
    }
}

fn template_blocks(
    hir: &Hir,
    sigs: &Signatures,
    id: crate::hir::DeclId,
    decl: &Decl,
    out: &mut Vec<Diagnostic>,
) {
    use crate::resolved::Builtin;
    let Some(body_id) = decl.body else { return };
    let body = hir.body(body_id);
    let mut roots = Vec::new();
    for e in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(e) {
            roots.extend(r.iter().copied());
        }
    }
    if roots.is_empty() {
        return;
    }
    let types = crate::infer::Types::of_decl(sigs, hir, id, body);
    let at = hir.decl_span(id);
    let related = || {
        vec![Related::new(
            at.clone(),
            format!("`{}` renders this", decl.name),
        )]
    };
    let malformed = |span: hir::Span, message: String| Diagnostic {
        code: crate::codes::MALFORMED_TEMPLATE_BLOCK.id,
        invariant: crate::codes::MALFORMED_TEMPLATE_BLOCK.invariant,
        reason: "malformed_template_block",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message,
        primary_span: span,
        related: related(),
        // Until 2026-09-25 a dropped `{:else}` rendered both of an `if`'s
        // branches at once.
        explanation: Some(
            "A block's markers decide what renders. A marker the block does not take \
             is not ignored safely: an `if` whose `{:else}` were dropped would render \
             both of its branches at once."
                .to_string(),
        ),
        repairs: Vec::new(),
    };

    let mut inside = BTreeSet::new();
    for n in body.walk_markup(&roots) {
        let Node::Block {
            directive,
            children,
            subject,
            close,
            ..
        } = body.node(n)
        else {
            continue;
        };
        let d = directive.trim();
        let span = body.node_span(n);
        // Each branch marker with what it carries: as written, whether it has
        // a condition, and the arm it names.
        type Marker<'b> = (hir::NodeId, String, bool, &'b Option<hir::TemplateArm>);
        let branches: Vec<Marker<'_>> = children
            .iter()
            .filter_map(|c| match body.node(*c) {
                Node::Branch {
                    marker,
                    condition,
                    arm,
                } => Some((*c, marker.trim().to_string(), condition.is_some(), arm)),
                _ => None,
            })
            .collect();
        inside.extend(branches.iter().map(|b| b.0));
        // A stray closer, which recovery keeps as a block of its own.
        if d.starts_with("{/") {
            out.push(malformed(span, format!("`{d}` closes no block")));
            continue;
        }
        let name: String = d
            .trim_start_matches("{#")
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !matches!(name.as_str(), "if" | "each" | "match") {
            out.push(malformed(
                span,
                format!(
                    "`{d}` is not a template block: the blocks are `{{#if}}`, `{{#each}}` \
                     and `{{#match}}`"
                ),
            ));
            continue;
        }
        let closer = close
            .trim()
            .trim_start_matches("{/")
            .trim_end_matches('}')
            .trim();
        if close.is_empty() {
            out.push(malformed(
                span.clone(),
                format!("`{{#{name}}}` is never closed"),
            ));
        } else if closer != name {
            out.push(malformed(
                span.clone(),
                format!(
                    "`{{#{name}}}` closes with `{}`, not `{{/{name}}}`",
                    close.trim()
                ),
            ));
        }
        match name.as_str() {
            "each" => {
                if let Some((b, marker, ..)) = branches.first() {
                    out.push(malformed(
                        body.node_span(*b),
                        format!(
                            "`{{#each}}` takes no `{marker}`; `{{#if xs}}` around the list \
                             says what renders when it is empty"
                        ),
                    ));
                }
            }
            "if" => {
                // **A case is tested with `{#match}`** (ADR-0153): each
                // condition, the block's and each `{:else if}`'s.
                let conditions =
                    subject
                        .iter()
                        .copied()
                        .chain(branches.iter().filter_map(|(b, ..)| match body.node(*b) {
                            Node::Branch { condition, .. } => *condition,
                            _ => None,
                        }));
                for c in conditions {
                    if let Some((value, case)) =
                        case_test(body, sigs, &types, sigs.unit_of(hir.module_of(id)), c)
                    {
                        out.push(case_tested_by_if(body, c, &value, &case, related()));
                    }
                }
                for (i, (b, marker, else_if, _)) in branches.iter().enumerate() {
                    let last = i + 1 == branches.len();
                    if !(*else_if || (marker == "{:else}" && last)) {
                        out.push(malformed(
                            body.node_span(*b),
                            format!(
                                "an `{{#if}}` takes `{{:else if c}}` markers and one \
                                 `{{:else}}`, last; not `{marker}` here"
                            ),
                        ));
                    }
                }
                // An `Option` or a `Result` is taken apart, not tested.
                if let Some(s) = subject
                    && let Some(ty) = types.of(body, *s)
                    && matches!(ty.as_builtin(), Some(Builtin::Option | Builtin::Result))
                {
                    let written = crate::infer::path_of(body, *s);
                    out.push(Diagnostic {
                        code: crate::codes::OPTION_USED_AS_VALUE.id,
                        invariant: crate::codes::OPTION_USED_AS_VALUE.invariant,
                        reason: "optional_tested_by_if",
                        detector: Detector::PatternMatrix,
                        severity: Severity::Error,
                        message: format!(
                            "`{{#if {written}}}` tests a value that may be absent; \
                             `{{#match {written}}}` takes it apart"
                        ),
                        primary_span: span.clone(),
                        related: related(),
                        explanation: None,
                        repairs: vec![Repair {
                            description: format!(
                                "write `{{#match {written}}}{{:Some(x)}} .. {{:None}} .. {{/match}}`"
                            ),
                            replacement: None,
                        }],
                    });
                }
            }
            "match" => {
                // `{#match}`: nothing before the first arm.
                let stray = children
                    .iter()
                    .take_while(|c| !matches!(body.node(**c), Node::Branch { .. }))
                    .any(|c| !matches!(body.node(*c), Node::Text(t) if t.trim().is_empty()));
                if stray {
                    out.push(malformed(
                        span.clone(),
                        "a `{#match}` holds only its arms: nothing may come before the first"
                            .to_string(),
                    ));
                }
                // What the subject is, and its cases with each one's field
                // count: the language's `Option` or `Result`, or a declared
                // sum type (ADR-0061). A subject of unknown type takes its
                // family from its first arm, if that arm is the language's.
                // A subject's family: its name, each case with its field
                // count, and the declaration of a declared one.
                type Family = (String, Vec<(String, usize)>, Option<crate::resolve::DefId>);
                let builtin = |family: &str| -> Family {
                    let cases = match family {
                        "Option" => vec![("Some".to_string(), 1), ("None".to_string(), 0)],
                        _ => vec![("Ok".to_string(), 1), ("Err".to_string(), 1)],
                    };
                    (family.to_string(), cases, None)
                };
                let family_of = |case: &str| match case {
                    "Some" | "None" => Some("Option"),
                    "Ok" | "Err" => Some("Result"),
                    _ => None,
                };
                let subject_ty = subject.and_then(|s| types.of(body, s));
                let mut cases: Option<Family> = match &subject_ty {
                    Some(ty) => match ty.as_builtin() {
                        Some(Builtin::Option) => Some(builtin("Option")),
                        Some(Builtin::Result) => Some(builtin("Result")),
                        _ => {
                            let declared = ty.def_id().and_then(|d| {
                                let vs = sigs.type_decl(d)?.variants.as_ref()?;
                                Some((d, vs))
                            });
                            let Some((def, vs)) = declared else {
                                out.push(malformed(
                                    span.clone(),
                                    format!(
                                        "a `{{#match}}` takes an `Option`, a `Result` or a \
                                             sum type apart, and its subject is `{ty}`"
                                    ),
                                ));
                                continue;
                            };
                            Some((
                                ty.to_string(),
                                vs.iter().map(|(n, fs)| (n.clone(), fs.len())).collect(),
                                Some(def),
                            ))
                        }
                    },
                    None => None,
                };
                let unit = sigs.unit_of(hir.module_of(id));
                let mut seen: Vec<String> = Vec::new();
                for (b, marker, _, arm) in &branches {
                    let Some(arm) = arm else {
                        out.push(malformed(
                            body.node_span(*b),
                            format!(
                                "`{marker}` is not an arm: a `{{#match}}` arm is a case, \
                                 `{{:Some(x)}}`, `{{:None}}` or a declared case like \
                                 `{{:Circle(r)}}`"
                            ),
                        ));
                        continue;
                    };
                    let short = arm.short().to_string();
                    if cases.is_none() {
                        match family_of(&short) {
                            Some(f) => cases = Some(builtin(f)),
                            None => {
                                out.push(malformed(
                                    body.node_span(*b),
                                    format!(
                                        "`{short}` is a declared type's case, and the \
                                         subject's type is not known here"
                                    ),
                                ));
                                continue;
                            }
                        }
                    }
                    let Some((ty_name, declared, def)) = cases.as_ref() else {
                        continue;
                    };
                    let not_a_case = |why: &str| Diagnostic {
                        code: crate::codes::PATTERN_CONSTRUCTOR.id,
                        invariant: crate::codes::PATTERN_CONSTRUCTOR.invariant,
                        reason: "pattern_names_a_constructor_its_type_lacks",
                        detector: Detector::PatternMatrix,
                        severity: Severity::Error,
                        message: format!("`{why}` is not a constructor of `{ty_name}`"),
                        primary_span: body.node_span(*b),
                        related: related(),
                        explanation: None,
                        repairs: vec![Repair {
                            description: format!(
                                "`{ty_name}`'s constructors are {}",
                                declared
                                    .iter()
                                    .map(|(c, _)| format!("`{c}`"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            replacement: None,
                        }],
                    };
                    // `{:Shape.Circle(r)}`: the qualifier names the type
                    // matched.
                    if let Some((q, _)) = arm.case.rsplit_once('.') {
                        use crate::resolve::{Namespace, Resolution};
                        let named = unit.map(|u| match q.contains('.') {
                            true => sigs.workspace().resolve_path_in(u, Namespace::Type, q),
                            false => sigs.workspace().resolve_in(u, Namespace::Type, q),
                        });
                        let same = matches!(
                            (named, def),
                            (Some(Resolution::Local(d) | Resolution::Imported { def: d, .. }), Some(t))
                                if d == *t
                        );
                        if !same {
                            out.push(not_a_case(&arm.case));
                            continue;
                        }
                    }
                    let Some((_, fields)) = declared.iter().find(|(c, _)| *c == short) else {
                        out.push(not_a_case(&short));
                        continue;
                    };
                    // A payload's fields, bound all or not at all.
                    if !arm.bindings.is_empty() && arm.bindings.len() != *fields {
                        out.push(Diagnostic {
                            code: crate::codes::CONSTRUCTOR_ARITY.id,
                            invariant: crate::codes::CONSTRUCTOR_ARITY.invariant,
                            reason: "constructor_pattern_arity_mismatch",
                            detector: Detector::PatternMatrix,
                            severity: Severity::Error,
                            message: format!(
                                "`{short}` binds {} field(s) but declares {fields}",
                                arm.bindings.len()
                            ),
                            primary_span: body.node_span(*b),
                            related: related(),
                            explanation: None,
                            repairs: vec![Repair {
                                description: format!(
                                    "bind each of `{short}`'s {fields} field(s), or none"
                                ),
                                replacement: None,
                            }],
                        });
                    }
                    // A declared case named like the language's own reaches a
                    // template as that name, which is the language's.
                    if def.is_some() && family_of(&short).is_some() {
                        out.push(malformed(
                            body.node_span(*b),
                            format!(
                                "`{short}` is a declared case named like the language's own, \
                                 which a template cannot tell apart"
                            ),
                        ));
                    }
                    if seen.contains(&short) {
                        out.push(malformed(
                            body.node_span(*b),
                            format!("a second `{{:{short}}}` arm can never render"),
                        ));
                    }
                    seen.push(short);
                }
                let Some((_, declared, _)) = &cases else {
                    continue;
                };
                let missing: Vec<&str> = declared
                    .iter()
                    .map(|(c, _)| c.as_str())
                    .filter(|c| !seen.iter().any(|s| s == c))
                    .collect();
                if !missing.is_empty() {
                    out.push(Diagnostic {
                        code: "PW0305",
                        invariant: "a match must cover every value its scrutinee can take",
                        reason: "non_exhaustive_match",
                        detector: Detector::PatternMatrix,
                        severity: Severity::Error,
                        message: format!("`{d}` does not cover `{}`", missing.join("` or `")),
                        primary_span: span.clone(),
                        related: related(),
                        explanation: Some(
                            "A template match is exhaustive, as every match is (ADR-0011): \
                             an arm that renders nothing is written, not implied."
                                .to_string(),
                        ),
                        repairs: missing
                            .iter()
                            .map(|m| Repair {
                                description: format!("add a `{{:{m}}}` arm"),
                                replacement: None,
                            })
                            .collect(),
                    });
                }
            }
            // Any other directive was refused above.
            _ => {}
        }
    }
    // A marker no block holds.
    for n in body.walk_markup(&roots) {
        if let Node::Branch { marker, .. } = body.node(n)
            && !inside.contains(&n)
        {
            out.push(malformed(
                body.node_span(n),
                format!("`{}` stands outside any block", marker.trim()),
            ));
        }
    }
}

fn markup_rules(hir: &Hir, decl: &Decl, out: &mut Vec<Diagnostic>) {
    let Some(body_id) = decl.body else { return };
    let body = hir.body(body_id);

    // The escape hatch must justify itself (charter §14 M5 task 6).
    //
    // A declaration that declares an audited `unsafe capability … because "…"`
    // covers the uses of it inside: A-024 justifies the capability once and
    // then calls `unsafe.imperative` twice, and demanding a fresh string at
    // every use would make the justification boilerplate rather than a reason.
    // A complete audit record is BOTH halves: a reason, and an owner answerable
    // for the consequence. A reason nobody is attributed for is a comment.
    let audited = body.walk().iter().any(|id| {
        matches!(
            body.expr(*id),
            Expr::Keyword {
                keyword,
                justification: Some(_),
                attribution: Some(_),
                ..
            } if keyword.starts_with("unsafe")
        )
    });
    for id in body.walk() {
        let Expr::Keyword {
            keyword,
            justification,
            attribution,
            ..
        } = body.expr(id)
        else {
            continue;
        };
        if !keyword.starts_with("unsafe") {
            continue;
        }
        // An attribution naming a declaration nobody can find is worse than
        // none: it reads as reviewed. Separate invariant, separate code —
        // and checked BEFORE the `audited` short-circuit, because a record
        // with both halves present is exactly what that short-circuit treats
        // as complete.
        if let Some(target) = attribution {
            let resolves = hir.all_decls().any(|(_, d)| &d.name == target) || decl.name == *target;
            if justification.is_some() && !resolves {
                out.push(Diagnostic {
                    code: crate::codes::UNSAFE_ATTRIBUTION_INVALID.id,
                    invariant: crate::codes::UNSAFE_ATTRIBUTION_INVALID.invariant,
                    reason: "attribution_target_unresolved",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message: format!(
                        "`{keyword}` is attributed to `{target}`, which names no declaration"
                    ),
                    primary_span: body.expr_span(id),
                    related: vec![Related::new(
                        hir.decl_span(decl_id_of(hir, decl)),
                        format!("`{}` opens the escape hatch here", decl.name),
                    )],
                    explanation: Some(
                        "An attribution target names who is answerable for the \
                         consequence. One that resolves to nothing reads as reviewed \
                         and is not — which is worse than an escape hatch with no \
                         attribution at all."
                            .to_string(),
                    ),
                    repairs: vec![Repair {
                        description: "name a declaration in this package, or an ADR".to_string(),
                        replacement: None,
                    }],
                });
                continue;
            }
        }
        if audited {
            continue;
        }
        let missing: Vec<&str> = [
            justification
                .is_none()
                .then_some("a `because` justification"),
            attribution.is_none().then_some("an attribution target"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if missing.is_empty() {
            continue;
        }
        out.push(Diagnostic {
            code: crate::codes::UNSAFE_AUDIT_INCOMPLETE.id,
            invariant: crate::codes::UNSAFE_AUDIT_INCOMPLETE.invariant,
            reason: "unsafe_audit_incomplete",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!("`{keyword}` requires {}", missing.join(" and ")),
            primary_span: body.expr_span(id),
            related: vec![Related::new(
                hir.decl_span(decl_id_of(hir, decl)),
                format!("`{}` opens an escape hatch here", decl.name),
            )],
            explanation: Some(
                "An escape hatch is a promise that the compiler's rule is wrong here \
                 and someone checked. The audit record has two halves: a written \
                 reason, so there is something to review and something to delete when \
                 the reason stops being true, and an attribution target, so the cost \
                 lands on a named owner rather than on the frame at large."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: format!(
                    "write `{keyword} because \"…\" attributes_forced_layout_to Owner`"
                ),
                replacement: None,
            }],
        });
    }

    // Everything else is about the element tree.
    let mut roots: Vec<crate::hir::NodeId> = Vec::new();
    for id in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(id) {
            roots.extend(r.iter().copied());
        }
    }

    // What names each form control, read once over the whole markup (ADR-0143).
    let order = body.walk_markup(&roots);
    let named = named_controls(body, &order);

    for &id in &order {
        match body.node(id) {
            // A list without a key cannot preserve identity across a reorder.
            // By the head's key (ADR-0242): a `(` anywhere in the directive
            // was a key, so a list computed by a call needed none.
            Node::Block {
                each: Some(each), ..
            } => {
                if each.key.is_some() {
                    continue;
                }
                out.push(Diagnostic {
                    code: "PW5011",
                    invariant: "a list over a mutable collection needs a stable key",
                    reason: "unkeyed_each",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message: "`#each` over a mutable collection requires a key expression"
                        .to_string(),
                    primary_span: body.node_span(id),
                    related: vec![Related::new(
                        hir.decl_span(decl_id_of(hir, decl)),
                        format!("`{}` renders this list", decl.name),
                    )],
                    explanation: Some(
                        "Without a stable key, reordering cannot preserve element identity, \
                         focus, or state: the renderer matches by position, so the third row \
                         keeps the second row's open menu and cursor."
                            .to_string(),
                    ),
                    repairs: vec![Repair {
                        description: "add a key: `{#each items as item (item.id)}`".to_string(),
                        replacement: None,
                    }],
                });
            }

            Node::Element { tag, attrs, .. } => {
                // Invalid nesting is `nesting`'s, which sees what a view
                // renders (ADR-0204).

                // Interactive behaviour on a non-interactive element. A
                // handler `bind:value` wrote is PW5304's (ADR-0142).
                let handler = attrs.iter().find(|a| {
                    a.name.starts_with("on:")
                        && !matches!(a.value, AttrValue::Expr(e) if body.bound.contains(&e))
                });
                // `<form on:submit>` is the normal way to submit a form, and
                // omitting `form` here made R-022 — a handler *type* mismatch —
                // report as an accessibility defect instead. Right file, wrong
                // rule, and the count would have looked one better than it was.
                let interactive = matches!(
                    tag.as_str(),
                    "button"
                        | "a"
                        | "input"
                        | "select"
                        | "textarea"
                        | "summary"
                        | "label"
                        | "option"
                        | "form"
                        | "details"
                        | "dialog"
                );
                let has_role = attrs.iter().any(|a| a.name == "role");
                let has_tabindex = attrs.iter().any(|a| a.name == "tabindex");
                if let Some(h) = handler.filter(|_| !interactive && !(has_role && has_tabindex)) {
                    {
                        out.push(Diagnostic {
                            code: "PW5013",
                            invariant: "interactive behaviour belongs on an element that can \
                                        receive it",
                            reason: "handler_on_inert_element",
                            detector: Detector::DeclarationRule,
                            severity: Severity::Error,
                            message: format!(
                                "`<{tag}>` with an `{}` handler is not keyboard accessible",
                                h.name
                            ),
                            primary_span: h.span.clone(),
                            related: vec![Related::new(
                                body.node_span(id),
                                format!("`<{tag}>` is not an interactive element"),
                            )],
                            explanation: Some(
                                "A pointer handler on an inert element is unreachable by \
                                 keyboard and invisible to assistive technology. The element \
                                 looks like a control and is not one."
                                    .to_string(),
                            ),
                            repairs: vec![
                                Repair {
                                    description: "use `<button>`, which is focusable and \
                                                  activates on Enter and Space"
                                        .to_string(),
                                    replacement: None,
                                },
                                Repair {
                                    description: "or supply `role`, `tabindex` and keyboard \
                                                  activation"
                                        .to_string(),
                                    replacement: None,
                                },
                            ],
                        });
                    }
                }

                // An image with nothing to stand for it (PW5053).
                if tag.eq_ignore_ascii_case("img") && !has_alt(attrs) {
                    out.push(image_without_alt(hir, decl, body, id));
                }

                // An image button with no text to name it (PW5053,
                // ADR-0405): its `alt` is its label, so HTML requires
                // one, and not empty.
                if tag.eq_ignore_ascii_case("input")
                    && input_type(attrs).as_deref() == Some("image")
                    && names_with(attrs, "alt") != Some(true)
                {
                    out.push(image_button_without_alt(hir, decl, body, id, attrs));
                }

                // A form control with nothing naming it.
                if ["input", "select", "textarea"]
                    .iter()
                    .any(|t| tag.eq_ignore_ascii_case(t))
                {
                    // `type` is an `<input>`'s alone.
                    let typ = tag
                        .eq_ignore_ascii_case("input")
                        .then(|| input_type(attrs))
                        .flatten();
                    // A `hidden` input is no control. An image button is
                    // named by its `alt`, which PW5053 holds to text above:
                    // the one defect is reported once (ADR-0405).
                    if matches!(typ.as_deref(), Some("hidden" | "image")) {
                        continue;
                    }
                    if named.contains(&id) {
                        continue;
                    }
                    // Named by a reference to ids the declaration does not
                    // have: the reference is the defect, PW5032's, and the
                    // control is not reported twice (ADR-0185).
                    if labelled_by_nothing(body, &order, attrs) {
                        continue;
                    }
                    // Its `id` is two elements', or every row's, so a
                    // `<label for>` names another: the id is the defect,
                    // PW5031's (ADR-0185).
                    if labelled_but_for_its_id(body, &order, id, attrs) {
                        continue;
                    }
                    let mut related = vec![Related::new(
                        hir.decl_span(decl_id_of(hir, decl)),
                        format!("`{}` renders this control", decl.name),
                    )];
                    related.extend(unnamed_because(body, &order, id, &decl.name));
                    // A button's label is its `value`, and the repair is one
                    // (ADR-0405).
                    let button = matches!(typ.as_deref(), Some("submit" | "reset" | "button"));
                    out.push(Diagnostic {
                        code: "PW5014",
                        invariant: "a form control must have something that names it",
                        reason: if button {
                            "button_without_label"
                        } else {
                            "control_without_label"
                        },
                        detector: Detector::DeclarationRule,
                        severity: Severity::Error,
                        message: match typ.as_deref() {
                            // HTML's word for a button's text is its label.
                            Some(t) if button => format!("`<input type=\"{t}\">` has no label"),
                            _ => format!("`<{tag}>` has no associated label"),
                        },
                        primary_span: body.node_span(id),
                        related,
                        explanation: Some(
                            if button {
                                "A button with no name is announced as \"button\" and nothing \
                                 else. An `<input>` button's label is its `value`: the text it \
                                 shows and what a screen reader reads. HTML gives a `submit` or \
                                 a `reset` with no `value` a label of its own, \"Submit\" or \
                                 \"Reset\", and an empty `value` takes its place, so the button \
                                 shows nothing; a `button` input has no label but its `value`."
                            } else {
                                "A control with no accessible name is announced as \"edit \
                                 text\" and nothing else. `name` is submitted to the server; it \
                                 is not read to the user. An `id` names nothing by itself: a \
                                 label names the control when it is in the same declaration, \
                                 has text, and its `for` is the control's `id` or it wraps the \
                                 control. A placeholder is not a label: it disappears when \
                                 someone types."
                            }
                            .to_string(),
                        ),
                        repairs: if button {
                            vec![
                                Repair {
                                    description: "write `value=\"…\"` saying what the button does"
                                        .to_string(),
                                    replacement: None,
                                },
                                Repair {
                                    description: "or supply `aria-label`/`aria-labelledby`, or \
                                                  wrap the button in a `<label>` with text"
                                        .to_string(),
                                    replacement: None,
                                },
                            ]
                        } else {
                            vec![
                                Repair {
                                    description: "associate a `<label for=...>`, or supply \
                                                  `aria-label`/`aria-labelledby`"
                                        .to_string(),
                                    replacement: None,
                                },
                                Repair {
                                    description: "or wrap the control in a `<label>` with text"
                                        .to_string(),
                                    replacement: None,
                                },
                            ]
                        },
                    });
                }
            }

            _ => {}
        }
    }
}

// --- what names a form control (ADR-0143) ------------------------------------

/// **The form controls something in one declaration's own markup names**
/// (ADR-0143). It follows the HTML standard's labeled control and the
/// accessible-name computation (accname 1.2):
///
/// - a `<label for=X>` with text names the first element, in tree order,
///   whose `id` is `X`;
/// - a `<label>` with no `for`, with text, names its first labelable
///   descendant;
/// - `aria-labelledby` names its element when one of its IDREFs is an element
///   here with text;
/// - an `aria-label` names it unless it is blank. One the program computes is
///   taken as a name: whether it is empty is known only when it runs;
/// - an `<input>` button names itself, by its `value` or by the label HTML
///   gives it (`names_itself`, ADR-0405).
///
/// An `id` names nothing by itself. Until ADR-0143 one counted as a name, on
/// the assumption that a label pointed at it, and nothing checked that one
/// did. The label must be in the same declaration, so a view that renders a
/// control is accessible wherever it is used. A `for`, an `id` or an IDREF
/// the program computes is not matched.
fn named_controls(body: &Body, order: &[hir::NodeId]) -> BTreeSet<hir::NodeId> {
    let by_id = first_with_id(body, order);
    let mut named = BTreeSet::new();
    for &n in order {
        let Node::Element { tag, attrs, .. } = body.node(n) else {
            continue;
        };
        if tag.eq_ignore_ascii_case("label")
            && let Some(control) = labeled_control(body, n, &by_id)
            && has_text(body, n, control)
        {
            named.insert(control);
        }
        let labelled = names_with(attrs, "aria-label") == Some(true);
        if labelled || labelled_by(body, n, &by_id).is_some() || names_itself(tag, attrs) {
            named.insert(n);
        }
    }
    named
}

/// What an attribute that names something gives (accname 1.2, step 2C;
/// HTML's button states): `None` when it is not written, `Some(true)` when
/// it has text or the program computes it, which is taken as a name
/// (ADR-0143), and `Some(false)` when it is blank or written alone, which
/// HTML reads as `""`.
fn names_with(attrs: &[hir::Attr], name: &str) -> Option<bool> {
    let a = attrs.iter().find(|a| a.name.eq_ignore_ascii_case(name))?;
    Some(match &a.value {
        AttrValue::Static(_) => non_blank(attrs, name),
        AttrValue::Expr(_) => true,
        AttrValue::None => false,
    })
}

/// An `<input>`'s `type` written as text, lower-cased: HTML matches its
/// keywords whatever their case. `None` when it is computed, or not
/// written, which makes a text field.
fn input_type(attrs: &[hir::Attr]) -> Option<String> {
    static_attr(attrs, "type").map(str::to_ascii_lowercase)
}

/// **Whether an `<input>` button names itself** (ADR-0405; HTML
/// 4.10.5.1.18, .20 and .21; HTML-AAM 4.1.2). Its label is its `value`, when
/// that has text or the program computes it. With no `value`, a `submit` or a
/// `reset` has the label HTML gives it, "Submit" or "Reset", and a `button`
/// has the empty string. An empty `value` is the label all the same: the
/// button shows nothing, and HTML-AAM gives the default only where `value` is
/// not written. An image button is named by its `alt`, PW5053's to hold.
fn names_itself(tag: &str, attrs: &[hir::Attr]) -> bool {
    let default = match (
        tag.to_ascii_lowercase().as_str(),
        input_type(attrs).as_deref(),
    ) {
        ("input", Some("submit" | "reset")) => true,
        ("input", Some("button")) => false,
        _ => return false,
    };
    names_with(attrs, "value").unwrap_or(default)
}

/// Whether a control's `aria-labelledby` names ids no element of the
/// declaration has, and none it computes could be (ADR-0185).
fn labelled_by_nothing(body: &Body, order: &[hir::NodeId], attrs: &[hir::Attr]) -> bool {
    let Some(value) = static_attr(attrs, "aria-labelledby") else {
        return false;
    };
    if value.contains('{') || value.trim().is_empty() {
        return false;
    }
    let ids = Ids::of(body, order);
    value
        .split_ascii_whitespace()
        .all(|token| !ids.written.contains_key(token) && !ids.may_be(token))
}

/// Whether a `<label for>` names a control's `id`, and the `id` is also an
/// earlier element's, or every row's, so that the label names another
/// (ADR-0185).
fn labelled_but_for_its_id(
    body: &Body,
    order: &[hir::NodeId],
    control: hir::NodeId,
    attrs: &[hir::Attr],
) -> bool {
    let Some(id) = static_attr(attrs, "id").filter(|id| !id.contains('{')) else {
        return false;
    };
    let has = |n: hir::NodeId, name: &str| match body.node(n) {
        Node::Element { attrs, .. } => static_attr(attrs, name) == Some(id),
        _ => false,
    };
    let labelled = order.iter().any(|&n| {
        matches!(body.node(n), Node::Element { tag, .. } if tag.eq_ignore_ascii_case("label"))
            && has(n, "for")
    });
    let earlier = order
        .iter()
        .take_while(|&&n| n != control)
        .any(|&n| has(n, "id"));
    labelled && (earlier || inside_each(body, order).contains(&control))
}

// --- what stands for an image (ADR-0404) --------------------------------

/// Whether an `<img>` has an `alt`, whatever it holds: text, `""` (or the
/// attribute alone, which HTML reads as `""`), or a value's. A `title`, an
/// `aria-label` and a `role` are not one: HTML requires `alt` of every
/// `<img>`, and only `alt` is shown where the image does not load. Its
/// name is read as HTML reads it, whatever its case (`ALT`), as is the
/// element's (`<iMg>`, train-14's review).
fn has_alt(attrs: &[hir::Attr]) -> bool {
    attrs.iter().any(|a| a.name.eq_ignore_ascii_case("alt"))
}

/// **An image with no `alt`** (PW5053), reported at the element.
fn image_without_alt(hir: &Hir, decl: &Decl, body: &Body, id: hir::NodeId) -> Diagnostic {
    Diagnostic {
        code: crate::codes::IMAGE_WITHOUT_ALT.id,
        invariant: crate::codes::IMAGE_WITHOUT_ALT.invariant,
        reason: "image_without_alt",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: "`<img>` has no `alt`".to_string(),
        primary_span: body.node_span(id),
        related: vec![Related::new(
            hir.decl_span(decl_id_of(hir, decl)),
            format!("`{}` renders this image", decl.name),
        )],
        explanation: Some(
            "Nothing stands for an image with no `alt`: a screen reader announces it as an \
             image, often by its file name, and where it does not load the page shows no \
             text in its place. HTML requires `alt` of every `<img>`: what the image shows, \
             or `\"\"` for one that shows nothing a reader needs, such as decoration or what \
             the text beside it already says, which a screen reader then passes over. An \
             `aria-label` or a `title` is not shown where the image fails to load."
                .to_string(),
        ),
        repairs: vec![
            Repair {
                description: "write `alt=\"…\"` saying what the image shows".to_string(),
                replacement: None,
            },
            Repair {
                description: "or `alt=\"\"` if it shows nothing a reader needs".to_string(),
                replacement: None,
            },
        ],
    }
}

/// **An image button with no text in its `alt`** (PW5053, ADR-0405),
/// reported at the element. HTML 4.10.5.1.19: the `alt` "must be
/// present, and must contain a non-empty string". Unlike an `<img>`'s, it is
/// never empty: it is the button's label, and a button always does something
/// a reader needs. A `title`, an `aria-label` or a `<label>` does not stand
/// in for it, as for an `<img>`.
fn image_button_without_alt(
    hir: &Hir,
    decl: &Decl,
    body: &Body,
    id: hir::NodeId,
    attrs: &[hir::Attr],
) -> Diagnostic {
    let alt = attrs.iter().find(|a| a.name.eq_ignore_ascii_case("alt"));
    let mut related = vec![Related::new(
        hir.decl_span(decl_id_of(hir, decl)),
        format!("`{}` renders this button", decl.name),
    )];
    related.extend(alt.map(|a| {
        Related::new(
            a.span.clone(),
            "this `alt` is the button's label, and it is blank".to_string(),
        )
    }));
    Diagnostic {
        code: crate::codes::IMAGE_WITHOUT_ALT.id,
        invariant: crate::codes::IMAGE_WITHOUT_ALT.invariant,
        reason: "image_button_without_alt",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message: match alt {
            None => "`<input type=\"image\">` has no `alt`",
            Some(_) => "`<input type=\"image\">` has a blank `alt`",
        }
        .to_string(),
        primary_span: body.node_span(id),
        related,
        explanation: Some(
            "An image button's `alt` is its label: what a screen reader announces, and the \
             text the button shows where the image does not load. HTML requires one, and not \
             empty, saying what the button does rather than what the image shows. An `<img>` \
             may have `alt=\"\"` where it shows nothing a reader needs; a button always does \
             something a reader needs. An `aria-label` or a `title` is not shown where the \
             image fails to load."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: "write `alt=\"…\"` saying what the button does".to_string(),
            replacement: None,
        }],
    }
}

/// An attribute's value when it is written as text, its quotes removed. The
/// attribute is found by its name as HTML reads it, whatever its case:
/// HTML's tokenizer lower-cases it (13.2.5.33), so `TYPE` is `type`.
fn static_attr<'a>(attrs: &'a [hir::Attr], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(name))
        .and_then(|a| match &a.value {
            AttrValue::Static(v) => Some(v.trim_matches(|c| c == '"' || c == '\'')),
            _ => None,
        })
}

fn non_blank(attrs: &[hir::Attr], name: &str) -> bool {
    static_attr(attrs, name).is_some_and(|v| !v.trim().is_empty())
}

/// The nodes inside an `{#each}`, which render once per row.
fn inside_each(body: &Body, order: &[hir::NodeId]) -> BTreeSet<hir::NodeId> {
    let mut rows = BTreeSet::new();
    for &n in order {
        if let Node::Block {
            directive,
            children,
            ..
        } = body.node(n)
            && directive.trim_start().starts_with("{#each")
        {
            rows.extend(body.walk_markup(children));
        }
    }
    rows
}

/// The first element, in tree order, with each `id` written as text: the one
/// a `<label for>` and an IDREF name, as HTML's reference does. An `id` inside
/// an `{#each}` names the first row's element. That every row has it is the
/// defect, PW5031's (ADR-0185), and what the reference names is not reported
/// again. Until 2026-10-11 such an `id` named no element, and a control an
/// `aria-labelledby` named through one was reported too (PW5014), the one
/// defect twice (ADR-0143, amended).
fn first_with_id<'b>(body: &'b Body, order: &[hir::NodeId]) -> BTreeMap<&'b str, hir::NodeId> {
    let mut by_id = BTreeMap::new();
    for &n in order {
        if let Node::Element { attrs, .. } = body.node(n)
            && let Some(id) = static_attr(attrs, "id")
        {
            by_id.entry(id).or_insert(n);
        }
    }
    by_id
}

/// Is an element labelable (HTML, "labelable elements")? `None` for a
/// composed view or a custom element, whose markup this declaration does not
/// see.
fn labelable(tag: &str, attrs: &[hir::Attr]) -> Option<bool> {
    // No HTML element's name has a capital, a `.` or a `-` in it.
    if tag.contains(['.', '-']) || tag.starts_with(|c: char| c.is_ascii_uppercase()) {
        return None;
    }
    Some(match tag.to_ascii_lowercase().as_str() {
        "input" => !static_attr(attrs, "type").is_some_and(|t| t.eq_ignore_ascii_case("hidden")),
        "button" | "meter" | "output" | "progress" | "select" | "textarea" => true,
        _ => false,
    })
}

/// A `<label>`'s labeled control (HTML, "the label element"). With `for`, it
/// is the first element whose `id` that is, when that one is labelable.
/// Without, it is the first labelable descendant. `None` when it names
/// nothing, or when what it names is not known here.
fn labeled_control(
    body: &Body,
    label: hir::NodeId,
    by_id: &BTreeMap<&str, hir::NodeId>,
) -> Option<hir::NodeId> {
    let Node::Element {
        attrs, children, ..
    } = body.node(label)
    else {
        return None;
    };
    if attrs.iter().any(|a| a.name.eq_ignore_ascii_case("for")) {
        let target = *by_id.get(static_attr(attrs, "for")?)?;
        let Node::Element { tag, attrs, .. } = body.node(target) else {
            return None;
        };
        return (labelable(tag, attrs) == Some(true)).then_some(target);
    }
    first_labelable(body, children)
}

/// The first labelable element under `children`, in tree order. `None` when
/// there is none, or when which is first depends on what renders: one inside
/// a block, or a composed view's markup.
fn first_labelable(body: &Body, children: &[hir::NodeId]) -> Option<hir::NodeId> {
    // `Err` when which one is first is not known here.
    fn walk(body: &Body, n: hir::NodeId, in_block: bool) -> Result<Option<hir::NodeId>, ()> {
        let (children, in_block) = match body.node(n) {
            Node::Element {
                tag,
                attrs,
                children,
                ..
            } => match labelable(tag, attrs) {
                Some(true) if !in_block => return Ok(Some(n)),
                Some(true) | None => return Err(()),
                Some(false) => (children, in_block),
            },
            Node::Block { children, .. } => (children, true),
            _ => return Ok(None),
        };
        for &c in children {
            if let Some(found) = walk(body, c, in_block)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
    for &c in children {
        match walk(body, c, false) {
            Ok(None) => {}
            Ok(found) => return found,
            Err(()) => return None,
        }
    }
    None
}

/// Does `n` hold text a name can be read from (accname 1.2, steps 2C to 2G)?
/// That is a non-blank text node or an interpolation inside it, or a
/// descendant's non-blank `aria-label` or `alt`. A hidden descendant gives
/// none, and `skip`'s subtree is left out: a control is not named by its own
/// options.
fn has_text(body: &Body, n: hir::NodeId, skip: hir::NodeId) -> bool {
    let children = match body.node(n) {
        Node::Element { children, .. } | Node::Block { children, .. } => children,
        _ => return false,
    };
    children.iter().any(|&c| {
        c != skip
            && match body.node(c) {
                Node::Text(t) => !t.trim().is_empty(),
                Node::Interpolation(_) => true,
                Node::Element { attrs, .. } => {
                    let hidden = attrs.iter().any(|a| a.name.eq_ignore_ascii_case("hidden"))
                        || static_attr(attrs, "aria-hidden") == Some("true");
                    !hidden
                        && (non_blank(attrs, "aria-label")
                            || non_blank(attrs, "alt")
                            || has_text(body, c, skip))
                }
                Node::Block { .. } => has_text(body, c, skip),
                Node::Branch { .. } => false,
            }
    })
}

/// The element `aria-labelledby` names a control by: the first of its IDREFs
/// that is an element here with text. IDREFs the program computes are not
/// known here.
fn labelled_by(
    body: &Body,
    n: hir::NodeId,
    by_id: &BTreeMap<&str, hir::NodeId>,
) -> Option<hir::NodeId> {
    let Node::Element { attrs, .. } = body.node(n) else {
        return None;
    };
    static_attr(attrs, "aria-labelledby")?
        .split_ascii_whitespace()
        .filter_map(|r| by_id.get(r).copied())
        .find(|&t| {
            t != n
                && match body.node(t) {
                    Node::Element { attrs, .. } => {
                        non_blank(attrs, "aria-label") || has_text(body, t, n)
                    }
                    _ => false,
                }
        })
}

/// Why nothing names a control, when its markup says: the note PW5014 adds,
/// so the repair is the right one.
fn unnamed_because(
    body: &Body,
    order: &[hir::NodeId],
    control: hir::NodeId,
    decl: &str,
) -> Option<Related> {
    let Node::Element { tag, attrs, .. } = body.node(control) else {
        return None;
    };
    let attr = |k: &str| attrs.iter().find(|a| a.name == k);
    let note = |a: &hir::Attr, label: String| Some(Related::new(a.span.clone(), label));
    if let Some(a) = attr("aria-label") {
        return note(a, "this `aria-label` is blank".to_string());
    }
    if let Some(a) = attr("aria-labelledby") {
        return note(
            a,
            match &a.value {
                AttrValue::Static(_) => {
                    format!("no element in `{decl}` with text has one of these ids")
                }
                _ => "these ids are computed, so what they name is not known here".to_string(),
            },
        );
    }
    // A button whose label is its `value`, and gives none (ADR-0405).
    // Only a `button` input with no `value` has none to give.
    if tag.eq_ignore_ascii_case("input")
        && let Some(t) = input_type(attrs)
        && matches!(t.as_str(), "submit" | "reset" | "button")
    {
        if let Some(a) = attr("value") {
            return note(
                a,
                "this `value` is the button's label, and it is blank".to_string(),
            );
        }
        if let Some(a) = attr("type") {
            return note(
                a,
                format!("an `<input type=\"{t}\">` is named by its `value`, and this one has none"),
            );
        }
    }
    // A label that wraps the control and does not name it.
    let by_id = first_with_id(body, order);
    for &l in order {
        let Node::Element {
            tag,
            attrs: label,
            children,
            ..
        } = body.node(l)
        else {
            continue;
        };
        if tag != "label" || !body.walk_markup(children).contains(&control) {
            continue;
        }
        let why = match labeled_control(body, l, &by_id) {
            Some(c) if c == control => "this `<label>` has no text",
            Some(_) if label.iter().any(|a| a.name.eq_ignore_ascii_case("for")) => {
                "this `<label>` has a `for`, so it names that element, not what it wraps"
            }
            Some(_) => "this `<label>` names the first control inside it, which is another",
            None => "which control this `<label>` names depends on what renders inside it",
        };
        return Some(Related::new(body.node_span(l), why.to_string()));
    }
    let a = attr("id")?;
    let Some(id) = static_attr(attrs, "id") else {
        return note(
            a,
            "this `id` is computed, so no `<label for>` can be matched to it".to_string(),
        );
    };
    if inside_each(body, order).contains(&control) {
        return note(
            a,
            format!(
                "every row of the `{{#each}}` has `id=\"{id}\"`, so a `<label for>` names \
                 only the first: wrap the control in its `<label>`"
            ),
        );
    }
    if by_id.get(id) != Some(&control) {
        return note(
            a,
            format!("an earlier element has `id=\"{id}\"`, and a `<label for>` names that one"),
        );
    }
    let empty = order.iter().copied().find(|&l| {
        matches!(body.node(l), Node::Element { tag, attrs, .. }
            if tag.eq_ignore_ascii_case("label") && static_attr(attrs, "for") == Some(id))
    });
    match empty {
        Some(l) => Some(Related::new(
            body.node_span(l),
            format!("this `<label for=\"{id}\">` has no text"),
        )),
        None => note(
            a,
            format!("no `<label for=\"{id}\">` in `{decl}` names this `id`"),
        ),
    }
}

// --- effect rows (E2D) ------------------------------------------------------

/// Does a declaration do what its effect row says?
///
/// Two questions, two codes. An **undeclared** effect is a row that understates
/// what the body does. A **forbidden** effect is one that no row could permit
/// here — a `view` reaching the database is not fixed by declaring it.
#[allow(clippy::too_many_arguments)]
fn effect_rows(
    hir: &Hir,
    sigs: &Signatures,
    inference: &crate::effects::Inference<'_>,
    // Phase legality is decided on an effect's semantic FACETS, which only its
    // declaration knows — see `ontology::Facet`.
    ontology: &crate::ontology::Ontology,
    ws: &crate::resolve::Workspace,
    at: usize,
    id: crate::hir::DeclId,
    decl: &Decl,
    out: &mut Vec<Diagnostic>,
) {
    use crate::effects::{Reuse, forbidden_in, forbidden_in_phase};

    // Interface clauses have their own term arena too: an unknown member
    // in an optimistic transition must be refused even without a body.
    let Some(body_id) = decl.terms_body() else {
        return;
    };
    let body = hir.body(body_id);
    // The SAME type environment the inference built, not a narrower one
    // assembled here.
    //
    // It used to be declaration parameters plus annotated `let`s, and a lambda
    // parameter — having no annotation — fell through to a by-spelling rule
    // that the architect ordered deleted on 2026-08-07. `Types::of_body` types
    // a callback's parameter from the collection it is applied to, so `el` in
    // `items |> List.map(fn(el) ..)` is an `ElementRef` here and
    // `el.getBoundingClientRect()` resolves through its RECEIVER.
    //
    // Two constructions of one environment is how the narrower one silently
    // wins, which is exactly what happened.
    let types = crate::infer::Types::of_decl(sigs, hir, id, body);
    let mut found = inference.infer_in_at(at, body, &types);

    // **A member read through a value whose type nothing states** (ADR-0351,
    // the generic-effects track): which member it is, what it performs and
    // what it carries are each read through the receiver's type, so neither
    // this row nor any caller's can be checked over it. Until then each
    // analysis skipped it unseen: `let f = (x) => x.label`, `label` reading
    // the database, held a `!{}` row.
    for read in types.untyped_receivers(body) {
        let Expr::Field { base, name } = body.expr(read) else {
            continue;
        };
        let receiver = match crate::infer::path_of(body, *base) {
            p if p.is_empty() => "this value".to_string(),
            p => format!("`{p}`"),
        };
        out.push(Diagnostic {
            code: crate::codes::MEMBER_RECEIVER_UNTYPED.id,
            invariant: crate::codes::MEMBER_RECEIVER_UNTYPED.invariant,
            reason: "member_receiver_untyped",
            detector: Detector::Signature,
            severity: Severity::Error,
            message: format!(
                "the type of {receiver} is not known here, so what `.{name}` reads cannot be checked"
            ),
            primary_span: body.expr_span(read),
            related: vec![Related::new(
                body.expr_span(*base),
                "no declaration gives this value a type, and no call solves one".to_string(),
            )],
            explanation: Some(format!(
                "A member read is a call (ADR-0048), found by its receiver's type: which \
                 `{name}` this is, the effects it performs and the label of what it gives are \
                 each read through that type, and `{}` cannot be checked against a read no \
                 analysis can see.",
                decl.name
            )),
            repairs: vec![Repair {
                description: format!(
                    "write {receiver}'s type where it is bound, as `(x: Item) => ..` does for \
                     a lambda's parameter"
                ),
                replacement: None,
            }],
        });
    }

    // **Plus the named roots that ARE this declaration's work.**
    //
    // A painter's `draw` block and a resource's `acquire`/`release` blocks
    // became named roots on 2026-08-11 so their headers could bind. They are
    // not reachable from `body.root`, and every rule below reads `found` — so
    // without this, `R-041`'s `dom.mutate` inside a painter stopped being seen
    // by the rule whose whole subject it is.
    //
    // An optimistic clause is excluded by `contributes_to_declaration`: it runs
    // on the client at a different time.
    for (_, root) in decl.term_roots() {
        if !root.context.contributes_to_declaration() {
            continue;
        }
        found
            .sources
            .extend(inference.infer_rooted(at, body, root.root).sources);
    }

    // Work that runs somewhere else does not contribute its effects here.
    // ONE model (`contexts.rs`) rather than a list of syntax exceptions: a
    // query dependency, a named declaration, a streamed region, a handler and
    // a later frame phase are the same fact seen six ways.
    let regions = crate::contexts::elsewhere(body);
    // A value the template computes performs nothing, wherever it is
    // rendered (below).
    let every = found.sources.clone();
    found
        .sources
        .retain(|s| crate::contexts::effects_belong_here(&regions, &s.span));
    found.effects = found.sources.iter().map(|s| s.effect.clone()).collect();
    let found = found;
    // Work inside an event handler, a streamed region or a later frame phase is
    // not done *during render*, so the render-time restriction does not apply
    // to it. Charter §7.5A — and it is the same question `regions` already
    // answers, asked from the other side.
    let at_render_time = |span: &crate::hir::Span| crate::contexts::at_render_time(&regions, span);

    // Forbidden first: it is the stronger statement, and reporting both for one
    // call would say the same thing twice.
    let mut reported: BTreeSet<String> = BTreeSet::new();

    // The frame phase an effect happens in decides what it may do, and that is
    // an ordering question rather than a question of which effects exist.
    for source in &found.sources {
        // EVERY enclosing phase, not just the innermost. A `measure { .. }`
        // opened inside `post_paint { .. }` is still in the post-paint frame,
        // and R-042 is exactly that program — caught until now only because a
        // synthesized effect happened to be recorded at the outer phase's span.
        let Some((phase, why)) = crate::effects::phases_at(body, &source.span)
            .into_iter()
            .find_map(|p| {
                forbidden_in_phase(&p, &ontology.facets_of(ws, at, &source.effect)).map(|w| (p, w))
            })
        else {
            continue;
        };
        if !reported.insert(source.effect.clone()) {
            continue;
        }
        out.push(Diagnostic {
            code: crate::codes::WRONG_FRAME_PHASE.id,
            invariant: crate::codes::WRONG_FRAME_PHASE.invariant,
            reason: "effect_in_wrong_phase",
            detector: Detector::PatternMatrix,
            severity: Severity::Error,
            message: format!(
                "`{}` happens in the `{phase}` phase, which may not do it",
                source.effect
            ),
            primary_span: source.span.clone(),
            related: vec![Related::new(
                hir.decl_span(decl_id_of(hir, decl)),
                format!("`{}` declares this frame", decl.name),
            )],
            explanation: Some(format!("{why}. The chain: {}.", source.via.describe())),
            repairs: vec![Repair {
                description: "move it to the phase that owns it — read in `measure`, \
                              write in `mutate`"
                    .to_string(),
                replacement: None,
            }],
        });
    }

    // Charter §7.9, §9.4: is this declaration's output computed once and
    // reused, or recomputed for its reader? A wall-clock read is only a defect
    // in the first case.
    let reuse = reuse_of(hir, decl);

    for source in &found.sources {
        let Some(why) = forbidden_in(
            decl,
            reuse,
            declared_world(hir, decl),
            ontology,
            &source.effect,
        ) else {
            continue;
        };
        if !at_render_time(&source.span) {
            continue;
        }
        if !reported.insert(source.effect.clone()) {
            continue;
        }
        out.push(Diagnostic {
            code: crate::codes::FORBIDDEN_EFFECT.id,
            invariant: crate::codes::FORBIDDEN_EFFECT.invariant,
            reason: "effect_forbidden_in_context",
            detector: Detector::PatternMatrix,
            severity: Severity::Error,
            message: match reuse {
                // Named for what makes it wrong. "which a page may not do" is
                // false — a page may read the clock; a page generated once at
                // build time may not.
                Reuse::Build => format!(
                    "a `Build`-placed {} may not use effect `{}`",
                    kind_noun(decl.kind),
                    source.effect
                ),
                Reuse::SharedPartition => format!(
                    "materialization `{}` may not depend on `{}`",
                    decl.name, source.effect
                ),
                Reuse::PerReader => format!(
                    "`{}` performs `{}`, which a {} may not do",
                    decl.name,
                    source.effect,
                    context_noun(decl)
                ),
            },
            primary_span: source.span.clone(),
            related: vec![Related::new(
                hir.decl_span(decl_id_of(hir, decl)),
                format!("`{}` is a {}", decl.name, context_noun(decl)),
            )],
            explanation: Some(format!(
                "{why}. The chain: {}. Declaring the effect would not help — the \
                 restriction is about where this runs, not about what it admits to.",
                source.via.describe()
            )),
            repairs: vec![Repair {
                description: "move the work into a query or command and pass its \
                              result in"
                    .to_string(),
                replacement: None,
            }],
        });
    }

    // **A value the template computes performs no effect** (ADR-0226, ruling
    // 0073-a). The compiler lifts each into a function of the values it
    // reads, which a host runs when the page renders and again when they
    // change, so an effect in one would happen then, and not when it should.
    // In a streamed region's arm too, which renders when its query settles.
    // Not an escape hatch's, which its audit record decides (below), and
    // not one already said to be forbidden here. Not in a page generated at
    // build either: it renders once, and what it reads is the build's input,
    // which a build renders and no host computes (ADR-0226 refuses it there).
    let computed = match reuse {
        Reuse::Build => Vec::new(),
        Reuse::PerReader | Reuse::SharedPartition => crate::template_ir::computed_values(body),
    };
    for value in computed {
        let region = body.expr_span(value);
        for source in every
            .iter()
            .filter(|s| s.span.start >= region.start && s.span.end <= region.end)
        {
            if crate::effects::family_of(&source.effect) == "unsafe"
                || !reported.insert(source.effect.clone())
            {
                continue;
            }
            out.push(Diagnostic {
                code: crate::codes::DERIVED_NOT_PURE.id,
                invariant: crate::codes::DERIVED_NOT_PURE.invariant,
                reason: "computed_value_performs_effects",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!(
                    "a value `{}`'s template computes performs `{}`",
                    decl.name, source.effect
                ),
                primary_span: source.span.clone(),
                related: vec![Related::new(
                    region.clone(),
                    "the template computes this value".to_string(),
                )],
                explanation: Some(format!(
                    "A value a template computes is a function of the values it reads: the \
                     compiler lifts it out, and a host runs it when the page renders and \
                     again when they change (ADR-0226). It performs nothing, as a `derived` \
                     value does (charter §7.5). {}.",
                    source.via.describe()
                )),
                repairs: vec![Repair {
                    description: "read it in a query, and write the query's value in the \
                                  template"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
    }

    // Charter §8.2, §17.5: an escape hatch needs an audit record, and a
    // declaration that has none is the most incomplete record there is.
    //
    // The rule in `markup_rules` checks a record the author WROTE for
    // completeness. This is the other half: `raw_html` is not a name the
    // compiler recognises, it is a function whose row says `unsafe.raw_html`,
    // so declaring a new escape hatch in a platform package brings its own
    // audit requirement with it.
    if !has_audit_record(body) {
        for source in &found.sources {
            if crate::effects::family_of(&source.effect) != "unsafe"
                || !reported.insert(source.effect.clone())
            {
                continue;
            }
            out.push(Diagnostic {
                code: crate::codes::UNSAFE_AUDIT_INCOMPLETE.id,
                invariant: crate::codes::UNSAFE_AUDIT_INCOMPLETE.invariant,
                reason: "unsafe_effect_without_audit_record",
                detector: Detector::PatternMatrix,
                severity: Severity::Error,
                message: format!(
                    "`{}` requires the `{}` capability and a justification",
                    source.via.callee(),
                    source.effect
                ),
                primary_span: source.span.clone(),
                related: vec![Related::new(
                    hir.decl_span(decl_id_of(hir, decl)),
                    format!("`{}` declares no capability", decl.name),
                )],
                explanation: Some(format!(
                    "Escaping is the default, so reaching `{}` is a decision rather \
                     than a detail — untrusted input reaching `{}` is an XSS sink. \
                     {}. Adding the effect to the row would not help: the row says \
                     what happens, and the capability says who decided it was \
                     necessary and why.",
                    source.via.callee(),
                    source.via.callee(),
                    source.via.describe()
                )),
                repairs: vec![Repair {
                    description: format!(
                        "write `unsafe capability {} because \"…\" \
                         attributes_forced_layout_to Owner`, or escape the value with \
                         `html.text`",
                        source.effect
                    ),
                    replacement: None,
                }],
            });
        }
    }

    // The row as written, by path.
    let declared: Option<Vec<String>> = decl
        .declared_effects
        .as_ref()
        .map(|r| r.iter().map(|e| e.written.clone()).collect());

    // A placement the author NAMED must be able to grant what the body needs.
    //
    // `rules.rs` already checks this against the DECLARED row. That is not
    // enough on its own, and R-025 is why: its query names `placement origin`
    // and declares no row at all, so there was nothing to compare and a
    // registered rule sat silent. The effects are inferred; the placement is
    // declared; the check is the intersection.
    if let Some(world) = declared_world(hir, decl) {
        let row = declared.as_deref().unwrap_or(&[]);
        let mut said: BTreeSet<String> = BTreeSet::new();
        for source in &found.sources {
            // Leave anything the row COVERS to `rules.rs`, which reports it
            // against the row the author wrote and at that row's span.
            //
            // Covers, not equals. R-026 declares `secret<Payments>` and the
            // inferred effect is `secret.read`; an exact match reported it
            // here as well, so one defect arrived twice in two vocabularies
            // and the fixture stopped being a single-defect test.
            // `Grant::Blocked` continues too, and for a different reason than
            // `Grant::Yes`: the effect names nothing, `check_effect_rows` owns
            // that, and "not available at placement origin" would confirm a
            // typo as an effect while blaming the placement.
            // **A handler is held where it runs** (ADR-0287): in the browser,
            // when its element is pressed, by its own rule (ADR-0113), where a
            // command it calls is a request the command performs. Until
            // ADR-0287 its work was held to the page's placement as well, so a
            // page placed at `build` whose button sends a command was refused:
            // "`database.write<Thing>` is not available at placement Build".
            if crate::contexts::context_of(&regions, &source.span)
                == crate::contexts::Context::Interaction
            {
                continue;
            }
            if crate::effects::row_covers(row, &source.effect)
                || world.grants(&source.effect, ontology) != crate::placement::Grant::No
                || !said.insert(source.effect.clone())
            {
                continue;
            }
            let family = crate::effects::family_of(&source.effect);
            // Where this effect IS meaningful, from its declaration. It came
            // from `World::worlds_for(family)`, a hard-coded family→world
            // table, and the two answers differ in a way worth having: the
            // table could only speak per family, so `secret<Payments>` — no
            // dot, no family — got an empty list and the explanation said
            // "only  can", with nothing in the gap.
            let elsewhere = match ontology.placement_of(&source.effect) {
                crate::placement::PlacementLookup::Known(worlds) => worlds
                    .iter()
                    .map(|w| format!("`{w:?}`"))
                    .collect::<Vec<_>>()
                    .join(" or "),
                _ => String::new(),
            };
            out.push(Diagnostic {
                code: crate::codes::DECLARED_PLACEMENT_CANNOT_GRANT.id,
                invariant: crate::codes::DECLARED_PLACEMENT_CANNOT_GRANT.invariant,
                reason: "inferred_effect_outside_declared_placement",
                detector: Detector::PatternMatrix,
                severity: Severity::Error,
                message: format!(
                    "`{}` is not available at placement {world:?}",
                    source.effect
                ),
                primary_span: source.span.clone(),
                related: vec![Related::new(
                    hir.decl_span(decl_id_of(hir, decl)),
                    format!("`{}` is placed in {world:?}", decl.name),
                )],
                explanation: Some(format!(
                    "The {world} world grants no {family} capabilities; only {} can. \
                     {}. The row does not name this effect, so nothing said so \
                     until the body was read.",
                    if elsewhere.is_empty() {
                        "no world"
                    } else {
                        elsewhere.as_str()
                    },
                    source.via.describe()
                )),
                repairs: vec![Repair {
                    description: format!(
                        "move this into a declaration placed in {elsewhere}, and pass \
                         its result in"
                    ),
                    replacement: None,
                }],
            });
        }
    }
    for source in found.undeclared(declared.as_deref()) {
        // Same distinction: an event handler's effects and a streamed region's
        // are not the enclosing view's row. The handler is a separate body that
        // runs later, and charging its effects to the render is what reported
        // three correct accepted programs as violations.
        if reported.contains(&source.effect) || !at_render_time(&source.span) {
            continue;
        }
        out.push(Diagnostic {
            code: crate::codes::UNDECLARED_EFFECT.id,
            invariant: crate::codes::UNDECLARED_EFFECT.invariant,
            reason: "effect_not_in_row",
            detector: Detector::PatternMatrix,
            severity: Severity::Error,
            message: format!(
                "`{}` performs `{}` but does not declare it",
                decl.name, source.effect
            ),
            primary_span: source.span.clone(),
            related: vec![Related::new(
                hir.decl_span(decl_id_of(hir, decl)),
                format!("`{}` declares its effects here", decl.name),
            )],
            explanation: Some(format!(
                "An effect row is a claim about what a body does, and a caller \
                 relies on it. {}. An effect that reaches a caller through a \
                 helper or a callback is still an effect the caller pays for.",
                source.via.describe()
            )),
            repairs: vec![Repair {
                description: format!("add `{}` to the row", source.effect),
                replacement: None,
            }],
        });
    }
}

/// Does this declaration carry an audit record for an escape hatch?
///
/// Deliberately not "does it declare the matching capability by name". The
/// record's job is to say that a human decided the compiler's rule is wrong
/// here and why; `markup_rules` then checks that record is complete. Requiring
/// a name match as well would report the same missing record twice, in two
/// vocabularies.
fn has_audit_record(body: &Body) -> bool {
    body.walk().into_iter().any(|id| {
        matches!(
            body.expr(id),
            Expr::Keyword { keyword, justification, .. }
                if keyword.starts_with("unsafe") && justification.is_some()
        )
    })
}

/// Is this declaration's output computed once and reused, or recomputed for
/// each reader?
///
/// Two spellings reach the same place. `placement build` says the output is a
/// file produced before any request exists; `partition public` says one cache
/// entry serves every reader. Charter §7.9 and §9.4 give them separate words
/// because they are separate mechanisms — but the same thing goes wrong in
/// both, so the check asks one question.
fn reuse_of(hir: &Hir, decl: &Decl) -> crate::effects::Reuse {
    use crate::effects::Reuse;
    if declared_world(hir, decl) == Some(World::Build) {
        return Reuse::Build;
    }
    // The policy first, the body scan second — the same order as
    // `declared_world` and `declared_cache`. A `materialize` block's clauses
    // are policies; a `page` writes `cache private` among its statements and
    // the scan is what reads those.
    if decl
        .policy("partition")
        .is_some_and(|p| p.value.trim() == "public")
    {
        return Reuse::SharedPartition;
    }
    if let Some(body_id) = decl.body {
        let body = hir.body(body_id);
        if let Some((v, _)) = name_pair(body, body.root, "partition")
            && v == "public"
        {
            return Reuse::SharedPartition;
        }
    }
    Reuse::PerReader
}

/// What to call this declaration in a diagnostic.
///
/// A painter has no `DeclKind` of its own — `paint X(..) !{ paint.custom }`
/// lowers as an ordinary declaration — so the row is what names it. Saying
/// "which a declaration may not do" would be true and useless.
fn context_noun(decl: &Decl) -> &'static str {
    if decl
        .declared_effects
        .as_ref()
        .is_some_and(|r| r.iter().any(|e| e.path == "paint.custom"))
    {
        return "painter";
    }
    kind_noun(decl.kind)
}

fn kind_noun(kind: DeclKind) -> &'static str {
    match kind {
        DeclKind::View => "view",
        DeclKind::Component => "component",
        DeclKind::Page => "page",
        DeclKind::Layout => "layout",
        DeclKind::Query => "query",
        DeclKind::Command => "command",
        _ => "declaration",
    }
}
