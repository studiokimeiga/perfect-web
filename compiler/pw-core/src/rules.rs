//! Declaration-level semantic rules.
//!
//! These are the rules decidable from a declaration *header* alone — policy
//! contradictions, placement-versus-effect conflicts, privacy-versus-cache
//! conflicts. They need no expression analysis, so they can run today against
//! the real corpus rather than waiting for body parsing.
//!
//! What this is **not**: the effect checker. A rule like "a network request
//! inside a view" (corpus R-001) lives in a function *body*, which E2 captures
//! as a token range and does not parse. Those stay unenforced, and
//! `docs/milestones/E2.md` says so rather than implying otherwise.
//!
//! Every rule carries a stable `PW####` code, a primary span, and — where the
//! rule involves two places — an origin span, per charter §16.3. The E0
//! diagnostic spike established that shape; this applies it to real files.
//!
//! # E6F: this reads the HIR
//!
//! It used to read `pw_syntax::ast::SourceFile`, a second declaration tree
//! produced by a second parser. Two parsers deciding what a program means is
//! two programs, and E6 showed what that costs: the tree grammar learned to
//! parse a `materialize` block's policies and this path did not, so the same
//! file had policies to one analysis and none to another.
//!
//! Architect ruling, 2026-08-06:
//!
//! > There is only one parser that decides what a `.pw` program means.
//!
//! The rules themselves did not change. What changed is where they read from.

use crate::hir::{Decl, DeclKind, Hir, Policy, Span};

use crate::diagnostics::{Detector, Diagnostic};

/// Declaration-level rules build `Diagnostic` values directly. The local
/// builder below only exists to keep each rule readable.
type Finding = Diagnostic;

fn err(
    code: &'static str,
    invariant: &'static str,
    message: impl Into<String>,
    span: Span,
) -> Diagnostic {
    Diagnostic::error(
        code,
        invariant_of(code, invariant),
        Detector::DeclarationRule,
        message,
        span,
    )
}

fn warn(
    code: &'static str,
    invariant: &'static str,
    message: impl Into<String>,
    span: Span,
) -> Diagnostic {
    Diagnostic::warning(
        code,
        invariant_of(code, invariant),
        Detector::DeclarationRule,
        message,
        span,
    )
}

/// **The registry's sentence for `code`**, where it registers one. A rule may
/// not restate an invariant in its own words; these did, unseen, until they
/// ran inside `check_sources` (ADR-0090). A code the registry does not hold,
/// an alias it redirects, keeps the rule's.
fn invariant_of(code: &'static str, written: &'static str) -> &'static str {
    crate::codes::lookup(code).map_or(written, |c| c.invariant)
}

fn policy<'a>(policies: &'a [Policy], name: &str) -> Option<&'a Policy> {
    policies.iter().find(|p| p.name == name)
}

/// The visibility keyword as written, or `""`.
fn vis(decl: &Decl) -> &str {
    decl.visibility.as_deref().unwrap_or("")
}

fn privacy_label(v: &str) -> &'static str {
    match v {
        "public" => "Public",
        "session" => "Session<SessionId>",
        "private" | "user" => "User<UserId>",
        _ => "unlabelled",
    }
}

/// The declaration keyword, for a message that names what the reader wrote.
///
/// A `DeclKind` and a noun are not the same thing: the kind is what the
/// compiler decided, the noun is what the author typed, and a diagnostic that
/// says "session query" when the source says `subscription` is describing a
/// different program from the one on screen.
fn noun_of(kind: DeclKind) -> &'static str {
    match kind {
        DeclKind::Query => "query",
        DeclKind::Command => "command",
        DeclKind::Subscription => "subscription",
        DeclKind::Resource => "resource",
        DeclKind::Materialize => "materialize",
        DeclKind::View => "view",
        DeclKind::Component => "component",
        DeclKind::Page => "page",
        DeclKind::Layout => "layout",
        DeclKind::Task => "task",
        DeclKind::Event => "event",
        DeclKind::Source => "source",
        DeclKind::Table => "table",
        DeclKind::Upload => "upload",
        DeclKind::Predicate => "predicate",
        DeclKind::Prelude => "prelude",
        DeclKind::Effect => "effect",
        DeclKind::Fn => "fn",
        DeclKind::Type | DeclKind::Opaque => "type",
        DeclKind::Let => "let",
        DeclKind::Signal => "signal",
        DeclKind::Import => "import",
        DeclKind::Other => "declaration",
    }
}

/// The declarations `check_resource` applies to: the ones with a policy block
/// describing a cached, placed or retried operation.
fn is_resource_like(kind: DeclKind) -> bool {
    matches!(
        kind,
        DeclKind::Query
            | DeclKind::Command
            | DeclKind::Subscription
            | DeclKind::Resource
            | DeclKind::Materialize
    )
}

/// Effects a placement cannot satisfy, and why.
pub(crate) fn placement_conflict(
    placement: &str,
    effect: &str,
) -> Option<(&'static str, &'static str)> {
    let browser_only = effect.starts_with("device")
        || effect.starts_with("layout")
        || effect.starts_with("dom")
        || effect.starts_with("observe")
        || effect.starts_with("paint")
        || effect.starts_with("animation");
    match placement {
        "browser" if effect.starts_with("database") => Some((
            "the browser world grants no database capability",
            "move this to an origin-placed query",
        )),
        "browser" if effect.starts_with("secret") => Some((
            "the browser world grants no secret capability",
            "keep secret access at the origin",
        )),
        "edge" if effect.starts_with("secret") => Some((
            "the edge world grants no secret capability",
            "move this operation to `origin`, or request an edge-scoped credential",
        )),
        "origin" if browser_only => Some((
            "the origin world grants no browser or device capabilities",
            "this must run in the browser",
        )),
        "build" if effect.starts_with("clock.wall") || effect.starts_with("random") => Some((
            "static generation requires a deterministic effect row",
            "remove the nondeterministic read, or drop the `build` placement",
        )),
        // ADR-0304: a value that compares the clock is read again when the
        // instant it compared passes, and a file generated once is not.
        "build" if effect.starts_with("clock.compare") => Some((
            "static generation is told nothing when the instant it compared passes",
            "read the clock where the page is served, or drop the `build` placement",
        )),
        _ => None,
    }
}

fn check_effects_against_placement(decl: &Decl, out: &mut Vec<Finding>) {
    let (Some(placement), Some(row)) = (
        policy(&decl.policies, "placement"),
        decl.declared_effects.as_ref(),
    ) else {
        return;
    };
    // One world pins the declaration; a list of worlds pins none
    // (`check::declared_world`), and no effect conflicts with a choice.
    let target = placement.value.trim();
    if target.contains(',') {
        return;
    }
    for e in row {
        // `path`, not `written`: the conflict is with the effect FAMILY, and
        // `style.mutate<LayoutAffect>` is as unavailable at `origin` as
        // `style.mutate` is. The message uses the written form, because that is
        // what the reader typed.
        if let Some((why, help)) = placement_conflict(target, &e.path) {
            out.push(
                err(
                    crate::codes::DECLARED_PLACEMENT_CANNOT_GRANT.id,
                    crate::codes::DECLARED_PLACEMENT_CANNOT_GRANT.invariant,
                    format!(
                        "effect `{}` is not available at placement `{target}`",
                        e.written
                    ),
                    e.span.clone(),
                )
                .related(
                    placement.span.clone(),
                    format!("placement `{target}` declared here"),
                )
                .explain(why)
                .repair(help),
            );
        }
    }
}

fn check_resource(decl: &Decl, out: &mut Vec<Finding>) {
    let name = decl.name.as_str();
    let noun = noun_of(decl.kind);
    let visibility = vis(decl);
    let policies: &[Policy] = &decl.policies;
    let label = privacy_label(visibility);
    let name_span: Span = decl.name_span.clone();

    // A reader's value in a shared cache is the label algebra's (PW5001),
    // which reads what the declaration observes as well as its keyword. This
    // rule wrote PW0100 for the same clause until ADR-0239: one mistake, two
    // errors under two numbers.

    // --- PW0101: read_your_writes on a public read --------------------------
    if let Some(c) = policy(policies, "consistency")
        && c.value.trim() == "read_your_writes"
        && visibility == "public"
    {
        out.push(
            err(
                "PW0101",
                "read-your-writes requires a session-scoped read",
                "`read_your_writes` requires a session-scoped query",
                c.span.clone(),
            )
            .related(
                name_span.clone(),
                "declared `public` here, so there is no session to read the writes of",
            )
            .explain(
                "read-your-writes promises a reader the writes of its own session; a public \
                 read has no session, so the promise names no one",
            )
            .repair("use `consistency snapshot` for public data, or declare the query `session`"),
        );
    }

    // --- PW0343: a last-known-good fallback for data that is not public -----
    // Charter §15.6 test 18 (ADR-0177): "Origin failure follows last-known-good
    // policy only for declared public data."
    // What PW0100 reads as not public: labelled `session` or `private`, or
    // kept privately. A shared materialization is public by ADR-0128's rule,
    // and an unlabelled declaration is public to the manifest.
    if let Some(f) = policy(policies, "fallback")
        && f.value.trim() == "last_known_good"
        && (matches!(visibility, "session" | "private" | "user")
            || policy(policies, "cache").is_some_and(|c| c.value.trim() == "private"))
    {
        out.push(
            err(
                "PW0343",
                "a last-known-good fallback serves only public data",
                format!(
                    "`{name}` answers a read whose origin failed with its last value, and its \
                     result is `{label}`"
                ),
                f.span.clone(),
            )
            .related(
                name_span.clone(),
                format!("`{name}` is declared here, so its result is labeled `{label}`"),
            )
            .explain(
                "a public value is the same for every reader, and the last one is still true \
                 of no one in particular; a session's own value kept to answer for a failed \
                 origin would be a copy of private data nothing was asked to keep, and would \
                 hide the session's own writes",
            )
            .repair("remove `fallback last_known_good`, or declare the query `public`"),
        );
    }

    // --- PW0102: a stale session read ---------------------------------------
    if let Some(f) = policy(policies, "freshness")
        && visibility == "session"
        // A duration, read once (ADR-0089): `05.seconds` began with `0` and
        // was five seconds stale.
        && crate::policy::duration(&f.value).is_some_and(|ms| ms > 0)
    {
        out.push(
            err(
                "PW0102",
                "session-owned state may not be served stale",
                format!("session {noun} `{name}` declares a {} staleness window", f.value),
                f.span.clone(),
            )
            .related(name_span.clone(), "declared `session` here")
            .explain(
                "a session's own state changes when the session acts, and a copy served from a \
                 staleness window hides the change it just made",
            )
            .repair("use `freshness 0.seconds` with `consistency read_your_writes` for session-owned state"),
        );
    }

    // --- PW0303: retry without idempotency ----------------------------------
    //
    // Whatever the policy retries on (ADR-0173). A request that failed in
    // transit may have arrived, and whoever sent it cannot tell: a browser's
    // `fetch` fails alike whether the request never left or its answer was
    // lost after the server committed. Until 2026-10-04 `transport_only` was
    // let through, as though a transport failure said the server never saw
    // the request. `retry none` retries nothing.
    if let Some(r) = policy(policies, "retry")
        && noun == "command"
        && policy(policies, "idempotent_by").is_none()
        && r.value.trim() != "none"
    {
        out.push(
            err(
                "PW0312",
                "a retryable mutation must be idempotent",
                // The policy as written, as R-014 expects (ADR-0090).
                format!(
                    "`{name}` declares `retry {}` but is not idempotent",
                    r.value.split('(').next().unwrap_or_default().trim()
                ),
                r.span.clone(),
            )
            .related(
                name_span.clone(),
                "declared here with no `idempotent_by` key",
            )
            .explain(
                "retrying a non-idempotent mutation can apply it more than once: a request \
                 that failed in transit may have arrived, and its sender cannot tell",
            )
            // Both steps, as PW0338's: the clause names a type the program
            // may not have yet (PW0335 refuses the clause alone).
            .repair(
                "add `idempotent_by InteractionId`, and `opaque type InteractionId = String` if the program has none",
            ),
        );
    }

    // --- PW0304: unbounded retry --------------------------------------------
    if let Some(r) = policy(policies, "retry")
        && r.value.trim() == "forever"
    {
        out.push(
            err("PW0313", "a retry policy must be bounded", "`retry forever` is not a permitted policy", r.span.clone())
                .related(name_span.clone(), format!("`{name}` would retry without end"))
                .explain("an unbounded retry is a self-inflicted denial of service")
                .repair("declare a finite bound with jitter, e.g. `bounded_exponential(max = 3, jitter = true)`"),
        );
    }

    // --- PW0327: `rollback` is derived, not written -------------------------
    //
    // Architect ruling, 2026-08-11 (ADR-0025), retiring the rule this code
    // used to be:
    //
    // > "an optimistic transition is a function from the current value to the
    // > next, and rollback is its inverse" — the first half is excellent. The
    // > second is generally false.
    //
    // A cart holding `Apple x 3`, optimistically `+2`, is `Apple x 5`; a
    // hand-written `remove(apple)` does not restore `Apple x 3`, and even
    // `subtract(2)` fails once there are concurrent updates, normalization, or
    // derived fields. The platform knows the exact value it held, so requiring
    // the author to describe an inverse was requiring a description of
    // something the runtime will not use.
    //
    // The code is REUSED rather than retired, because its subject is the same
    // declaration and a reader looking it up should find what replaced it.
    if let Some(r) = policy(policies, "rollback") {
        out.push(
            err(
                "PW0327",
                "an optimistic transition's reversal is derived, not written",
                format!("`{name}` declares a `rollback`"),
                r.span.clone(),
            )
            .related(
                name_span.clone(),
                format!("the platform restores what `{name}` held before"),
            )
            .explain(
                "the platform restores the resource value it held before the speculative one, \
                 which it knows exactly. A written inverse describes a different operation: \
                 reversing `add(2)` with `remove(..)` does not restore the previous value once \
                 there are concurrent updates, normalization, or derived fields",
            )
            .repair("delete the `rollback` clause"),
        );
    }

    // --- a key that changes: `signals.rs`'s PW5309, no longer here ----------
    //
    // Until the stale-key ADR, PW0325 asked a keyed query alone for its
    // `on_key_change`, and only where it declared a `concurrency`: so of a
    // query a URL keys, whose key no page changes, and not of the same query
    // without one. A key changes while its read is in flight only where a
    // page's signal keys the query (ADR-0152), and there PW5309 asks for the
    // policy whatever else the query declares. PW0325 is an old number for
    // PW5309 now, which `diagnostics.rs` lists.

    // --- PW0307: a subscription escaping its component ----------------------
    if let Some(s) = policy(policies, "scope")
        && matches!(noun, "subscription" | "resource")
        && matches!(s.value.trim(), "application" | "session")
    {
        out.push(
            // ONE code per invariant (architect ruling). `PW2004` is canonical:
            // "a resource cannot outlive the scope that owns it". This detector
            // finds it from the declaration header; `pw-core`'s scope graph finds
            // the same invariant from handle flow. `PW0326` is an old number the
            // corpus wrote for it, read nowhere a code is compared (track
            // evidence-integrity-2): the old numbers are listed in
            // `diagnostics.rs`, for the reference that prints them.
            err(
                "PW2004",
                "a resource cannot outlive the scope that owns it",
                format!("{noun} `{name}` declares `{}` scope", s.value),
                s.span.clone(),
            )
            .reason("declared_scope_exceeds_owner")
            .related(name_span.clone(), "created inside a component")
            .explain(
                "the subscription would outlive the component and push updates into a dead scope",
            )
            .repair(
                "use `scope component`, or hoist the declaration to the scope you actually want",
            ),
        );
    }

    // --- PW0200 (warning): a shared cache with nothing to invalidate it -----
    if let Some(cache) = policy(policies, "cache")
        && cache.value.trim() == "shared"
        && visibility == "public"
        && policy(policies, "freshness").is_none()
        && policy(policies, "invalidates_on").is_none()
        && policy(policies, "invalidates").is_none()
    {
        out.push(
            warn(
                "PW0200",
                "a shared materialization needs a freshness or invalidation source",
                format!("shared materialization of `{name}` declares no freshness policy"),
                cache.span.clone(),
            )
            .related(name_span.clone(), format!("`{name}` is cached for every reader"))
            .explain("with neither a freshness window nor an invalidation source, a shared entry is unbounded")
            .repair("add `freshness <n>.seconds`, or an `invalidates_on <Event>` clause"),
        );
    }
}

/// **PW0332 — an effect is not a callable, so it does not name an operation.**
///
/// Its own pass, run for EVERY declaration. It first sat inside
/// `check_resource`, which is called only for `is_resource_like(kind)` — so an
/// effect never reached it and the rule was silent. That is the enumeration
/// pattern `docs/RISK_QUEUE.md` records, caught here by the discriminator half
/// of its own test rather than by review.
/// **PW0332 — an effect is not a callable, so it does not name an operation.**
///
/// Its own pass, run for EVERY declaration. It first sat inside
/// [`check_resource`], which is called only when `is_resource_like(kind)` — so
/// an effect never reached it and the rule was silent while looking present.
/// That is the enumeration pattern `docs/RISK_QUEUE.md` records, and what
/// caught it was the discriminator half of its own test rather than review.
fn check_effect_names_no_operation(decl: &Decl, out: &mut Vec<Finding>) {
    // `effect session.read { host "pw:host/session#read" }` parsed, was
    // recorded in the ontology, and was read by NOTHING. It is the fossil of
    // deriving an operation from a capability, which ADR-0026 killed:
    //
    //     effect      what computation does
    //     capability  the authority
    //     operation   the callable ABI
    //
    // While it existed, a reader walking declarations could answer *which
    // operation is this* with an effect's clause instead of a callable's — and
    // one did, rendering the effect's empty signature over the function's real
    // one. Architect ruling, 2026-08-20:
    //
    // > Don't merely stop consuming it. You've learned repeatedly that
    // > semantically dead syntax survives for a long time if it still parses.
    //
    // So it is a diagnostic and not a silent ignore. The two failures are
    // different and must not be observationally equivalent: an UNKNOWN policy
    // head is the parser's business; a KNOWN head on a declaration that cannot
    // mean it is this.
    if decl.kind != DeclKind::Effect {
        return;
    }
    let Some(h) = policy(&decl.policies, "host") else {
        return;
    };
    let name = decl.name.as_str();
    out.push(
        err(
            crate::codes::EFFECT_NAMES_AN_OPERATION.id,
            crate::codes::EFFECT_NAMES_AN_OPERATION.invariant,
            format!("effect `{name}` declares a `host` operation"),
            h.span.clone(),
        )
        .related(
            decl.name_span.clone(),
            "an effect, which names an authority and no operation",
        )
        .explain(
            "a capability authorizes an operation and does not identify one (ADR-0026). Two \
             callables can require one authority and have different ABIs, so an effect cannot \
             stand for either of them. The operation belongs on the `fn` that performs it",
        )
        .repair(
            "delete the clause, and write `host \"interface#operation\"` on the bodiless `fn` \
             the deployment supplies",
        ),
    );
}

/// **PW0623: an invariant the language reads** (ADR-0179): bounds on an
/// `Int` representation's `value`, or on a `String` representation's length
/// (ADR-0225), each a comparison with an integer, joined by `&`, that some
/// value holds. Anything else is refused by name, so a predicate is never
/// kept as a claim nothing checks.
fn check_invariant(decl: &Decl, out: &mut Vec<Finding>) {
    let Some(inv) = &decl.invariant else {
        return;
    };
    let name = &decl.name;
    let refuse = |out: &mut Vec<Finding>, span: Span, message: String, repair: &str| {
        out.push(
            err(
                "PW0623",
                "an opaque type's invariant is bounds the build and a host can check",
                message,
                span,
            )
            .related(decl.name_span.clone(), format!("`{name}` is declared here"))
            .explain(
                "an invariant is checked where a value of the type is built, by the build, and \
                 where one arrives from outside, by the host, which runs none of the program's \
                 code; bounds on an `Int`, and on a `String`'s length in code points, are what \
                 both can decide",
            )
            .repair(repair),
        );
    };
    for (span, why) in &inv.unread {
        refuse(
            out,
            span.clone(),
            format!("`{name}`'s invariant is not read: {why}"),
            "write bounds on `value`: `value >= 1`, or `value >= 0 & value <= 100`; or on a \
             `String`'s length: `String.length(value) <= 280`",
        );
    }
    // An `Int`'s value, or a `String`'s length (ADR-0225), and each by its
    // own representation.
    let representation = decl.opaque_of.as_ref().map(|t| t.written());
    let (wanted, bounded) = match inv.measure {
        crate::hir::Measure::Value => ("Int", "`value`"),
        crate::hir::Measure::Length => ("String", "`String.length(value)`"),
    };
    if representation.as_deref() != Some(wanted) {
        refuse(
            out,
            inv.span.clone(),
            format!(
                "`{name}` states an invariant on {bounded} over `{}`, which only an `{wanted}` \
                 has",
                representation.unwrap_or_default()
            ),
            &format!("state no invariant, or represent the type by an `{wanted}`"),
        );
    }
    // A bound past what an `Int` holds is no bound a host could compare a
    // value with, and the contract has nowhere to carry it.
    let beyond = |b: Option<i128>| b.is_some_and(|b| i64::try_from(b).is_err());
    if beyond(inv.at_least) || beyond(inv.at_most) {
        refuse(
            out,
            inv.span.clone(),
            format!(
                "`{name}`'s invariant, `{}`, bounds `value` beyond what an `Int` holds",
                inv.written
            ),
            "bound `value` within an `Int`'s range, -9223372036854775808 to 9223372036854775807",
        );
    }
    // A length is never below 0 (ADR-0225).
    let least = match inv.measure {
        crate::hir::Measure::Value => inv.at_least,
        crate::hir::Measure::Length => Some(inv.at_least.unwrap_or(0).max(0)),
    };
    if let (Some(lo), Some(hi)) = (least, inv.at_most)
        && lo > hi
    {
        refuse(
            out,
            inv.span.clone(),
            format!("`{name}`'s invariant, `{}`, holds of no value", inv.written),
            &format!("bound {bounded} so that some value holds it"),
        );
    }
}

/// **PW0625: a case is named with a capital letter** (ADR-0197, ADR-0195's
/// ruling 2). A pattern's name is a case by its capital and a binding
/// otherwise, so a case named in lowercase could never be matched by name:
/// `active` in a pattern would bind, and match everything.
fn check_case_names(decl: &Decl, out: &mut Vec<Finding>) {
    for v in decl.variants.iter().flatten() {
        if pw_syntax::pattern_kind(&v.name) == pw_syntax::PatternKind::Case {
            continue;
        }
        let name = &v.name;
        let mut capital = name.clone();
        if let Some(first) = capital.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        out.push(
            err(
                "PW0625",
                "a sum type's case is named with a capital letter, so a pattern tells it \
                 from a binding",
                format!(
                    "`{}`'s case `{name}` is not named with a capital letter",
                    decl.name
                ),
                v.span.clone(),
            )
            .related(
                decl.name_span.clone(),
                format!("`{}` is declared here", decl.name),
            )
            .explain(
                "a pattern's name is a case when it begins with a capital letter, and a new \
                 binding otherwise (as in Haskell, OCaml and Elm), so a case named in \
                 lowercase would be read as a binding wherever a pattern names it, and match \
                 every value",
            )
            .repair(format!("name it `{capital}`")),
        );
    }
}

/// Run every declaration-level rule over one program's HIR.
///
/// Nested declarations included (`all_decls`), which the AST walk did not do:
/// a `fn` inside a component is a declaration with an effect row, and its
/// placement conflict is the same conflict.
pub fn check(hir: &Hir) -> Vec<Finding> {
    let mut out = Vec::new();
    for (_, decl) in hir.all_decls() {
        if is_resource_like(decl.kind) {
            check_resource(decl, &mut out);
        }
        check_effects_against_placement(decl, &mut out);
        check_effect_names_no_operation(decl, &mut out);
        check_invariant(decl, &mut out);
        check_case_names(decl, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pw_syntax::parse_tree;

    fn findings(src: &str) -> Vec<Finding> {
        let p = parse_tree(src);
        assert!(p.ok(), "parse errors: {:?}", p.errors);
        check(&crate::lower::lower_file(src, &p.green))
    }

    fn codes(src: &str) -> Vec<&'static str> {
        findings(src).iter().map(|f| f.code).collect()
    }

    #[test]
    fn read_your_writes_on_a_public_query_is_rejected() {
        let src = "module s\npublic query Store(id: StoreId) -> Store\n    consistency read_your_writes\n{\n    0\n}\n";
        assert!(codes(src).contains(&"PW0101"));
    }

    #[test]
    fn a_last_known_good_fallback_is_public_data_alone() {
        // ADR-0177, charter §15.6 test 18.
        let public = "module c\npublic query Store(id: Int) -> Int\n    freshness 30.seconds\n    cache shared\n    fallback last_known_good\n{\n    0\n}\n";
        assert!(!codes(public).contains(&"PW0343"));
        for (what, src) in [
            (
                "a session's",
                "module c\nsession query Cart(s: Int) -> Int\n    freshness 0.seconds\n    fallback last_known_good\n{\n    0\n}\n",
            ),
            (
                "one kept privately",
                "module c\npublic query Mine(id: Int) -> Int\n    cache private\n    fallback last_known_good\n{\n    0\n}\n",
            ),
        ] {
            assert!(codes(src).contains(&"PW0343"), "{what}: {:?}", codes(src));
        }
        // A shared materialization of public data, as the store's menu
        // fragment is.
        let materialized = "module c\nmaterialize Fragment(id: Int) {\n    partition public\n    fallback last_known_good\n}\n";
        assert!(
            !codes(materialized).contains(&"PW0343"),
            "{:?}",
            codes(materialized)
        );
        // `fallback empty` keeps nothing.
        let empty = "module c\nsession query Cart(s: Int) -> Int\n    freshness 0.seconds\n    fallback empty\n{\n    0\n}\n";
        assert!(!codes(empty).contains(&"PW0343"));
    }

    #[test]
    fn a_stale_session_query_is_rejected_but_a_fresh_one_is_not() {
        let stale = "module c\nsession query Cart(s: SessionId) -> Cart\n    freshness 30.seconds\n{\n    0\n}\n";
        assert!(codes(stale).contains(&"PW0102"));
        let fresh = "module c\nsession query Cart(s: SessionId) -> Cart\n    freshness 0.seconds\n{\n    0\n}\n";
        assert!(!codes(fresh).contains(&"PW0102"));
    }

    #[test]
    fn retry_without_idempotency_is_rejected() {
        let bad = "module p\ncommand charge(o: OrderId) -> Receipt\n    requires SignedIn\n    retry bounded_exponential(max = 5, jitter = true)\n{\n    0\n}\n";
        assert!(codes(bad).contains(&"PW0312"));
        let good = "module p\ncommand charge(o: OrderId) -> Receipt\n    requires SignedIn\n    idempotent_by InteractionId\n    retry bounded_exponential(max = 5, jitter = true)\n{\n    0\n}\n";
        assert!(!codes(good).contains(&"PW0312"));
    }

    #[test]
    fn unbounded_retry_is_rejected() {
        let src =
            "module s\npublic query Store(id: StoreId) -> Store\n    retry forever\n{\n    0\n}\n";
        assert!(codes(src).contains(&"PW0313"));
    }

    #[test]
    fn a_written_rollback_is_rejected_and_an_optimistic_alone_is_not() {
        // The inversion of the rule this code used to carry, and it is a real
        // inversion rather than a deletion: `optimistic` alone was the error
        // and is now correct; `rollback` was required and is now refused.
        let bad = "module c\ncommand add(i: ItemId) -> Cart\n    optimistic Cart() as c => c.add(i)\n    rollback c => c.remove(i)\n{\n    0\n}\n";
        assert!(codes(bad).contains(&"PW0327"));
        let good = "module c\ncommand add(i: ItemId) -> Cart\n    optimistic Cart() as c => c.add(i)\n{\n    0\n}\n";
        assert!(!codes(good).contains(&"PW0327"));
    }

    #[test]
    fn an_effect_naming_a_host_operation_is_rejected_and_a_fn_naming_one_is_not() {
        // The discriminator the architect asked for: a KNOWN policy on a
        // declaration that cannot mean it. The same clause on the callable that
        // performs the effect is correct and must stay silent — otherwise this
        // would read as "`host` is refused", which is the opposite of ADR-0026.
        let bad = "module p\n\neffect session.read {\n    capability session.read\n    \
                   host \"pw:host/session#read\"\n}\n";
        assert!(codes(bad).contains(&"PW0332"), "{:?}", codes(bad));

        let good = "module p\n\nfn current_session() -> Session<SessionId> !{ session.read }\n    \
                    host \"pw:host/session#read\"\n";
        assert!(!codes(good).contains(&"PW0332"), "{:?}", codes(good));

        // And an effect without the clause is unaffected, so the rule is about
        // the clause and not about effects.
        let plain = "module p\n\neffect session.read {\n    capability session.read\n}\n";
        assert!(!codes(plain).contains(&"PW0332"));
    }

    #[test]
    fn a_subscription_declaring_application_scope_is_rejected() {
        let src = "module o\nsubscription Tracking(o: OrderId) -> Stream\n    scope application\n{\n    0\n}\n";
        assert!(
            codes(src).contains(&"PW2004"),
            "PW2004 is the canonical invariant code"
        );
        let ok = "module o\nsubscription Tracking(o: OrderId) -> Stream\n    scope component\n{\n    0\n}\n";
        assert!(!codes(ok).contains(&"PW2004"));
    }

    #[test]
    fn an_effect_the_placement_cannot_grant_is_rejected() {
        let src = "module e\nview Receipt(o: OrderId) !{ secret<Payments> }\n    placement edge\n{\n    0\n}\n";
        let f = findings(src);
        let e = f.iter().find(|f| f.code == "PW5005").expect("PW5005");
        assert!(e.message.contains("secret"), "{}", e.message);
        assert!(
            !e.related.is_empty(),
            "must name where the placement was declared"
        );
        assert!(
            e.explanation
                .as_ref()
                .unwrap()
                .contains("edge world grants no secret")
        );
    }

    #[test]
    fn a_browser_only_effect_at_origin_is_rejected() {
        let src = "module d\nview Estimate(s: SessionId) !{ device.location }\n    placement origin\n{\n    0\n}\n";
        assert!(codes(src).contains(&"PW5005"));
    }

    #[test]
    fn a_shared_cache_with_no_invalidation_warns_but_does_not_error() {
        let src =
            "module s\npublic query Store(id: StoreId) -> Store\n    cache shared\n{\n    0\n}\n";
        let f = findings(src);
        let w = f.iter().find(|f| f.code == "PW0200").expect("PW0200");
        assert!(!w.is_error(), "this is advisory until E6");
    }

    #[test]
    fn a_finding_records_its_detector_and_reason_as_metadata() {
        // The developer learns the invariant; tooling learns which pass found
        // it and which repair applies.
        let src = "module o\nsubscription Tracking(o: OrderId) -> Stream\n    scope application\n{\n    0\n}\n";
        let f = findings(src);
        let v = f.iter().find(|f| f.code == "PW2004").expect("PW2004");
        assert_eq!(v.reason, "declared_scope_exceeds_owner");
        assert_eq!(v.detector, Detector::DeclarationRule);
    }

    #[test]
    fn the_rules_can_go_green_as_well_as_red() {
        // docs/RISK_QUEUE.md: a check that cannot fail is not a check — and one
        // that always fails is just as useless.
        let clean = "module s\npublic query Store(id: StoreId) -> Store\n    freshness 30.seconds\n    consistency snapshot\n    cache shared\n    invalidates_on StoreChanged(id)\n    on_key_change cancel\n    concurrency one_per_key\n{\n    Stores.get(id)\n}\n";
        assert!(findings(clean).is_empty(), "{:?}", findings(clean));
    }
}

#[cfg(test)]
mod corpus_tests {
    use super::*;
    use pw_syntax::parse_tree;
    use std::path::{Path, PathBuf};

    fn corpus(bucket: &str) -> Vec<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join(bucket);
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .expect("corpus dir")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        v.sort();
        v
    }

    /// The property that matters most: a rule that fires on a program the
    /// corpus says is CORRECT is worse than a rule that misses one.
    #[test]
    fn no_accepted_corpus_file_produces_an_error() {
        let mut offenders = Vec::new();
        for path in corpus("accepted") {
            let src = std::fs::read_to_string(&path).expect("read");
            let p = parse_tree(&src);
            assert!(p.ok(), "{} failed to parse: {:?}", path.display(), p.errors);
            let hir = crate::lower::lower_file(&src, &p.green);
            for f in check(&hir).into_iter().filter(|f| f.is_error()) {
                offenders.push(format!(
                    "{}: [{}] {}",
                    path.file_name().unwrap().to_string_lossy(),
                    f.code,
                    f.message
                ));
            }
        }
        assert!(
            offenders.is_empty(),
            "false positives on accepted programs:\n{}",
            offenders.join("\n")
        );
    }

    /// Where a rejected file IS caught, it must be caught by the rule its
    /// `@rule` header declares — not merely by something.
    #[test]
    fn every_caught_rejection_matches_its_declared_rule() {
        let mut mismatches = Vec::new();
        let mut caught = 0;
        for path in corpus("rejected") {
            let src = std::fs::read_to_string(&path).expect("read");
            let p = parse_tree(&src);
            // `@rule:` is a doc attribute in a comment, so it is read from the
            // source text rather than from either tree.
            let Some(expected) = src
                .lines()
                .find_map(|l| l.trim().strip_prefix("// @rule:"))
                .map(str::trim)
            else {
                continue;
            };
            let hir = crate::lower::lower_file(&src, &p.green);
            let found: Vec<&str> = check(&hir)
                .into_iter()
                .filter(|f| f.is_error())
                .map(|f| f.code)
                .collect();
            if found.is_empty() {
                continue; // needs body parsing or type checking; see below
            }
            caught += 1;
            // As written: an old number is not the code `pw check` reports
            // (track evidence-integrity-2).
            if !found.contains(&expected) {
                mismatches.push(format!(
                    "{}: declares @rule {expected}, got {found:?}",
                    path.file_name().unwrap().to_string_lossy()
                ));
            }
        }
        assert!(caught > 0, "no rejected file was caught at all");
        assert!(
            mismatches.is_empty(),
            "caught by the wrong rule:\n{}",
            mismatches.join("\n")
        );
    }

    /// Records how much of the rejected corpus declaration-level rules cover.
    ///
    /// This is a **ratchet**, not a target. It exists so that coverage cannot
    /// silently regress, and so the honest number is in the test suite rather
    /// than only in prose. Raise the floor when a milestone raises the coverage.
    #[test]
    fn declaration_level_coverage_of_the_rejected_corpus_does_not_regress() {
        let mut caught = 0;
        let mut total = 0;
        for path in corpus("rejected") {
            total += 1;
            let src = std::fs::read_to_string(&path).expect("read");
            let p = parse_tree(&src);
            let hir = crate::lower::lower_file(&src, &p.green);
            if check(&hir).iter().any(|f| f.is_error()) {
                caught += 1;
            }
        }
        // E2 checks declaration HEADERS only. The rest need body parsing (E2
        // continuation), effect checking (E1), or privacy/placement solving (E5).
        //
        // **The floor moved from 4 to 3 on 2026-08-11, and it is a movement to
        // record rather than a regression.** `R-029` was caught here for
        // `optimistic` without `rollback`; ADR-0025 retired that invariant —
        // the platform restores the value it held, so a written inverse
        // describes a different operation — and the fixture now carries
        // `optimistic_not_pure`, which is an EFFECT question. `check.rs`
        // catches it, `rules.rs` cannot, and a header rule that could would be
        // guessing at what a call performs.
        //
        // A floor going down is the shape `docs/RISK_QUEUE.md` warns about, so
        // the compensating assertion is below: R-029 is still caught, by the
        // whole checker, for its declared code.
        assert!(
            caught >= 3,
            "declaration-level coverage regressed: {caught}/{total} rejected files caught"
        );
        assert!(
            total >= 40,
            "expected the full rejected corpus, saw {total}"
        );

        // The fixture that left this count is still caught, by the checker as
        // a whole. Without this, lowering the floor would be indistinguishable
        // from losing a catch.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files: Vec<(String, String)> = Vec::new();
        for d in [
            "packages/pw-std",
            "packages/pw-platform-web",
            "examples/lib",
        ] {
            let mut ps: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
                .expect("dir")
                .map(|e| e.expect("entry").path())
                .filter(|p| p.extension().is_some_and(|x| x == "pw"))
                .collect();
            ps.sort();
            for p in ps {
                files.push((
                    p.file_name().unwrap().to_string_lossy().to_string(),
                    std::fs::read_to_string(&p).expect("read"),
                ));
            }
        }
        files.push((
            "domain.pw".to_string(),
            std::fs::read_to_string(root.join("examples/domain.pw")).expect("domain"),
        ));
        let r029 = std::fs::read_dir(root.join("examples/rejected"))
            .expect("rejected")
            .map(|e| e.expect("entry").path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("R-029")
            })
            .expect("R-029");
        files.push((
            "R-029.pw".to_string(),
            std::fs::read_to_string(&r029).expect("read"),
        ));
        let codes: Vec<&str> = crate::check::check_sources(&files)
            .iter()
            .flat_map(|(_, ds)| ds.iter())
            .map(|d| d.code)
            .collect();
        assert!(
            codes.contains(&"PW0330"),
            "R-029 left the declaration-level count and must still be caught \
             for its declared invariant: {codes:?}"
        );
    }
}
