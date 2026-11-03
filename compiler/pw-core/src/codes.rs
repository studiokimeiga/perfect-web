//! The one registry of diagnostic codes.
//!
//! Architect ruling, 2026-08-06:
//!
//! > Raw code strings should no longer be declared independently by parser and
//! > semantic passes. Every checker emits the enum, not a raw `"PW5010"`.
//!
//! The `PW0100` collision is why. It was simultaneously the parser's
//! "expected X, found Y" and `rules.rs`'s shared-cache invariant, so a corpus
//! fixture declaring `PW0100` could be satisfied by an unrelated *syntax* error
//! — the harness would go green while the semantic rule it meant to test had
//! never run.
//!
//! # Ranges
//!
//! ```text
//! PW00xx   lexical and syntax
//! PW01xx+  semantic invariants
//! PW50xx   placement, capability and unsafe boundaries
//! PW55xx   the identity track's (ADR-0253): accounts, sign-in, `requires`
//! PW56xx   the uploads track's (ADR-0253): typed uploads and blob storage
//! PW57xx   the notifications track's (ADR-0253): the typed principal, notifications
//! PW58xx   the messages track's (ADR-0253): direct messages
//! PW60xx   the kiokun track's (ADR-0253): kiokun.com in Pleris
//! PW61xx   the todomvc track's (ADR-0253): TodoMVC in Pleris
//! PW64xx   the payments track's (NEXT 6c): a command the platform invokes
//! PW66xx   check-is-build: executable program support
//! PW68xx   the device-store track's: a value the reader's device keeps
//! PW69xx   the dating-app note's H tracks (ADR-0384): predicates, private fields
//! ```
//!
//! A parallel track registers its codes in its own block, with its own
//! owner, and nothing else may (docs/PARALLEL.md).
//!
//! Ranges are enforced by a test, not by convention.

use std::fmt;

/// One diagnostic code, with everything the registry knows about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code {
    /// The stable public string, e.g. `"PW5002"`. For users and documentation.
    pub id: &'static str,
    /// The **semantic identity**, and the thing a fixture declares. Two rules
    /// may not share one, and a number may not mean two things — `PW0323`
    /// meant three at once and nothing could see it, because matching on the
    /// number alone is matching on a label rather than on a meaning.
    pub symbol: &'static str,
    /// Bumped when the invariant's MEANING changes, so a fixture pinned to an
    /// older revision fails loudly instead of silently checking something else.
    pub revision: u16,
    /// One sentence naming the invariant, in the developer's vocabulary.
    pub invariant: &'static str,
    /// Which analysis owns it. Metadata — never part of the public code.
    pub owner: Owner,
    /// The Rust constant's own name, so a test can ask whether any checker
    /// refers to this code — by constant or by literal — without a second
    /// hand-maintained list.
    pub konst: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Syntax,
    Resolution,
    Effects,
    DeclarationRules,
    Exhaustiveness,
    ScopeGraph,
    Placement,
    Privacy,
    Markup,
    /// Types: what a value is, and where it may be used as one.
    Types,
    /// Charter §7.5A relations that are not effect-row violations: an
    /// ordering, a cycle, a declared assertion that does not hold.
    Layout,
    /// E6: the resource dependency graph. What a materialization depends on,
    /// what invalidates it, and what its cache key must separate.
    ResourceGraph,
    /// E8: what a component's authority NAMES.
    ///
    /// Separate from `Placement`, which decides where a body may run given the
    /// capabilities it needs. These are about whether the capability is a
    /// capability at all — a family, an operation and a type argument that
    /// resolve to something the program declares.
    ///
    /// A capability nobody can grant is not a deployment problem to discover
    /// on a node. It is a source-program error with a span and an obvious
    /// repair.
    Capability,
    /// ADR-0130: UI state. Where a signal may be read and written, and what
    /// may hold UI state at all.
    UiState,
    /// ADR-0148: a streamed region. What reads a query declared `delivery
    /// streamed`, and the states a `<stream>` shows.
    Streaming,
    /// ADR-0253: the identity track's codes, PW55xx: accounts, sign-in and
    /// sign-out, a session's principal, and the `requires` evaluator.
    Identity,
    /// ADR-0253: the uploads track's codes, PW56xx: a typed upload, its
    /// limits, a deployment's blob storage, and serving it safely.
    Uploads,
    /// ADR-0253: the notifications track's codes, PW57xx: the typed
    /// principal (`context.current_user()`, a user's handle the host's alone
    /// to make) and the notifications on it.
    Notifications,
    /// ADR-0253: the messages track's codes, PW58xx: direct messages, a
    /// conversation read from each side, and who may message whom.
    Messages,
    /// ADR-0253: the store-on-PostgreSQL track's codes, PW59xx: the store's
    /// data behind the DataLayer seam on a database of its own.
    StorePg,
    /// ADR-0253: the kiokun track's codes, PW60xx: kiokun.com, the owner's
    /// dictionary, rewritten in Pleris (docs/PARALLEL.md, W6's plan).
    Kiokun,
    /// ADR-0253: the store-accounts track's codes, PW62xx: the DoorDash
    /// store's customer side, a cart and an order a user's, and delivery
    /// addresses (docs/PARALLEL.md, W8's plan).
    StoreAccounts,
    /// ADR-0253: the store-search track's codes, PW63xx: the DoorDash
    /// store's customer search (docs/PARALLEL.md, W9). None registered yet.
    StoreSearch,
    /// The payments track's codes, PW64xx (NEXT 6c, the integrator's): a
    /// command the platform invokes on its provider's word, and no reader
    /// presses.
    Payments,
    /// The scheduled-commands track's codes, PW65xx (the integrator's): a
    /// command the platform runs when the clock passes its time.
    Schedules,
    /// Full executable-program support, reserved for check-is-build.
    CheckIsBuild,
    /// ADR-0253: the todomvc track's codes, PW61xx: TodoMVC in Pleris, held
    /// to its own spec and tests (docs/PARALLEL.md, W7's plan). A signal a
    /// page binds to its address's fragment.
    Todomvc,
    /// The device-store track's codes, PW68xx (ADR-0366, a block asked of
    /// the integrator): a value the reader's device keeps, charter §9.1's
    /// fourth kind of state, and the `Device` label that keeps it there.
    DeviceStore,
    /// The dating-app note's H tracks' codes, PW69xx (ADR-0384's ruling 1):
    /// what is not a table clause's refusal. Track `row-ownership` (H1, H2):
    /// a predicate's body, `requires` on a query, a private field's write.
    Dating,
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id)
    }
}

macro_rules! codes {
    ($( $konst:ident = $id:literal / $symbol:ident / $rev:literal, $owner:ident, $invariant:literal; )*) => {
        $(
            pub const $konst: Code = Code {
                id: $id,
                symbol: stringify!($symbol),
                revision: $rev,
                invariant: $invariant,
                owner: Owner::$owner,
                konst: stringify!($konst),
            };
        )*

        /// Every registered code. The registry's own source of truth.
        pub const ALL: &[Code] = &[ $($konst),* ];
    };
}

codes! {
    // --- check-is-build (PW66xx) ------------------------------------------
    EXECUTABLE_PROGRAM = "PW6601" / executable_program / 1, CheckIsBuild,
        "a semantically valid program is supported by the compiler's executable backends";

    // --- syntax (PW00xx) --------------------------------------------------
    EXPECTED = "PW0001" / expected / 1, Syntax, "the parser expected a different token here";
    UNCLOSED_TYPE_ARGS = "PW0002" / unclosed_type_args / 1, Syntax, "a type argument list must be closed";
    UNCLOSED_EFFECT_ROW = "PW0003" / unclosed_effect_row / 1, Syntax, "an effect row must be closed";
    EFFECT_ROW_NEEDS_BRACE = "PW0004" / effect_row_needs_brace / 1, Syntax, "an effect row opens with `{`";
    UNCLOSED_PARAMS = "PW0005" / unclosed_params / 1, Syntax, "a parameter list must be closed";
    UNCLOSED_BLOCK = "PW0006" / unclosed_block / 1, Syntax, "a block must be closed";
    UNKNOWN_DECLARATION = "PW0007" / unknown_declaration / 1, Syntax, "this does not begin a declaration";
    UNCLOSED_PAREN = "PW0008" / unclosed_paren / 1, Syntax, "a parenthesis must be closed";
    EXPECTED_EXPRESSION = "PW0009" / expected_expression / 1, Syntax, "an expression was expected here";
    UNCLOSED_ARGS = "PW0010" / unclosed_args / 1, Syntax, "an argument list must be closed";
    UNCLOSED_LIST = "PW0011" / unclosed_list / 1, Syntax, "a list literal must be closed";
    UNCLOSED_MATCH = "PW0012" / unclosed_match / 1, Syntax, "a match must be closed";
    // Its own code, not PW0009's "an expression was expected": ADR-0041 first
    // emitted PW0009 for it, which named the wrong invariant.
    // Revision 2, ADR-0195 (ruling 3): every word that begins a statement or
    // an expression, and a declaration named by one that begins an
    // expression; `let return = n` had checked.
    KEYWORD_AS_NAME = "PW0013" / keyword_as_name / 2, Syntax,
        "a word that begins a statement or an expression in a body names no binding, parameter or declaration";
    // ADR-0049: a string's escapes are the language's. Until 2026-09-25 a
    // backslash had no defined meaning (A-023), and backends either refused it
    // or passed it to their targets' rules.
    STRING_ESCAPE = "PW0014" / string_escape / 1, Syntax,
        "a string's escapes and holes must be ones the language defines";
    // ADR-0200: an expression the parser accepted and the lowering has no
    // meaning for was an error node the checker typed as anything, since a
    // parser's error says what is wrong; none did. `()` checked as an `Int`.
    UNREAD = "PW0015" / unread / 1, Syntax,
        "every expression and pattern in a file that parses is one the compiler reads";
    // ADR-0237: what the declaration grammar keeps as text and lowering
    // parses, a clause's value, a string's hole and a block marker's
    // expression. Its errors were dropped, so `invalidates Cart(s) Order(s)`
    // invalidated no order, and `{#if flag other}` was decided by `flag`.
    // The grammar emitted them as PW0103 to PW0105, numbers in the semantic
    // range no one registered, since no one saw them.
    READ_WHOLE = "PW0016" / read_whole / 1, Syntax,
        "a clause's value, a string's hole and a block marker's expression are read whole, and nothing follows what their grammar reads";
    OPTIMISTIC_CLAUSE = "PW0017" / optimistic_clause / 1, Syntax,
        "an optimistic clause names an entry, binds its value with `as`, and gives its transition after `=>`";
    // ADR-0239: the parser wrote PW0102 for this, a number the declaration
    // rules wrote for a stale session read too. Neither was registered.
    FOR_NEEDS_IN = "PW0018" / for_needs_in / 1, Syntax,
        "a `for` loop names what it iterates after `in`";
    // ADR-0242: an `{#each}`'s head was split at ` as ` in five places, and
    // one with no `as` was read as a list with no name each row binds.
    EACH_HEAD = "PW0019" / each_head / 1, Syntax,
        "an `{#each}` names its list, and after `as` the name each row binds";
    // ADR-0243: `{ a b }` was two statements, the first evaluated and
    // dropped, and checked. PW002x is name resolution's.
    STATEMENTS_SEPARATED = "PW0030" / statements_separated / 1, Syntax,
        "a block's statements are separated by `;` or a line";
    // ADR-0322: `a | b & c` was `(a | b) & c`, one power left to right, where
    // most languages read `a | (b & c)` (W9's finding, track store-hours).
    LOGICAL_MIXED = "PW0031" / logical_mixed / 1, Syntax,
        "`|` and `&` mixed in one chain are parenthesized: neither binds tighter";
    NO_PROGRESS = "PW0099" / no_progress / 1, Syntax, "the parser made no progress";

    // --- name resolution (PW002x) -----------------------------------------
    //
    // Allocated through this registry, not chosen in prose. They sit in the
    // syntax range because resolution failures are about the *program text*
    // naming something that is not there, not about what the program means.
    UNRESOLVED_MODULE = "PW0020" / unresolved_module / 1, Resolution,
        "an imported module must exist in the workspace";
    UNRESOLVED_NAME = "PW0021" / unresolved_name / 1, Resolution,
        "an imported name must be declared by the module it comes from";
    AMBIGUOUS_NAME = "PW0022" / ambiguous_name / 1, Resolution,
        "a name must resolve to exactly one declaration";
    PRIVATE_ACCESS = "PW0023" / private_access / 1, Resolution,
        "a private declaration is not visible outside its module";
    // ADR-0324: `private` says a declaration is not importable, and nothing
    // of whose data it holds; until 2026-10-12 it meant the user's too.
    PRIVATE_DATA = "PW0029" / private_data / 1, Resolution,
        "a declaration that holds data says whose with `user`, `session` or `public`, not `private`";
    DUPLICATE_DECLARATION = "PW0024" / duplicate_declaration / 1, Resolution,
        "a module may declare each name once per namespace";
    IMPORT_CYCLE = "PW0025" / import_cycle / 1, Resolution,
        "modules must not import each other in a cycle";
    // **A written type that names nothing.** `fn f(x: Stroe)` passed `pw
    // check` until 2026-09-24: the signature held `Unresolved`, every relation
    // correctly refused to run on it, and nothing told the author. E9-V5.
    UNRESOLVED_TYPE = "PW0026" / unresolved_type / 1, Resolution,
        "a written type must name a type visible here, applied to exactly its parameters";
    // ADR-0098: `fn f(x: Int, x: String)`, a field or a case declared twice,
    // `cache private` then `cache shared`, and `href` given twice checked
    // until 2026-09-26.
    DECLARED_TWICE = "PW0028" / declared_twice / 1, Resolution,
        "a parameter, a field, a case, a policy or an attribute is written once";
    // ADR-0087: a view, a page, an event or an effect called as a function
    // passed `pw check` until 2026-09-26, answered for as a call to nothing.
    NOT_A_TERM = "PW0027" / not_a_term / 1, Resolution,
        "a call names a function, a data operation, or a type it builds";

    // --- declaration rules (PW01xx-PW03xx) --------------------------------
    // ADR-0239: the declaration rules wrote these and the registry did not
    // have them, so a diagnostic carrying either had no symbol.
    READ_YOUR_WRITES_NEEDS_A_SESSION = "PW0101" / read_your_writes_needs_a_session / 1, DeclarationRules,
        "read-your-writes requires a session-scoped read";
    SESSION_STATE_IS_FRESH = "PW0102" / session_state_is_fresh / 1, DeclarationRules,
        "session-owned state may not be served stale";
    RETRY_NOT_IDEMPOTENT = "PW0312" / retry_not_idempotent / 1, DeclarationRules,
        "a command that retries must be idempotent";
    // ADR-0154: a browser's request can be delivered twice, whatever the
    // program does, and the platform gives each press an interaction. A
    // command a page's handler calls says how a second delivery is answered.
    SENT_WITHOUT_IDEMPOTENCY = "PW0338" / sent_without_idempotency / 1, DeclarationRules,
        "a command a page's handler calls declares `idempotent_by`";
    // ADR-0157: a command is one request, and its caller is answered whether
    // it committed. Called from a declaration, its body ran without its
    // policies, and the checker and the backend typed the call differently.
    COMMAND_OUTSIDE_A_HANDLER = "PW0339" / command_outside_a_handler / 1, DeclarationRules,
        "a command is called only by a page's handler";
    // ADR-0160: a page is served at its route, and the address gives its
    // parameters. A route that named one the page lacks, or left one out,
    // served a page with a parameter nothing gave.
    ROUTE_NAMES_ITS_PARAMETERS = "PW0340" / route_names_its_parameters / 1, DeclarationRules,
        "a route is `/` and segments, a word or a `{parameter}` each, and names each of its page's parameters once";
    ROUTE_DECLARED_TWICE = "PW0341" / route_declared_twice / 1, DeclarationRules,
        "one route is one page's: an address names the page it is for";
    // ADR-0163: a page that cannot be read was answered 503 whatever the
    // reason, and an address naming nothing is 404's.
    NOT_FOUND_NAMES_A_CASE = "PW0342" / not_found_names_a_case / 1, DeclarationRules,
        "a page's `not_found_on` names a case of an error a query it reads can answer";
    // ADR-0177: charter §15.6 test 18, origin failure follows last-known-good
    // only for declared public data.
    LAST_KNOWN_GOOD_IS_PUBLIC = "PW0343" / last_known_good_is_public / 1, DeclarationRules,
        "a last-known-good fallback serves only public data";
    // ADR-0207: a data source states what it guarantees, and nothing asks it
    // for more.
    READS_MORE_THAN_GIVEN = "PW0344" / reads_more_than_given / 1, DeclarationRules,
        "a query asks no more consistency of a source than its reads give";
    WRITES_TWO_SOURCES = "PW0345" / writes_two_sources / 1, DeclarationRules,
        "a command's writes are one transaction, in one source";
    ISOLATION_MORE_THAN_GIVEN = "PW0346" / isolation_more_than_given / 1, DeclarationRules,
        "a command asks no more isolation of a source than its transactions give";
    EVENTS_WITHOUT_COMMIT = "PW0347" / events_without_commit / 1, DeclarationRules,
        "a command's events are sent if and only if its writes commit";
    IDEMPOTENT_WITHOUT_COMMIT = "PW0348" / idempotent_without_commit / 1, DeclarationRules,
        "a command's record of an interaction commits with its writes";
    SOURCE_MALFORMED = "PW0349" / source_malformed / 1, DeclarationRules,
        "a source holds what the program's effects name, and each is held by one";
    // ADR-0295: a page whose address is another address of the page answers
    // a redirect there, as kiokun.com's word page does.
    REDIRECT_NAMES_A_CASE = "PW0350" / redirect_names_a_case / 1, DeclarationRules,
        "a page's `redirect_on` names a case a query it reads can answer, which carries the one parameter of the page's route, and says whether the move is permanent";
    // ADR-0302: a refusal is told in the words of the predicate that refused.
    PREDICATE_DECLARED = "PW0351" / predicate_declared / 1, DeclarationRules,
        "a predicate a program declares says, in one string with no hole, what a reader is told when it refuses, is declared once, and is given its parameters by type wherever a `requires` names it";
    RETRY_UNBOUNDED = "PW0313" / retry_unbounded / 1, DeclarationRules,
        "a retry policy must be bounded";
    // ADR-0303: a page is shown in its layout, which the pages that name it
    // share.
    LAYOUT_NAMES_A_LAYOUT = "PW0352" / layout_names_a_layout / 1, DeclarationRules,
        "a page's `layout` clause names a layout, once";
    LAYOUT_TAKES_NOTHING = "PW0353" / layout_takes_nothing / 1, DeclarationRules,
        "a layout is given nothing by the page it shows: it declares no parameters";
    // ADR-0338 (track `tables`, D1): an app's data, declared as tables, whose
    // generated functions are the program's only way to their rows. The
    // integrator's assignment (ruling 4); PW0354 to PW0360 and PW0363 are
    // the three-database note's other tracks'.
    TABLE_MALFORMED = "PW0361" / table_malformed / 1, DeclarationRules,
        "a table's row is a record declared beside it, in the module of its name, and its key, order, indexes and references name the row's fields, each of a type every adapter keys a row by";
    DATA_NOT_DECLARED = "PW0362" / data_not_declared / 1, DeclarationRules,
        "a table's rows are reached only through the functions its declaration gives";
    ALL_UNBOUNDED = "PW0364" / all_unbounded / 1, DeclarationRules,
        "a table is read whole, by `all()`, only where it states `bounded`";
    REFERS_ACROSS_SOURCES = "PW0365" / refers_across_sources / 1, DeclarationRules,
        "a reference to a table another source holds is held by no database: the referred row may be missing, which `get` answers as `None`";
    MIGRATION_UNSTATED = "PW0366" / migration_unstated / 1, DeclarationRules,
        "a change from the schema released that loses or reinterprets data is stated by a `migrate`, and each `migrate` states one";
    // Track `row-ownership` (ADR-0384's H1): whose a table's rows are, who
    // else reads them, and what their owner alone reads. The table clauses'
    // refusals are declaration rules, next free after D1's (the integrator's
    // ruling 1 on ADR-0384).
    OWNER_NAMES_A_PRINCIPAL = "PW0367" / owner_names_a_principal / 1, DeclarationRules,
        "`owned by` names one field of an owned table's row holding a principal's id (`capability.UserId`), and `shares with` others, never its owner, each once";
    // Its bound too (the review of the track, finding 2): a bound counts
    // every owner's rows, so a write refused by it would tell one owner how
    // many rows the others hold.
    OWNED_READ_BY_ITS_READER = "PW0368" / owned_read_by_its_reader / 1, DeclarationRules,
        "an owned table is read by its reader alone: its key leads with its owner, each index with its owner or a field it shares with, a unique index holds its owner, no bound counts its rows across owners, and it is never read whole";
    PRIVATE_FIELD_UNSEEN = "PW0369" / private_field_unseen / 1, DeclarationRules,
        "a private field is read by its owner alone: a field of an owned table's row, never its owner, a field it shares with, in its key, its order where it shares, or an index another reads by; and its seen row is the table's own name";

    STALE_KEY_POLICY = "PW0325" / stale_key_policy / 1, DeclarationRules,
        "a keyed query must say what happens when its key changes";
    // Renamed 2026-08-11 (ADR-0025). It required a written `rollback`; it now
    // refuses one, because the platform restores the value it held and a
    // hand-written inverse describes a different operation. Same code, same
    // declaration, opposite verdict — a reader looking it up finds what
    // replaced it rather than a dead entry.
    WRITTEN_ROLLBACK = "PW0327" / written_rollback / 1, DeclarationRules,
        "an optimistic transition's reversal is derived, not written";
    OPTIMISTIC_NOT_PURE = "PW0330" / optimistic_not_pure / 1, DeclarationRules,
        "an optimistic transition must be a pure function of the resource's value";
    // ADR-0082: the charter's `derived` is a pure value computed from other
    // values, and until 2026-09-26 nothing held it to that.
    // ADR-0226: and a value a template computes, which a host computes now.
    DERIVED_NOT_PURE = "PW0334" / derived_not_pure / 2, DeclarationRules,
        "a `derived` value, and a value a template computes, is computed from other values, and performs no effect";
    // ADR-0089: `cache Shared`, `placement originn` and `retry nope(..)`
    // checked until 2026-09-26, and each reader of the clause decided alone
    // what it meant.
    POLICY_VALUE = "PW0335" / policy_value / 1, DeclarationRules,
        "a policy's value is one its domain has";
    OPTIMISTIC_TARGET_MISMATCH = "PW0331" / optimistic_target_mismatch / 1, DeclarationRules,
        "an optimistic transition must produce the value type of the resource it targets";
    // ADR-0107: `query Other(id, other)` with `key id` and a body reading
    // `other` checked until 2026-09-26, and two calls shared one entry.
    KEY_OMITS_A_READ = "PW0336" / key_omits_a_read / 1, DeclarationRules,
        "a cache key names each parameter its entry depends on";
    // **A known policy in the wrong place**, which is a different failure from
    // an unknown one. `effect a.b { host ".." }` parsed, was recorded, and was
    // read by nothing — and while it existed a reader walking declarations could
    // answer "which operation is this" with an effect's clause instead of a
    // callable's. Architect ruling, 2026-08-20:
    //
    // > Don't merely stop consuming it. You've learned repeatedly that
    // > semantically dead syntax survives for a long time if it still parses.
    EFFECT_NAMES_AN_OPERATION = "PW0332" / effect_names_an_operation / 1, DeclarationRules,
        "an effect is not a callable, so it does not name a host operation";
    CACHE_NO_INVALIDATION = "PW0200" / cache_no_invalidation / 1, DeclarationRules,
        "a shared cache should declare how it is invalidated";

    // --- effects (PW04xx) -------------------------------------------------
    // ADR-0351 (the generic-effects track; the number is the integrator's to
    // confirm): a member is read through its receiver's type, so a value
    // whose type nothing states, read as `x.label`, names a member no
    // analysis can read, nor what it performs. Until then each analysis
    // skipped it, and a `!{}` row held over a database read.
    MEMBER_RECEIVER_UNTYPED = "PW0408" / member_receiver_untyped / 1, Effects,
        "a member is read through a value whose type the program states, so what it performs is read";
    UNDECLARED_EFFECT = "PW0400" / undeclared_effect / 1, Effects,
        "an effect row must name every effect the body performs";
    FORBIDDEN_EFFECT = "PW0401" / forbidden_effect / 1, Effects,
        "some effects are not permitted where a declaration runs, whatever it declares";
    WRONG_FRAME_PHASE = "PW0402" / wrong_frame_phase / 1, Effects,
        "each frame phase permits only the work it exists to do";
    // 4c, a read is bounded by construction (track `bounded-checker`): all
    // three read off the inferred row, where its source reads are, so
    // Effects'. The integrator assigned the first two, 2026-10-14. A read
    // keyed by a loop's element is a read per element (N+1), refused; and a
    // set read on a source that cannot batch is a call per key, warned. The
    // third is the integrator's ruling of 2026-10-15: a `set` that does not
    // fit its keyed read pairs it with nothing, and is the pair's own rule
    // (it was a policy's value, PW0335), so each refusal names it.
    READ_PER_ELEMENT = "PW0405" / read_per_element / 1, Effects,
        "a read in a loop over data is not keyed by the loop's element: the list's reads are one set read, before the loop";
    SET_READ_PER_KEY = "PW0406" / set_read_per_key / 1, Effects,
        "a page's or a command's calls do not grow with its data: a set read is on a source that batches";
    SET_PAIR_SHAPE = "PW0407" / set_pair_shape / 1, Effects,
        "a keyed read's set form is a function of the same source, given a list of its key and answering a list of its results, each with its key";

    // --- layout relations (PW04xx) ----------------------------------------
    OBSERVATION_FEEDBACK_CYCLE = "PW0403" / observation_feedback_cycle / 1, Layout,
        "an observation must not cause the change it observes";
    FALSE_INDEPENDENCE = "PW0404" / false_independence / 1, Layout,
        "a subtree declared independent must not depend on anything outside it";

    // --- exhaustiveness ---------------------------------------------------
    NON_EXHAUSTIVE_MATCH = "PW0305" / non_exhaustive_match / 1, Exhaustiveness,
        "a match must cover every value its scrutinee can take";
    // ADR-0076. The analysis found unreachable arms from the start, and until
    // 2026-09-26 nothing reported one.
    UNREACHABLE_ARM = "PW0333" / unreachable_arm / 1, Exhaustiveness,
        "every arm of a match must be reached by some value";
    // ADR-0153: in a template a case is tested with `{#match}`, which PW0305
    // holds to every case. An `{#if}` chain over a value's cases is held to
    // none: one that forgets a case added later shows nothing for it.
    CASE_TESTED_BY_IF = "PW0337" / case_tested_by_if / 1, Exhaustiveness,
        "a template tests a value's case with `{#match}`, which names every case";

    // --- types (PW06xx) ---------------------------------------------------
    OPTION_USED_AS_VALUE = "PW0600" / option_used_as_value / 1, Types,
        "a value that may be absent must be matched before it is used";
    UNCHECKED_EXTERNAL_CAST = "PW0601" / unchecked_external_cast / 1, Types,
        "an external value must be decoded, not cast";
    HANDLER_SIGNATURE_MISMATCH = "PW0602" / handler_signature_mismatch / 1, Types,
        "a handler must accept the event its attribute delivers";
    CONSTRUCTOR_ARITY = "PW0603" / constructor_arity / 1, Types,
        "a constructor pattern must bind exactly the fields its constructor declares";
    // **The first check of an ordinary call.** Until 2026-08-20 no call site
    // was checked at all — not the arity, not the argument types, not the
    // result — while `docs/MILESTONES.md` recorded E9, *permanent value type
    // checker*, as complete. Arity first because it needs no inference: the
    // callee's declaration says how many parameters it has, and the call says
    // how many arguments it passes. See `docs/RISK_QUEUE.md`.
    CALL_ARITY = "PW0604" / call_arity / 1, Types,
        "a call must supply exactly the arguments its callee declares";
    // **The value relations**, 2026-09-24 (E9-V1, V3, V4). By resolved
    // identity: two opaque types with one representation are two types.
    ARGUMENT_TYPE = "PW0605" / argument_type / 1, Types,
        "a value given for a declared parameter or field must have its declared type";
    RETURN_TYPE = "PW0606" / return_type / 1, Types,
        "a body must produce the result type its signature declares";
    BINDING_TYPE = "PW0607" / binding_type / 1, Types,
        "an annotated binding must be initialised with a value of its declared type";
    // Until 2026-09-25 a comparison was typed `Bool` whatever it compared,
    // and `1 == "a"` checked; the backend was the first to refuse it.
    // Ruling 0071-a (ADR-0230): a condition is a `Bool`, or a `List` or a
    // `String` tested non-empty, where a number and a record were the
    // renderer's truth.
    OPERAND_TYPE = "PW0609" / operand_type / 2, Types,
        "an operator's operands, and a condition, must have the types they take";
    // Until 2026-09-25 such a pattern read as a wildcard, so `Ok(x)` and
    // `Err(e)` arms proved a match over an `Option` exhaustive. Revision 2
    // (ADR-0059): a case built through its type, `Shape.Bogus(1)`, is held to
    // the same invariant; until 2026-09-26 it was undecided. Revision 3
    // (ADR-0197): a pattern's name is a case by its capital, so a bare
    // `Circel` its type lacks is held to it too; it had been a binding.
    PATTERN_CONSTRUCTOR = "PW0608" / pattern_constructor / 3, Types,
        "a constructor must name a case of the type it builds or matches";
    // Until 2026-09-25 a member the type does not have was unknown, and A-015
    // read `box.x` from a snapshot of a `Rect`, which has no `x` (ADR-0048).
    UNKNOWN_MEMBER = "PW0610" / unknown_member / 1, Types,
        "a read or a call through a value must name a member its type has";
    // ADR-0051: `x = e` compiles now, and until 2026-09-25 nothing said which
    // bindings may be assigned: a parameter, or a `let` without `mut`, was.
    ASSIGN_IMMUTABLE = "PW0611" / assign_immutable / 1, Types,
        "an assignment's target must be a binding declared `let mut`";
    // ADR-0067: until 2026-09-26 a record built by its fields' names was
    // checked field by field against the fields it gave, and nothing else. A
    // field left out, a field the type does not declare, and a field given
    // twice each passed, and the backend was the first to refuse them.
    RECORD_FIELDS = "PW0612" / record_fields / 1, Types,
        "a record must be built with each field its type declares, once, and no other";
    // ADR-0068: until 2026-09-26 the branches of an `if` or a `match` were
    // related to nothing but a declared result, so `let x = if c { 1 } else
    // { "a" }` passed.
    BRANCH_TYPES = "PW0613" / branch_types / 1, Types,
        "the branches of an `if` or a `match` whose value is used must produce one type";
    // ADR-0068: a call through a function value was checked by nothing until
    // 2026-09-26: not its arguments, not its result, not that the value was a
    // function at all.
    NOT_CALLABLE = "PW0614" / not_callable / 1, Types,
        "a value called must be a function";
    // ADR-0069: until 2026-09-26 a list's items were joined, and where two
    // disagreed the list's element was a hole: `[1, "a"]` was a list of
    // something unstated, and passed where a `List<Int>` is declared.
    LIST_ITEMS = "PW0615" / list_items / 1, Types,
        "a list's items must share one type";
    // ADR-0069: an `Int` is 64 bits (ADR-0039). A literal past its range was
    // an `Int` to the checker until 2026-09-26; the backend was the first to
    // refuse it.
    INT_LITERAL_RANGE = "PW0616" / int_literal_range / 1, Types,
        "an `Int` literal must fit in 64 bits";
    // ADR-0081: until 2026-09-26 a named argument's type was related to
    // nothing, and the backend passed arguments in written order, so
    // `g(b = 1, a = 10)` computed `g(1, 10)`.
    NAMED_ARGUMENT = "PW0617" / named_argument / 1, Types,
        "a named argument is given to the parameter of its name, once, after the positional ones";
    // ADR-0136: `<MenuRow item={item} />`. Until 2026-10-02 a view used in
    // another was refused whole, and its props were checked by nothing.
    VIEW_PROPS = "PW0619" / view_props / 1, Types,
        "a view is given each of its parameters, and nothing else";
    // ADR-0307: `{#if InteractionId}` (a type), `{#if P}` (a page) and
    // `{#if SignedIn}` (a predicate) checked and built, and the page failed
    // where it was served: "no value for `SignedIn`".
    NAMES_NO_VALUE = "PW0629" / names_no_value / 1, Types,
        "a name a template reads is a value: a binding, a parameter, a signal, a constant, or a predicate the page asks of its reader";
    // ADR-0099: a statement's `Result` was discarded, and its failure with
    // it, until 2026-09-26.
    RESULT_DROPPED = "PW0618" / result_dropped / 1, Types,
        "a failure is handled: a `Result` is taken apart, returned with `?`, or discarded by name";
    // ADR-0157: a command answers `Ok` without its value, which reaches the
    // page from a query. A handler that bound it read nothing, and was told
    // so only by the backend, in terms of the value's type.
    // ADR-0160: an address carries text, and a route's parameter is given
    // what its segment says.
    ROUTE_PARAMETER_IS_TEXT = "PW0621" / route_parameter_is_text / 1, Types,
        "a route's parameter is text: a `String`, or an opaque type over one";
    // ADR-0309: a route's query part is RFC 6570's form-style expansion,
    // `{?q}`, and an address may leave each of its names out, so each is
    // given as an `Option` of text.
    QUERY_PARAMETER_IS_OPTIONAL_TEXT = "PW0630" / query_parameter_is_optional_text / 1, Types,
        "a query parameter is an `Option` of text: an address may leave it out";
    ANSWER_READ_FOR_A_VALUE = "PW0620" / answer_read_for_a_value / 1, Types,
        "what a command answers carries no value: a handler matches `Ok(_)`, and reads the value from a query";
    // ADR-0179: `opaque type PositiveInt = Int` stated no invariant, so
    // `PositiveInt(0)` built one, and a browser's quantity of 0 reached the
    // cart as a line of nothing.
    INVARIANT_NOT_SHOWN = "PW0622" / invariant_not_shown / 1, Types,
        "a value is built of an opaque type only where the build shows it holds the type's invariant";
    // ADR-0225: and on a `String`'s length, which a post's text needed.
    INVARIANT_UNREAD = "PW0623" / invariant_unread / 2, Types,
        "an opaque type's invariant is bounds on its `Int` value, `value >= 1`, or on its `String`'s length, `String.length(value) <= 280`, joined by `&`, that some value holds";
    // ADR-0194: a type may contain itself, and one each of whose values
    // holds another of itself, `type Loop = Loop { again: Loop }`, has no
    // value at all.
    NO_FINITE_VALUE = "PW0624" / no_finite_value / 1, Types,
        "a declared type has a finite value: one that contains itself has a way to be built without itself";
    // ADR-0197 (ADR-0195, ruling 2): a pattern tells a case from a binding by
    // its capital, as Haskell, OCaml and Elm do.
    CASE_NAME_CAPITALIZED = "PW0625" / case_name_capitalized / 1, Types,
        "a sum type's case is named with a capital letter, so a pattern tells it from a binding";
    // ADR-0214 (ADR-0210's urgent defect 8): `browser.value` read a
    // `LayoutSnapshot`, and in `browser` `.value` named both it and the
    // representation.
    REPRESENTATION_SHADOWED = "PW0626" / representation_shadowed / 1, Types,
        "an opaque type's own module reads its representation as `.value`, so it declares no member of that name";
    // ADR-0248 (ruling 0057-a): a `Map<Float, V>` checked, and was refused
    // only when the backend built it.
    MAP_KEY = "PW0627" / map_key / 1, Types,
        "a map's key and a set's element are an `Int`, a `String`, a `Bool`, or an opaque type over one";
    // ADR-0254: a member named by more than one module's declaration. It was
    // whichever was registered last.
    AMBIGUOUS_MEMBER = "PW0628" / ambiguous_member / 1, Types,
        "a member names one declaration: its type's, or the one its module declares or imports";

    // --- structured concurrency (PW20xx) ----------------------------------
    HANDLE_ESCAPES = "PW2001" / handle_escapes / 1, ScopeGraph,
        "a handle cannot outlive the scope that owns it";
    AFFINE_NOT_CONSUMED_ONCE = "PW2005" / affine_not_consumed_once / 1, ScopeGraph,
        "an affine value must be consumed exactly once, in the scope that acquired it";
    TASK_DETACHED = "PW2002" / task_detached / 1, ScopeGraph,
        "an ordinary task cannot be detached from its scope";
    HANDLE_USED_LATE = "PW2003" / handle_used_late / 1, ScopeGraph,
        "a handle cannot be used after its owning scope has exited";
    SCOPE_OUTLIVES_OWNER = "PW2004" / scope_outlives_owner / 1, ScopeGraph,
        "a subscription cannot declare a scope that outlives its owner";

    // --- privacy, placement, unsafe boundaries (PW50xx) -------------------
    PRIVATE_IN_SHARED_CACHE = "PW5001" / private_in_shared_cache / 1, Privacy,
        "a value that is not public cannot live in a shared cache";
    NO_FEASIBLE_PLACEMENT = "PW5002" / no_feasible_placement / 1, Placement,
        "every declaration must have somewhere it can run";
    SECRET_TO_BROWSER = "PW5003" / secret_to_browser / 1, Privacy,
        "a secret cannot be rendered to the browser";
    CACHE_KEY_OMITS_PARTITION = "PW5004" / cache_key_omits_partition / 1, Privacy,
        "a shared cache key must carry every partition its value depends on";
    // Distinct from PW5002 on purpose. PW5002 is the solver finding NO world
    // that can run a declaration; this is a world the author NAMED that cannot
    // grant what the declaration needs. A body that would run fine in the
    // browser, pinned to the origin, is wrong without being unplaceable.
    DECLARED_PLACEMENT_CANNOT_GRANT = "PW5005" / declared_placement_cannot_grant / 1, Placement,
        "a declared placement must be able to grant every effect it requires";
    // ADR-0114: a `placement build` page rendering its `id` checked until
    // 2026-09-26, and the file it is built into exists before any request.
    BUILT_BEFORE_ITS_PARAMETERS = "PW5026" / built_before_its_parameters / 1, Placement,
        "what is built before any request reads no request's value";
    VALUE_EXCEEDS_SINK_LEVEL = "PW5006" / value_exceeds_sink_level / 1, Privacy,
        "a sink accepts only values its declared privacy level admits";
    PRIVATE_IN_RESUME_MANIFEST = "PW5007" / private_in_resume_manifest / 1, Privacy,
        "the resume manifest ships with the public shell and may hold only public values";
    UNSERIALIZABLE_CAPTURE = "PW5008" / unserializable_capture / 1, Privacy,
        "a resumable handler may capture only what can be written to its manifest";
    // Named narrowly, on the architect's correction. This is NOT
    // manifest-versus-handler artifact agreement — that comparison needs E7's
    // generator to exist so there are two artifacts to disagree, and it will
    // get its own registry entry when it does. This proves the narrower thing
    // it actually proves: a capture must have a stable type identity from
    // which this build can derive a schema.
    RESUME_CAPTURE_SCHEMA_UNNAMEABLE = "PW5016" / resume_capture_schema_unnameable / 1, Privacy,
        "a resumable capture must have a type this build can derive a schema from";
    // Its own entry, not a reuse of PW5016. That one is about a capture having
    // a nameable type at all; this is about two independently generated
    // artifacts within ONE build describing the same handler differently.
    RESUME_ARTIFACT_CONTRACT_MISMATCH = "PW5017" / resume_artifact_contract_mismatch / 1, Privacy,
        "a resume manifest and its handler artifact must describe the same contract";
    // A region declared private WITHOUT a principal. Architect ruling,
    // 2026-08-08: `private` means "not globally shareable" and says nothing
    // about whom it is shareable WITH, so it cannot supply the destination a
    // privacy flow is checked against. Blocked rather than guessed — mapping
    // `private` to `Session` let a user-partitioned document accept a session
    // value, and the reverse, in silence.
    RESUME_DESTINATION_UNKNOWN = "PW5018" / resume_destination_unknown / 1, Privacy,
        "a private resumable region must say which principal it is private to";
    // ADR-0110: `resumable() => add_to_cart(item.id, ..)` inside an
    // `{#each}` checked until 2026-09-26, and the handler build refused it.
    HANDLER_READS_UNCAPTURED = "PW5025" / handler_reads_uncaptured / 1, Privacy,
        "a resumable handler reads what it captures, and what it binds itself";
    // ADR-0263: a session's, a user's or an organization's handle is what
    // reads its data, and a program could make one: `Session("…")` checked
    // clean, and so did a command whose session the browser supplies.
    HANDLE_MADE = "PW5037" / handle_made / 1, Privacy,
        "a session's, a user's or an organization's handle is the platform's to make: a program constructs none";
    HANDLE_ANSWERED = "PW5038" / handle_answered / 1, Privacy,
        "a data layer answers ids, never a handle: only the platform's operations answer a session's, a user's or an organization's";
    HANDLE_FROM_BROWSER = "PW5039" / handle_from_browser / 1, Privacy,
        "nothing the browser supplies holds a handle: no command's or page's parameter, and no signal, holds a session's, a user's or an organization's";
    // ADR-0264: a page could print its session's handle, and a session's
    // id is its cookie's value, which `HttpOnly` keeps from scripts.
    HANDLE_TO_BROWSER = "PW5040" / handle_to_browser / 1, Privacy,
        "a session's handle never reaches the browser: no query's, subscription's or command's answer, and nothing markup prints, holds one";
    DEAD_INTERNAL_LINK = "PW5009" / dead_internal_link / 1, Markup,
        "an internal link must name a route the program declares";
    UNSAFE_AUDIT_INCOMPLETE = "PW5010" / unsafe_audit_incomplete / 1, DeclarationRules,
        "an unsafe escape hatch must carry a complete audit record";
    // The architect proposed PW5011 for this. That number was already the
    // unkeyed-list invariant — exactly the collision this registry exists to
    // prevent, caught by the registry on its first day. PW5015 instead.
    UNSAFE_ATTRIBUTION_INVALID = "PW5015" / unsafe_attribution_invalid / 1, DeclarationRules,
        "an escape hatch's attribution target must name a real owner";
    UNKEYED_LIST = "PW5011" / unkeyed_list / 1, Markup,
        "a list over a mutable collection needs a stable key";
    INVALID_NESTING = "PW5012" / invalid_nesting / 1, Markup,
        "an element may only contain the children HTML permits";
    HANDLER_ON_INERT = "PW5013" / handler_on_inert / 1, Markup,
        "interactive behaviour belongs on an element that can receive it";
    CONTROL_WITHOUT_LABEL = "PW5014" / control_without_label / 1, Markup,
        "a form control must have something that names it";
    // ADR-0042. Until 2026-09-25 nothing read a block's markers: `{:else}`
    // was dropped and both of an `if`'s branches rendered together.
    MALFORMED_TEMPLATE_BLOCK = "PW5019" / malformed_template_block / 1, Markup,
        "a template block closes with its own name and holds only the markers it takes";
    // ADR-0072. Until 2026-09-26 `<Money value={p} />` built as an HTML
    // element named `Money`: the view's markup was never rendered, and its
    // props were checked by nothing.
    VIEW_ELEMENT = "PW5020" / view_element / 1, Markup,
        "an element named with a capital letter is a view the compiler composes";
    // ADR-0073. Until 2026-09-26 a loop's key was read by its last segment:
    // `(item.id)` in a loop over `x` keyed on `x.id`, and `(k.r.id)` on `k.id`.
    LOOP_KEY = "PW5021" / loop_key / 1, Markup,
        "a loop's key is its element, or a field read from it";
    // ADR-0094: `<script>{msg}</script>`, `onclick={msg}` and `srcdoc={msg}`
    // checked and built until 2026-09-26, escaped as text or an attribute.
    CODE_IN_MARKUP = "PW5023" / code_in_markup / 1, Markup,
        "a template writes no code, and no value into a stylesheet";
    // ADR-0096: `<base href={msg}>` moved where the platform's runtime loads
    // from, and `<animate attributeName="href" values={msg}>` a link's
    // target, and both checked until 2026-09-26.
    MOVES_A_URL = "PW5024" / moves_a_url / 1, Markup,
        "a template moves no URL: no `<base>`, and no animation of a link or a handler";
    // ADR-0093: `on:clik={go}` checked until 2026-09-26, and ran by
    // accident, because the runtime listens for a click whatever the name.
    UNKNOWN_EVENT = "PW5022" / unknown_event / 1, Markup,
        "an element handles an event the platform declares";
    // ADR-0138: `on:submit|prevent`. A modifier the runtime does not know
    // would be dropped, and the browser's own action, a form's submission,
    // taken.
    UNKNOWN_MODIFIER = "PW5027" / unknown_modifier / 1, Markup,
        "an event's modifier is one the runtime applies";
    // ADR-0167: `// the slots` between two elements checked, and the page
    // showed it (found by ADR-0165).
    COMMENT_AS_TEXT = "PW5028" / comment_as_text / 1, Markup,
        "a comment in markup is `<!-- -->`: text that reads as one would be shown";
    // ADR-0183: every store's page was titled "Store" by the host, since a
    // page could not say what it is (WCAG 2.4.2, failure F25; found by
    // ADR-0182's audit).
    TITLE_MISSING = "PW5029" / title_missing / 1, Markup,
        "a page served at a route states its title";
    TITLE_MISPLACED = "PW5030" / title_misplaced / 1, Markup,
        "a title is the page's: once, at the top of its view, written as text and values";
    // ADR-0185: charter §8.2 names duplicate ids and invalid ARIA relations
    // among the compiler's checks; until 2026-10-04 they were the browser's
    // to find (ADR-0182's audit), and a misspelt `aria-labeledby` was
    // ignored by every browser without a word.
    DUPLICATE_ID = "PW5031" / duplicate_id / 1, Markup,
        "an id names one element of its page";
    REFERENCE_NAMES_NOTHING = "PW5032" / reference_names_nothing / 1, Markup,
        "an id reference names an element its page shows whenever the referrer is shown";
    ARIA_UNKNOWN = "PW5033" / aria_unknown / 1, Markup,
        "an ARIA attribute, its value and a role are ones WAI-ARIA defines";
    // ADR-0186: a page said what it is to a person, its title (ADR-0183),
    // and nothing to what reads it without showing it: a search engine's
    // result, a link's preview. Lighthouse's SEO audit failed every page.
    METADATA_MISPLACED = "PW5034" / metadata_misplaced / 1, Markup,
        "a page's metadata is the page's, at the top of its view: named once as text, its content text and values, and a name HTML allows once stated once";
    // ADR-0189: a `<link rel="canonical">` or `rel="icon"` in a view checked
    // and built, and was written into the body, where HTML does not allow
    // it and nothing reads it.
    LINK_NOT_IN_BODY = "PW5035" / link_not_in_body / 1, Markup,
        "a `<link>` in markup is one HTML allows in the body, each relation body-ok or an item's property, or a page's canonical address, once at the top of its view";
    // ADR-0311: a page could not say which address is its own. Its
    // canonical link names a page, as `navigate` does (ADR-0280): an
    // address written as text could name no page, or one served elsewhere.
    CANONICAL_ADDRESS = "PW5051" / canonical_address / 1, Markup,
        "a page's canonical address is a page's, `href={Page(args)}`, served at a route without a query part, each argument a value the page reads or text";
    // ADR-0195's ruling 13, the build's half: the host's policy allows a
    // page its own origin alone, so a load written from another built, and
    // the browser refused it.
    LOAD_FROM_ANOTHER_ORIGIN = "PW5052" / load_from_another_origin / 1, Markup,
        "a page loads from its own origin: an address its markup fetches, written as text, is a path, and a `<link>`'s address is written as text";
    // ADR-0221: `<textarea bind:value={draft}>` was written as an attribute
    // a textarea does not have, and a first value showed nothing until the
    // runtime set it; a `<select>`'s likewise. Found by the feed (ADR-0220).
    FORM_CONTROL_VALUE = "PW5036" / form_control_value / 1, Markup,
        "a form control's value is written where HTML reads it: a `<textarea>`'s as its text, a signal or text, once; a `<select>`'s by the option it marks `selected`";
    // ADR-0265: no form but a file's was checked, so a form could send a
    // `get` to `/sign-out`, which answers a `post`, or a `post` to a page.
    FORM_ANSWERED_BY_NOTHING = "PW5041" / form_answered_by_nothing / 1, Markup,
        "a form's `action` is a route that answers its `method`: a page's, the relying party's, or an upload's";
    // ADR-0309: a link's query, and a GET form's fields, name only the query
    // parameters of the route they reach: another name carries its value to
    // nothing.
    ADDRESS_NAMES_NO_QUERY_PARAMETER = "PW5050" / address_names_no_query_parameter / 1, Markup,
        "a link's query and a GET form's fields name only the query parameters of the route they reach";
    // ADR-0280: a handler goes to a page once its command commits. A link
    // names a route by its text; `navigate` names the page.
    NAVIGATE_NAMES_NO_PAGE = "PW5042" / navigate_names_no_page / 1, Markup,
        "a `navigate` names a page by its declaration";
    NAVIGATE_BEFORE_A_COMMIT = "PW5043" / navigate_before_a_commit / 1, Markup,
        "a `navigate` is a handler's, last in the `Ok` arm of the command's answer nearest it: a page is left only once a command has committed";
    // ADR-0303: where a layout shows the page that names it.
    SLOT_MISPLACED = "PW5044" / slot_misplaced / 1, Markup,
        "a `<slot />` is its layout's: once, at the top of the layout's own view, outside any block, loop, match or stream, with nothing in it";
    LAYOUT_WITHOUT_SLOT = "PW5045" / layout_without_slot / 1, Markup,
        "a layout shows the page that names it in a `<slot />`";
    // ADR-0303: a page's document holds its layout's values and captures.
    LAYOUT_AUDIENCE = "PW5046" / layout_audience / 1, Privacy,
        "a page declares at least its layout's audience: its document holds what its layout shows and its resume manifest what the layout's handlers capture";
    // ADR-0243's amendment of 2026-10-09: `<p>a</p> + 1` checked, rendered
    // the paragraph and dropped the sum.
    MARKUP_AS_AN_OPERAND = "PW5047" / markup_as_an_operand / 1, Markup,
        "markup is shown where it is written, and is no operand of an arithmetic, comparison or logical operator";
    // ADR-0307: the owner's finding of 2026-10-08, a press refused in
    // silence; the compiler holds what the page can tell before the press.
    CONTROL_WHERE_REFUSED = "PW5048" / control_where_refused / 1, Markup,
        "a control whose command requires a predicate is shown where the page asks that predicate of its reader and it holds";
    READER_PREDICATE_SHARED = "PW5049" / reader_predicate_shared / 1, Privacy,
        "a page that asks a predicate of its reader is its reader's: a page served to everyone asks none";

    // --- the resource dependency graph (PW51xx, E6) -----------------------
    //
    // Separate from `PW5001` (private data in a shared CACHE), which is about
    // a query's own partition. These are about the GRAPH: what a
    // materialization pulls in, what it listens for, and what its key
    // separates. A page can be correct on its own and wrong as a node.
    GRAPH_EDGE_UNRESOLVED = "PW5100" / graph_edge_unresolved / 1, ResourceGraph,
        "a dependency, event or invalidation target must name something the program declares";
    PRIVATE_IN_SHARED_MATERIALIZATION = "PW5101" / private_in_shared_materialization / 1, ResourceGraph,
        "a shared materialization may only depend on data every reader of its cache entry may see";
    // ADR-0088: `invalidates CartChanged(..)` invalidated an event and `emits
    // Cart(..)` emitted a resource, and both checked. The graph looked a name
    // up wherever it might be, and did not ask what it found.
    // ADR-0092: a query that `emits`, a command that listens and a `fn` that
    // emits each checked until 2026-09-26, and nothing read what they said.
    // ADR-0216: every head, since `freshness` on a command did too.
    CLAUSE_OUT_OF_PLACE = "PW5105" / clause_out_of_place / 1, ResourceGraph,
        "a clause belongs to a declaration that reads it";
    // ADR-0091: `invalidates_on InventoryChanged(id, item)`, with `item`
    // naming nothing, checked until 2026-09-26.
    LISTENER_KEY = "PW5104" / listener_key / 1, ResourceGraph,
        "a listener's argument is one of its declaration's parameters, or `_`";
    // Revision 2 (ADR-0255, ruling 10): a materialization may read another.
    CLAUSE_NAMES_ANOTHER_KIND = "PW5103" / clause_names_another_kind / 2, ResourceGraph,
        "a clause names a declaration of its kind: a resource or a materialization to read, a resource to invalidate, an event to emit or listen for";
    // ADR-0101: `add_to_cart` without `invalidates` or `emits` checked until
    // 2026-09-26, and the cart it wrote stayed as it was on every page.
    WRITE_NOT_INVALIDATED = "PW5106" / write_not_invalidated / 1, ResourceGraph,
        "a command invalidates what it writes: each cached reader of the data it changes, by name or through an event the reader listens for";
    // ADR-0105: `optimistic Cart(..)` with neither `invalidates` nor an event
    // `Cart` hears checked until 2026-09-26, and the speculation stayed.
    OPTIMISTIC_NOT_RECONCILED = "PW5107" / optimistic_not_reconciled / 1, ResourceGraph,
        "a command invalidates the entry it speculates on, so what it committed replaces the speculation";
    // ADR-0212 (ruling 0108-a): `let v = query helper(id)` over a `fn`
    // checked, and the page kept the function's answer as an entry.
    READ_NAMES_ANOTHER_KIND = "PW5108" / read_names_another_kind / 1, ResourceGraph,
        "a `query` reads a query or a resource, or in a materialization's body one that derives its value; and a `subscription` a subscription";
    // ADR-0255 (ADR-0195's ruling 10): a materialization may read another,
    // and the build refuses a cycle.
    MATERIALIZATION_CYCLE = "PW5109" / materialization_cycle / 1, ResourceGraph,
        "a materialization depends on nothing that depends on it";
    // ADR-0273 (ruling 10's last part): a materialization that derives its
    // value reads what it depends on, and the graph takes those reads.
    DEPENDENCY_STATED_TWICE = "PW5110" / dependency_stated_twice / 1, ResourceGraph,
        "a materialization that derives its value states what it depends on by reading it, and nowhere else";

    // --- what a capability names (PW52xx, E8) -----------------------------
    //
    // A capability that resolves to nothing is not a deployment problem. It
    // becomes "the node does not grant `database.read<Stroes>`" at a moment
    // when the person who typed `Stroes` is nowhere near, and the message
    // describes the deployment rather than the typo.
    UNRESOLVED_CAPABILITY_ARGUMENT = "PW5200" / unresolved_capability_argument / 1, Capability,
        "a capability's type argument must name a type the program declares";
    // The three the effect ontology makes possible. Before it, `log` and a
    // misspelled `databse` were indistinguishable to every checker: nothing
    // declared what a family was, so there was nothing to be unknown against.
    //
    // Split three ways rather than one "unknown effect", because they send a
    // reader to three different places. `databse.read` is a typo in the family;
    // `database.reed` is a typo in the operation, and the family's real
    // operations can be listed; `database.read` with no argument is neither —
    // the name is right and the application is wrong.
    UNKNOWN_EFFECT_FAMILY = "PW5201" / unknown_effect_family / 1, Capability,
        "an effect must name a family some package declares";
    UNKNOWN_EFFECT_OPERATION = "PW5202" / unknown_effect_operation / 1, Capability,
        "an effect must name an operation its family declares";
    EFFECT_ARITY_MISMATCH = "PW5203" / effect_arity_mismatch / 1, Capability,
        "an effect must be given exactly the type arguments its declaration binds";
    // A PLATFORM error rather than an application one: it is the effect
    // declaration that is wrong, and the program writing the effect is fine.
    //
    // `impact layout_write when LayoutAffect` naming nothing is the shape
    // `LayoutAffect` itself had for three milestones — source that looks
    // meaningful while the compiler quietly assigns it none. Inert for one
    // commit, which was better than a spelling fallback and still wrong: a
    // package could lose an import and a facet would stop applying in silence.
    UNRESOLVED_IMPACT_MARKER = "PW5204" / unresolved_impact_marker / 1, Capability,
        "an effect's impact condition must name a type the declaring module can see";

    // --- UI state (PW53xx, ADR-0130) -------------------------------------
    //
    // A signal lives in the browser and changes when a person acts. So it is
    // written by a handler, read where the browser can read it again, and it
    // is the one thing a handler changes.
    SIGNAL_WRITTEN_OUTSIDE_HANDLER = "PW5300" / signal_written_outside_handler / 1, UiState,
        "a signal changes only in a handler";
    SIGNAL_READ_WHERE_IT_CANNOT_CHANGE = "PW5301" / signal_read_where_it_cannot_change / 1, UiState,
        "a signal is read only where the browser reads it again when it changes: a template part, a handler, or a page query's key";
    UI_STATE_NOT_A_SIGNAL = "PW5302" / ui_state_not_a_signal / 1, UiState,
        "a handler changes a signal, not a binding of the body it is written in";
    // ADR-0141: a `<dialog>` a signal shows is the browser's modal dialog,
    // which Escape closes by itself. A dialog nothing shows, or one whose
    // closing the signal never hears of, is a page that cannot open it again.
    MODAL_DIALOG = "PW5303" / modal_dialog / 1, UiState,
        "a modal dialog is shown by a signal's block, and says what closing it does";
    // ADR-0142: `bind:value={s}`. A binding of a value the server holds, or
    // of a signal no codec turns text into, would be a field whose typing
    // changes nothing, or changes it to a value of another type.
    BOUND_VALUE = "PW5304" / bound_value / 1, UiState,
        "an input binds its value to a signal of `String`, by its name";
    // ADR-0144: a signal declared in a module is given its value by a page or
    // view, for everything that body contains. A view that needs one where
    // nothing provides it would read a value no one gave.
    SIGNAL_NOT_PROVIDED = "PW5305" / signal_not_provided / 1, UiState,
        "a provided signal is read only where a page or view around it provides it";
    PROVIDE = "PW5306" / provide / 1, UiState,
        "a body provides a signal declared in a module, once";
    // Each row of a loop would need its own instance, kept by the row's key,
    // and the browser holds one per use (ADR-0144).
    SIGNAL_IN_A_ROW = "PW5307" / signal_in_a_row / 1, UiState,
        "a view that holds a signal is not used in a loop's row yet";
    // ADR-0152: a page's query given a signal is read again, for the new key,
    // when the signal changes. A key crosses to the server and is compared
    // exactly, and the query says what happens to the old key's work.
    SIGNAL_KEY_TYPE = "PW5308" / signal_key_type / 1, UiState,
        "a signal that keys a query is a `String`, an `Int` or a `Bool`";
    SIGNAL_KEY_STALE_WORK = "PW5309" / signal_key_stale_work / 1, UiState,
        "a query a signal keys says what its stale work does";

    // --- streamed regions (PW54xx, ADR-0148) -------------------------------
    //
    // A query declared `delivery streamed` is sent after its page, into a
    // `<stream>` that shows a placeholder until it settles. A page that waits
    // for one, a region with no arm for a state its query reaches, or a
    // region that may wait forever, is a page whose loading or failure shows
    // nothing.
    STREAMED_READ_OUTSIDE_STREAM = "PW5400" / streamed_read_outside_stream / 1, Streaming,
        "a query declared `delivery streamed` is read only by a `<stream>`";
    STREAM_STATES = "PW5401" / stream_states / 1, Streaming,
        "a `<stream>` shows each state its query can be in, and no other";
    STREAMED_WITHOUT_TIMEOUT = "PW5402" / streamed_without_timeout / 1, Streaming,
        "a query declared `delivery streamed` declares how long it may take";

    // --- the uploads track (PW56xx, ADR-0253; track `uploads`, ADR-0260) ----
    //
    // An `upload` declaration states where a form posts a file, where what is
    // committed is served, and its limits, each a literal the host holds a
    // browser to: the program states its invariants, as `PostText`'s 280 is.
    UPLOAD_MALFORMED = "PW5601" / upload_malformed / 1, Uploads,
        "an upload states its route, where it is served, and each of its limits, once and as literals";
    UPLOAD_ROUTE_TAKEN = "PW5602" / upload_route_taken / 1, Uploads,
        "an upload's paths are its own: no page's, and no other upload's";
    FILE_FORM_POSTS_TO_NO_UPLOAD = "PW5603" / file_form_posts_to_no_upload / 1, Uploads,
        "a form that sends a file posts it, as multipart/form-data, to an upload the program declares";

    // --- the todomvc track (PW61xx; track `todomvc`, W7) --------------------
    //
    // `fragment filter "/" => "all", "/active" => "active"`: a signal a page
    // binds to its address's fragment, which no server sees, with the forms
    // the fragment takes. The runtime keeps the two in step; a server renders
    // the signal's first value.
    FRAGMENT_NAMES_NO_SIGNAL = "PW6101" / fragment_names_no_signal / 1, Todomvc,
        "a page's `fragment` clause names a signal the page declares";
    FRAGMENT_SIGNAL_NOT_TEXT = "PW6102" / fragment_signal_not_text / 1, Todomvc,
        "a signal bound to the address's fragment is a `String`: a fragment is text";
    FRAGMENT_FORMS = "PW6103" / fragment_forms / 1, Todomvc,
        "a fragment's forms are written `\"form\" => \"value\"`, each form once, in characters a fragment holds";
    FRAGMENT_DEFAULT = "PW6104" / fragment_default / 1, Todomvc,
        "a signal bound to the fragment starts at a value one of the forms gives";
    FRAGMENT_SIGNAL_ASSIGNED = "PW6105" / fragment_signal_assigned / 1, Todomvc,
        "a signal bound to the fragment is written by the address alone, and no handler assigns it";
    LINK_NAMES_NO_FORM = "PW6106" / link_names_no_form / 1, Todomvc,
        "a link to a fragment of a page that binds its fragment names a form the page takes";

    // --- the payments track (PW64xx; NEXT 6c, the integrator's) ------------
    //
    // `invoked_by Payments`: the command a payment's outcome invokes, which
    // the host calls on its provider's word and no reader presses (Convex's
    // internal functions; a WASI handler export).
    INVOKED_COMMAND_REQUIRES = "PW6401" / invoked_command_requires / 1, Payments,
        "a command the platform invokes holds no reader to a predicate: it declares no `requires`";
    INVOKED_COMMAND_PARAMETERS = "PW6402" / invoked_command_parameters / 1, Payments,
        "a command `invoked_by Payments` is given one `payments.Settled`, and nothing else";
    INVOKED_TWICE = "PW6403" / invoked_twice / 1, Payments,
        "one command answers a payment's outcome: a second would leave the host two answers";
    INVOKED_COMMAND_PRESSED = "PW6404" / invoked_command_pressed / 1, Payments,
        "no page presses a command the platform invokes";
    INVOKED_COMMAND_NOT_IDEMPOTENT = "PW6405" / invoked_command_not_idempotent / 1, Payments,
        "a command the platform invokes is idempotent: its word, a provider's or the clock's, may come more than once";

    // --- the scheduled-commands track (PW65xx; the integrator's) ----------
    //
    // `schedule cmd(args) at when` and `unschedule cmd(args)`: a command
    // `invoked_by Clock`, run when the clock passes its time, written with
    // the scheduling command's writes (Convex's scheduled functions; Oban's
    // jobs unique by their arguments).
    SCHEDULE_NAMES_NO_CLOCKED_COMMAND = "PW6501" / schedule_names_no_clocked_command / 1, Schedules,
        "a `schedule` names a command `invoked_by Clock`, written as a call";
    SCHEDULED_OUTSIDE_A_COMMAND = "PW6502" / scheduled_outside_a_command / 1, Schedules,
        "a `schedule` or an `unschedule` is a command's statement: what a page shows schedules nothing";
    SCHEDULE_TIME = "PW6503" / schedule_time / 1, Schedules,
        "a scheduled run's time is `at` an `Instant` or `after` a `Duration`";

    // --- the device-store track (PW68xx; ADR-0366) ------------------------
    //
    // `signal todos: List<Todo> = []` at module level with `storage device
    // "todos-pleris"`, `privacy Device` and `conflict last_write_wins`: a
    // value the reader's device keeps, which never leaves it.
    DEVICE_VALUE_TO_A_COMMAND = "PW6801" / device_value_to_a_command / 1, DeviceStore,
        "a value the device keeps is no command's argument, and decides no command's call: a command runs on the server";
    DEVICE_VALUE_KEYS_A_QUERY = "PW6802" / device_value_keys_a_query / 1, DeviceStore,
        "a value the device keeps keys no page's query: the key is sent to the server to read it again";
    DEVICE_VALUE_SENT = "PW6803" / device_value_sent / 1, DeviceStore,
        "a value the device keeps is written nowhere the browser sends: a navigation, an address, a form";
    DEVICE_KEEPS_ITS_OWN = "PW6804" / device_keeps_its_own / 1, DeviceStore,
        "a signal the device keeps is assigned no session's, user's, organization's or secret's value";
    DEVICE_SIGNAL_CLAUSES = "PW6805" / device_signal_clauses / 1, DeviceStore,
        "a signal the device keeps says where it rests, `storage device \"name\"` (one name a program), whose it is, `privacy Device`, and what two tabs' writes do, `conflict last_write_wins`";
    DEVICE_SIGNAL_VERSIONS = "PW6806" / device_signal_versions / 1, DeviceStore,
        "a signal the device keeps has a pure migration for each version before its own, each taking the one before it to the next";
    DEVICE_SIGNAL_FORM = "PW6807" / device_signal_form / 1, DeviceStore,
        "a signal the device keeps is of a type the device can hold: data, no function, no type that contains itself, no opaque type with a `where`";
    DEVICE_SIGNAL_PROVIDED = "PW6808" / device_signal_provided / 1, DeviceStore,
        "a signal the device keeps is provided by the device: no `provide` gives it a value";

    // --- the dating-app note's H tracks (PW69xx; ADR-0384) ---------------
    //
    // Track `row-ownership` (H2): a predicate the program gives a body, the
    // program's authorization, evaluated by the host before the body it
    // guards (ADR-0115 as amended).
    PREDICATE_BODY_READS_ONLY = "PW6905" / predicate_body_reads_only / 1, Dating,
        "a predicate's body reads and does nothing else: no write, no event, no schedule, no network, no log, no clock but its comparison";
    // ADR-0384's H2 code for a query's `requires` (ADR-0339's ruling 3; the
    // reviews of the track, finding 7b and the second review's finding 1):
    // an answer given where a predicate holds for its reader is that
    // reader's, and a read keyed without its reader, a shared cache's or a
    // flight another reader joins, would give it to readers never asked.
    REQUIRED_QUERY_UNSHARED = "PW6907" / required_query_unshared / 1, Dating,
        "a query that `requires` a predicate is answered to its reader alone, once the predicate holds for them: it is cached `private`, or is a `user` or `session` query, so every read of it is keyed by its reader, and no materialization depends on it";
    // The review of the track (ADR-0384's H1), finding 1: a database checks
    // a reference across every row of both tables, whoever owns them.
    REFERENCE_WITHIN_ONE_OWNER = "PW6924" / reference_within_one_owner / 1, Dating,
        "a reference a database holds stays within one owner's rows: an owned table refers to an owned table from its own owner field alone, a table no one owns to no owned table, and an owned table to no table no one owns";
    // The review of the track (ADR-0384's H2; ADR-0115 as amended), findings
    // 3 and 4: a predicate the program gives a body is the program's
    // authorization, which a deployment's predicate of the same name would
    // answer otherwise, a page's ask among them (ADR-0307).
    PREDICATE_ANSWERED_BY_ITS_BODY = "PW6925" / predicate_answered_by_its_body / 1, Dating,
        "a predicate the program gives a body is answered by that body alone: a `requires` naming it sees it where it is written, no page asks it of its reader, and no predicate the deployment answers shares its name";
    // The second review of the track (ADR-0384's H2; ADR-0339's ruling 3),
    // finding 2: a query's body called from another declaration is compiled
    // into that declaration, whose contract requires nothing, as PW0339
    // holds a command to its handler.
    REQUIRED_QUERY_BOUND_BY_A_PAGE = "PW6926" / required_query_bound_by_a_page / 1, Dating,
        "a query that `requires` a predicate is read by a page's binding alone, `query Q(..)`, where the host asks its reader: no query, command, function, predicate or materialization calls it, binds it or names it, which would run its body unasked";
}

/// Codes that were registered, are no longer emitted, and whose numbers must
/// never be reused.
///
/// Distinct from [`crate::diagnostics::DEPRECATED_ALIASES`], which redirect an
/// old number to a live invariant. A retired code has no successor: the
/// invariant it named is now guaranteed by construction, so there is nothing
/// for an author to repair and nothing to redirect to.
///
/// The numbers are held because a code is a public identifier. Reusing
/// `PW5102` for something else would make every historical mention of it —
/// a commit message, a corpus fixture, somebody's notes — silently describe a
/// different rule.
pub const RETIRED: &[(&str, &str)] = &[(
    "PW5102",
    "a shared materialization's cache key must separate the build that produced \
     it. Retired 2026-08-06: the compatibility generation is now injected by the \
     compiler for every materialized entry, so the key cannot omit it. If a \
     generated storage key ever lacks its compatibility namespace that is an \
     internal compiler invariant, not an error an application author can repair.",
)];

/// Look a code up by its public string.
pub fn lookup(id: &str) -> Option<Code> {
    ALL.iter().copied().find(|c| c.id == id)
}

/// What a corpus fixture's `@rule` resolves to.
///
/// Architect ruling, 2026-08-06: model the categories explicitly rather than
/// treating every unregistered code as one generic exception. An unknown code
/// is a typo or a fixture nobody can ever satisfy; a *known gap* is the corpus
/// doing its job, naming an invariant the compiler has not built yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleStatus {
    Registered(Code),
    /// Declared by a fixture, not yet implemented, with an owner.
    KnownGap {
        code: &'static str,
        intended_owner: &'static str,
        missing_analysis: &'static str,
    },
    /// Neither. Always a failure.
    Unknown,
}

/// Invariants the corpus specifies and the compiler has not built.
///
/// Each names the milestone that will own it and the analysis that is missing,
/// so the list is a work plan rather than a list of excuses. It shrinks as
/// milestones land; it grows only when a new specification fixture is added.
pub const KNOWN_GAPS: &[(&str, &str, &str)] = &[
    // (code, intended owner, missing analysis)
];

/// Classify a corpus `@rule` as written: an old number the corpus once wrote
/// (`diagnostics::DEPRECATED_ALIASES`) is no code `pw check` reports, and is
/// `Unknown` (track evidence-integrity-2, the integrator's ruling 1: nothing
/// translates an old number where a code is compared).
pub fn rule_status(declared: &str) -> RuleStatus {
    if let Some(c) = lookup(declared) {
        return RuleStatus::Registered(c);
    }
    if let Some((code, owner, analysis)) = KNOWN_GAPS.iter().find(|(c, _, _)| *c == declared) {
        return RuleStatus::KnownGap {
            code,
            intended_owner: owner,
            missing_analysis: analysis,
        };
    }
    RuleStatus::Unknown
}

impl Owner {
    /// The code range this owner's diagnostics must sit in, or `""` when the
    /// owner is semantic and may live anywhere at `PW01xx` or above.
    ///
    /// Part of the registry's contract rather than a test helper: a new owner
    /// has to answer this question before it can register a code.
    pub fn range(self) -> &'static str {
        match self {
            // The parallel tracks' blocks (ADR-0253), each held to its owner.
            Owner::Identity => "PW55",
            Owner::Uploads => "PW56",
            Owner::Notifications => "PW57",
            Owner::Messages => "PW58",
            Owner::StorePg => "PW59",
            Owner::Kiokun => "PW60",
            Owner::Todomvc => "PW61",
            Owner::StoreAccounts => "PW62",
            Owner::StoreSearch => "PW63",
            Owner::Payments => "PW64",
            Owner::Schedules => "PW65",
            Owner::CheckIsBuild => "PW66",
            Owner::DeviceStore => "PW68",
            Owner::Dating => "PW69",
            Owner::Syntax | Owner::Resolution => "PW00",
            Owner::Placement | Owner::Privacy | Owner::Markup => "PW50",
            Owner::Types => "PW06",
            Owner::ResourceGraph => "PW51",
            Owner::Capability => "PW52",
            Owner::UiState => "PW53",
            Owner::Streaming => "PW54",
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::DEPRECATED_ALIASES;

    #[test]
    fn every_code_is_unique() {
        let mut ids: Vec<&str> = ALL.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "a code is registered twice");
    }

    #[test]
    fn every_code_sits_in_its_owner_range() {
        // The `PW0100` collision in one assertion: a syntax code outside PW00xx,
        // or a placement code outside PW50xx, is how two rules end up sharing a
        // number and a corpus fixture goes green for the wrong reason.
        for c in ALL {
            let want = c.owner.range();
            if want.is_empty() {
                assert!(
                    !c.id.starts_with("PW00"),
                    "{} is semantic but sits in the syntax range",
                    c.id
                );
                continue;
            }
            assert!(
                c.id.starts_with(want),
                "{} is owned by {:?} and must start with {want}",
                c.id,
                c.owner
            );
        }
    }

    #[test]
    fn a_tracks_block_holds_only_its_tracks_codes() {
        // ADR-0253: two tracks work in parallel, each registering codes in
        // its own block. A code of any other owner there is how two of them
        // take one number.
        for c in ALL {
            for (owner, block) in [
                (Owner::Identity, "PW55"),
                (Owner::Uploads, "PW56"),
                (Owner::Notifications, "PW57"),
                (Owner::Messages, "PW58"),
                (Owner::StorePg, "PW59"),
                (Owner::Kiokun, "PW60"),
                (Owner::Todomvc, "PW61"),
                (Owner::StoreAccounts, "PW62"),
                (Owner::StoreSearch, "PW63"),
                (Owner::Payments, "PW64"),
                (Owner::Schedules, "PW65"),
                (Owner::CheckIsBuild, "PW66"),
                (Owner::DeviceStore, "PW68"),
                (Owner::Dating, "PW69"),
            ] {
                assert!(
                    !c.id.starts_with(block) || c.owner == owner,
                    "{} sits in {block}, the {owner:?} track's block",
                    c.id
                );
            }
        }
    }

    /// Track evidence-integrity-2, the integrator's ruling 1: a corpus
    /// `@rule` is read as written. An old number is no code `pw check`
    /// reports, so a header naming one is `Unknown`, which the corpus's
    /// ratchet fails; until 2026-10-15 it was classified as the code it became.
    #[test]
    fn an_old_number_is_no_registered_rule() {
        assert!(!DEPRECATED_ALIASES.is_empty());
        for (old, now) in DEPRECATED_ALIASES {
            assert_eq!(rule_status(old), RuleStatus::Unknown, "{old} (now {now})");
            assert!(matches!(rule_status(now), RuleStatus::Registered(c) if c.id == *now));
        }
    }

    #[test]
    fn every_alias_resolves_to_a_registered_code() {
        for (alias, canonical) in DEPRECATED_ALIASES {
            assert!(
                lookup(canonical).is_some(),
                "alias {alias} resolves to {canonical}, which is not registered"
            );
            assert!(
                lookup(alias).is_none(),
                "{alias} is both an alias and a registered code"
            );
        }
    }

    #[test]
    fn every_corpus_rule_is_registered_or_an_owned_known_gap() {
        // The ratchet the architect specified:
        //   unknown == 0
        //   known_gaps only shrink, or grow with an approved new fixture
        // An implemented invariant must not silently fall back to "known gap".
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rejected");
        let (mut registered, mut gaps, mut unknown) = (0, 0, Vec::new());

        for e in std::fs::read_dir(&root).expect("rejected/") {
            let path = e.expect("entry").path();
            if path.extension().is_none_or(|x| x != "pw") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read");
            let Some(rule) = src
                .lines()
                .find_map(|l| l.trim().strip_prefix("// @rule:"))
                .map(str::trim)
            else {
                continue;
            };
            match rule_status(rule) {
                RuleStatus::Registered(_) => registered += 1,
                RuleStatus::KnownGap { .. } => gaps += 1,
                RuleStatus::Unknown => unknown.push(format!(
                    "{}: @rule {rule}",
                    path.file_name().unwrap().to_string_lossy()
                )),
            }
        }

        eprintln!("  Corpus invariants");
        eprintln!("  - registered: {registered}");
        eprintln!("  - known gaps: {gaps}");
        eprintln!("  - unknown:    {}", unknown.len());

        assert!(
            unknown.is_empty(),
            "a fixture names an invariant that is neither built nor planned — a \
             typo, or a rule nobody can ever satisfy:\n{}",
            unknown.join("\n")
        );
        assert!(
            registered >= 19,
            "registered rules regressed to {registered}"
        );
        assert!(
            gaps <= 25,
            "known gaps grew to {gaps} without a new fixture"
        );
    }

    #[test]
    fn every_known_gap_names_an_owner_and_the_missing_analysis() {
        // A gap list without owners is a list of excuses. With them it is a
        // work plan, and `docs/NEXT.md` can be generated from it.
        for (code, owner, analysis) in KNOWN_GAPS {
            assert!(
                lookup(code).is_none(),
                "{code} is registered AND listed as a gap"
            );
            // A gap that is an old number names an invariant that already has
            // its code: a work item that will never be picked up, and a
            // number that reads as unowned when it is not.
            if let Some((_, now)) = DEPRECATED_ALIASES.iter().find(|(old, _)| old == code) {
                panic!(
                    "{code} is listed as a gap but is an old number for {now}: the gap entry is dead"
                );
            }
            assert!(
                owner.starts_with('E') || owner.starts_with('P'),
                "{code}'s owner {owner:?} is not a milestone"
            );
            assert!(
                analysis.len() > 15,
                "{code}'s missing analysis is not described: {analysis:?}"
            );
        }
    }

    /// A registered code must be a code some checker can actually emit.
    ///
    /// `PW0323` was registered as `RESOURCE_NO_RELEASE` — "a resource must
    /// declare how it is released" — while the only thing that ever emitted
    /// `PW0323` was the placement rule, and the corpus fixture declaring it
    /// (R-025) is about placement too. Nothing enforced resource-release, so
    /// the registry asserted an invariant the compiler did not have, on a
    /// number that already meant something else. Had the placement rule and
    /// the fixture met, the ratchet would have counted a correct catch under a
    /// description of a different rule.
    #[test]
    fn every_registered_code_is_one_a_checker_can_emit() {
        // The compiler's own source, its tests left out (ADR-0239): a code
        // only a test names is one no checker emits.
        let sources: String = written().into_iter().map(|(_, src)| src).collect();
        let mut dead = Vec::new();
        for c in ALL {
            // Either by constant (`codes::UNDECLARED_EFFECT.id`) or by the
            // literal, which `rules.rs` still uses.
            if !sources.contains(&format!("\"{}\"", c.id)) && !sources.contains(c.konst) {
                dead.push(format!("{} ({})", c.id, c.invariant));
            }
        }
        assert!(
            dead.is_empty(),
            "registered but unreachable — the registry claims an invariant \
             nothing enforces:\n  {}",
            dead.join("\n  ")
        );
    }

    /// **Every code a checker writes is registered** (ADR-0239), the other
    /// half of rustc's `tidy` check on its error codes. The declaration
    /// rules wrote PW0101 and PW0102 unregistered, and the parser PW0102 for
    /// a `for` with no `in`: one number, two meanings, and neither had a
    /// symbol. Nothing compared what is written with what is registered.
    #[test]
    fn every_code_a_checker_writes_is_registered() {
        let mut written_codes = Vec::new();
        for (file, src) in written() {
            for (at, _) in src.match_indices("\"PW") {
                let Some(code) = src.get(at + 1..at + 7) else {
                    continue;
                };
                if code[2..].bytes().all(|b| b.is_ascii_digit()) && src[at + 7..].starts_with('"') {
                    let line = src[..at].lines().count();
                    written_codes.push((code.to_string(), format!("{file}:{line}")));
                }
            }
        }
        assert!(
            written_codes.len() > 20,
            "the compiler was read: {written_codes:?}"
        );
        let unregistered: Vec<&(String, String)> = written_codes
            .iter()
            .filter(|(code, _)| lookup(code).is_none())
            .collect();
        assert!(
            unregistered.is_empty(),
            "written and not registered: {unregistered:?}"
        );
    }

    /// The source of each crate that writes codes, by file, without its tests
    /// or the registry. A file's tests are at its end, behind `#[cfg(test)]`,
    /// and are cut there; the aliases are numbers the corpus wrote.
    fn written() -> Vec<(String, String)> {
        let compiler = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .canonicalize()
            .expect("compiler/");
        let mut out = Vec::new();
        let mut stack: Vec<std::path::PathBuf> = ["pw-syntax/src", "pw-core/src", "pw-cli/src"]
            .iter()
            .map(|d| compiler.join(d))
            .collect();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).expect("src") {
                let p = e.expect("entry").path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.file_name().is_some_and(|n| n == "codes.rs")
                    || p.extension().is_none_or(|x| x != "rs")
                {
                    continue;
                }
                let src = std::fs::read_to_string(&p).expect("read");
                for (i, _) in src.match_indices("#[cfg(test)]") {
                    let next = src[i..].lines().nth(1).unwrap_or("").trim();
                    assert!(
                        next.starts_with("mod ") || next.starts_with("pub mod "),
                        "{}: `#[cfg(test)]` before `{next}`, which is no test module",
                        p.display()
                    );
                }
                let own = src.split("#[cfg(test)]").next().unwrap_or("");
                let own = own
                    .split("pub const DEPRECATED_ALIASES")
                    .next()
                    .unwrap_or("");
                out.push((p.display().to_string(), own.to_string()));
            }
        }
        out
    }

    /// A retired number must never come back as something else.
    ///
    /// A code is a public identifier. If `PW5102` were reused, every historical
    /// mention of it — a commit message, a corpus fixture, somebody's notes —
    /// would silently start describing a different rule.
    #[test]
    fn a_retired_code_is_never_reused() {
        for (id, why) in RETIRED {
            assert!(
                lookup(id).is_none(),
                "`{id}` is retired and registered again. It was: {why}"
            );
            assert!(
                !crate::diagnostics::DEPRECATED_ALIASES
                    .iter()
                    .any(|(alias, _)| alias == id),
                "`{id}` is retired, so it has no successor to alias to"
            );
        }
    }

    /// Retirement is not a way to make the registry-closure test pass.
    ///
    /// Without this, a code that becomes inconvenient can be moved to `RETIRED`
    /// with an empty reason and the dashboard gets greener.
    #[test]
    fn every_retired_code_says_why_and_what_replaced_it() {
        for (id, why) in RETIRED {
            assert!(
                why.len() > 80,
                "`{id}` is retired with no account of what now guarantees it"
            );
            assert!(
                why.contains("Retired"),
                "`{id}`'s reason must record when and why"
            );
        }
    }

    #[test]
    fn an_invariant_reads_as_a_sentence_about_the_program() {
        // A code whose invariant names an implementation pass would teach the
        // developer the wrong thing — the architect's one-code-per-invariant
        // ruling is about vocabulary, not just numbering.
        for c in ALL {
            assert!(!c.invariant.is_empty(), "{} has no invariant", c.id);
            for banned in ["checker", "pass", "analysis", "detector", "algorithm"] {
                assert!(
                    !c.invariant.contains(banned),
                    "{}'s invariant mentions the implementation: {:?}",
                    c.id,
                    c.invariant
                );
            }
        }
    }
}
