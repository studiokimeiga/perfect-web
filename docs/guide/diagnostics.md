# Diagnostic codes

<!-- Written by scripts/guide_diagnostics.py from compiler/pw-core/src/codes.rs:
     do not edit it by hand. Run `python3 scripts/guide_diagnostics.py` after a
     change to the registry; `just ci` fails while this page is not current. -->

Every diagnostic `pw check` prints carries a code, such as `[PW0606]`, and
says in one sentence what the program must hold to. This page lists every
code the compiler can emit, from its registry, `compiler/pw-core/src/codes.rs`.

- **Code**: the number `pw check` prints. A number is never reused.
- **Symbol**: the code's name in the compiler and in the corpus's
  fixtures (`@rule`).
- **What the program must hold to**: the registry's own sentence.
- **ADR**: the decisions its registry entry cites, where it cites one.

230 codes are registered; 103 of them cite an ADR in the registry.
A code whose entry cites none is marked `—`: search `docs/DECISIONS/` for its
number or its symbol.

## Check is build

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW6601 | `executable_program` | a semantically valid program is supported by the compiler's executable backends | — |

## Syntax

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW0001 | `expected` | the parser expected a different token here | — |
| PW0002 | `unclosed_type_args` | a type argument list must be closed | — |
| PW0003 | `unclosed_effect_row` | an effect row must be closed | — |
| PW0004 | `effect_row_needs_brace` | an effect row opens with `{` | — |
| PW0005 | `unclosed_params` | a parameter list must be closed | — |
| PW0006 | `unclosed_block` | a block must be closed | — |
| PW0007 | `unknown_declaration` | this does not begin a declaration | — |
| PW0008 | `unclosed_paren` | a parenthesis must be closed | — |
| PW0009 | `expected_expression` | an expression was expected here | — |
| PW0010 | `unclosed_args` | an argument list must be closed | — |
| PW0011 | `unclosed_list` | a list literal must be closed | — |
| PW0012 | `unclosed_match` | a match must be closed | — |
| PW0013 | `keyword_as_name` | a word that begins a statement or an expression in a body names no binding, parameter or declaration | [0041](../DECISIONS/ADR-0041-kiokun-in-pleris.md), [0195](../DECISIONS/ADR-0195-the-owners-rulings-on-fifteen-open-questions.md) |
| PW0014 | `string_escape` | a string's escapes and holes must be ones the language defines | [0049](../DECISIONS/ADR-0049-string-escapes.md) |
| PW0015 | `unread` | every expression and pattern in a file that parses is one the compiler reads | [0200](../DECISIONS/ADR-0200-unit-is-a-value-and-nothing-unread-checks.md) |
| PW0016 | `read_whole` | a clause's value, a string's hole and a block marker's expression are read whole, and nothing follows what their grammar reads | [0237](../DECISIONS/ADR-0237-what-lowering-parses-it-reports.md) |
| PW0017 | `optimistic_clause` | an optimistic clause names an entry, binds its value with `as`, and gives its transition after `=>` | — |
| PW0018 | `for_needs_in` | a `for` loop names what it iterates after `in` | [0239](../DECISIONS/ADR-0239-every-code-the-compiler-writes-is-registered.md) |
| PW0019 | `each_head` | an `{#each}` names its list, and after `as` the name each row binds | [0242](../DECISIONS/ADR-0242-an-each-head-is-read-once.md) |
| PW0030 | `statements_separated` | a block's statements are separated by `;` or a line | [0243](../DECISIONS/ADR-0243-a-blocks-statements-are-separated.md) |
| PW0031 | `logical_mixed` | `\|` and `&` mixed in one chain are parenthesized: neither binds tighter | [0322](../DECISIONS/ADR-0322-or-and-and-are-parenthesized-where-they-mix.md) |
| PW0099 | `no_progress` | the parser made no progress | — |

## Names and imports

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW0020 | `unresolved_module` | an imported module must exist in the workspace | — |
| PW0021 | `unresolved_name` | an imported name must be declared by the module it comes from | — |
| PW0022 | `ambiguous_name` | a name must resolve to exactly one declaration | — |
| PW0023 | `private_access` | a private declaration is not visible outside its module | — |
| PW0029 | `private_data` | a declaration that holds data says whose with `user`, `session` or `public`, not `private` | [0324](../DECISIONS/ADR-0324-private-says-a-declaration-is-not-importable-and-nothing-more.md) |
| PW0024 | `duplicate_declaration` | a module may declare each name once per namespace | — |
| PW0025 | `import_cycle` | modules must not import each other in a cycle | — |
| PW0026 | `unresolved_type` | a written type must name a type visible here, applied to exactly its parameters | — |
| PW0028 | `declared_twice` | a parameter, a field, a case, a policy or an attribute is written once | [0098](../DECISIONS/ADR-0098-a-name-is-written-once-where-it-is-declared.md) |
| PW0027 | `not_a_term` | a call names a function, a data operation, or a type it builds | [0087](../DECISIONS/ADR-0087-a-call-names-a-term.md) |

## Declarations and their clauses

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW0101 | `read_your_writes_needs_a_session` | read-your-writes requires a session-scoped read | [0239](../DECISIONS/ADR-0239-every-code-the-compiler-writes-is-registered.md) |
| PW0102 | `session_state_is_fresh` | session-owned state may not be served stale | — |
| PW0312 | `retry_not_idempotent` | a command that retries must be idempotent | — |
| PW0338 | `sent_without_idempotency` | a command a page's handler calls declares `idempotent_by` | [0154](../DECISIONS/ADR-0154-a-command-a-page-sends-declares-idempotent-by.md) |
| PW0367 | `who_may_run` | a command a reader can run says who may: `requires` and the predicates every request must hold, or `requires Anyone`, which stands alone and is the platform's | [0406](../DECISIONS/ADR-0406-a-command-says-who-may-run-it.md) |
| PW0339 | `command_outside_a_handler` | a command is called only by a page's handler | [0157](../DECISIONS/ADR-0157-a-handler-is-answered-what-its-command-did.md) |
| PW0340 | `route_names_its_parameters` | a route is `/` and segments, a word or a `{parameter}` each, and names each of its page's parameters once | [0160](../DECISIONS/ADR-0160-a-page-s-route.md) |
| PW0341 | `route_declared_twice` | one route is one page's: an address names the page it is for | — |
| PW0342 | `not_found_names_a_case` | a page's `not_found_on` names a case of an error a query it reads can answer | [0163](../DECISIONS/ADR-0163-a-page-says-when-it-is-absent.md) |
| PW0343 | `last_known_good_is_public` | a last-known-good fallback serves only public data | [0177](../DECISIONS/ADR-0177-a-public-read-whose-origin-fails-is-answered-with-the-last-value-kept.md) |
| PW0344 | `reads_more_than_given` | a query asks no more consistency of a source than its reads give | [0207](../DECISIONS/ADR-0207-a-data-source-states-what-it-guarantees.md) |
| PW0345 | `writes_two_sources` | a command's writes are one transaction, in one source | — |
| PW0346 | `isolation_more_than_given` | a command asks no more isolation of a source than its transactions give | — |
| PW0347 | `events_without_commit` | a command's events are sent if and only if its writes commit | — |
| PW0348 | `idempotent_without_commit` | a command's record of an interaction commits with its writes | — |
| PW0349 | `source_malformed` | a source holds what the program's effects name, and each is held by one | — |
| PW0350 | `redirect_names_a_case` | a page's `redirect_on` names a case a query it reads can answer, which carries the one parameter of the page's route, and says whether the move is permanent | [0295](../DECISIONS/ADR-0295-a-page-at-another-address-of-the-page-is-moved-there.md) |
| PW0351 | `predicate_declared` | a predicate a program declares says, in one string with no hole, what a reader is told when it refuses, is declared once, and is given its parameters by type wherever a `requires` names it | [0302](../DECISIONS/ADR-0302-a-refusal-is-told-where-the-press-was.md) |
| PW0313 | `retry_unbounded` | a retry policy must be bounded | — |
| PW0352 | `layout_names_a_layout` | a page's `layout` clause names a layout, once | [0303](../DECISIONS/ADR-0303-a-page-is-shown-in-its-layout.md) |
| PW0353 | `layout_takes_nothing` | a layout is given nothing by the page it shows: it declares no parameters | — |
| PW0361 | `table_malformed` | a table's row is a record declared beside it, in the module of its name, and its key, order, indexes and references name the row's fields, each of a type every adapter keys a row by | [0338](../DECISIONS/ADR-0338-three-databases.md) |
| PW0362 | `data_not_declared` | a table's rows are reached only through the functions its declaration gives | — |
| PW0364 | `all_unbounded` | a table is read whole, by `all()`, only where it states `bounded` | — |
| PW0365 | `refers_across_sources` | a reference to a table another source holds is held by no database: the referred row may be missing, which `get` answers as `None` | — |
| PW0366 | `migration_unstated` | a change from the schema released that loses or reinterprets data is stated by a `migrate`, and each `migrate` states one | — |
| PW0370 | `owner_names_a_principal` | `owned by` names one field of an owned table's row holding a principal's id (`capability.UserId`), and `shares with` others, never its owner, each once | [0384](../DECISIONS/ADR-0384-a-dating-app.md), [0406](../DECISIONS/ADR-0406-a-command-says-who-may-run-it.md), 0395 |
| PW0368 | `owned_read_by_its_reader` | an owned table is read by its reader alone: its key leads with its owner, each index with its owner or a field it shares with, a unique index holds its owner, no bound counts its rows across owners, and it is never read whole | — |
| PW0369 | `private_field_unseen` | a private field is read by its owner alone: a field of an owned table's row, never its owner, a field it shares with, in its key, its order where it shares, or an index another reads by; and its seen row is the table's own name | — |
| PW0327 | `written_rollback` | an optimistic transition's reversal is derived, not written | [0025](../DECISIONS/ADR-0025-an-optimistic-clause-targets-a-resource-entry.md) |
| PW0330 | `optimistic_not_pure` | an optimistic transition must be a pure function of the resource's value | — |
| PW0334 | `derived_not_pure` | a `derived` value, and a value a template computes, is computed from other values, and performs no effect | [0082](../DECISIONS/ADR-0082-a-derived-value-performs-no-effect.md), [0226](../DECISIONS/ADR-0226-a-value-the-template-computes-compiles.md) |
| PW0335 | `policy_value` | a policy's value is one its domain has | [0089](../DECISIONS/ADR-0089-a-policy-value-is-one-its-domain-has.md) |
| PW0331 | `optimistic_target_mismatch` | an optimistic transition must produce the value type of the resource it targets | — |
| PW0336 | `key_omits_a_read` | a cache key names each parameter its entry depends on | [0107](../DECISIONS/ADR-0107-a-cache-key-names-what-its-entry-depends-on.md) |
| PW0332 | `effect_names_an_operation` | an effect is not a callable, so it does not name a host operation | — |
| PW0200 | `cache_no_invalidation` | a shared cache should declare how it is invalidated | — |
| PW5010 | `unsafe_audit_incomplete` | an unsafe escape hatch must carry a complete audit record | — |
| PW5015 | `unsafe_attribution_invalid` | an escape hatch's attribution target must name a real owner | — |

## Effects

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW0408 | `member_receiver_untyped` | a member is read through a value whose type the program states, so what it performs is read | [0351](../DECISIONS/ADR-0351-generic-effects.md) |
| PW0400 | `undeclared_effect` | an effect row must name every effect the body performs | — |
| PW0401 | `forbidden_effect` | some effects are not permitted where a declaration runs, whatever it declares | — |
| PW0402 | `wrong_frame_phase` | each frame phase permits only the work it exists to do | — |
| PW0405 | `read_per_element` | a read in a loop over data is not keyed by the loop's element: the list's reads are one set read, before the loop | — |
| PW0406 | `set_read_per_key` | a page's or a command's calls do not grow with its data: a set read is on a source that batches | — |
| PW0407 | `set_pair_shape` | a keyed read's set form is a function of the same source, given a list of its key and answering a list of its results, each with its key | — |

## Layout and frame phases

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW0403 | `observation_feedback_cycle` | an observation must not cause the change it observes | — |
| PW0404 | `false_independence` | a subtree declared independent must not depend on anything outside it | — |

## Matching

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW0305 | `non_exhaustive_match` | a match must cover every value its scrutinee can take | — |
| PW0333 | `unreachable_arm` | every arm of a match must be reached by some value | [0076](../DECISIONS/ADR-0076-an-arm-no-value-reaches.md) |
| PW0337 | `case_tested_by_if` | a template tests a value's case with `{#match}`, which names every case | [0153](../DECISIONS/ADR-0153-a-template-tests-a-case-with-match.md) |

## Types

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW0600 | `option_used_as_value` | a value that may be absent must be matched before it is used | — |
| PW0601 | `unchecked_external_cast` | an external value must be decoded, not cast | — |
| PW0602 | `handler_signature_mismatch` | a handler must accept the event its attribute delivers | — |
| PW0603 | `constructor_arity` | a constructor pattern must bind exactly the fields its constructor declares | — |
| PW0604 | `call_arity` | a call must supply exactly the arguments its callee declares | — |
| PW0605 | `argument_type` | a value given for a declared parameter or field must have its declared type | — |
| PW0606 | `return_type` | a body must produce the result type its signature declares | — |
| PW0607 | `binding_type` | an annotated binding must be initialised with a value of its declared type | — |
| PW0609 | `operand_type` | an operator's operands, and a condition, must have the types they take | [0230](../DECISIONS/ADR-0230-a-condition-is-a-bool-or-tested-non-empty.md) |
| PW0608 | `pattern_constructor` | a constructor must name a case of the type it builds or matches | [0059](../DECISIONS/ADR-0059-declared-sum-types.md), [0197](../DECISIONS/ADR-0197-a-pattern-tells-a-case-from-a-binding-by-its-capital.md) |
| PW0610 | `unknown_member` | a read or a call through a value must name a member its type has | [0048](../DECISIONS/ADR-0048-members-exist.md) |
| PW0611 | `assign_immutable` | an assignment's target must be a binding declared `let mut` | [0051](../DECISIONS/ADR-0051-early-return-and-loops.md) |
| PW0612 | `record_fields` | a record must be built with each field its type declares, once, and no other | [0067](../DECISIONS/ADR-0067-a-record-is-built-with-its-fields.md) |
| PW0613 | `branch_types` | the branches of an `if` or a `match` whose value is used must produce one type | [0068](../DECISIONS/ADR-0068-what-each-construct-takes.md) |
| PW0614 | `not_callable` | a value called must be a function | [0068](../DECISIONS/ADR-0068-what-each-construct-takes.md) |
| PW0615 | `list_items` | a list's items must share one type | [0069](../DECISIONS/ADR-0069-list-items-and-int-literals.md) |
| PW0616 | `int_literal_range` | an `Int` literal must fit in 64 bits | [0069](../DECISIONS/ADR-0069-list-items-and-int-literals.md), [0039](../DECISIONS/ADR-0039-pure-computation.md) |
| PW0617 | `named_argument` | a named argument is given to the parameter of its name, once, after the positional ones | [0081](../DECISIONS/ADR-0081-a-named-argument-is-its-parameters.md) |
| PW0619 | `view_props` | a view is given each of its parameters, and nothing else | [0136](../DECISIONS/ADR-0136-a-view-is-written-where-it-is-used.md) |
| PW0629 | `names_no_value` | a name a template reads is a value: a binding, a parameter, a signal, a constant, or a predicate the page asks of its reader | [0307](../DECISIONS/ADR-0307-a-control-is-shown-where-its-commands-predicates-hold.md) |
| PW0618 | `result_dropped` | a failure is handled: a `Result` is taken apart, returned with `?`, or discarded by name | [0099](../DECISIONS/ADR-0099-a-failure-is-handled.md) |
| PW0621 | `route_parameter_is_text` | a route's parameter is text: a `String`, or an opaque type over one | [0157](../DECISIONS/ADR-0157-a-handler-is-answered-what-its-command-did.md), [0160](../DECISIONS/ADR-0160-a-page-s-route.md) |
| PW0630 | `query_parameter_is_optional_text` | a query parameter is an `Option` of text: an address may leave it out | [0309](../DECISIONS/ADR-0309-a-pages-query-parameters-are-typed.md) |
| PW0620 | `answer_read_for_a_value` | what a command answers carries no value: a handler matches `Ok(_)`, and reads the value from a query | — |
| PW0622 | `invariant_not_shown` | a value is built of an opaque type only where the build shows it holds the type's invariant | [0179](../DECISIONS/ADR-0179-an-opaque-type-states-its-invariant.md) |
| PW0623 | `invariant_unread` | an opaque type's invariant is bounds on its `Int` value, `value >= 1`, or on its `String`'s length, `String.length(value) <= 280`, joined by `&`, that some value holds | [0225](../DECISIONS/ADR-0225-a-strings-length-is-an-invariant.md) |
| PW0624 | `no_finite_value` | a declared type has a finite value: one that contains itself has a way to be built without itself | [0194](../DECISIONS/ADR-0194-a-type-that-contains-itself-crosses-as-its-nodes.md) |
| PW0625 | `case_name_capitalized` | a sum type's case is named with a capital letter, so a pattern tells it from a binding | [0197](../DECISIONS/ADR-0197-a-pattern-tells-a-case-from-a-binding-by-its-capital.md), [0195](../DECISIONS/ADR-0195-the-owners-rulings-on-fifteen-open-questions.md) |
| PW0626 | `representation_shadowed` | an opaque type's own module reads its representation as `.value`, so it declares no member of that name | [0214](../DECISIONS/ADR-0214-an-opaque-types-value-is-its-representation.md), [0210](../DECISIONS/ADR-0210-the-owners-rulings-on-the-pre-delegation-marks.md) |
| PW0627 | `map_key` | a map's key and a set's element are an `Int`, a `String`, a `Bool`, or an opaque type over one | [0248](../DECISIONS/ADR-0248-a-maps-key-is-ordered-and-refused-at-check.md) |
| PW0628 | `ambiguous_member` | a member names one declaration: its type's, or the one its module declares or imports | [0254](../DECISIONS/ADR-0254-a-member-names-what-its-module-sees.md) |

## Scopes, tasks and resources

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW2001 | `handle_escapes` | a handle cannot outlive the scope that owns it | — |
| PW2005 | `affine_not_consumed_once` | an affine value must be consumed exactly once, in the scope that acquired it | — |
| PW2002 | `task_detached` | an ordinary task cannot be detached from its scope | — |
| PW2003 | `handle_used_late` | a handle cannot be used after its owning scope has exited | — |
| PW2004 | `scope_outlives_owner` | a subscription cannot declare a scope that outlives its owner | — |

## Privacy

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW5001 | `private_in_shared_cache` | a value that is not public cannot live in a shared cache | — |
| PW5003 | `secret_to_browser` | a secret cannot be rendered to the browser | — |
| PW5004 | `cache_key_omits_partition` | a shared cache key must carry every partition its value depends on | — |
| PW5006 | `value_exceeds_sink_level` | a sink accepts only values its declared privacy level admits | — |
| PW5007 | `private_in_resume_manifest` | the resume manifest ships with the public shell and may hold only public values | — |
| PW5008 | `unserializable_capture` | a resumable handler may capture only what can be written to its manifest | — |
| PW5016 | `resume_capture_schema_unnameable` | a resumable capture must have a type this build can derive a schema from | — |
| PW5017 | `resume_artifact_contract_mismatch` | a resume manifest and its handler artifact must describe the same contract | — |
| PW5018 | `resume_destination_unknown` | a private resumable region must say which principal it is private to | — |
| PW5025 | `handler_reads_uncaptured` | a resumable handler reads what it captures, and what it binds itself | [0110](../DECISIONS/ADR-0110-a-resumable-handler-reads-what-it-captures.md) |
| PW5037 | `handle_made` | a session's, a user's or an organization's handle is the platform's to make: a program constructs none | [0263](../DECISIONS/ADR-0263-a-handle-is-the-platforms-to-make.md) |
| PW5038 | `handle_answered` | a data layer answers ids, never a handle: only the platform's operations answer a session's, a user's or an organization's | — |
| PW5039 | `handle_from_browser` | nothing the browser supplies holds a handle: no command's or page's parameter, and no signal, holds a session's, a user's or an organization's | — |
| PW5040 | `handle_to_browser` | a session's handle never reaches the browser: no query's, subscription's or command's answer, and nothing markup prints, holds one | [0264](../DECISIONS/ADR-0264-a-sessions-handle-never-reaches-the-browser.md) |
| PW5046 | `layout_audience` | a page declares at least its layout's audience: its document holds what its layout shows and its resume manifest what the layout's handlers capture | [0303](../DECISIONS/ADR-0303-a-page-is-shown-in-its-layout.md) |
| PW5049 | `reader_predicate_shared` | a page that asks a predicate of its reader is its reader's: a page served to everyone asks none | — |

## Placement

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW5002 | `no_feasible_placement` | every declaration must have somewhere it can run | — |
| PW5005 | `declared_placement_cannot_grant` | a declared placement must be able to grant every effect it requires | — |
| PW5026 | `built_before_its_parameters` | what is built before any request reads no request's value | [0114](../DECISIONS/ADR-0114-what-is-built-before-any-request-reads-no-requests-value.md) |

## Markup

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW5009 | `dead_internal_link` | an internal link must name a route the program declares | — |
| PW5011 | `unkeyed_list` | a list over a mutable collection needs a stable key | — |
| PW5012 | `invalid_nesting` | an element may only contain the children HTML permits | — |
| PW5013 | `handler_on_inert` | interactive behaviour belongs on an element that can receive it | — |
| PW5014 | `control_without_label` | a form control must have something that names it | — |
| PW5053 | `image_without_alt` | an image says what it shows: an `<img>` has an `alt`, empty only where it shows nothing a reader needs, and an image button an `alt` that says what it does | [0404](../DECISIONS/ADR-0404-an-image-says-what-it-shows.md), [0343](../DECISIONS/ADR-0343-a-guide-whose-examples-compile.md), [0405](../DECISIONS/ADR-0405-an-input-button-names-itself.md) |
| PW5019 | `malformed_template_block` | a template block closes with its own name and holds only the markers it takes | [0042](../DECISIONS/ADR-0042-template-branches-and-attributes.md) |
| PW5020 | `view_element` | an element named with a capital letter is a view the compiler composes | [0072](../DECISIONS/ADR-0072-an-element-named-with-a-capital-is-a-view.md) |
| PW5021 | `loop_key` | a loop's key is its element, or a field read from it | [0073](../DECISIONS/ADR-0073-a-template-reads-each-value-by-path.md) |
| PW5023 | `code_in_markup` | a template writes no code, and no value into a stylesheet | [0094](../DECISIONS/ADR-0094-a-template-writes-no-code.md) |
| PW5024 | `moves_a_url` | a template moves no URL: no `<base>`, and no animation of a link or a handler | [0096](../DECISIONS/ADR-0096-a-template-moves-no-url.md) |
| PW5022 | `unknown_event` | an element handles an event the platform declares | [0093](../DECISIONS/ADR-0093-an-element-handles-an-event-the-platform-declares.md) |
| PW5027 | `unknown_modifier` | an event's modifier is one the runtime applies | [0138](../DECISIONS/ADR-0138-a-handler-is-given-its-event.md) |
| PW5028 | `comment_as_text` | a comment in markup is `<!-- -->`: text that reads as one would be shown | [0167](../DECISIONS/ADR-0167-markup-text-is-text.md), [0165](../DECISIONS/ADR-0165-the-store-s-estimate-and-recommendations.md) |
| PW5029 | `title_missing` | a page served at a route states its title | [0183](../DECISIONS/ADR-0183-a-page-states-its-title.md), [0182](../DECISIONS/ADR-0182-keyboard-and-screen-reader-semantics-remain-valid.md) |
| PW5030 | `title_misplaced` | a title is the page's: once, at the top of its view, written as text and values | — |
| PW5031 | `duplicate_id` | an id names one element of its page | [0185](../DECISIONS/ADR-0185-ids-and-the-aria-that-names-them-checked-at-build.md), [0182](../DECISIONS/ADR-0182-keyboard-and-screen-reader-semantics-remain-valid.md) |
| PW5032 | `reference_names_nothing` | an id reference names an element its page shows whenever the referrer is shown | — |
| PW5033 | `aria_unknown` | an ARIA attribute, its value and a role are ones WAI-ARIA defines | — |
| PW5034 | `metadata_misplaced` | a page's metadata is the page's, at the top of its view: named once as text, its content text and values, and a name HTML allows once stated once | [0186](../DECISIONS/ADR-0186-a-page-states-its-description.md), [0183](../DECISIONS/ADR-0183-a-page-states-its-title.md) |
| PW5035 | `link_not_in_body` | a `<link>` in markup is one HTML allows in the body, each relation body-ok or an item's property, or a page's canonical address, once at the top of its view | [0189](../DECISIONS/ADR-0189-a-link-is-written-where-html-allows-it.md) |
| PW5051 | `canonical_address` | a page's canonical address is a page's, `href={Page(args)}`, served at a route without a query part, each argument a value the page reads or text | [0311](../DECISIONS/ADR-0311-a-page-states-its-canonical-address.md), [0280](../DECISIONS/ADR-0280-a-handler-navigates-after-its-command-commits.md) |
| PW5052 | `load_from_another_origin` | a page loads from its own origin: an address its markup fetches, written as text, is a path, and a `<link>`'s address is written as text | [0195](../DECISIONS/ADR-0195-the-owners-rulings-on-fifteen-open-questions.md) |
| PW5036 | `form_control_value` | a form control's value is written where HTML reads it: a `<textarea>`'s as its text, a signal or text, once; a `<select>`'s by the option it marks `selected` | [0221](../DECISIONS/ADR-0221-a-form-controls-value-is-written-where-html-reads-it.md), [0220](../DECISIONS/ADR-0220-the-feed-is-served-in-browsers.md) |
| PW5041 | `form_answered_by_nothing` | a form's `action` is a route that answers its `method`: a page's, the relying party's, or an upload's | [0265](../DECISIONS/ADR-0265-a-form-goes-where-something-answers-it.md) |
| PW5050 | `address_names_no_query_parameter` | a link's query and a GET form's fields name only the query parameters of the route they reach | [0309](../DECISIONS/ADR-0309-a-pages-query-parameters-are-typed.md) |
| PW5042 | `navigate_names_no_page` | a `navigate` names a page by its declaration | [0280](../DECISIONS/ADR-0280-a-handler-navigates-after-its-command-commits.md) |
| PW5043 | `navigate_before_a_commit` | a `navigate` is a handler's, last in the `Ok` arm of the command's answer nearest it: a page is left only once a command has committed | — |
| PW5044 | `slot_misplaced` | a `<slot />` is its layout's: once, at the top of the layout's own view, outside any block, loop, match or stream, with nothing in it | [0303](../DECISIONS/ADR-0303-a-page-is-shown-in-its-layout.md) |
| PW5045 | `layout_without_slot` | a layout shows the page that names it in a `<slot />` | — |
| PW5047 | `markup_as_an_operand` | markup is shown where it is written, and is no operand of an arithmetic, comparison or logical operator | [0243](../DECISIONS/ADR-0243-a-blocks-statements-are-separated.md) |
| PW5048 | `control_where_refused` | a control whose command requires a predicate is shown where the page asks that predicate of its reader and it holds | [0307](../DECISIONS/ADR-0307-a-control-is-shown-where-its-commands-predicates-hold.md) |

## Caches, keys and invalidation

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW5100 | `graph_edge_unresolved` | a dependency, event or invalidation target must name something the program declares | — |
| PW5101 | `private_in_shared_materialization` | a shared materialization may only depend on data every reader of its cache entry may see | — |
| PW5105 | `clause_out_of_place` | a clause belongs to a declaration that reads it | [0088](../DECISIONS/ADR-0088-a-clause-names-a-declaration-and-gives-it-its-key.md), [0092](../DECISIONS/ADR-0092-a-graph-clause-belongs-to-a-declaration-that-can-mean-it.md), [0216](../DECISIONS/ADR-0216-every-clause-belongs-to-a-declaration-that-reads-it.md) |
| PW5104 | `listener_key` | a listener's argument is one of its declaration's parameters, or `_` | [0091](../DECISIONS/ADR-0091-a-listener-binds-its-entrys-key.md) |
| PW5103 | `clause_names_another_kind` | a clause names a declaration of its kind: a resource or a materialization to read, a resource to invalidate, an event to emit or listen for | [0255](../DECISIONS/ADR-0255-a-materialization-may-read-another.md) |
| PW5106 | `write_not_invalidated` | a command invalidates what it writes: each cached reader of the data it changes, by name or through an event the reader listens for | [0101](../DECISIONS/ADR-0101-a-command-invalidates-what-it-writes.md) |
| PW5107 | `optimistic_not_reconciled` | a command invalidates the entry it speculates on, so what it committed replaces the speculation | [0105](../DECISIONS/ADR-0105-a-command-invalidates-the-entry-it-speculates-on.md) |
| PW5108 | `read_names_another_kind` | a `query` reads a query or a resource, or in a materialization's body one that derives its value; and a `subscription` a subscription | [0212](../DECISIONS/ADR-0212-a-query-reads-a-query.md) |
| PW5109 | `materialization_cycle` | a materialization depends on nothing that depends on it | [0255](../DECISIONS/ADR-0255-a-materialization-may-read-another.md), [0195](../DECISIONS/ADR-0195-the-owners-rulings-on-fifteen-open-questions.md) |
| PW5110 | `dependency_stated_twice` | a materialization that derives its value states what it depends on by reading it, and nowhere else | [0273](../DECISIONS/ADR-0273-a-materialization-is-a-value-its-body-derives.md) |

## Capabilities

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW5200 | `unresolved_capability_argument` | a capability's type argument must name a type the program declares | — |
| PW5201 | `unknown_effect_family` | an effect must name a family some package declares | — |
| PW5202 | `unknown_effect_operation` | an effect must name an operation its family declares | — |
| PW5203 | `effect_arity_mismatch` | an effect must be given exactly the type arguments its declaration binds | — |
| PW5204 | `unresolved_impact_marker` | an effect's impact condition must name a type the declaring module can see | — |

## Signals and UI state

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW5300 | `signal_written_outside_handler` | a signal changes only in a handler | [0130](../DECISIONS/ADR-0130-ui-state-is-a-signal-provided-where-it-lives.md) |
| PW5301 | `signal_read_where_it_cannot_change` | a signal is read only where the browser reads it again when it changes: a template part, a handler, or a page query's key | — |
| PW5302 | `ui_state_not_a_signal` | a handler changes a signal, not a binding of the body it is written in | — |
| PW5303 | `modal_dialog` | a modal dialog is shown by a signal's block, and says what closing it does | [0141](../DECISIONS/ADR-0141-a-dialog-a-signal-shows-is-the-browsers-modal-dialog.md) |
| PW5304 | `bound_value` | an input binds its value to a signal of `String`, by its name | [0142](../DECISIONS/ADR-0142-an-input-bound-to-a-signal.md) |
| PW5305 | `signal_not_provided` | a provided signal is read only where a page or view around it provides it | [0144](../DECISIONS/ADR-0144-a-view-holds-signals-and-a-page-provides-them.md) |
| PW5306 | `provide` | a body provides a signal declared in a module, once | — |
| PW5307 | `signal_in_a_row` | a view that holds a signal is not used in a loop's row yet | [0144](../DECISIONS/ADR-0144-a-view-holds-signals-and-a-page-provides-them.md) |
| PW5308 | `signal_key_type` | a signal that keys a query is a `String`, an `Int` or a `Bool` | [0152](../DECISIONS/ADR-0152-a-key-a-page-changes.md) |
| PW5309 | `signal_key_stale_work` | a query a signal keys says what its stale work does | — |

## Streamed regions

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW5400 | `streamed_read_outside_stream` | a query declared `delivery streamed` is read only by a `<stream>` | [0148](../DECISIONS/ADR-0148-a-stream-region-shows-its-query-s-state.md) |
| PW5401 | `stream_states` | a `<stream>` shows each state its query can be in, and no other | — |
| PW5402 | `streamed_without_timeout` | a query declared `delivery streamed` declares how long it may take | — |

## Uploads

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW5601 | `upload_malformed` | an upload states its route, where it is served, and each of its limits, once and as literals | [0253](../DECISIONS/ADR-0253-two-tracks-are-built-in-parallel-each-in-files-of-its-own.md), [0260](../DECISIONS/ADR-0260-an-image-on-a-post-a-typed-upload-served-safely.md) |
| PW5602 | `upload_route_taken` | an upload's paths are its own: no page's, and no other upload's | — |
| PW5603 | `file_form_posts_to_no_upload` | a form that sends a file posts it, as multipart/form-data, to an upload the program declares | — |

## Todomvc

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW6101 | `fragment_names_no_signal` | a page's `fragment` clause names a signal the page declares | — |
| PW6102 | `fragment_signal_not_text` | a signal bound to the address's fragment is a `String`: a fragment is text | — |
| PW6103 | `fragment_forms` | a fragment's forms are written `"form" => "value"`, each form once, in characters a fragment holds | — |
| PW6104 | `fragment_default` | a signal bound to the fragment starts at a value one of the forms gives | — |
| PW6105 | `fragment_signal_assigned` | a signal bound to the fragment is written by the address alone, and no handler assigns it | — |
| PW6106 | `link_names_no_form` | a link to a fragment of a page that binds its fragment names a form the page takes | — |

## Payments

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW6401 | `invoked_command_requires` | a command the platform invokes holds no reader to a predicate: it declares no `requires` | — |
| PW6402 | `invoked_command_parameters` | a command `invoked_by Payments` is given one `payments.Settled`, and nothing else | — |
| PW6403 | `invoked_twice` | one command answers a payment's outcome: a second would leave the host two answers | — |
| PW6404 | `invoked_command_pressed` | no page presses a command the platform invokes | — |
| PW6405 | `invoked_command_not_idempotent` | a command the platform invokes is idempotent: its word, a provider's or the clock's, may come more than once | — |

## Scheduled commands

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW6501 | `schedule_names_no_clocked_command` | a `schedule` names a command `invoked_by Clock`, written as a call | — |
| PW6502 | `scheduled_outside_a_command` | a `schedule` or an `unschedule` is a command's statement: what a page shows schedules nothing | — |
| PW6503 | `schedule_time` | a scheduled run's time is `at` an `Instant` or `after` a `Duration` | — |

## Device store

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW6801 | `device_value_to_a_command` | a value the device keeps is no command's argument, and decides no command's call: a command runs on the server | [0366](../DECISIONS/ADR-0366-device-local-storage.md) |
| PW6802 | `device_value_keys_a_query` | a value the device keeps keys no page's query: the key is sent to the server to read it again | — |
| PW6803 | `device_value_sent` | a value the device keeps is written nowhere the browser sends: a navigation, an address, a form | — |
| PW6804 | `device_keeps_its_own` | a signal the device keeps is assigned no session's, user's, organization's or secret's value | — |
| PW6805 | `device_signal_clauses` | a signal the device keeps says where it rests, `storage device "name"` (one name a program), whose it is, `privacy Device`, and what two tabs' writes do, `conflict last_write_wins` | — |
| PW6806 | `device_signal_versions` | a signal the device keeps has a pure migration for each version before its own, each taking the one before it to the next | — |
| PW6807 | `device_signal_form` | a signal the device keeps is of a type the device can hold: data, no function, no type that contains itself, no opaque type with a `where` | — |
| PW6808 | `device_signal_provided` | a signal the device keeps is provided by the device: no `provide` gives it a value | — |

## Declared tables, served

| Code | Symbol | What the program must hold to | ADR |
|---|---|---|---|
| PW6901 | `row_holds_handle` | a table's row holds no session's, user's or organization's handle: every reader of the table reads its rows, and a handle is its own reader's alone; a row names someone by their id | [0338](../DECISIONS/ADR-0338-three-databases.md) |
| PW6905 | `predicate_body_reads_only` | a predicate's body reads and does nothing else: no write, no event, no schedule, no network, no log, no clock but its comparison | [0384](../DECISIONS/ADR-0384-a-dating-app.md), 0395, [0115](../DECISIONS/ADR-0115-a-commands-requires-is-held-before-its-body-runs.md) |
| PW6907 | `required_query_unshared` | a query that `requires` a predicate is answered to its reader alone, once the predicate holds for them: it is cached `private`, or is a `user` or `session` query, so every read of it is keyed by its reader, and no materialization depends on it | [0384](../DECISIONS/ADR-0384-a-dating-app.md), [0339](../DECISIONS/ADR-0339-a-collaborative-document-app.md) |
| PW6924 | `reference_within_one_owner` | a reference a database holds stays within one owner's rows: an owned table refers to an owned table from its own owner field alone, a table no one owns to no owned table, and an owned table to no table no one owns | [0384](../DECISIONS/ADR-0384-a-dating-app.md) |
| PW6925 | `predicate_answered_by_its_body` | a predicate the program gives a body is answered by that body alone: a `requires` naming it sees it where it is written, no page asks it of its reader, and no predicate the deployment answers shares its name | [0384](../DECISIONS/ADR-0384-a-dating-app.md), [0115](../DECISIONS/ADR-0115-a-commands-requires-is-held-before-its-body-runs.md), [0307](../DECISIONS/ADR-0307-a-control-is-shown-where-its-commands-predicates-hold.md) |
| PW6926 | `required_query_bound_by_a_page` | a query that `requires` a predicate is read by a page's binding alone, `query Q(..)`, where the host asks its reader: no query, command, function, predicate or materialization calls it, binds it or names it, which would run its body unasked | [0384](../DECISIONS/ADR-0384-a-dating-app.md), [0339](../DECISIONS/ADR-0339-a-collaborative-document-app.md) |

## Retired codes

No longer emitted, and never reused.

| Code | Why |
|---|---|
| PW5102 | a shared materialization's cache key must separate the build that produced it. Retired 2026-08-06: the compatibility generation is now injected by the compiler for every materialized entry, so the key cannot omit it. If a generated storage key ever lacks its compatibility namespace that is an internal compiler invariant, not an error an application author can repair. |

## Old numbers

Numbers that named an invariant the code beside them names now: one invariant
has one code. A diagnostic carries that code, never one of these, and a
fixture's `@rule` must name a current registered code. An old number is never
given to another invariant.

| Old number | Now | Symbol |
|---|---|---|
| PW0326 | PW2004 | `scope_outlives_owner` |
| PW0311 | PW2002 | `task_detached` |
| PW0309 | PW2005 | `affine_not_consumed_once` |
| PW0310 | PW2005 | `affine_not_consumed_once` |
| PW0301 | PW5002 | `no_feasible_placement` |
| PW0324 | PW5002 | `no_feasible_placement` |
| PW0323 | PW5005 | `declared_placement_cannot_grant` |
| PW0302 | PW5003 | `secret_to_browser` |
| PW0308 | PW5008 | `unserializable_capture` |
| PW3011 | PW5007 | `private_in_resume_manifest` |
| PW0328 | PW5007 | `private_in_resume_manifest` |
| PW0304 | PW5006 | `value_exceeds_sink_level` |
| PW0303 | PW5004 | `cache_key_omits_partition` |
| PW0100 | PW5001 | `private_in_shared_cache` |
| PW3001 | PW0401 | `forbidden_effect` |
| PW3002 | PW0401 | `forbidden_effect` |
| PW3004 | PW0402 | `wrong_frame_phase` |
| PW3008 | PW0402 | `wrong_frame_phase` |
| PW3003 | PW0402 | `wrong_frame_phase` |
| PW3006 | PW0401 | `forbidden_effect` |
| PW3007 | PW0401 | `forbidden_effect` |
| PW3005 | PW0403 | `observation_feedback_cycle` |
| PW3009 | PW0404 | `false_independence` |
| PW0300 | PW0401 | `forbidden_effect` |
| PW0314 | PW0401 | `forbidden_effect` |
| PW0315 | PW0401 | `forbidden_effect` |
| PW0306 | PW0600 | `option_used_as_value` |
| PW0307 | PW0601 | `unchecked_external_cast` |
| PW0320 | PW0602 | `handler_signature_mismatch` |
| PW0316 | PW5011 | `unkeyed_list` |
| PW0321 | PW5009 | `dead_internal_link` |
| PW0317 | PW5012 | `invalid_nesting` |
| PW0318 | PW5013 | `handler_on_inert` |
| PW0319 | PW5014 | `control_without_label` |
| PW0329 | PW5010 | `unsafe_audit_incomplete` |
| PW0322 | PW5010 | `unsafe_audit_incomplete` |
| PW3010 | PW5010 | `unsafe_audit_incomplete` |
| PW0325 | PW5309 | `signal_key_stale_work` |
