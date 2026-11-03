//! The platform library is compiler **input**, and gets checked like it.
//!
//! Architect ruling, 2026-08-06, after `Stores.find` was found returning
//! `Result<Store, StoreError>` — which is `get`'s type:
//!
//! > E2C declarations cannot be treated as unquestioned truth merely because
//! > they replaced hard-coded tables. The architecture is still right — one
//! > signature should feed types, effects, privacy, and placement — but the
//! > signatures themselves need validation.
//!
//! The failure that prompted it is worth stating precisely, because it is
//! subtle. R-008 annotated `let store: Option<Store> = Stores.find(id)`. The
//! annotation was right about the intent and wrong about the library, and the
//! rule read the annotation — so **the fixture's own text was establishing the
//! library's return type**. The fixture passed for a reason that had nothing
//! to do with the library being correct.
//!
//! What is checked here:
//!
//! - the platform parses, resolves and produces no diagnostics;
//! - every signature is reachable and complete;
//! - a declaration that promises a label or an effect actually carries it;
//! - the accepted corpus exercises the signatures the checkers depend on.
//!
//! What is NOT checked, and cannot be: whether a declaration matches an
//! implementation that does not exist. `browser.pw`'s `offsetWidth` has no
//! body — it describes something the platform does. Those are a **trusted
//! contract**, and the last test in this file hashes them so a change to one
//! is a visible event rather than a diff nobody reviewed.

use std::collections::BTreeSet;

use pw_core::check::check_sources;

fn platform() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in ["packages/pw-std", "packages/pw-platform-web"] {
        for e in std::fs::read_dir(root.join(dir)).unwrap_or_else(|e| panic!("{dir}: {e}")) {
            let p = e.expect("entry").path();
            if p.extension().is_some_and(|x| x == "pw") {
                out.push((
                    p.file_name().unwrap().to_string_lossy().to_string(),
                    std::fs::read_to_string(&p).expect("read"),
                ));
            }
        }
    }
    out.push((
        "domain.pw".to_string(),
        std::fs::read_to_string(root.join("examples/domain.pw")).expect("domain.pw"),
    ));
    out.sort();
    out
}

fn with_lib() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = platform();
    for e in std::fs::read_dir(root.join("examples/lib")).expect("examples/lib") {
        let p = e.expect("entry").path();
        if p.extension().is_some_and(|x| x == "pw") {
            out.push((
                p.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn the_platform_library_parses_resolves_and_checks_clean() {
    let mut problems = Vec::new();
    for (name, diags) in check_sources(&with_lib()) {
        for d in &diags {
            problems.push(format!("{name}: [{}] {}", d.code, d.message));
        }
    }
    assert!(
        problems.is_empty(),
        "the platform library does not check clean, so every rule that reads it \
         is reading something the compiler itself rejects:\n  {}",
        problems.join("\n  ")
    );
}

/// A declaration that promises something must carry it.
///
/// Three internal consistency rules, each of which `Stores.find` would have
/// been caught by if it had promised anything — it did not, which is why this
/// test also has a fourth check that is about the corpus rather than the file.
#[test]
fn every_platform_signature_is_internally_consistent() {
    use pw_core::lower::lower_file;
    use pw_core::resolve::Workspace;
    use pw_core::signatures::Signatures;
    use pw_syntax::parse_tree;

    let sources = with_lib();
    let hirs: Vec<_> = sources
        .iter()
        .map(|(_, s)| lower_file(s, &parse_tree(s).green))
        .collect();
    let refs: Vec<&pw_core::hir::Hir> = hirs.iter().collect();
    let sigs = Signatures::build(&Workspace::build(&refs), &refs);

    let mut problems = Vec::new();
    for (path, sig) in sigs.iter() {
        // A row naming a type argument must name a type: `secret<Payments>`
        // and `resource.acquire<MapHandle>` are checkable claims, and a
        // misspelt one silently stops matching anything.
        for e in &sig.effects {
            let Some((_, rest)) = e.split_once('<') else {
                continue;
            };
            let Some(arg) = rest.strip_suffix('>') else {
                problems.push(format!("{path}: effect `{e}` has an unclosed argument"));
                continue;
            };
            if arg.is_empty() {
                problems.push(format!("{path}: effect `{e}` has an empty argument"));
            }
        }
        // Every written type must resolve completely, not merely have a
        // nonempty outer argument list. The resolver owns formation/arity.
        for (position, resolution) in sig.params.iter().enumerate() {
            if let Some(r) = resolution
                && !r.is_resolved()
            {
                problems.push(format!("{path}: argument {position}: {r}"));
            }
        }
        if let Some(r) = &sig.returns
            && !r.is_resolved()
        {
            problems.push(format!("{path}: result: {r}"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n  "));
}

/// The signatures the checkers depend on are exercised by a program.
///
/// A declaration nothing uses is a declaration nothing has checked. These are
/// the ones a rule reads directly, so a change to one silently changes what a
/// rule does.
#[test]
fn the_signatures_the_checkers_depend_on_are_exercised() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut used = String::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).expect("examples") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "pw") {
                used.push_str(&std::fs::read_to_string(&p).expect("read"));
            }
        }
    }

    // `(declaration, what a program that exercises it contains, which rule
    // reads it)`.
    //
    // The middle column is not always the declaration's name, and that is the
    // point of having it. `set_transform` is how `layout.rs` learns that
    // `transform` is compositable, but a program exercising it writes
    // `transform:` in an animation and never names the setter. Checking for
    // the declaration's own name reported it as unexercised — a false alarm
    // that would have been "fixed" by deleting the row.
    const LOAD_BEARING: &[(&str, &str, &str)] = &[
        (
            "set_padding",
            "set_padding",
            "layout.rs — layout-affecting write",
        ),
        (
            "set_transform",
            "transform:",
            "layout.rs — the compositable properties",
        ),
        (
            "set_opacity",
            "opacity:",
            "layout.rs — the compositable properties",
        ),
        ("raw_html", "raw_html", "check.rs — the unsafe-audit rule"),
        (
            "now_wall",
            "now_wall",
            "effects.rs — the reuse/determinism rule",
        ),
        (
            "current_location",
            "current_location",
            "check.rs — declared placement",
        ),
        (
            "set_text",
            "set_text",
            "effects.rs — a painter reaching the document",
        ),
        (
            "events.submit",
            "on:submit",
            "annotations.rs — the event a handler takes",
        ),
        (
            "begin",
            "Database.begin",
            "affine.rs — resource acquisition",
        ),
        ("rollback", "rollback", "affine.rs — resource release"),
        (
            "payments",
            "secrets.payments",
            "labels.rs — a secret's origin",
        ),
        ("public", "log.public", "check.rs — the public sink"),
        (
            "find",
            "Stores.find",
            "annotations.rs — an Option-returning initialiser",
        ),
        (
            "style",
            "self.style",
            "infer.rs — the ElementRef -> Style member chain",
        ),
    ];
    let mut unexercised = Vec::new();
    for (name, token, why) in LOAD_BEARING {
        if !used.contains(token) {
            unexercised.push(format!("{name} (looked for {token:?}) — {why}"));
        }
    }
    assert!(
        unexercised.is_empty(),
        "these platform declarations are read by a rule and used by no program, \
         so nothing would notice if they changed:\n  {}",
        unexercised.join("\n  ")
    );
}

/// The trusted contract, content-hashed.
///
/// `offsetWidth`, `now_wall`, `raw_html` and the rest have no implementation
/// here — they describe what the platform does, and nothing in this repository
/// can verify that description. They are therefore **trusted**, and the only
/// honest handling of a trusted input is to make a change to it visible.
///
/// When this test fails, the platform contract changed. That is allowed. What
/// is not allowed is it changing without anyone deciding to.
#[test]
fn the_trusted_platform_contract_is_hashed() {
    // FNV-1a. Not cryptographic — this detects an unreviewed edit, not an
    // adversary — and it is stable across machines and Rust versions, which a
    // DefaultHasher is not.
    fn fnv(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut hash: u64 = 0;
    for (name, src) in platform() {
        // Comments and blank lines are documentation, not contract.
        let contract: String = src
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        hash ^= fnv(format!("{name}\n{contract}").as_bytes());
        names.insert(name);
    }

    // Changed 2026-08-06: `examples/domain.pw` gained `MenuItem { id, name }`.
    // `Menu` returned `List<MenuItemId>` while the store page rendered
    // `item.name` — describing a list of ids and reading a list of items. That
    // was invisible while nothing depended on the element's type and stopped
    // being invisible when a resumable handler inside the loop began hashing
    // it into a capture schema.
    //
    // Changed 2026-08-07: `packages/pw-platform-web/style.pw` gained
    // `type LayoutAffect`. `style.mutate<LayoutAffect>` appears in five corpus
    // files — R-034, R-035, A-020 and two generality witnesses — and
    // `LayoutAffect` was declared NOWHERE. The distinction the module's own
    // comment calls "the point" rested on a spelling no checker could resolve.
    // Found by `PW5200` on its first run against the real corpus.
    //
    // Changed 2026-08-07: `packages/pw-std/effects.pw` and
    // `packages/pw-platform-web/effects.pw` are new — the effect ontology's
    // slice 3, declaring all 25 effects the corpus writes. This is the largest
    // single change to the trusted contract so far and it is deliberately a
    // change to the contract rather than to any checker: `interface_for` still
    // formats `pw:host/{family}`, and the `host` clauses here are what will
    // replace it. Nothing reads them yet, so this commit changes what the
    // platform SAYS and not yet what the compiler does.
    //
    // Two declared arities disagree with the corpus and are recorded rather
    // than reconciled — `tests/effect_vocabulary.rs`'s `ARITY_UNSETTLED`.
    //
    // Changed again 2026-08-07: every effect that has one gained a `placement`
    // clause. Architect ruling — an effect with `capability none` still needs
    // somewhere it is meaningful, and for browser-semantic effects the
    // declaration is the only thing that can say so. This is the fact
    // `World::worlds_for` holds as a hard-coded table, and moving it here is
    // what eventually lets that table be deleted rather than relocated.
    // Changed again 2026-08-07: `style.pw` gained `type PaintOnly` and its two
    // bare `style.mutate` rows became `style.mutate<PaintOnly>`. Architect
    // ruling — `effect style.mutate<T>` binds a parameter and a use must
    // supply it, so the bare form said "does not invalidate layout" by
    // omission. There was no way to say it positively; now there is.
    // Changed again 2026-08-07: `std.effects` and `web.effects` gained
    // `prelude Effect`, and `log.pw` gained `import capability.{ Private,
    // Public }`. Architect ruling — effect NAMES are ambient because the
    // package exports them; their ARGUMENTS are not, and `log<Public>` had
    // been naming a type it never imported.
    // Changed again 2026-08-07: `effect database.connect` is new and
    // `database.write` became `database.write<T>`. Architect rulings —
    // `Database.connect` does not perform `database.read<Database>` and only
    // said so for want of vocabulary; and a component permitted
    // `database.write<Carts>` should not thereby write Payments. The second is
    // the ontology deliberately leading the corpus rather than describing it.
    // Changed again 2026-08-07: effects declare `impact` facets — what they do
    // to the FRAME, as opposed to what family they are named after. Architect
    // ruling: `style.mutate<LayoutAffect>` is not a layout-family effect but it
    // IS layout-affecting, so phase legality could not be keyed on the family.
    // `web.effects` also imports `style.{ LayoutAffect }`, because
    // `impact layout_write when LayoutAffect` must name a declaration.
    // Changed again 2026-08-07: `session.read` lost its `placement` clause.
    // Architect ruling — `Session<A>` says who may observe a value and
    // `session.read` says what authority reads session state; deriving the
    // second from the first was a guess. Topology decides where a session
    // provider exists.
    // Changed again 2026-08-10: `effect build.input.read<T>` is new, and
    // `packages/pw-platform-web/build.pw` declares `include_markdown` and
    // `WorkspaceFile`. Architect ruling — `include_markdown` is a tracked build
    // input operation, not an exempt compiler spelling and not an unrestricted
    // filesystem read. `placement build` is what carries A-013's determinism
    // claim; before this the call named nothing and the page's contract
    // permitted the browser.
    // Changed again 2026-08-20: `current_session` carries
    // `host "pw:host/session#read"` and has no body. Architect ruling — host
    // implementation is explicit declaration metadata, never inferred from a
    // missing or `todo` body or from the effect row. The Wasm encoder found
    // why: a capability authorizes an operation and does not identify one, so
    // the binding has to name the callable.
    // Changed again 2026-08-20, later the same day, and by TWO changes:
    //
    // - every `effect` declaration lost its `host` clause. Architect ruling —
    //   an effect is what computation does, a capability is authority, and an
    //   operation is a callable ABI; an effect naming an operation is the
    //   fossil of deriving one from the other, which ADR-0026 killed. It is
    //   `PW0332` now rather than ignored, because dead syntax that still parses
    //   survives.
    // - `context.pw` imports `SessionId`, `UserId` and `OrganizationId`.
    //   `current_session() -> Session<SessionId>` named a type it had not
    //   imported, and nothing required it to resolve: a privacy label compares
    //   written spellings and never looked its argument up. Putting the
    //   operation's signature on the ABI is what asked the question.
    // Changed again 2026-08-20: `examples/domain.pw` no longer declares
    // `opaque type SessionId = String`. Architect ruling — "current session" is
    // supplied by the execution environment, so the session principal is a
    // PLATFORM type and the application does not get to define independently
    // what identity type the platform's current session has. There were two
    // declarations wrapping `String`, `add_to_cart` passed the platform's into
    // the application's, and nothing compared them. Twenty-five files now
    // import `capability.SessionId`.
    // 2026-09-16: List explicitly imports decode.Unknown and decode.Decoder;
    // all written platform parameter/result types now resolve in their module.
    // 2026-09-25: `device.pw` declares `type Location` instead of importing it
    // from `examples/domain.pw`. The platform package imported a type from the
    // store example, so every program using the platform had to include the
    // store's domain; the kiokun slice, the second application, was the first
    // to notice. The platform package now checks on its own.
    // 2026-09-25: `List`'s operations are `intrinsic` declarations with no
    // bodies, where `map` had returned `[]` and `length` 0, and `string.pw`
    // declares the `String` module (ADR-0040). The compiler supplies both.
    // Later the same day `List.group_by` joined them, for kiokun's ranking
    // (ADR-0041).
    // 2026-09-25: `float.pw` declares the `Float` module and `from_int`
    // (ADR-0043). Arithmetic takes two values of one type, which the checker
    // now enforces, so a count used in a `Float` computation is converted.
    // 2026-09-25: `examples/domain.pw` declares `count` for `PositiveInt` and
    // `line_count` for `Cart`, and imports `List` for the second (ADR-0048).
    // A member no type has is refused now: A-001 read `.value` from an opaque
    // `PositiveInt` outside its module, and the store's page rendered a
    // `cart.line_count` that `Cart` did not have.
    // 2026-09-25: `List` declares `drop`, `slice` and `reverse`, and `String`
    // `slice` (ADR-0055). `List.sum` and `List.maximum` are written in Pleris,
    // where their placeholder bodies answered 0.0, and `maximum` returns an
    // `Option<Float>`. `List.enumerate`, which answered `[]` with no result
    // type, is removed: the language has no tuple type.
    // 2026-09-25: `String` declares `to_lower` and `to_upper`, Unicode 17.0's
    // mapping per code point, from tables the compiler generates (ADR-0056).
    // 2026-09-25: `map.pw` and `set.pw` declare the `Map` and `Set` modules,
    // over the language's `Map<K, V>` and `Set<T>` (ADR-0057).
    // 2026-10-02: `examples/domain.pw` declares `added`, `same_item` and
    // `unpriced`, the helpers `Carts.with_line` is written with now that an
    // optimistic transition runs (ADR-0122). Its body was `cart`, a stub.
    // 2026-10-02: `events.SubmitEvent`'s `prevented` is a `Bool`, whether the
    // form's own submission was stopped, now that the runtime reads each
    // event's record in the listener (ADR-0138). It was a `String` nothing
    // read or filled.
    // 2026-10-02: `events` declares `close`, a dialog's, whose record is its
    // return value (ADR-0141): a modal dialog's closing is heard by the
    // signal that shows it.
    // 2026-10-03: `examples/domain.pw`'s `Store` and `MenuItem` declare a
    // `description`, charter §15.1's, which the store's page shows
    // (ADR-0166).
    // 2026-10-03: `examples/domain.pw`'s `MenuItem` declares a `price`, a
    // `Money<USD>`, charter §15.1's, and `display` and `grouped` write one
    // as a menu's row shows it, as en-US writes dollars (ADR-0169).
    // 2026-10-04: `examples/domain.pw`'s `CartLine` records its item's `name`
    // beside its price, and `fewer`, `total` and `subtotal` are declared: the
    // store's cart lists its lines, each with − and +, its total, and the
    // cart's subtotal (ADR-0172). `unpriced` is removed: a new line takes the
    // item's price, as the page showed it.
    // 2026-10-04: `examples/domain.pw`'s `MenuItem` declares `available`,
    // charter §15.1's, which the menu's row decides its Add by, before the
    // press (ADR-0178).
    // 2026-10-04: `examples/domain.pw`'s `PositiveInt` states its invariant,
    // `value >= 1`, which every construction is shown to hold and every
    // boundary checks, and `fewer` answers an `Option` (ADR-0179).
    // 2026-10-04: `clock` declares `Instant`, a point in time, and
    // `examples/domain.pw`'s `DeliveryEstimate` is a range, `min_minutes` to
    // `max_minutes`, each a `PositiveInt`, and when it was made,
    // `generated_at` (ADR-0180, charter §15.1).
    // 2026-10-04: `examples/domain.pw`'s `MenuItem` declares its `store_id`
    // and its `category`, a `MenuCategory`, and `MenuSection` groups a
    // store's menu by category, charter §15.1's (ADR-0181).
    // 2026-10-04: `examples/domain.pw` declares `OrderStatus`, where a
    // session's order is, and `OrderError`, why one was not placed: the store
    // places its cart as an order and follows it (ADR-0193).
    // ADR-0208 declared `outbox.write`, the effect of emitting an event, and
    // ADR-0214 renamed `LayoutSnapshot`'s accessor from `value` to `measured`.
    // ADR-0304: `clock` declares `TimeZone`, `LocalDate`, `LocalTime` and
    // `Weekday`, the host's `passed`, `today_in`, `at`, `date_of` and
    // `time_of`, and `clock.compare` and `clock.zone`, the wall clock read by
    // comparing it and the host's tz database.
    // 2026-10-09: `examples/domain.pw`'s `CartError` gains `OutOfRange`, an
    // item's store that does not reach the reader's chosen address, and
    // `OrderError` `NoCoverage`, an order such a store's item is in (track
    // `store-accounts`, milestone 2).
    // 2026-10-12: `examples/domain.pw`'s `Store` gains `cuisine`, what the
    // store serves in its own words (ADR-0308, amended).
    // 2026-10-12: `clock` gains `day_number`, `numbered_day`, `midnight`
    // and `minute_of_day`, a date as its number and back, a day's first
    // minute and a time's minute; `domain.pw` gains `same_store`,
    // `OrderError.Closed(Closing)`, `Closing`, `Opening` and `OpeningDay`
    // (track `store-hours`; the integrator's rulings of 2026-10-12).
    // 2026-10-13: `examples/domain.pw`'s `Money<C>` is opaque, an amount's
    // minor units read in `domain` alone, with `cents`, `plus`, `minus`,
    // `times` and `sum`, and a part of an amount only at a `Rate` and a named
    // `Rounding` (`at_rate`; ADR-0327, NEXT 6a); with store-hours' change
    // beside it on train-2.
    // 2026-10-13: `domain.pw` gains `is_less`, which ADR-0327 named and no
    // module declared, `OrderError.Promo(PromoError)` and
    // `OrderError.TotalChanged(Money<USD>)`, and the checkout's `Tip`,
    // `PromoError`, `TipError`, `FundedBy`, `PartCharged`, `OrderCharged` and
    // `Charges` (track `order-total`, NEXT 6b).
    // 2026-10-13: `payments` joins the platform (NEXT 6c, step six): a
    // `Payment`, its `PaymentStatus`, a `Settled` outcome and `begin`,
    // effect `payments.begin`, at the origin alone as the deployment's key.
    // 2026-10-13: `payments.begin` takes no key, which is the host's (step
    // seven); a `Payment` carries the provider's `page` for it, and
    // `BeginError` gains `NotTaken`. `domain.pw`'s `OrderStatus` gains
    // `AwaitingPayment(PaymentPage)`, its `OrderError` `AwaitingPayment` and
    // `PaymentUnavailable`, and it begins a payment (`begin_payment`).
    // 2026-10-13: step eight. `payments` gains `cancel`, effect
    // `payments.cancel`, and its `CancelError`; `domain.pw`'s `OrderStatus`
    // gains `Canceled`, and it declares `CancelOrderError` and
    // `AwaitedPayment`.
    // 2026-10-13: `domain.pw`'s `TipError` loses `NotOffered`: the tip is
    // one `choose_tip(Tip)`, a case a browser sends (ADR-0331, track
    // `store-switch`).
    // 2026-10-14: `clock.pw` declares `Duration` and `seconds`, `minutes`
    // and `hours`; `effects.pw` declares `clock.schedule` (the
    // scheduled-commands track).
    // 2026-10-14: `clock.pw` declares `later`, `earlier` and `is_before`,
    // pure: an instant moved along the clock or ordered against another,
    // never the clock read (track `scheduled-orders`). And `examples/domain.pw`
    // declares `ScheduledFor`, where and for when an order is scheduled; its
    // `OrderStatus` gains `Scheduled(ScheduledFor)`, and its `OrderError`
    // `NotOffered(Instant)` and `Scheduled`. Accepted by the integrator,
    // 2026-10-15 (the hash did not move for the ruling).
    // 2026-10-14, merged 2026-10-20 (track `device-store`): `events`
    // declares `dblclick`, a double press, read as a press is, and `blur`, a
    // control's losing focus, with a `BlurEvent` holding the value it held
    // (track `todomvc`: an edit begun by a double press and saved on blur).
    // 2026-10-16: `browser` declares the opaque `PaintContext` given to a
    // painter's draw binder and `rect` with four Float arguments and
    // `paint.custom`, so A-021's operation is typed and its effect observed
    // (ADR-0351-generic-effects; declaration-only platform scope).
    // 2026-11-02 (track `data-path`): `context` declares `Account` and
    // `account_of` (`pw:host/principal#account`), the reader's account: the
    // id their handle names (ADR-0384's H1 `id_of`) and the name their
    // provider gave them, none for no one and for a guest, whose user's id
    // holds its session: what a program that declares its data needs of its
    // reader, which the guestbook read through the feed's layer.
    // The integration also retains capability.id_of(User<UserId>) -> UserId
    // (ADR-0384 H1), beside context.account_of's Account record and host
    // operation. Both declarations are intentional trusted-platform input;
    // neither track's isolated source hash describes their union.
    const EXPECTED: u64 = 0x0c6761b484fa478d;
    eprintln!(
        "  platform contract: {} files, hash {hash:#018x}",
        names.len()
    );
    {
        assert_eq!(
            hash, EXPECTED,
            "the trusted platform contract changed. If that was intended, update \
             EXPECTED and say in the commit message which declaration changed and \
             why — every checker that reads it is affected."
        );
    }
}
