//! E8-0 — the compiler→host contract, and the one rule that governs it.
//!
//! Architect ruling, 2026-08-07:
//!
//! > Freeze the compiler→host `ComponentContract` […] Rule: actual Wasm
//! > imports ⊆ statically allowed set, never a superset. E8 consumes
//! > capabilities; it does not define effect semantics.
//!
//! and the principle the whole milestone hangs on:
//!
//! > The compiler decides what authority code needs. The host decides whether
//! > that authority physically exists. Neither should reconstruct the other's
//! > answer.
//!
//! # What each test is defending against
//!
//! Every one of these has a plausible implementation that passes a naive test.
//! A contract derived from *declared* placements rather than solved ones looks
//! identical until a declaration is wrong. A capability that drops its type
//! argument looks identical until two arguments exist. An audit that reports
//! "satisfied" for an unparseable import looks identical until one appears.
//! So each test names the thing that would still work.

use pw_core::contract::{Audit, Capability, ComponentContract, audit, contracts};
use pw_core::hir::Hir;
use pw_core::lower::lower_file;
use pw_core::resolve::Workspace;
use pw_core::signatures::Signatures;
use pw_syntax::parse_tree;

mod support;

/// A program with three components whose authority differs in every dimension
/// that matters: family, operation, type argument, and none at all.
const PROGRAM: &str = "\
module shop.origin

// **Host-bound, explicitly.** An effect row is not a host binding — architect
// ruling, 2026-08-20, after the Wasm encoder found that a capability cannot be
// a callable identity. A `fn` with a row and no `host` policy is ordinary
// compiled Pleris however privileged it is, so these say where they come from.
fn read_stores() -> Int !{ database.read<Stores> }
    host \"pw:host/database#read\"

fn write_stores() -> Int !{ database.write<Stores> }
    host \"pw:host/database#write\"

public query Menu(id: Int) -> Int
    freshness   5.minutes
    consistency snapshot
    cache       shared
    key         id
{
    read_stores()
}

command Restock(id: Int) -> Int
    requires SignedIn
{
    write_stores()
}
";

const BROWSER_ONLY: &str = "\
module shop.view

fn paint(x: Int) -> Int !{ dom.mutate }
    host \"pw:host/dom#mutate\"

component Badge() {
    paint(1)
}
";

const PURE: &str = "\
module shop.pure

component Label() {
    1
}
";

fn build(sources: &[&str]) -> Vec<ComponentContract> {
    // The effect vocabulary, selected the way a real program selects one.
    // Since `World::worlds_for` was deleted an effect nothing declares has no
    // capability and no placement, so a fixture that performs `database.read`
    // has to say what `database.read` is — see `tests/support/mod.rs`.
    let hirs: Vec<Hir> = std::iter::once(&support::VOCABULARY)
        .chain(sources.iter())
        .map(|s| lower_file(s, &parse_tree(s).green))
        .collect();
    let refs: Vec<&Hir> = hirs.iter().collect();
    let ws = Workspace::build(&refs);
    let sigs = Signatures::build(&ws, &refs);
    contracts(&refs, &sigs, &ws)
}

fn one(sources: &[&str], module: &str) -> ComponentContract {
    build(sources)
        .into_iter()
        .find(|c| c.component_id == module)
        .unwrap_or_else(|| panic!("no contract for {module}"))
}

#[test]
fn a_contract_carries_the_six_semantic_fields_and_no_others() {
    // Frozen means frozen. Serialized and read back as a map, so a seventh
    // SEMANTIC field added later fails here rather than being discovered by a
    // host that silently ignores it.
    //
    // `capability_mapping` is not a seventh: it says which mapping produced the
    // other six, so a host can tell a representation change from an authority
    // change. A contract carrying a mapping version it does not know must be
    // refused rather than interpreted under its own.
    let c = one(&[PROGRAM], "shop.origin.Menu");
    let json = serde_json::to_value(&c).expect("serialize");
    let mut keys: Vec<&str> = json
        .as_object()
        .expect("object")
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "abi_schema",
            "allowed_placements",
            "capability_mapping",
            "component_id",
            "exports",
            "imports",
            "required_capabilities",
        ]
    );
    assert_eq!(c.capability_mapping, pw_core::contract::CAPABILITY_MAPPING);
}

#[test]
fn a_capabilitys_argument_is_resolved_against_the_programs_types() {
    use pw_core::contract::{Capability as Cap, NotACapability};
    use pw_core::ontology::Ontology;
    use std::collections::BTreeSet;

    let declared: BTreeSet<String> = ["Stores".to_string()].into_iter().collect();
    let hir = lower_file(support::VOCABULARY, &parse_tree(support::VOCABULARY).green);
    let ws = Workspace::build(&[&hir]);
    let ontology = Ontology::build_with(&[&hir], &ws);

    // Resolved.
    assert_eq!(
        Cap::resolve("database.read<Stores>", &declared, &ontology)
            .expect("resolves")
            .name(),
        "database.read<Stores>"
    );

    // An effect declared with `capability none` needs no host authority.
    assert_eq!(
        Cap::resolve("trace", &declared, &ontology),
        Err(NotACapability::Unrestricted {
            family: "trace".into()
        })
    );

    // A misspelled argument is REPORTED, not accepted silently. Without this,
    // `database.read<Stroes>` becomes a capability nothing will ever grant and
    // the failure appears at deployment rather than at build.
    assert_eq!(
        Cap::resolve("database.read<Stroes>", &declared, &ontology),
        Err(NotACapability::UnknownArgument {
            family: "database".into(),
            argument: "Stroes".into()
        })
    );

    // And an effect the program never declared is neither of those. It used to
    // be `Unrestricted` whenever the deleted `worlds_for` table had no row for
    // its family, which is the same answer `trace` gets above — so a typo and
    // a deliberately authority-free effect were indistinguishable.
    assert_eq!(
        Cap::resolve("databse.read<Stores>", &declared, &ontology),
        Err(NotACapability::Undeclared {
            effect: "databse.read<Stores>".into()
        })
    );
}

#[test]
fn an_unresolvable_argument_does_not_remove_authority() {
    // The direction that matters. Over-stating a capability is refused work;
    // under-stating it is authority nobody approved — so a capability whose
    // argument does not resolve is KEPT, and the mistake surfaces as a refusal
    // rather than as a silent grant.
    let program = "\
module shop.typo

fn read() -> Int !{ database.read<Stroes> } { 0 }

public query Menu(id: Int) -> Int
    freshness   5.minutes
    consistency snapshot
    cache       shared
    key         id
{
    read()
}
";
    let c = one(&[program], "shop.typo.Menu");
    assert_eq!(
        c.required_capabilities
            .iter()
            .map(Capability::name)
            .collect::<Vec<_>>(),
        ["database.read<Stroes>"],
        "the capability survives even though nothing will grant it"
    );
}

#[test]
fn requires_is_an_export_precondition_and_part_of_the_abi() {
    let restock = one(&[PROGRAM], "shop.origin.Restock");
    let export = restock.exports[0]
        .component
        .as_ref()
        .expect("compiled export");
    assert_eq!(export.authorization.len(), 1);
    assert_eq!(export.authorization[0].predicate, "SignedIn");
    assert!(export.authorization[0].arguments.is_empty());

    let parameterized = PROGRAM.replace("requires SignedIn", "requires SignedIn, OwnsOrder(id)");
    let c = one(&[&parameterized], "shop.origin.Restock");
    let requirements = &c.exports[0]
        .component
        .as_ref()
        .expect("compiled export")
        .authorization;
    assert_eq!(requirements.len(), 2);
    assert_eq!(requirements[1].predicate, "OwnsOrder");
    assert_eq!(requirements[1].arguments, [0]);

    // Removing a precondition changes the ABI identity even though the Wasm
    // function's value signature is unchanged. A deployment must not confuse
    // the weaker invocation contract with the stronger one.
    let unrestricted = PROGRAM.replace("    requires SignedIn\n", "");
    let without = one(&[&unrestricted], "shop.origin.Restock");
    assert_ne!(restock.abi_schema, without.abi_schema);
}

/// **`requires Anyone` asks nothing** (ADR-0406). `Anyone` is the
/// platform's predicate, which every reader holds: the export carries no
/// requirement for it, and its ABI is the unrestricted one's, since a host
/// runs the two alike.
#[test]
fn requires_anyone_is_no_precondition() {
    let anyone = PROGRAM.replace("requires SignedIn", "requires Anyone");
    let c = one(&[&anyone], "shop.origin.Restock");
    let export = c.exports[0].component.as_ref().expect("compiled export");
    assert!(
        export.authorization.is_empty(),
        "{:?}",
        export.authorization
    );

    let unrestricted = PROGRAM.replace("    requires SignedIn\n", "");
    let without = one(&[&unrestricted], "shop.origin.Restock");
    assert_eq!(c.abi_schema, without.abi_schema);
}

/// **`idempotent_by` reaches the host, and is part of the ABI** (ADR-0121).
/// It was checked and read by nothing until 2026-10-02, so a retried request
/// ran its command twice.
#[test]
fn idempotent_by_is_an_export_property_and_part_of_the_abi() {
    let restock = one(&[PROGRAM], "shop.origin.Restock");
    let export = restock.exports[0]
        .component
        .as_ref()
        .expect("compiled export");
    assert_eq!(export.idempotent_by, None, "not declared, not claimed");

    let keyed = PROGRAM.replace(
        "    requires SignedIn\n",
        "    requires SignedIn\n    idempotent_by InteractionId\n",
    );
    let c = one(&[&keyed], "shop.origin.Restock");
    let export = c.exports[0].component.as_ref().expect("compiled export");
    assert_eq!(export.idempotent_by.as_deref(), Some("InteractionId"));

    // A host holding the keyed contract must not take the unkeyed one for
    // it: one runs a key's invocation once, the other every time.
    assert_ne!(restock.abi_schema, c.abi_schema);
}

#[test]
fn required_capabilities_come_from_the_effect_row() {
    // Per DECLARATION, so the query that reads does not also get the write the
    // command in the same module needs. Aggregating by module would give both
    // both — over-granting, and then E5's solver would place neither, because
    // the union of two demands can be unsatisfiable when each alone is not.
    let menu = one(&[PROGRAM], "shop.origin.Menu");
    let restock = one(&[PROGRAM], "shop.origin.Restock");
    let names = |c: &ComponentContract| -> Vec<String> {
        c.required_capabilities
            .iter()
            .map(Capability::name)
            .collect()
    };
    assert_eq!(names(&menu), ["database.read<Stores>"]);
    assert_eq!(names(&restock), ["database.write<Stores>"]);
}

#[test]
fn a_component_with_no_effects_needs_no_capabilities_and_may_import_nothing() {
    // The boundary case that makes the rule bite hardest: an empty allowed set
    // means EVERY import is undeclared. A contract that treated "no
    // capabilities" as "no constraint" would give the most trusted-looking
    // component unlimited authority.
    let c = one(&[PURE], "shop.pure.Label~ui");
    assert!(c.required_capabilities.is_empty());
    assert!(c.imports.is_empty());
    assert_eq!(
        audit(&c, &["pw:host/database#read".into()]),
        Audit::Undeclared(vec!["pw:host/database#read".into()]),
    );
    assert_eq!(audit(&c, &[]), Audit::Satisfied);
}

#[test]
fn allowed_placements_are_solved_rather_than_declared() {
    // E5's solver, not a table here. A component that reads the database can
    // only run at the origin; one that mutates the DOM only in the browser;
    // one that does neither can run anywhere.
    assert_eq!(
        one(&[PROGRAM], "shop.origin.Menu").allowed_placements,
        ["origin"]
    );
    assert_eq!(
        one(&[BROWSER_ONLY], "shop.view.Badge~ui").allowed_placements,
        ["browser"]
    );
    assert_eq!(
        one(&[PURE], "shop.pure.Label~ui").allowed_placements,
        ["build", "browser", "edge", "origin"],
        "a component with no restricted capability runs anywhere"
    );
}

#[test]
fn placement_is_not_a_capability() {
    // The architect's distinction, as a fact about the data. Two components
    // with the SAME single placement need different capabilities, and two with
    // the same capability family need the same placement — so neither field is
    // recoverable from the other, and a host that inferred one from the other
    // would be wrong in one of these two cases.
    let reader = one(&[PROGRAM], "shop.origin.Menu");
    let writer = one(&[PROGRAM], "shop.origin.Restock");
    let browser = one(&[BROWSER_ONLY], "shop.view.Badge~ui");

    assert_eq!(reader.allowed_placements, ["origin"]);
    assert_eq!(browser.allowed_placements, ["browser"]);
    assert_ne!(reader.required_capabilities, browser.required_capabilities);

    // And the reverse direction: `database.read` and `database.write` are
    // different capabilities at the SAME placement, so a host cannot infer one
    // field from the other in either direction.
    assert_eq!(reader.allowed_placements, writer.allowed_placements);
    assert_ne!(reader.required_capabilities, writer.required_capabilities);
    assert_eq!(
        reader.required_capabilities[0].family,
        writer.required_capabilities[0].family
    );
}

// --- the rule ------------------------------------------------------------

#[test]
fn actual_imports_may_be_a_subset() {
    // Fewer is fine: dead code, or a branch never compiled in.
    let c = one(&[PROGRAM], "shop.origin.Menu");
    assert_eq!(c.imports.len(), 1);
    assert_eq!(audit(&c, &[c.imports[0].key()]), Audit::Satisfied);
    assert_eq!(audit(&c, &[]), Audit::Satisfied);
}

#[test]
fn actual_imports_may_never_be_a_superset() {
    let c = one(&[PROGRAM], "shop.origin.Menu");
    let mut actual: Vec<String> = c.imports.iter().map(|i| i.key()).collect();
    actual.push("pw:host/secret#read".into());

    assert_eq!(
        audit(&c, &actual),
        Audit::Undeclared(vec!["pw:host/secret#read".into()]),
        "one import outside the allowed set is a refusal, not a warning"
    );
}

#[test]
fn every_undeclared_import_is_reported_not_only_the_first() {
    // A build that fixes the first and rediscovers the second is a slow way to
    // learn the shape of a problem.
    let c = one(&[PURE], "shop.pure.Label~ui");
    let actual = vec![
        "pw:host/secret#read".to_string(),
        "pw:host/database#write".to_string(),
    ];
    assert_eq!(
        audit(&c, &actual),
        Audit::Undeclared(vec![
            "pw:host/database#write".into(),
            "pw:host/secret#read".into(),
        ]),
    );
}

#[test]
fn an_unrecognisable_import_is_undeclared_rather_than_ignored() {
    // Fail-closed. "I could not tell what this asks for" and "this asks for
    // nothing" must never produce the same verdict — the audit is membership
    // in a set, so there is no parse to fail and no unknown-interface branch
    // to forget.
    let c = one(&[PROGRAM], "shop.origin.Menu");
    for weird in ["", "not-an-interface", "pw:host/database", "#read", "\u{0}"] {
        assert!(
            !audit(&c, &[weird.to_string()]).is_satisfied(),
            "`{weird}` must not be treated as satisfied"
        );
    }
}

#[test]
fn the_operation_distinguishes_two_imports_of_one_family() {
    // A host that granted `pw:host/database` wholesale would satisfy a
    // component that only ever asked to read.
    let read = one(&[PROGRAM], "shop.origin.Menu");
    let write = one(&[PROGRAM], "shop.origin.Restock");
    assert_eq!(
        read.imports.iter().map(|i| i.key()).collect::<Vec<_>>(),
        ["pw:host/database#read"]
    );
    assert_eq!(
        write.imports.iter().map(|i| i.key()).collect::<Vec<_>>(),
        ["pw:host/database#write"]
    );
    assert_eq!(
        audit(&read, &["pw:host/database#write".into()]),
        Audit::Undeclared(vec!["pw:host/database#write".into()]),
        "reading does not authorise writing, however close the two look"
    );

    // A read-only component may not import write.
    let read_only = "\
module shop.readonly

fn read_stores() -> Int !{ database.read<Stores> } { 0 }

public query Menu(id: Int) -> Int
    freshness   5.minutes
    consistency snapshot
    cache       shared
    key         id
{
    read_stores()
}
";
    let ro = one(&[read_only], "shop.readonly.Menu");
    assert_eq!(
        audit(&ro, &["pw:host/database#write".into()]),
        Audit::Undeclared(vec!["pw:host/database#write".into()]),
    );
}

#[test]
fn an_import_names_the_capability_that_authorises_it() {
    // So a refusal can say why. "This module imports `pw:host/database#read`
    // and nothing in its effect row asks for `database.read<Stores>`" is
    // actionable; "undeclared import" is not.
    let c = one(&[PROGRAM], "shop.origin.Menu");
    let read = c
        .imports
        .iter()
        .find(|i| i.name == "read")
        .expect("the read import");
    assert_eq!(read.capability, "database.read<Stores>");
}

// --- the schema ----------------------------------------------------------

#[test]
fn the_schema_ignores_formatting_and_notices_authority() {
    // Over the INTERFACE, not the source. A comment must not change it; a new
    // capability must. A schema over source text is `resume_artifacts` scheme
    // 1, which this project superseded for exactly this reason.
    let plain = one(&[PROGRAM], "shop.origin.Menu");

    let commented = PROGRAM.replace(
        "public query Menu",
        "// a comment that changes nothing about the interface\npublic query Menu",
    );
    assert_eq!(
        one(&[&commented], "shop.origin.Menu").abi_schema,
        plain.abi_schema,
        "a comment is not an interface change"
    );

    let widened = PROGRAM.replace(
        "fn read_stores() -> Int !{ database.read<Stores> }",
        "fn read_stores() -> Int !{ database.read<Stores>, secret<Payments> }",
    );
    let widened = one(&[&widened], "shop.origin.Menu");
    assert_ne!(
        widened.abi_schema, plain.abi_schema,
        "a new capability IS an interface change"
    );
    assert!(
        widened
            .required_capabilities
            .iter()
            .any(|c| c.family == "secret"),
        "and it reaches the contract"
    );
}

#[test]
fn a_new_sibling_adds_a_contract_and_perturbs_no_other() {
    // Per-declaration contracts have a property module-wide ones cannot: a new
    // declaration is a new contract, and every existing one is byte-identical.
    // A host can therefore reload exactly what changed, and "the schema
    // changed" carries information instead of tracking the file.
    let before = build(&[PROGRAM]);
    let extra = format!(
        "{PROGRAM}\ncommand Discount(id: Int) -> Int\n    requires SignedIn\n{{\n    id\n}}\n"
    );
    let after = build(&[&extra]);

    assert_eq!(after.len(), before.len() + 1);
    for old in &before {
        let same = after
            .iter()
            .find(|c| c.component_id == old.component_id)
            .expect("an existing component is still there");
        assert_eq!(same, old, "{} was perturbed by a sibling", old.component_id);
    }
    assert!(
        after
            .iter()
            .any(|c| c.component_id == "shop.origin.Discount")
    );
}

#[test]
fn renaming_an_export_changes_that_contract() {
    // The control for the test above. If a sibling's arrival changes nothing
    // and a rename changes nothing either, the schema is a constant.
    let before = one(&[PROGRAM], "shop.origin.Menu");
    let renamed = PROGRAM.replace("query Menu(", "query Catalogue(");
    let after = one(&[&renamed], "shop.origin.Catalogue");
    assert_ne!(after.abi_schema, before.abi_schema);
    assert_eq!(after.exports[0].name, "Catalogue");
}

#[test]
fn a_page_does_not_inherit_its_handlers_authority() {
    // Least authority, at the place it is easiest to lose.
    //
    // A page that renders `on:press={.. => add_to_cart(..)}` CONTAINS a call
    // to a command that writes the database. It does not perform that write:
    // the handler does, when someone presses the button, and E7-L already
    // makes the handler a separately loaded unit. A contract that walked the
    // whole body would require `database.write` to RENDER a page — and a host
    // granting it would hand the render path authority it never uses.
    let program = "\
module shop.ui

fn write_stores() -> Int !{ database.write<Stores> } { 0 }

command Buy(id: Int) -> Int
    requires SignedIn
{
    write_stores()
}

page Shop(id: Int) {
    view {
        <button on:press={resumable() => Buy(id)}>Buy</button>
    }
}
";
    let page = one(&[program], "shop.ui.Shop~ui");
    let command = one(&[program], "shop.ui.Buy");

    assert!(
        page.required_capabilities.is_empty(),
        "rendering requires no authority, got {:?}",
        page.required_capabilities
    );
    assert!(!audit(&page, &["pw:host/database#write".into()]).is_satisfied());

    // And the authority is not LOST — it is recorded once, against the thing
    // that performs it.
    assert_eq!(
        command
            .required_capabilities
            .iter()
            .map(Capability::name)
            .collect::<Vec<_>>(),
        ["database.write<Stores>"]
    );
    assert_eq!(command.allowed_placements, ["origin"]);
    assert_eq!(
        page.allowed_placements,
        ["build", "browser", "edge", "origin"],
        "and the page can be rendered anywhere, which is the point"
    );
}

#[test]
fn a_body_change_that_changes_neither_authority_nor_interface_keeps_the_schema() {
    // The control for the two tests above: if the schema changed on every
    // edit, "it changed" would carry no information and a host would reload on
    // noise.
    let plain = one(&[PROGRAM], "shop.origin.Menu");
    let rebodied = PROGRAM.replace(
        "fn write_stores() -> Int !{ database.write<Stores> } { 0 }",
        "fn write_stores() -> Int !{ database.write<Stores> } { 1 }",
    );
    assert_eq!(
        one(&[&rebodied], "shop.origin.Menu").abi_schema,
        plain.abi_schema
    );
}

#[test]
fn the_contract_round_trips_through_json() {
    // ADR-0018's boundary: the host deserializes this, and a field that does
    // not survive the trip is a field the host never sees.
    let c = one(&[PROGRAM], "shop.origin.Menu");
    let text = serde_json::to_string(&c).expect("serialize");
    let back: ComponentContract = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(back, c);
}

#[test]
fn two_declarations_sharing_a_name_do_not_share_authority() {
    // `Inference::known` is keyed by the BARE declaration name, so
    // `Resources.Cart` and `store.page.Cart` share one entry in it. Deriving a
    // contract from that map gave a query whose body is `todo` the database
    // capability of an unrelated query in another module — authority granted
    // for a body that performs nothing, and it looked entirely reasonable in
    // the output.
    //
    // Contracts therefore infer from the BODY. This pins that: the two `Read`
    // declarations below differ only in what they do.
    let empty = "\
module a

component Read() {
    1
}
";
    let reads = "\
module b

fn touch() -> Int !{ database.read<Stores> } { 0 }

component Read() {
    touch()
}
";
    let all = build(&[empty, reads]);
    let a = all
        .iter()
        .find(|c| c.component_id == "a.Read~ui")
        .expect("a.Read~ui");
    let b = all
        .iter()
        .find(|c| c.component_id == "b.Read~ui")
        .expect("b.Read~ui");

    assert!(
        a.required_capabilities.is_empty(),
        "a component that does nothing needs nothing, got {:?}",
        a.required_capabilities
    );
    assert_eq!(
        b.required_capabilities
            .iter()
            .map(Capability::name)
            .collect::<Vec<_>>(),
        ["database.read<Stores>"],
        "and the one that does still needs it"
    );
    assert_ne!(a.abi_schema, b.abi_schema);
}

// --- binding support, through the whole contract path ------------------------

/// **An export says what its own signature permits.**
///
/// Through `contracts()`, not through `binding::remote_support` directly. The
/// unit tests in `binding.rs` prove the analysis; this proves it is REACHED —
/// `binding_support` is looked up by `DefId` and falls back to the permissive
/// default when no signature is found, so a lookup that silently missed would
/// mark every export transferable and look exactly like a program whose
/// signatures all are.
#[test]
fn an_export_that_passes_a_handle_is_not_remotely_bindable() {
    use pw_core::binding::RemoteSupport;

    let program = "\
module shop.tx

opaque type OpenTransaction = String
type Store = Store { id: Int }
type StoreError = StoreError { why: String }
opaque type StoreId = String

fn begin() -> OpenTransaction !{ resource.acquire<OpenTransaction> } { todo }

command Commit(t: OpenTransaction) -> Int
    requires SignedIn
{
    0
}

query Lookup(id: StoreId) -> Result<Store, StoreError>
    freshness   5.minutes
    consistency snapshot
    cache       shared
    key         id
{
    todo
}
";
    let refused = one(&[program], "shop.tx.Commit");
    let RemoteSupport::Refused { positions } = &refused.exports[0].binding.remote else {
        panic!("{:?}", refused.exports[0].binding);
    };
    assert_eq!(positions[0].position, "argument 0");
    assert_eq!(positions[0].ty.as_deref(), Some("OpenTransaction"));

    // The discriminating half, in the SAME program: a query over a key and a
    // record is transferable. Without it, the assertion above would also pass
    // for an analysis that refused everything.
    let ok = one(&[program], "shop.tx.Lookup");
    assert_eq!(ok.exports[0].binding.remote, RemoteSupport::Transferable);

    // And local is unaffected in both. Two questions, two fields — an enum
    // would have to answer them with one word.
    assert_eq!(
        refused.exports[0].binding.local,
        pw_core::binding::LocalSupport::Direct
    );
}

// --- the callable-operation layer -------------------------------------------

/// **Every operation's authority is held by the component that calls it.**
///
/// Architect ruling, 2026-08-20, adding this as its own layer between the
/// artifact audit and admission:
///
/// ```text
/// artifact audit        did you import only the operations your contract names?
/// contract consistency  does your contract possess every authority those
///                       operations require?
/// admission             does this node actually grant that authority?
/// ```
///
/// Controls A and D of the four the ruling froze.
#[test]
fn an_operations_authority_must_be_held_by_its_caller() {
    use pw_core::contract::consistent;

    // A — the operation requires `database.read<Stores>` and the component
    // holds it.
    let c = one(&[PROGRAM], "shop.origin.Menu");
    assert!(
        consistent(&c).is_satisfied(),
        "{:?} vs {:?}",
        c.imports,
        c.required_capabilities
    );
    assert!(
        !c.imports.is_empty() && !c.required_capabilities.is_empty(),
        "and it is not vacuous: there is an operation and there is authority"
    );

    // D — the discriminator that forced the whole model. Two callables, one
    // capability, different ABIs, both accepted independently.
    let src = "\
module shop.two

fn add(session: Int, item: Int, quantity: Int) -> Int !{ database.write<Stores> }
    host \"store:data/carts#add\"

fn clear(session: Int) -> Int !{ database.write<Stores> }
    host \"store:data/carts#clear\"

command Both(id: Int) -> Int
    requires SignedIn
{
    add(id, id, id) + clear(id)
}
";
    let c = one(&[src], "shop.two.Both");
    let mut keys: Vec<String> = c.imports.iter().map(|i| i.key()).collect();
    keys.sort();
    assert_eq!(keys, ["store:data/carts#add", "store:data/carts#clear"]);

    let sig = |k: &str| -> usize {
        c.imports
            .iter()
            .find(|i| i.key() == k)
            .and_then(|i| i.signature.as_ref())
            .map(|s| s.params.len())
            .unwrap_or_else(|| panic!("no signature for {k}"))
    };
    assert_eq!(sig("store:data/carts#add"), 3);
    assert_eq!(sig("store:data/carts#clear"), 1);

    // One authority, and the component holds it once.
    for i in &c.imports {
        assert_eq!(
            i.capabilities.iter().map(|x| x.name()).collect::<Vec<_>>(),
            ["database.write<Stores>"]
        );
    }
    assert!(consistent(&c).is_satisfied());
}

/// **B — an operation whose authority the component does not hold is refused.**
///
/// The negative control. Built by removing the capability from the contract
/// rather than by writing a program the checker would reject earlier, because
/// the subject is the CONSISTENCY relation and not the effect pipeline.
#[test]
fn an_operation_whose_authority_is_not_held_is_refused() {
    use pw_core::contract::consistent;

    let mut c = one(&[PROGRAM], "shop.origin.Menu");
    assert!(consistent(&c).is_satisfied(), "it starts consistent");

    c.required_capabilities.clear();
    let verdict = consistent(&c);
    assert!(!verdict.is_satisfied(), "{verdict:?}");
    let pw_core::contract::Audit::Undeclared(missing) = verdict else {
        unreachable!()
    };
    assert!(
        missing.iter().any(|m| m.contains("database.read<Stores>")),
        "and it names the authority the operation needs: {missing:?}"
    );
}

/// **C — an operation requiring nothing is satisfied by a component holding
/// nothing.**
///
/// The case that must survive. A browser-semantic operation is
/// placement-constrained and needs no capability, and a rule written as
/// equality rather than ⊆ would refuse it.
#[test]
fn an_operation_that_needs_no_authority_is_consistent_with_none() {
    use pw_core::contract::consistent;

    let c = one(&[BROWSER_ONLY], "shop.view.Badge~ui");
    assert!(
        c.required_capabilities.is_empty(),
        "{:?}",
        c.required_capabilities
    );
    assert!(consistent(&c).is_satisfied());

    // And it really has an operation, so the emptiness is not why it passes.
    assert!(
        c.imports.iter().any(|i| i.key() == "pw:host/dom#mutate"),
        "{:?}",
        c.imports.iter().map(|i| i.key()).collect::<Vec<_>>()
    );
}

/// **The component may require MORE than its imports do.**
///
/// > because a component can have required authority whose use isn't
/// > represented by a particular callable import shape, and we shouldn't force
/// > the ABI to become the definition of semantic effects.
///
/// So the relation is ⊆ and not equality, and this is what says so: adding an
/// unrelated capability to the contract keeps it consistent.
#[test]
fn a_component_may_hold_authority_no_import_uses() {
    use pw_core::contract::{Capability, consistent};

    let mut c = one(&[PROGRAM], "shop.origin.Menu");
    c.required_capabilities
        .push(Capability::parse("secret<Payments>"));
    assert!(
        consistent(&c).is_satisfied(),
        "a component's authority is not defined by its ABI"
    );
}

/// **Same `interface#operation`, wrong ABI → rejected.**
///
/// The mutation control the ruling asked for, and it mutates only the
/// SIGNATURE — the `ImportId` is preserved exactly, so every other layer stays
/// satisfied:
///
/// ```text
/// audit(identity)        satisfied — the name is the one the contract names
/// consistent(authority)  satisfied — the capabilities are untouched
/// abi(shape)             REFUSED
/// ```
///
/// That is the whole reason it is a separate function. An artifact that imports
/// the right operation with the wrong type validates as Wasm, resolves as a
/// world, and passes an identity audit; the arguments are simply read as the
/// wrong shape at the boundary. Architect ruling, 2026-08-20: *"that becomes
/// important immediately for the Component Model."*
#[test]
fn an_operation_with_the_declared_name_and_a_different_abi_is_refused() {
    use pw_core::contract::{Signature, abi, consistent};

    // An operation that actually takes arguments — `PROGRAM`'s `read_stores()`
    // takes none, and dropping a parameter from an empty list is a mutation
    // that mutates nothing.
    let src = "\
module shop.abi

fn add(session: Int, item: Int, quantity: Int) -> Int !{ database.write<Stores> }
    host \"store:data/carts#add\"

command Add(id: Int) -> Int
    requires SignedIn
{
    add(id, id, id)
}
";
    let c = one(&[src], "shop.abi.Add");
    let key = c.imports[0].key();
    let declared = c.imports[0]
        .signature
        .clone()
        .unwrap_or_else(|| panic!("{key} has no declared ABI, so this proves nothing"));
    assert_eq!(declared.params.len(), 3, "the shape being mutated");

    // The artifact that agrees: satisfied.
    assert_eq!(
        abi(&c, &[(key.clone(), declared.clone())]),
        Audit::Satisfied
    );

    // The artifact that dropped a parameter: refused, with both shapes named.
    let mut fewer = declared.clone();
    fewer.params.pop();
    let verdict = abi(&c, &[(key.clone(), fewer.clone())]);
    assert!(!verdict.is_satisfied(), "{verdict:?}");
    let Audit::Undeclared(wrong) = verdict else {
        unreachable!()
    };
    assert_eq!(wrong.len(), 1);
    assert!(
        wrong[0].contains(&key) && wrong[0].contains(&declared.render()),
        "the refusal names the operation and the ABI the contract fixed: {wrong:?}"
    );

    // And the result type alone is enough, with the arity identical.
    let other_result = Signature {
        params: declared.params.clone(),
        result: pw_core::resolved::StableTypeId::Primitive("String".into()),
    };
    assert!(
        !abi(&c, &[(key.clone(), other_result)]).is_satisfied(),
        "an ABI differs in its result as much as in its parameters"
    );

    // **The discriminator.** The mutation is invisible to the other two
    // layers — which is what makes this check load-bearing rather than a
    // second spelling of the identity audit.
    assert_eq!(audit(&c, std::slice::from_ref(&key)), Audit::Satisfied);
    assert!(consistent(&c).is_satisfied());

    // An operation the contract declares no ABI for is NOT checked. A contract
    // that has not said and a contract that agrees must not be the same
    // verdict, so this reports nothing rather than a spurious match.
    let mut quiet = c.clone();
    quiet.imports[0].signature = None;
    assert_eq!(abi(&quiet, &[(key, fewer)]), Audit::Satisfied);
}

/// **Who DEFINES an operation is recorded, separately from who implements it.**
///
/// Architect ruling, 2026-08-20: *"the host process currently provides the
/// implementation" is a deployment fact; it doesn't need to collapse their
/// semantic ownership.*
#[test]
fn platform_operations_are_distinguished_from_application_ones() {
    use pw_core::contract::Ownership;

    let src = "\
module shop.mixed

fn ctx() -> Int !{ session.read }
    host \"pw:host/session#read\"

fn rows(id: Int) -> Int !{ database.read<Stores> }
    host \"store:data/stores#get\"

command Uses(id: Int) -> Int
    requires SignedIn
{
    ctx() + rows(id)
}
";
    let c = one(&[src], "shop.mixed.Uses");
    let owner = |k: &str| {
        c.imports
            .iter()
            .find(|i| i.key() == k)
            .map(|i| i.owner)
            .unwrap_or_else(|| panic!("no import {k}"))
    };
    assert_eq!(owner("pw:host/session#read"), Ownership::Platform);
    assert_eq!(
        owner("store:data/stores#get"),
        Ownership::External,
        "an application repository method is not a Pleris platform primitive \
         merely because the host implements it today"
    );
}
