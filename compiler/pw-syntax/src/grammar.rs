//! The tree-emitting parser: declarations **and** core expression bodies.
//!
//! ADR-0012 sequenced this deliberately — Rowan first, then body parsing — so
//! the body grammar is written once, against the durable representation.
//!
//! Architect ruling on scope: build the **real** expression syntax now, and
//! stage how much *semantic meaning* is implemented over it. Both shortcuts were
//! rejected with reasons worth restating, because they will look tempting again:
//!
//! - an *effect-only* grammar creates a second mini-language. The real parser
//!   eventually sees one program and the effect parser currently sees another;
//!   it can misassociate lambda bodies, call arguments, operator precedence,
//!   nested blocks or match arms while still producing plausible effect results.
//! - *declarations alone* cannot analyse
//!   `List.map(items, item => database.read(item.id))` — the higher-order case
//!   the corpus exists to catch.
//!
//! Trivia is attached before each significant token, so the tree stays lossless
//! by construction rather than by a later repair pass.

use crate::kind::{SyntaxKind as K, SyntaxNode};
use crate::lexer::{Kind, Span, Token, lex};
use crate::tree::TreeBuilder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
}

pub struct Parse {
    pub green: SyntaxNode,
    pub errors: Vec<SyntaxError>,
}

impl Parse {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Keywords that may begin a declaration; also the resynchronisation set.
///
/// `pub(crate)` because `parser.rs` had its own copy of this list and they had
/// to agree. They stopped agreeing the moment E6 added `event`: the tree
/// grammar accepted the declaration and the declaration parser reported
/// "expected a declaration", for the same file, in the same build. Two lists
/// that must be identical are one list.
pub const DECL_STARTERS: &[&str] = &[
    "module",
    "import",
    "opaque",
    "type",
    "fn",
    "view",
    "component",
    "page",
    // A page's layout: the markup and the bindings the pages that name it
    // share, the page shown in its `<slot />`. A declaration only where a
    // name follows: `layout.measure` is an effect's path.
    "layout",
    "query",
    "command",
    "subscription",
    "resource",
    "materialize",
    "event",
    // ADR-0207: a data source, and what it guarantees.
    "source",
    // Track `tables` (ADR-0338, D1): an app's data, declared: a table's row,
    // key, order and indexes, from which its reads and writes are generated.
    "table",
    // Track `uploads` (ADR-0260): a file a form posts, and its limits.
    "upload",
    // ADR-0302: a predicate a command `requires`, and the words a refusal
    // by it is told in.
    "predicate",
    "effect",
    "prelude",
    "replicated",
    "paint",
    "handler_policy",
    "public",
    "session",
    "private",
    "let",
];

/// Elements HTML forbids a closing tag on.
///
/// `<img src="x">` has no `</img>` and takes no children, and writing either is
/// a parse error the browser silently repairs. The grammar has to know the same
/// list the serializer does, or a page written correctly does not parse.
///
/// E7 task 2 gate 4 is what surfaced this: `examples/render/tricky.pw` is
/// ordinary HTML and `pw check` reported "unclosed block" on it.
pub const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

pub const UI_NOUNS: &[&str] = &["view", "component", "page", "layout"];

/// Statement keywords that may appear inside a body and take a
/// `kw [name] [(args)] [-> Type] [{ block }]` shape.
///
/// These are the §7.5A and resource-lifecycle forms. They are recognised
/// syntactically here; their *meaning* is staged (E4 for resources, E7 for the
/// frame phases), which is exactly the split the architect asked for.
const STMT_KEYWORDS: &[&str] = &[
    // `let cart = query Cart(session)` is ONE expression. Without this the
    // parser stopped after `query`, leaving the call as a sibling statement —
    // so a page that DECLARED a dependency on a query looked like a page that
    // CALLED into the database, and every such page was reported for reaching
    // the database while rendering. The corpus never showed it because the one
    // fixture with that shape was rejected for another reason anyway.
    "query",
    "command",
    "subscription",
    // `Ok(_) => navigate OrderPage()`: a handler goes to a page once its
    // command commits (ADR-0280). One expression, as `query` is.
    "navigate",
    // `schedule expire_payment(order) after minutes(30)`: a command the
    // platform runs when the clock passes its time, and `unschedule` the
    // same command and arguments (the scheduled-commands track).
    "schedule",
    "unschedule",
    "use",
    "observe",
    "animate",
    "frame",
    "measure",
    "mutate",
    "post_paint",
    "subtree",
    "resource",
    "subscribe",
    "unsafe",
];
pub const RESOURCE_NOUNS: &[&str] = &[
    "query",
    "command",
    "subscription",
    "resource",
    "materialize",
    // E6. A typed event is a declaration because `invalidates_on
    // InventoryChanged(id, _)` has to be checkable against
    // something — otherwise a materialization can name an event that does not
    // exist and nothing notices until the materializer never fires.
    "event",
    // ADR-0207: what a database the program's effects name guarantees:
    // `source StoreData  holds Carts, Orders  transactions serializable`.
    "source",
    // Track `tables` (ADR-0338, D1): `table Todos  row Todo  key id`.
    "table",
    // Track `uploads` (ADR-0260): `upload PostImage  route "/uploads/post-image"
    // serves "/images"  max_bytes 5_000_000  types png, jpeg`.
    "upload",
    // ADR-0302: `predicate OwnsPost(post: PostId)  says "…"`.
    "predicate",
    "replicated",
    "paint",
];

/// Policy clause keywords. A value runs until the next one, a `{`, or a
/// declaration keyword.
/// Words that continue a statement onto the next line rather than beginning a
/// new one.
///
/// The newline rule that ends a statement is right for almost everything and
/// wrong for these: charter §14 M5 task 6 mandates a `because "…"` on an
/// escape hatch, and it is written on its own line. Without this the statement
/// ended early, `because` became a statement of its own, and the justification
/// never reached the declaration that needed it — so an audited escape hatch
/// was reported as unjustified.
/// Declaration node kinds — the ones a visibility keyword may precede.
fn is_decl_kind(k: K) -> bool {
    matches!(
        k,
        K::FnDecl
            | K::UiDecl
            | K::ResourceDecl
            | K::TypeDecl
            | K::RecordDecl
            | K::UnionDecl
            | K::OpaqueDecl
            | K::LetDecl
            | K::ImportDecl
            | K::ModuleDecl
    )
}

/// Words that begin an EXPRESSION. Never a policy head, and never a policy
/// value's continuation.
///
/// The fourth table that must not overlap in interpretation. Needed once
/// declarations began reading their policies inside their braces
/// (2026-08-11): a `materialize` whose first statement is `if ..` reported
/// *unknown policy `if`*, because nothing said that `if` starts an expression.
const EXPR_KEYWORDS: &[&str] = &[
    "if", "elif", "else", "match", "let", "for", "fn", "return", "true", "false",
    // A page's `signal open: Bool = false` (ADR-0130) is a statement, not a
    // policy clause named `signal`; nor is `provide drawer = false`
    // (ADR-0144).
    "signal", "provide",
];

/// **Is `word` reserved?** (ADR-0195, ruling 3): a word that can begin a
/// statement or an expression in a body, so a name spelled so would read as
/// one there. Every other keyword is contextual, as `session` is, and names
/// whatever the program likes.
pub fn reserved(word: &str) -> bool {
    STMT_KEYWORDS.contains(&word) || EXPR_KEYWORDS.contains(&word) || word == "derived"
}

/// **What a name in a pattern is** (ADR-0195, ruling 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternKind {
    /// A case, resolved in the type of what is matched.
    Case,
    /// A new binding.
    Binding,
}

/// **Is a pattern's name a case or a binding?** (ADR-0195, ruling 2). Its
/// first letter decides, as in Haskell, OCaml and Elm: an uppercase name is a
/// case, and any other, `_x` among them, binds. So a misspelt case, `Circel`,
/// is a case its type lacks, refused, and never a binding that matches
/// everything. A case is declared with an uppercase name for the same reason
/// (PW0625).
pub fn pattern_kind(name: &str) -> PatternKind {
    match name.as_bytes().first() {
        Some(b'A'..=b'Z') => PatternKind::Case,
        _ => PatternKind::Binding,
    }
}

pub const STMT_CLAUSE_KEYWORDS: &[&str] =
    &["because", "attributes_forced_layout_to", "when", "respects"];

/// **Clause heads that are also words a body may begin with**: each heads a
/// clause only where a name follows it on its line. `layout StoreLayout` is a
/// page's clause and `layout.measure` an effect's path. `set` and `sets`
/// take names; a table's clauses take their named fields. A call `set.size()`
/// or `row.word`, or a binding `unique` alone on its line, is not a clause.
const NAMED_HEADS: &[&str] = &[
    "layout", "set", "sets", "row", "order", "index", "unique", "refers", "window", "asset",
    "migrate",
];

/// **A table's ownership clauses** (track `row-ownership`, ADR-0384 H1):
/// `owned by owner` and `shares with to` head a clause only where a name
/// follows on their line, as [`NAMED_HEADS`] do; `owned` and `shares` are
/// names a body may begin with.
const OWNERSHIP_HEADS: &[&str] = &["owned", "shares"];

/// A table's bounded clause takes a literal count on its line, and so does
/// the version of a signal the device keeps (track `device-store`):
/// `version 2`.
const COUNTED_HEADS: &[&str] = &["bounded", "version"];

pub const POLICY_KEYWORDS: &[&str] = &[
    // E8's effect ontology: `capability database.read<T>` and
    // `host "pw:host/database#read"` inside an `effect` block. Policy clauses
    // rather than expressions, because `database.read<T>` is a NAME and the
    // expression grammar reads `<` as a comparison — the same reason a
    // `materialize` block's policies live inside its braces (E6).
    "capability",
    "host",
    // ADR-0040: a standard-library operation the compiler supplies, as
    // `host` names one the host supplies. Declaration metadata, never
    // inferred from a name.
    "intrinsic",
    // What an effect does to the frame: `impact layout_write when LayoutAffect`.
    // A policy clause for the same reason `capability` is — the value is a
    // declarative name, not an expression.
    "impact",
    "freshness",
    "consistency",
    "cache",
    "invalidates_on",
    "invalidates",
    // E6 task 4: a command emits typed events in the same transaction as its
    // state change. Distinct from `invalidates`, which names a RESOURCE — an
    // event names a fact about the world, and which resources it affects is
    // the graph's answer rather than the command's.
    "emits",
    // ADR-0207: what a data source holds and guarantees: the isolation its
    // transactions have, what its reads may promise, and whether it tells
    // what changed.
    "holds",
    "transactions",
    "reads",
    "changes",
    // 4c, a read is bounded by construction: what one set read costs a
    // source, `sets one_call`, and a keyed read's set form, `set
    // hours_of_stores` on `fn hours_of`. A clause only where a name follows
    // on its line (`at_policy_head`): `set` and `sets` are names a body may
    // begin with.
    "sets",
    "set",
    // Track `tables` (ADR-0338, D1): a table's row, its order, its indexes,
    // its references, its bound, and its change from the schema released.
    // Its `key` is the clause a query's cache key is written with. Each is a
    // clause only where its value follows on its line (`NAMED_HEADS`,
    // `COUNTED_HEADS`).
    "row",
    "order",
    "index",
    "unique",
    "refers",
    "window",
    "asset",
    "bounded",
    "migrate",
    // Track `row-ownership` (ADR-0384, H1): whose a table's rows are, `owned
    // by owner`, who else reads them, `shares with to`, and the fields their
    // owner alone reads, `private sent, day`. `owned` and `shares` are
    // clauses only where a name follows on their line (`NAMED_HEADS`);
    // `private` is a declaration's visibility too, and heads a clause only
    // where a field's name follows it (`private_heads_a_clause`).
    "owned",
    "shares",
    "private",
    // Track `device-store` (ADR-0366): the version of a value the device
    // keeps, a clause only where its count follows on its line
    // (`COUNTED_HEADS`); its `migrate` is a table's word, for the same
    // thing: its change from the version released.
    "version",
    // Track `uploads` (ADR-0260): where an upload's committed files are
    // served, and its limits, each a literal.
    "serves",
    "max_bytes",
    "types",
    "max_width",
    "max_height",
    // ADR-0302: the words a refusal by a predicate is told in.
    "says",
    "fallback",
    "retry",
    "concurrency",
    "timeout",
    "requires",
    // NEXT 6c: a command the platform invokes, never a reader: `invoked_by
    // Payments`, a payment's outcome the host has from its provider.
    "invoked_by",
    "idempotent_by",
    "transaction",
    "optimistic",
    "rollback",
    "placement",
    // Charter §8.2. A page's route was a bare `Name` pair in its executable
    // body until 2026-08-11, and `routes::table` read it from there. Once UI
    // declarations parsed their policies inside their braces it became an
    // UNKNOWN policy — which is the parser saying, correctly, that a clause it
    // is being asked to classify is not in its vocabulary.
    "route",
    // ADR-0163: the declared error that means a page's address names
    // nothing, answered 404 rather than 503.
    "not_found_on",
    // ADR-0295: the declared error that means a page's address is another
    // address of the page, answered 308 or 307 there.
    "redirect_on",
    // Track `todomvc` (ADR-0367): the signal a page binds to its address's
    // fragment, and the forms the fragment takes: `fragment filter "/" =>
    // "all", "/active" => "active"`.
    "fragment",
    // The layout a page is shown in: `layout StoreLayout`. A clause only
    // where a name follows on its line (`at_policy_head`): `layout.measure`
    // is an effect's path, and a body may begin with one.
    "layout",
    "privacy",
    "storage",
    "offline",
    "sync",
    "conflict",
    "scope",
    "transport",
    "reconnect",
    "dedupe_by",
    "on_scope_exit",
    "delivery",
    "partition",
    "depends_on",
    "regenerate",
    "stampede",
    "code_version",
    "locale",
    // E6 cache-key dimensions (charter §14 M6 task 1). Without these in the
    // table a policy value ran on past the newline and swallowed the next
    // clause, so `locale included_in_key` and `tenant included_in_key` became
    // one policy named `locale` — and the tenant dimension was simply absent
    // from the key audit that exists to notice absences.
    "tenant",
    "policy_version",
    "affine",
    "acquire",
    "release",
    "identity",
    "captures",
    "load",
    "revision",
    "inputs",
    "isolated",
    "draw",
    "key",
    "on_key_change",
    "on_conflict_unresolved",
    "on_version_mismatch",
    "intrinsic_height",
    "attributes_forced_layout_to",
    "because",
];

/// **Does `word`, a statement alone, head a clause whose value follows it on
/// its line?** (ADR-0243). A block keeps a clause as statements, its head's
/// word and then its value, on one line: `scope component`. The names check
/// reads them as one clause (ADR-0047), and the policy table says what each
/// head's value is. A head whose value is code takes a block, as any
/// statement may.
pub fn heads_a_clause(word: &str) -> bool {
    POLICY_KEYWORDS.contains(&word) || STMT_CLAUSE_KEYWORDS.contains(&word)
}

/// **What the statements before, on their line, take after them**
/// (ADR-0243): what a block's readers read with them as one.
#[derive(Clone, Copy, PartialEq)]
enum Takes {
    /// Nothing: what follows on the line is a second statement.
    Nothing,
    /// The one statement after: a `return`'s value, which the checker reads
    /// as the statement after it (ADR-0038).
    One,
    /// The rest of the line, to a block: a clause's value.
    Rest,
}

/// What a template region holds open, waiting to be finished.
#[derive(PartialEq)]
enum Open {
    /// An element, until its close tag.
    Element,
    /// A `{#..}` block, until its `{/..}`, or its element's close tag.
    Block,
}

struct P<'a> {
    src: &'a str,
    toks: Vec<Token>,
    /// Index into `toks`, including trivia.
    pos: usize,
    b: TreeBuilder<'a>,
    errors: Vec<SyntaxError>,
    /// Guards against a grammar rule that consumes nothing in a loop.
    fuel: u32,
    /// A checkpoint taken before a visibility keyword, so the declaration that
    /// follows can re-parent it (see [`P::start`]).
    pending_vis: Option<rowan::Checkpoint>,
    /// What the end of the text is, when it is not the file's (ADR-0237): a
    /// standalone parse reads a hole or a clause's value, which ends in the
    /// middle of its file.
    end: String,
}

impl<'a> P<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            toks: lex(src),
            pos: 0,
            b: TreeBuilder::new(src),
            errors: Vec::new(),
            fuel: 0,
            pending_vis: None,
            end: "end of file".to_string(),
        }
    }

    /// A standalone parse of text whose end is `end` (ADR-0237).
    fn standalone(src: &'a str, end: String) -> Self {
        Self {
            end,
            ..Self::new(src)
        }
    }

    /// Where the last significant token read ends.
    fn read_to(&self) -> usize {
        self.toks[..self.pos]
            .iter()
            .rev()
            .find(|t| !t.kind.is_trivia())
            .map_or(0, |t| t.span.end)
    }

    /// The current token, as an error names what it found.
    fn found(&self) -> String {
        match self.cur() {
            Kind::Eof => self.end.clone(),
            k => k.describe().to_string(),
        }
    }

    // --- token access, skipping trivia ------------------------------------

    fn nth(&self, n: usize) -> &Token {
        let mut i = self.pos;
        let mut seen = 0;
        while i < self.toks.len() {
            if !self.toks[i].kind.is_trivia() {
                if seen == n {
                    return &self.toks[i];
                }
                seen += 1;
            }
            i += 1;
        }
        self.toks.last().expect("lex always emits Eof")
    }
    fn cur(&self) -> Kind {
        self.nth(0).kind
    }
    fn cur_text(&self) -> &'a str {
        let t = self.nth(0);
        &self.src[t.span.clone()]
    }
    fn cur_span(&self) -> Span {
        self.nth(0).span.clone()
    }
    fn at_eof(&self) -> bool {
        self.cur() == Kind::Eof
    }
    fn at(&self, k: Kind) -> bool {
        self.cur() == k
    }
    fn at_kw(&self, kw: &str) -> bool {
        self.cur() == Kind::Ident && self.cur_text() == kw
    }

    /// **At a declaration's visibility** (track `store-accounts`, ADR-0318):
    /// `public`, `session` or `private`, reserved; or `user`, a visibility
    /// only where a declaration's keyword follows it, so a field, a parameter
    /// or a binding named `user` is still a name.
    fn at_visibility(&self) -> bool {
        if self.at_kw("public") || self.at_kw("session") || self.at_kw("private") {
            return true;
        }
        if !self.at_kw("user") {
            return false;
        }
        let next = self.nth(1);
        let text = &self.src[next.span.clone()];
        next.kind == Kind::Ident
            && (UI_NOUNS.contains(&text)
                || RESOURCE_NOUNS.contains(&text)
                || matches!(
                    text,
                    "type" | "opaque" | "fn" | "let" | "prelude" | "effect"
                ))
    }
    fn nth_is(&self, n: usize, k: Kind) -> bool {
        self.nth(n).kind == k
    }

    /// Is there a line break between the last consumed token and the next
    /// significant one?
    ///
    /// Statements are newline-terminated, so any loop that consumes bare
    /// identifiers must stop here. Without this guard a trailing-modifier loop
    /// swallows the first token of the *next* statement and the error surfaces
    /// one line later, pointing at innocent code.
    /// Is there unconsumed trivia before the next significant token?
    fn trivia_pending(&self) -> bool {
        self.toks.get(self.pos).is_some_and(|t| t.kind.is_trivia())
    }

    /// Is there a newline between the current significant token and the `n`th
    /// one after it?
    ///
    /// A policy clause's value is on the head's own line: `partiton public`.
    /// A bare identifier alone on a line is not a clause — and `nth` skips
    /// trivia without advancing, so this has to look at the raw stream.
    fn nth_starts_line(&self, n: usize) -> bool {
        let mut seen = 0usize;
        for t in &self.toks[self.pos..] {
            if t.kind.is_trivia() {
                if seen > 0 && self.src[t.span.clone()].contains('\n') {
                    return true;
                }
                continue;
            }
            seen += 1;
            if seen > n {
                return false;
            }
        }
        true
    }

    fn newline_ahead(&self) -> bool {
        self.toks[self.pos..]
            .iter()
            .take_while(|t| t.kind.is_trivia())
            .any(|t| self.src[t.span.clone()].contains('\n'))
    }

    /// **Does an expression begin here?** The tokens `expr_lhs` reads one
    /// from; at any other it refuses (PW0009).
    fn at_expression(&self) -> bool {
        matches!(
            self.cur(),
            Kind::Int
                | Kind::Float
                | Kind::Str
                | Kind::UnterminatedStr
                | Kind::Minus
                | Kind::Bang
                | Kind::LParen
                | Kind::LBracket
                | Kind::LBrace
                | Kind::LAngle
                | Kind::Underscore
                | Kind::Ident
        )
    }

    /// The word the tokens from `from` to here are, when they are one name
    /// alone (ADR-0243).
    fn one_word(&self, from: usize) -> Option<&'a str> {
        let mut read = self.toks[from..self.pos]
            .iter()
            .filter(|t| !t.kind.is_trivia());
        match (read.next(), read.next()) {
            (Some(t), None) if t.kind == Kind::Ident => Some(&self.src[t.span.clone()]),
            _ => None,
        }
    }

    /// Emit pending trivia into the current node, so the tree stays lossless.
    fn eat_trivia(&mut self) {
        while self.pos < self.toks.len() && self.toks[self.pos].kind.is_trivia() {
            let t = self.toks[self.pos].clone();
            self.b.token(&t);
            self.pos += 1;
        }
    }

    /// Consume the current significant token into the tree.
    fn bump(&mut self) {
        self.eat_trivia();
        if self.pos < self.toks.len() && self.toks[self.pos].kind != Kind::Eof {
            let t = self.toks[self.pos].clone();
            self.b.token(&t);
            self.pos += 1;
        }
    }

    fn eat(&mut self, k: Kind) -> bool {
        if self.at(k) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn start(&mut self, k: K) {
        // A declaration preceded by a visibility keyword re-parents that
        // keyword into itself, so `private type X` is ONE declaration whose
        // first token says `private`. Without it the keyword became a
        // declaration of its own and the type looked public.
        if let Some(cp) = self.pending_vis.take() {
            if is_decl_kind(k) {
                self.b.start_at(cp, k);
                return;
            }
            self.pending_vis = Some(cp);
        }
        // Trivia belongs *outside* a node it merely precedes, so it is flushed
        // before the node opens. Without this, a comment before `fn` would end
        // up inside the FnDecl and the formatter would move it.
        self.eat_trivia();
        self.b.start(k);
    }

    /// Open a node **without** flushing trivia first.
    ///
    /// The one exception to the rule above, and it is markup: whitespace
    /// between two inline elements is content, not formatting. Flushing it
    /// first put it outside the `Text` node that was created to hold it, so the
    /// node came out empty and the space never reached HIR — which renders
    /// `<span>a</span> <span>b</span>` with the words run together.
    fn start_keeping_trivia(&mut self, k: K) {
        self.b.start(k);
    }
    fn finish(&mut self) {
        self.b.finish_node();
    }

    fn error(&mut self, code: &'static str, msg: impl Into<String>) {
        let span = self.cur_span();
        self.errors.push(SyntaxError {
            code,
            message: msg.into(),
            span,
            help: None,
        });
    }
    fn error_help(&mut self, code: &'static str, msg: impl Into<String>, help: impl Into<String>) {
        let span = self.cur_span();
        self.errors.push(SyntaxError {
            code,
            message: msg.into(),
            span,
            help: Some(help.into()),
        });
    }

    /// **One arm of an optimistic clause** (ADR-0238): the entry, a name for
    /// its current value, and its transition.
    fn transition_arm(&mut self) {
        self.start(K::TransitionArm);
        // The resource entry. Above `as`'s power so the cast rule is not
        // reached; see `P::expr`.
        self.expr(10);
        if self.at_kw("as") {
            self.bump();
            self.name("a name for the resource's current value");
        } else {
            self.error(
                "PW0017",
                "expected `as <name>` — an optimistic clause binds the \
                 resource's current value",
            );
        }
        // The transition is read without its arrow too, so the arrow is the
        // one error (ADR-0237). After the arrow, a transition is expected,
        // and its absence is the error; the comma after it stays the next
        // arm's.
        let arrow = self.eat(Kind::FatArrow);
        if !arrow {
            self.error("PW0017", "expected `=>` and a transition expression");
        }
        if !self.at_eof() && !self.at(Kind::Comma) {
            self.expr(0);
        } else if arrow {
            let found = self.found();
            self.error("PW0009", format!("expected an expression, found {found}"));
        }
        self.finish();
    }

    /// **What a standalone parse leaves unread** (ADR-0237): one error over
    /// the whole rest, kept in the tree as an `ErrorExpr` so the parse stays
    /// lossless. It was an error per token, which no one saw: lowering
    /// dropped every error a standalone parse made.
    fn rest_unread(&mut self, message: &str, help: Option<&str>) {
        if self.at_eof() {
            return;
        }
        let start = self.cur_span().start;
        let mut end = start;
        self.start(K::ErrorExpr);
        while !self.at_eof() {
            end = self.cur_span().end;
            self.bump();
        }
        self.finish();
        self.errors.push(SyntaxError {
            code: "PW0016",
            message: message.to_string(),
            span: start..end,
            help: help.map(str::to_string),
        });
    }

    /// **Items separated by commas, each read by `item`** (ADR-0237), as
    /// rustc reads a sequence (`parse_seq_to_before_tokens`):
    /// - an item with no comma before it is read, and the missing comma is
    ///   the error, with its repair. What the list holds is what was meant;
    /// - one that does not parse there is no item: its own errors are
    ///   dropped, and the missing comma is the error, over all that is left.
    fn comma_separated(&mut self, item: impl Fn(&mut Self), message: &str, repair: &str) {
        let mut first = true;
        while !self.at_eof() {
            let missing = !first && !self.eat(Kind::Comma);
            if self.at_eof() {
                break;
            }
            let (before, start, errors) = (self.pos, self.cur_span().start, self.errors.len());
            item(self);
            if self.pos == before {
                break;
            }
            if missing {
                let read = self.errors.len() == errors;
                self.errors.truncate(errors);
                if !read {
                    self.start(K::ErrorExpr);
                    while !self.at_eof() {
                        self.bump();
                    }
                    self.finish();
                }
                let end = self.read_to();
                self.errors.push(SyntaxError {
                    code: "PW0016",
                    message: message.to_string(),
                    span: start..end,
                    help: read.then(|| repair.to_string()),
                });
                if !read {
                    break;
                }
            }
            first = false;
        }
        self.rest_unread(message, None);
    }

    /// Consume `k` or report a diagnostic and keep going. Recovery is
    /// deliberate: a missing closer should not truncate the rest of the file.
    fn expect(&mut self, k: Kind, ctx: &str) -> bool {
        if self.eat(k) {
            return true;
        }
        let found = self.found();
        let want = k.describe();
        self.error("PW0001", format!("expected {want} {ctx}, found {found}"));
        false
    }

    fn name(&mut self, what: &str) -> bool {
        if self.at(Kind::Ident) {
            self.start(K::Name);
            self.bump();
            self.finish();
            true
        } else {
            let found = self.found();
            self.error("PW0001", format!("expected {what}, found {found}"));
            false
        }
    }

    /// A dotted path consumed as one `Name` node: `store.pricing`, `database.read`.
    fn dotted_name(&mut self, what: &str) -> bool {
        if !self.at(Kind::Ident) {
            let found = self.found();
            self.error("PW0001", format!("expected {what}, found {found}"));
            return false;
        }
        self.start(K::Name);
        self.bump();
        while self.at(Kind::Dot) && self.nth_is(1, Kind::Ident) {
            self.bump();
            self.bump();
        }
        self.finish();
        true
    }

    // --- types --------------------------------------------------------------

    fn type_ref(&mut self) -> bool {
        // The unit type `()`.
        if self.at(Kind::LParen) && self.nth_is(1, Kind::RParen) {
            self.start(K::TypeRef);
            self.bump();
            self.bump();
            self.finish();
            return true;
        }
        // **A function type**: `fn(A, B) -> R`. E9, 2026-09-24 (ADR-0031).
        // A callback had no type to be declared with, so `pw-std` wrote
        // `List.map(items: List<Unknown>, f: Decoder)` — a record standing in
        // for a function — and nothing could relate a lambda to what it is
        // passed as. The result is not optional: `fn(A) -> ()` says it.
        if self.at_kw("fn") && self.nth_is(1, Kind::LParen) {
            self.start(K::TypeRef);
            self.start(K::Name);
            self.bump(); // `fn`
            self.finish();
            self.start(K::FnTypeArgs);
            self.bump(); // `(`
            loop {
                if self.at(Kind::RParen) || self.at_eof() {
                    break;
                }
                if !self.type_ref() {
                    break;
                }
                if !self.eat(Kind::Comma) {
                    break;
                }
            }
            if !self.eat(Kind::RParen) {
                self.error("PW0005", "unclosed parameter list, expected `)`");
            }
            if self.eat(Kind::Arrow) {
                self.type_ref();
            } else {
                self.error_help(
                    "PW0001",
                    "expected `->` and the function type's result",
                    "a function type always states its result: `fn(A) -> ()` returns nothing",
                );
            }
            self.finish(); // FnTypeArgs
            self.finish(); // TypeRef
            return true;
        }
        if !self.at(Kind::Ident) {
            let found = self.found();
            self.error("PW0001", format!("expected a type, found {found}"));
            return false;
        }
        self.start(K::TypeRef);
        self.dotted_name("a type name");
        if self.at(Kind::LAngle) {
            self.start(K::TypeArgList);
            self.bump();
            loop {
                if self.at(Kind::RAngle) || self.at_eof() {
                    break;
                }
                if !self.type_ref() {
                    break;
                }
                if !self.eat(Kind::Comma) {
                    break;
                }
            }
            if !self.eat(Kind::RAngle) {
                self.error("PW0002", "unclosed type argument list, expected `>`");
            }
            self.finish();
        }
        self.finish();
        true
    }

    /// `!{ a, b<C> }`
    fn effect_row(&mut self) {
        self.start(K::EffectRow);
        self.bump(); // `!`
        if self.eat(Kind::LBrace) {
            loop {
                if self.at(Kind::RBrace) || self.at_eof() {
                    break;
                }
                self.start(K::EffectRef);
                let ok = self.type_ref();
                self.finish();
                if !ok {
                    break;
                }
                if !self.eat(Kind::Comma) {
                    break;
                }
            }
            if !self.eat(Kind::RBrace) {
                self.error("PW0003", "unclosed effect row, expected `}`");
            }
        } else {
            self.error_help(
                "PW0004",
                "expected `{` after `!` to open an effect row",
                "write an empty row as `!{}` to claim purity explicitly",
            );
        }
        self.finish();
    }

    /// `<C>` on a declaration. Wrapped in a `TypeArgList` node rather than
    /// skipped as a balanced range, so the formatter knows the angle brackets
    /// delimit types and does not space them like comparisons —
    /// `opaque type Secret<C>`, not `opaque type Secret < C >`.
    fn type_params(&mut self) {
        if !self.at(Kind::LAngle) {
            return;
        }
        self.start(K::TypeArgList);
        self.bump();
        loop {
            if self.at(Kind::RAngle) || self.at_eof() {
                break;
            }
            if self.at(Kind::Ident) {
                self.start(K::TypeRef);
                self.name("a type parameter");
                self.finish();
            } else {
                self.bump();
            }
            if !self.eat(Kind::Comma) {
                break;
            }
        }
        self.eat(Kind::RAngle);
        self.finish();
    }

    fn param_list(&mut self) {
        if !self.at(Kind::LParen) {
            return;
        }
        self.start(K::ParamList);
        self.bump();
        loop {
            if self.at(Kind::RParen) || self.at_eof() {
                break;
            }
            self.start(K::Param);
            self.not_a_statement_keyword("a parameter");
            if !self.name("a parameter name") {
                self.finish();
                self.skip_balanced(Kind::LParen, Kind::RParen);
                break;
            }
            if self.eat(Kind::Colon) {
                self.type_ref();
            }
            self.finish();
            if !self.eat(Kind::Comma) {
                break;
            }
        }
        if !self.eat(Kind::RParen) {
            self.error("PW0005", "unclosed parameter list, expected `)`");
        }
        self.finish();
    }

    fn skip_balanced(&mut self, open: Kind, close: Kind) {
        let mut depth = 1i32;
        while !self.at_eof() {
            if self.at(open) {
                depth += 1;
            } else if self.at(close) {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            self.bump();
        }
    }

    // --- expressions --------------------------------------------------------

    /// Binding powers. Higher binds tighter. Real precedence, because an
    /// approximation here would misassociate operands while still producing a
    /// tree that looks parsed.
    fn infix_power(k: Kind, text: &str) -> Option<(u8, u8)> {
        let _ = text;
        let bp = match k {
            Kind::PipeGt => 1, // `a |> f |> g`, lowest precedence, left-assoc
            // `100.px -> 120.px -> 100.px`: an animation keyframe sequence.
            // Only reachable in expression position; a return type or a
            // statement arrow is consumed by the type and statement grammars
            // before `expr` ever sees it.
            Kind::Arrow => 2,
            Kind::Eq => 2,
            Kind::Amp => 3,
            Kind::Pipe => 3,
            // `==` `!=` `<=` `>=` sit with `<` and `>`: same precedence class,
            // and `<`/`>` are only comparisons here because a type argument
            // list is parsed by the type grammar, never by `expr`.
            Kind::Cmp | Kind::LAngle | Kind::RAngle => 5,
            Kind::Plus | Kind::Minus => 7,
            Kind::Star | Kind::Slash | Kind::Percent => 8,
            _ => return None,
        };
        Some((bp, bp + 1))
    }

    fn expr(&mut self, min_bp: u8) {
        // The checkpoint MUST be taken before the left operand is emitted.
        // Taking it afterwards makes `start_at` wrap nothing, so `a + b * c`
        // still produces BinaryExpr nodes — just empty ones in the wrong place.
        // The precedence test caught that; a test that only asserted
        // "a BinaryExpr exists" would not have.
        // Flush leading trivia BEFORE the checkpoint, so whitespace preceding an
        // expression stays outside its node. Otherwise every expression node
        // begins with the spaces in front of it, and a formatter reading node
        // boundaries would reflow them.
        self.eat_trivia();
        // **`return` is not an operand.** It is a statement of its own, and a
        // block reads the statement after it as the value it returns
        // (`values::result_sites`). Parsed as an ordinary name, `return (x)`
        // was a call of `return`, refused as unresolved, and `return -1` was
        // a subtraction whose left operand was `return`, accepted silently.
        if self.at_kw("return") {
            self.start(K::NameExpr);
            self.bump();
            self.finish();
            return;
        }
        let cp = self.b.checkpoint();
        self.expr_lhs(cp);
        // The operator a chain's `|` or `&` is, the first one written, and
        // whether its mixing is already refused: see below.
        let mut logical: Option<(Kind, bool)> = None;
        loop {
            if self.at_eof() {
                break;
            }
            // `=>` binds looser than every operator and is right-associative,
            // so the parameter is whatever was just parsed and the body is the
            // rest. Handling it here rather than as an `ident =>` lookahead is
            // what makes `(a, b) => e` and `resumable(captures = { .. }) => e`
            // parse with the same shape as `x => e`.
            if self.at(Kind::FatArrow) && min_bp == 0 {
                self.b.start_at(cp, K::LambdaExpr);
                self.bump();
                self.expr(0);
                self.finish();
                continue;
            }
            let k = self.cur();
            // `<`, `-` and `!` are both infix and prefix. At the start of a
            // line they begin a new statement — `<main>` opens markup, it does
            // not compare the previous line to `main`. Without this rule
            // `let s = f(id)` followed by `<main>` silently parses as one
            // comparison and the error surfaces three lines later.
            if matches!(k, Kind::LAngle | Kind::Minus | Kind::Bang) && self.newline_ahead() {
                break;
            }
            let text = self.cur_text();
            // `as` is a keyword operator whose right operand is a TYPE. It
            // binds tighter than every arithmetic operator, so `a as T + b`
            // is `(a as T) + b`.
            if k == Kind::Ident && text == "as" && !self.newline_ahead() {
                if 9 < min_bp {
                    break;
                }
                self.b.start_at(cp, K::CastExpr);
                self.bump();
                if !self.type_ref() {
                    self.error("PW0009", "expected a type after `as`");
                }
                self.finish();
                continue;
            }
            let Some((lbp, rbp)) = Self::infix_power(k, text) else {
                break;
            };
            if lbp < min_bp {
                break;
            }
            // **`|` and `&` have no precedence between them** (ADR-0322): a
            // chain that mixes them is refused, once, where the mix begins,
            // as Carbon refuses `and` beside `or`. Parsed left to right, as
            // one power, `a | b & c` was `(a | b) & c`, where most languages
            // read `a | (b & c)`: W9's "open until" read one way and ran the
            // other (track store-hours, 2026-10-12).
            if matches!(k, Kind::Amp | Kind::Pipe) {
                match logical {
                    Some((first, false)) if first != k => {
                        self.error_help(
                            "PW0031",
                            "`|` and `&` are mixed without parentheses, and neither binds tighter",
                            "write `(a | b) & c` or `a | (b & c)`, whichever is meant",
                        );
                        logical = Some((first, true));
                    }
                    Some(_) => {}
                    None => logical = Some((k, false)),
                }
            }
            self.b.start_at(cp, K::BinaryExpr);
            self.bump(); // operator
            self.expr(rbp);
            self.finish();
        }
    }

    fn expr_lhs(&mut self, cp: rowan::Checkpoint) {
        match self.cur() {
            Kind::Int | Kind::Float | Kind::Str | Kind::UnterminatedStr => {
                // A string's escapes and holes are the language's (ADR-0049):
                // one it does not define is refused here, at its token,
                // rather than given whatever meaning a backend's target has.
                if self.at(Kind::Str)
                    && let Err(e) = crate::strings::pieces(self.cur_text())
                {
                    let span = self.cur_span();
                    let at = span.start + e.at;
                    self.errors.push(SyntaxError {
                        code: "PW0014",
                        message: e.message,
                        span: at..(at + 1).min(span.end),
                        help: None,
                    });
                }
                self.start(K::LiteralExpr);
                self.bump();
                self.finish();
            }
            Kind::Minus | Kind::Bang => {
                self.start(K::UnaryExpr);
                self.bump();
                self.eat_trivia();
                let inner = self.b.checkpoint();
                self.expr_lhs(inner);
                self.finish();
            }
            // `(e: InputEvent) => ..`: a lambda's parameters with their types
            // (ADR-0138), the list `fn(e: InputEvent) ..` writes. Nothing
            // else begins `( name :`.
            Kind::LParen if self.nth_is(1, Kind::Ident) && self.nth_is(2, Kind::Colon) => {
                self.param_list();
                if !self.at(Kind::FatArrow) {
                    self.error(
                        "PW0009",
                        "a typed parameter list is a lambda's, `(e: InputEvent) => ..`",
                    );
                }
            }
            Kind::LParen => {
                // `(a, b) => a + b` writes a lambda's parameters as a
                // parenthesised list, which is a `TupleExpr` until `=>` makes
                // it parameters. Until 2026-09-25 only one expression was
                // read here, so the comma was an unclosed parenthesis, and the
                // form the lambda grammar below describes never parsed.
                let cp = self.b.checkpoint();
                self.bump();
                let mut tuple = false;
                if !self.at(Kind::RParen) {
                    self.expr(0);
                    while self.eat(Kind::Comma) {
                        tuple = true;
                        if self.at(Kind::RParen) {
                            break;
                        }
                        self.expr(0);
                    }
                }
                if !self.eat(Kind::RParen) {
                    self.error("PW0008", "unclosed parenthesis, expected `)`");
                }
                // The language has no tuple type: a list in parentheses is a
                // lambda's parameters or nothing.
                if tuple && !self.at(Kind::FatArrow) {
                    self.error(
                        "PW0009",
                        "a parenthesised list is a lambda's parameters, `(a, b) => ..`; \
                         the language has no tuple value",
                    );
                }
                self.b
                    .start_at(cp, if tuple { K::TupleExpr } else { K::ParenExpr });
                self.finish();
            }
            Kind::LBracket => {
                self.start(K::ListExpr);
                self.bump();
                loop {
                    if self.at(Kind::RBracket) || self.at_eof() {
                        break;
                    }
                    self.expr(0);
                    if !self.eat(Kind::Comma) {
                        break;
                    }
                }
                self.expect(Kind::RBracket, "to close a list literal");
                self.finish();
            }
            // `{#each ..}` / `{:else}` in expression position opens a template
            // region; a plain `{` is a block.
            Kind::LBrace
                if self.nth_is(1, Kind::Hash)
                    || self.nth_is(1, Kind::Slash)
                    || self.nth_is(1, Kind::Colon) =>
            {
                self.template_region()
            }
            Kind::LBrace => self.block_expr(),
            // HTML-like markup in a view or component body.
            Kind::LAngle => self.template_region(),
            Kind::Underscore => {
                self.start(K::NameExpr);
                self.bump();
                self.finish();
            }
            Kind::Ident => {
                if self.at_kw("if") {
                    return self.if_expr();
                }
                if self.at_kw("match") {
                    return self.match_expr();
                }
                if self.at_kw("for") {
                    return self.for_expr();
                }
                if self.at_kw("let") {
                    return self.let_stmt();
                }
                // **`signal x: T = e`: UI state the page holds** (ADR-0130).
                // A keyword only before a name on its line, so a value named
                // `signal` elsewhere is still a name.
                if self.at_kw("signal") && self.nth_is(1, Kind::Ident) && !self.nth_starts_line(1) {
                    return self.signal_stmt();
                }
                // **`provide drawer = false`: a module's signal, given its
                // value for what this body contains** (ADR-0144). A keyword
                // only before a name on its line, as `signal` is.
                if self.at_kw("provide") && self.nth_is(1, Kind::Ident) && !self.nth_starts_line(1)
                {
                    return self.provide_stmt();
                }
                // **`derived e`: a pure value computed from other values**
                // (charter §7.5). A bare name until 2026-09-25, so `let total =
                // derived widths |> List.sum()` parsed as `let total = derived`
                // and then a statement whose value was discarded: `total` named
                // nothing, and the sum was computed and thrown away. The name
                // check found it (ADR-0047).
                if self.at_kw("derived") {
                    return self.derived_expr();
                }
                if STMT_KEYWORDS.contains(&self.cur_text()) && !self.at_ordinary_call() {
                    return self.keyword_stmt();
                }
                if self.at_kw("fn") && self.nth_is(1, Kind::LParen) {
                    self.start(K::LambdaExpr);
                    self.bump();
                    self.param_list();
                    self.expr(0);
                    self.finish();
                    return;
                }
                // A bare name. `a.b` is NOT absorbed here: in expression
                // position the parser cannot tell a module path from a field
                // access, and it must not pretend to. `postfix` gives every
                // `.ident` the same shape and name resolution folds the
                // leading segments that turn out to be a module path.
                self.start(K::NameExpr);
                self.bump();
                self.finish();
            }
            _ => {
                self.start(K::ErrorExpr);
                let found = self.found();
                let current = self.nth(0);
                let previous = self.pos.checked_sub(1).and_then(|p| self.toks.get(p));
                let next = self.nth(1);
                let doubled_logical = matches!(current.kind, Kind::Amp | Kind::Pipe)
                    && (previous.is_some_and(|p| {
                        p.kind == current.kind && p.span.end == current.span.start
                    }) || (next.kind == current.kind && current.span.end == next.span.start));
                if doubled_logical {
                    self.error_help(
                        "PW0009",
                        format!("expected an expression, found {found}"),
                        "Pleris writes `&` for and and `|` for or; write one operator",
                    );
                } else {
                    self.error("PW0009", format!("expected an expression, found {found}"));
                }
                self.bump();
                self.finish();
                return;
            }
        }
        self.postfix(cp);
    }

    /// Field access and calls, left-associatively: `a.b(c).d`.
    ///
    /// `cp` marks the position *before* the receiver was emitted, so each
    /// postfix step wraps everything to its left. Same trap as `expr`.
    fn postfix(&mut self, cp: rowan::Checkpoint) {
        loop {
            if self.at(Kind::Dot) && self.nth_is(1, Kind::Ident) {
                self.b.start_at(cp, K::FieldExpr);
                self.bump();
                self.bump();
                self.finish();
                continue;
            }
            // `Store { id: .. }` and `store.Inner { .. }` — a record literal.
            // Guarded by `record_body_ahead` so `if x { .. }` is unaffected.
            if self.record_body_ahead() {
                self.b.start_at(cp, K::RecordExpr);
                self.record_fields();
                self.finish();
                continue;
            }
            // A call's arguments open on the callee's line. A `(` at the start
            // of a line begins a new statement, as `<`, `-` and `!` do: `g(n)`
            // then `()` on the next line are a call and the unit value. Until
            // 2026-09-26 they parsed as one call, `g(n)()`, which a checker
            // refused as a name that does not resolve (ADR-0083).
            if self.at(Kind::LParen) && !self.newline_ahead() {
                self.b.start_at(cp, K::CallExpr);
                self.start(K::ArgList);
                self.bump();
                loop {
                    if self.at(Kind::RParen) || self.at_eof() {
                        break;
                    }
                    // Named arguments: `max = 3`, `jitter: true`. The name is
                    // consumed here so the value parses as an ordinary
                    // expression rather than tripping on `=` or `:`.
                    if self.at(Kind::Ident)
                        && (self.nth_is(1, Kind::Eq) || self.nth_is(1, Kind::Colon))
                    {
                        self.bump();
                        self.bump();
                    }
                    self.expr(0);
                    if !self.eat(Kind::Comma) {
                        break;
                    }
                }
                if !self.eat(Kind::RParen) {
                    self.error("PW0010", "unclosed argument list, expected `)`");
                }
                self.finish(); // ArgList
                self.finish(); // CallExpr
                continue;
            }
            if self.at(Kind::Question) {
                // `?` propagation, a node of its own so the operand is a
                // child: the operator changes both the value and the control
                // flow, and a token no node owns tells no analysis either.
                self.b.start_at(cp, K::TryExpr);
                self.bump();
                self.finish();
                continue;
            }
            break;
        }
    }

    /// Does the brace ahead look like `{ field: ..` or `{}` rather than a block?
    fn record_body_ahead(&self) -> bool {
        if !self.at(Kind::LBrace) {
            return false;
        }
        self.nth_is(1, Kind::RBrace) || (self.nth_is(1, Kind::Ident) && self.nth_is(2, Kind::Colon))
    }

    fn record_fields(&mut self) {
        self.bump(); // `{`
        loop {
            if self.at(Kind::RBrace) || self.at_eof() {
                break;
            }
            self.start(K::Field);
            self.name("a field name");
            if self.eat(Kind::Colon) {
                self.expr(0);
            }
            self.finish();
            if !self.eat(Kind::Comma) {
                break;
            }
        }
        if !self.eat(Kind::RBrace) {
            self.error("PW0012", "unclosed record literal, expected `}`");
        }
    }

    /// **Does the markup go on, on this line, after a root?** A view's
    /// roots are statements, one a line (ADR-0243), and two written on one
    /// line, `<h1>a</h1><p>b</p>`, as HTML writes them, were a root and then
    /// a comparison with the second: PW0009 at its `/` or `>`. What follows a
    /// finished element on its line is markup if it begins as markup does,
    /// an element, a comment or a block, since markup is never compared. The
    /// space between them is kept as text, as it is between two elements
    /// inside one (`<span>a</span> <span>b</span>`).
    fn sibling_on_line(&mut self) -> bool {
        let next = (self.at(Kind::LAngle)
            && (self.nth_is(1, Kind::Ident)
                || self.src[self.cur_span().start..].starts_with("<!--")))
            || (self.at(Kind::LBrace) && self.nth_is(1, Kind::Hash));
        if !next || self.newline_ahead() {
            return false;
        }
        if self.trivia_pending() {
            self.start_keeping_trivia(K::Text);
            self.eat_trivia();
            self.finish();
        }
        true
    }

    /// A template region: `<main>...</main>`, `{#each ..}`, `{/each}`.
    ///
    /// Charter §14 M2 task 9 asks for "only enough template parsing to
    /// represent static HTML, expressions, conditions, and keyed loops". The
    /// markup itself is a region; its `{...}` interpolations are parsed as real
    /// expressions, which is what matters for effect analysis — a forbidden
    /// call inside `{store.name}` must be visible.
    ///
    /// Deliberately NOT a full HTML grammar: nesting validity, ARIA rules and
    /// keyed-loop checking are E3 and are not pretended here.
    /// A markup region, as a **tree**: elements, their attributes, their text.
    ///
    /// The first version consumed tags as a run of tokens. That round-tripped
    /// and parsed, and it was still wrong: E3 has to lower this to a renderer
    /// and E5 has to know which attribute carries which value, and neither can
    /// read a token soup. Losslessness was never the hard part — structure is.
    fn template_region(&mut self) {
        self.start(K::TemplateRegion);
        let mut depth = 0i32;
        let mut guard = 0;
        // Elements are opened and closed by separate tokens, so the tree is
        // built by starting a node on `<tag` and finishing it on `</tag`.
        // `open` holds what waits to be finished, innermost last: an
        // `Element` its close tag, a `{#..}` block its `{/..}`.
        let mut open: Vec<Open> = Vec::new();
        while !self.at_eof() {
            guard += 1;
            if guard > 50_000 {
                self.error("PW0099", "template made no progress");
                break;
            }
            // `//` in markup content is text, which the lexer read as a
            // comment (ADR-0167).
            if depth > 0 {
                self.markup_text();
            }
            // Whitespace between markup is CONTENT. It is what separates two
            // inline elements, and leaving it as trivia drops it from HIR — so
            // `<span>a</span> <span>b</span>` renders without the space.
            if depth > 0 && self.trivia_pending() {
                self.start_keeping_trivia(K::Text);
                self.eat_trivia();
                self.finish();
            }
            match self.cur() {
                // `<!-- … -->`, which the page does not show (ADR-0167).
                Kind::LAngle if self.src[self.cur_span().start..].starts_with("<!--") => {
                    self.markup_comment();
                }
                Kind::LAngle if self.nth_is(1, Kind::Slash) => {
                    // A close tag ends the blocks opened inside its element
                    // and left open, `<main>{#if a}<p>A</p></main>`. Each
                    // keeps no closing marker, and the checker says it is
                    // never closed (PW5019, ADR-0200). Until ADR-0200 the
                    // block took the element's close tag, and every `}` after
                    // it, and the error fell at the end of the file.
                    let blocks = open.iter().rev().take_while(|o| **o == Open::Block).count();
                    if blocks < open.len() {
                        for _ in 0..blocks {
                            open.pop();
                            depth -= 1;
                            self.finish(); // MarkupBlock
                        }
                    }
                    depth -= 1;
                    self.start(K::CloseTag);
                    while !self.at_eof() && !self.at(Kind::RAngle) {
                        self.bump();
                    }
                    self.eat(Kind::RAngle);
                    self.finish(); // CloseTag
                    if open.last() == Some(&Open::Element) {
                        open.pop();
                        self.finish(); // Element
                    }
                    if depth <= 0 {
                        if self.sibling_on_line() {
                            continue;
                        }
                        break;
                    }
                }
                Kind::LAngle => {
                    depth += 1;
                    self.start(K::Element);
                    let self_closing = self.open_tag();
                    if self_closing {
                        depth -= 1;
                        self.finish(); // Element
                    } else {
                        open.push(Open::Element);
                    }
                    if depth <= 0 {
                        if self.sibling_on_line() {
                            continue;
                        }
                        break;
                    }
                }
                // `{/each}` closes the block it belongs to.
                Kind::LBrace if self.nth_is(1, Kind::Slash) => {
                    depth -= 1;
                    self.interpolation();
                    if let Some(i) = open.iter().rposition(|o| *o == Open::Block) {
                        open.remove(i);
                        self.finish(); // MarkupBlock
                    }
                    if depth <= 0 {
                        if self.sibling_on_line() {
                            continue;
                        }
                        break;
                    }
                }
                // `{#each ..}` opens one. Nesting it — rather than leaving the
                // marker and its contents as siblings — is what lets a renderer
                // emit the children *inside* the loop.
                Kind::LBrace if self.nth_is(1, Kind::Hash) => {
                    depth += 1;
                    self.start_keeping_trivia(K::MarkupBlock);
                    self.interpolation();
                    open.push(Open::Block);
                }
                Kind::LBrace => {
                    self.interpolation();
                }
                // A `}` at depth 0 belongs to the enclosing block, not to us.
                Kind::RBrace if depth <= 0 => break,
                _ => self.text_run(),
            }
        }
        // An unclosed element or block must still produce a well-formed tree.
        // Recovery that leaves nodes open would corrupt every ancestor's text
        // range, and the losslessness suite would then fail far from the cause.
        for _ in 0..open.len() {
            self.finish();
        }
        self.finish(); // TemplateRegion
    }

    /// `<tag attr="v" on:press={h} />`. Returns whether it self-closed.
    ///
    /// A void element self-closes whether or not a slash was written, because
    /// HTML says so — the slash is ignored by every browser parser and a
    /// closing tag is discarded.
    fn open_tag(&mut self) -> bool {
        self.start(K::OpenTag);
        self.bump(); // `<`
        let mut void = false;
        // The tag name, which may be dotted for a component: `<store.Card />`.
        if self.at(Kind::Ident) {
            void = VOID_ELEMENTS.contains(&self.cur_text());
            self.start(K::Name);
            self.bump();
            while self.at(Kind::Dot) && self.nth_is(1, Kind::Ident) {
                self.bump();
                self.bump();
            }
            self.finish();
        }
        let mut self_closing = false;
        while !self.at_eof() && !self.at(Kind::RAngle) {
            if self.at(Kind::Slash) && self.nth_is(1, Kind::RAngle) {
                self_closing = true;
                self.bump();
                continue;
            }
            if self.at(Kind::Ident) {
                self.attribute();
                continue;
            }
            self.bump(); // stray punctuation; kept so the tree stays lossless
        }
        self.eat(Kind::RAngle);
        self.finish(); // OpenTag
        self_closing || void
    }

    /// `class="x"`, `on:press={handler}`, `style:width={w}`, `disabled`.
    fn attribute(&mut self) {
        self.start(K::Attr);
        self.start(K::AttrName);
        self.bump();
        // A namespaced attribute: `on:press`, `style:width`.
        while self.at(Kind::Colon) && self.nth_is(1, Kind::Ident) {
            self.bump();
            self.bump();
        }
        // `aria-label` lexes as three tokens; the name is all of them.
        while self.at(Kind::Minus) && self.nth_is(1, Kind::Ident) {
            self.bump();
            self.bump();
        }
        // An event's modifiers, `on:submit|prevent` (ADR-0131): part of the
        // name, so the attribute is one attribute and not three.
        while self.at(Kind::Pipe) && self.nth_is(1, Kind::Ident) {
            self.bump();
            self.bump();
        }
        self.finish(); // AttrName
        if self.eat(Kind::Eq) {
            // Whitespace may stand between `=` and the value, as in HTML. A
            // value that begins with `//` begins no comment (ADR-0167): it is
            // read before the value's node is started, which would take a
            // comment as trivia.
            while self
                .toks
                .get(self.pos)
                .is_some_and(|t| t.kind == Kind::Whitespace)
            {
                let t = self.toks[self.pos].clone();
                self.b.token(&t);
                self.pos += 1;
            }
            if self
                .toks
                .get(self.pos)
                .is_some_and(|t| matches!(t.kind, Kind::LineComment | Kind::DocAttr))
            {
                self.read_unquoted_value();
            }
            self.start(K::AttrValue);
            if self.at(Kind::LBrace) {
                self.interpolation();
            } else if self.at(Kind::Str) || self.at(Kind::MarkupText) {
                self.bump();
            } else if !(self.at_eof()
                || self.at(Kind::RAngle)
                || self.at(Kind::Slash) && self.nth_is(1, Kind::RAngle))
            {
                self.read_unquoted_value();
                self.bump();
            }
            self.finish(); // AttrValue
        }
        self.finish(); // Attr
    }

    /// **An unquoted attribute value, read as HTML reads it** (ADR-0167):
    /// the token at the cursor, and what follows it up to a space, `>` or
    /// `/>`, made one token. Until 2026-10-03 it was read as code, so a value
    /// was one word or number, and `href=http://x.y` built `href="http"`: the
    /// `//` began a comment that took the rest of the line.
    fn read_unquoted_value(&mut self) {
        let i = self.pos;
        let start = self.toks[i].span.start;
        let rest = &self.src[start..];
        let len = rest
            .char_indices()
            .find(|(at, c)| c.is_whitespace() || *c == '>' || rest[*at..].starts_with("/>"))
            .map_or(rest.len(), |(at, _)| at);
        if len > 0 {
            self.read_again(
                i,
                Token {
                    kind: Kind::MarkupText,
                    span: start..start + len,
                },
            );
        }
    }

    /// Character data between tags, as one node per run.
    fn text_run(&mut self) {
        self.start(K::Text);
        let mut moved = false;
        loop {
            // A `//` met inside the run is text too (ADR-0167): read as a
            // comment, it ran to the line's end, over the closing tag.
            self.markup_text();
            if self.at_eof()
                || self.at(Kind::LAngle)
                || self.at(Kind::LBrace)
                || self.at(Kind::RBrace)
            {
                break;
            }
            // A character the lexer has no rule for is text here, `·`, `—`
            // or an emoji as much as a letter, as HTML reads any character
            // between tags. In code it stays `Unknown`, an error there. Until
            // ADR-0226 it was `Unknown` here too, in the tree the compiler
            // reads: A-032's "1 reply · 2 likes".
            if self.at(Kind::Unknown)
                && let Some(i) =
                    (self.pos..self.toks.len()).find(|&i| !self.toks[i].kind.is_trivia())
            {
                self.toks[i].kind = Kind::MarkupText;
            }
            self.bump();
            moved = true;
        }
        if !moved {
            self.bump(); // never spin
        }
        self.finish();
    }

    /// **Markup text is text** (ADR-0167). The lexer does not know markup,
    /// and read a `//` in it as a comment, which runs to the line's end: in
    /// `<p>http://example.com</p>` it took the closing tag, and the page did
    /// not parse. Where pending markup content holds such a comment, the
    /// source is read again from it: as text up to the next `<`, `{` or `}`,
    /// and lexed as before from there.
    fn markup_text(&mut self) {
        let Some(at) = self.toks[self.pos..]
            .iter()
            .take_while(|t| t.kind.is_trivia())
            .position(|t| matches!(t.kind, Kind::LineComment | Kind::DocAttr))
        else {
            return;
        };
        let i = self.pos + at;
        let start = self.toks[i].span.start;
        let rest = &self.src[start..];
        let end = start + rest.find(['<', '{', '}']).unwrap_or(rest.len());
        self.read_again(
            i,
            Token {
                kind: Kind::MarkupText,
                span: start..end,
            },
        );
    }

    /// **`<!-- … -->`, a comment in markup** (ADR-0167): kept in the tree,
    /// and shown by no page. Until 2026-10-03 it was read as an element with
    /// no name, and what followed it was broken: `<!-- x --><p>one</p>`
    /// rendered `< x p>one</>`. Read from the source, since the lexer read its
    /// inside as code. An unclosed one runs to the end, as HTML's does.
    fn markup_comment(&mut self) {
        self.eat_trivia();
        let i = self.pos;
        let start = self.toks[i].span.start;
        let rest = &self.src[start..];
        let (end, closed) = match rest.find("-->") {
            Some(at) => (start + at + "-->".len(), true),
            None => (self.src.len(), false),
        };
        self.read_again(
            i,
            Token {
                kind: Kind::MarkupComment,
                span: start..end,
            },
        );
        if !closed {
            self.error_help(
                "PW0006",
                "a comment in markup is not closed",
                "close it with `-->`",
            );
        }
        self.bump();
    }

    /// The tokens from `i` on, replaced by `first` and the source after it,
    /// lexed again. The tokens still cover every byte (the lexer's
    /// invariant): `first` starts where token `i` did.
    fn read_again(&mut self, i: usize, first: Token) {
        let end = first.span.end;
        self.toks.truncate(i);
        self.toks.push(first);
        self.toks
            .extend(lex(&self.src[end..]).into_iter().map(|t| Token {
                kind: t.kind,
                span: t.span.start + end..t.span.end + end,
            }));
    }

    /// `{ expr }`, `{#each items as x (x.id)}`, `{/each}` inside a template.
    ///
    /// Returns the nesting delta: `{#..}` opens, `{/..}` closes, everything
    /// else is neutral.
    fn interpolation(&mut self) -> i32 {
        self.start(K::Interpolation);
        self.bump(); // `{`
        let delta = match self.cur() {
            Kind::Hash => 1,
            Kind::Slash => -1,
            _ => 0,
        };
        if self.at(Kind::Hash) || self.at(Kind::Slash) || self.at(Kind::Colon) {
            // A block marker: `{#each ..}`, `{/each}`, `{:else}`. Its contents
            // are directives, not a single expression, so they are consumed as
            // a region — except the parenthesised key, which is an expression.
            while !self.at(Kind::RBrace) && !self.at_eof() {
                self.bump();
            }
        } else if !self.at(Kind::RBrace) {
            self.expr(0);
        }
        self.expect(Kind::RBrace, "to close an interpolation");
        self.finish();
        delta
    }

    fn block_expr(&mut self) {
        self.block_expr_with(false);
    }

    /// The same block, optionally reading POLICY CLAUSES first.
    ///
    /// One block parser, not two. `policy_block_body` was a second, simplified
    /// loop written for `materialize`, whose bodies are policies and nothing
    /// else; pointing UI declarations at it on 2026-08-11 lost the nested `fn`
    /// branch and the labelled-statement branch, and `A-017` and `A-022` were
    /// rejected for effects their frame phases permit. The parser that decides
    /// what a `.pw` program means is one parser (E6F), and this is the same
    /// lesson inside a single file.
    fn block_expr_with(&mut self, policies_first: bool) {
        self.start(K::BlockExpr);
        self.bump(); // `{`
        if policies_first {
            self.policies(true);
        }
        let mut guard = 0;
        // Whether the statement before ended with its separator, or there
        // was none: the block's first.
        let mut separated = true;
        let mut takes = Takes::Nothing;
        // Whether the statement before was read without an error. After one,
        // where it ended is the recovery's guess, and a second statement on
        // its line is the error's, not the program's.
        let mut read = true;
        while !self.at(Kind::RBrace) && !self.at_eof() {
            guard += 1;
            if guard > 20_000 {
                self.error("PW0099", "block made no progress");
                break;
            }
            // **Two statements on one line are separated** (ADR-0243), by
            // `;` or a line, as Go, Swift and Kotlin separate them. `{ a b }`
            // was two statements, `a` evaluated and dropped, and checked.
            // What a block's readers read as one is not two: a block after
            // whatever precedes it on its line (`release(h) { .. }`), and
            // what the statements before take (`Takes`). Reported, and the
            // second read, so the block means what was written and nothing
            // after fails for it.
            let same_line = !separated && !self.newline_ahead();
            let block = self.at(Kind::LBrace);
            // A token no expression begins with is the expression parser's
            // to refuse (PW0009), and not a second statement.
            if same_line && !block && takes == Takes::Nothing && read && self.at_expression() {
                self.error_help(
                    "PW0030",
                    "two statements on one line are separated by `;`",
                    "write `;` between them, or the second on a line of its own",
                );
            }
            let before = self.pos;
            let errors = self.errors.len();
            // A named `fn` nested in a component or view body is a
            // declaration, not an expression. `fn(` with no name is still a
            // lambda. The nested-declaration set is deliberately this small:
            // it grows when a corpus file needs it, not on speculation.
            if self.at_kw("fn") && self.nth_is(1, Kind::Ident) {
                self.eat_trivia();
                self.decl();
            } else if self.at(Kind::Ident)
                && self.nth_is(1, Kind::Colon)
                && !STMT_KEYWORDS.contains(&self.cur_text())
            {
                // A labelled statement: `transform: scale(1.0) -> scale(1.08)`,
                // `duration: 180.milliseconds`. Same shape as a record field,
                // so it gets the same node kind — an `animate` block really is
                // a set of named property bindings.
                self.eat_trivia();
                self.start(K::Field);
                self.name("a property name");
                self.bump(); // `:`
                self.expr(0);
                self.finish();
            } else {
                self.expr(0);
            }
            takes = match self.one_word(before) {
                Some("return") => Takes::One,
                Some(word) if heads_a_clause(word) => Takes::Rest,
                // A clause's value runs to its line's end, or to a block,
                // its body, as the names check reads it.
                _ if takes == Takes::Rest && same_line && !block => Takes::Rest,
                _ => Takes::Nothing,
            };
            read = self.errors.len() == errors;
            let comma = self.eat(Kind::Comma);
            let semi = self.eat(Kind::Semi);
            separated = comma || semi;
            if self.pos == before {
                self.bump(); // never spin
            }
        }
        if !self.eat(Kind::RBrace) {
            self.error("PW0006", "unclosed block, expected `}`");
        }
        self.finish();
    }

    /// `use key: Secret<Payments> = secrets.payments()`
    /// `observe resize(self) -> Float`
    /// `frame { measure { .. } mutate { .. } }`
    /// `unsafe capability synchronous_geometry because "..."`
    ///
    /// One syntactic shape covering the body-level statement family. Their
    /// semantics are staged: recognising them is E2's job, checking them is E4
    /// and E7's.
    /// **Is this statement keyword's token sequence an ordinary call?**
    ///
    /// Architect ruling, 2026-08-11:
    ///
    /// > A keyword spelling may select a syntactic production only when the
    /// > tokens actually match that production. […] `measure(el)` must parse as
    /// > the ordinary call expression grammar and then resolve normally. It
    /// > must never gain or lose semantics merely because its spelling appears
    /// > in `STMT_KEYWORDS`.
    ///
    /// The twin of the `for` defect, from the other direction. There, syntax was
    /// promoted to a call by its position; here a call was demoted to syntax by
    /// its spelling — and `Expr::Keyword` contributes no effects, so
    /// `measure(el)` performed nothing while `helper(el)` beside it performed a
    /// layout read.
    ///
    /// The discriminator is exactly the ruling's: `kw ( .. )` with **no block
    /// after the closing paren** is a call. `measure { .. }` keeps the phase
    /// production, because a block is not part of any call. `release(h) { .. }`
    /// keeps it too, for the same reason — the block is what makes it a
    /// statement rather than an invocation.
    /// `( .. ) {` — a parameter list followed by a block, at a policy head.
    fn at_block_policy_header(&self) -> bool {
        if !self.at(Kind::LParen) {
            return false;
        }
        let mut i = 0;
        let mut depth = 0i32;
        loop {
            match self.nth(i).kind {
                Kind::LParen => depth += 1,
                Kind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return self.nth(i + 1).kind == Kind::LBrace;
                    }
                }
                Kind::Eof => return false,
                _ => {}
            }
            i += 1;
            if i > 4_000 {
                return false;
            }
        }
    }

    fn at_ordinary_call(&self) -> bool {
        if !self.nth_is(1, Kind::LParen) {
            return false;
        }
        // Walk to the matching `)` and look at what follows. Counting rather
        // than assuming one level: `measure(f(x))` closes twice.
        let mut i = 1;
        let mut depth = 0i32;
        loop {
            match self.nth(i).kind {
                Kind::LParen => depth += 1,
                Kind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return self.nth(i + 1).kind != Kind::LBrace;
                    }
                }
                Kind::Eof => return false,
                _ => {}
            }
            i += 1;
            if i > 4_000 {
                return false;
            }
        }
    }

    fn keyword_stmt(&mut self) {
        self.start(K::LetStmt);
        let keyword = self.cur_text().to_string();
        self.bump(); // the keyword
        // **`use _ = ..`** (ADR-0250): `_` is `let`'s discard. A `use` names
        // what it holds until its block ends; this read as `use`, then a
        // second statement `_ = ..`, and was PW0030.
        if keyword == "use" && self.at(Kind::Underscore) {
            self.error_help(
                "PW0001",
                "`use` binds a name, and `_` binds nothing",
                "name what it holds, `use handle = ..`: its block ends it when the block ends",
            );
            self.bump();
        }
        // A qualified keyword: `unsafe.imperative`, `unsafe.lifecycle`.
        while self.at(Kind::Dot) && self.nth_is(1, Kind::Ident) {
            self.bump();
            self.bump();
        }
        // An optional chain of modifier words before any punctuation:
        // `unsafe capability synchronous_geometry`, `observe resize`.
        while self.at(Kind::Ident)
            && (!self.newline_ahead() || STMT_CLAUSE_KEYWORDS.contains(&self.cur_text()))
            && !STMT_KEYWORDS.contains(&self.cur_text())
        {
            self.bump();
            if self.at(Kind::LParen)
                || self.at(Kind::Colon)
                || self.at(Kind::Eq)
                || self.at(Kind::Arrow)
                || self.at(Kind::LBrace)
                || self.at(Kind::Str)
            {
                break;
            }
        }
        if self.at(Kind::Str) {
            self.bump(); // a `because "..."` justification
        }
        // A statement may declare its own effect row: `post_paint !{ post_paint,
        // trace } { .. }`. Without this the row terminated the statement and the
        // block that followed became a sibling — so the work inside it looked
        // like it happened during render.
        if self.at(Kind::Bang) {
            self.effect_row();
        }
        if self.at(Kind::LParen) {
            self.start(K::ArgList);
            self.bump();
            loop {
                if self.at(Kind::RParen) || self.at_eof() {
                    break;
                }
                // A named argument, as in a call: `observe intersection(self,
                // threshold = 0.1)`. Until 2026-09-25 this list read `threshold
                // = 0.1` as an assignment to a name nothing declares (ADR-0047).
                if self.at(Kind::Ident) && (self.nth_is(1, Kind::Eq) || self.nth_is(1, Kind::Colon))
                {
                    self.bump();
                    self.bump();
                }
                self.expr(0);
                if !self.eat(Kind::Comma) {
                    break;
                }
            }
            self.expect(Kind::RParen, "to close a statement argument list");
            self.finish();
        }
        // **When a scheduled command runs**: `at` an instant, or `after` a
        // duration from the commit, an expression of its own, which a
        // trailing modifier word could not hold (`at order.deadline` ended the
        // statement at its `.`).
        if keyword == "schedule"
            && matches!(self.cur_text(), "at" | "after")
            && self.at(Kind::Ident)
        {
            self.bump();
            self.expr(0);
        }
        if self.eat(Kind::Colon) {
            self.type_ref();
        }
        if self.eat(Kind::Arrow) {
            self.type_ref();
        }
        if self.eat(Kind::Eq) {
            self.expr(0);
        }
        // Trailing modifiers before a block: `resource map when visible { .. }`.
        while self.at(Kind::Ident)
            && (!self.newline_ahead() || STMT_CLAUSE_KEYWORDS.contains(&self.cur_text()))
        {
            self.bump();
        }
        if self.at(Kind::LBrace) {
            self.block_expr();
        }
        self.finish();
    }

    /// `derived e`: the keyword and the expression it computes, one
    /// statement whose initialiser is the value (ADR-0047).
    fn derived_expr(&mut self) {
        self.start(K::LetStmt);
        self.bump(); // derived
        self.expr(0);
        self.finish();
    }

    /// **A statement keyword cannot name a value.** `let query = ..` parsed,
    /// and then every use of `query` in an expression parsed as a `query ..`
    /// statement: the program checked, because the checker reads such a
    /// statement as a value of no known type, and meant something else. Found
    /// 2026-09-25, writing kiokun's ranking in Pleris.
    ///
    /// Since ADR-0195 (ruling 3), every reserved word: each that can begin a
    /// statement or an expression in a body. `let return = n` checked, and
    /// the binding's next use read as a `return`. The repair suggests a name,
    /// with the trailing underscore PEP 8 gives a name that would be a
    /// keyword.
    fn not_a_statement_keyword(&mut self, what: &str) {
        if self.at(Kind::Ident) && reserved(self.cur_text()) {
            let word = self.cur_text().to_string();
            self.error_help(
                "PW0013",
                format!(
                    "`{word}` begins a statement or an expression, and cannot name {what}: a \
                     use of it would read as `{word} ..`"
                ),
                format!("name it `{word}_`, or after what it holds"),
            );
        }
    }

    /// **A declaration's name is not a word that begins an expression**
    /// (ADR-0195, ruling 3). A declaration is used by a call, and a call by
    /// a statement word reads as a call (`document.query(..)`, `query(..)`),
    /// so those name declarations, as the platform's `query`, `measure` and
    /// `mutate` do. A call by an expression word does not: `if (..)`,
    /// `return (..)` and `fn (..)` read as what they begin.
    fn not_an_expression_keyword(&mut self, what: &str) {
        if self.at(Kind::Ident)
            && (self.cur_text() == "derived"
                || (EXPR_KEYWORDS.contains(&self.cur_text())
                    && !matches!(self.cur_text(), "signal" | "provide")))
        {
            self.not_a_statement_keyword(what);
        }
    }

    /// `signal x: T = e` (ADR-0130): a `let`, whose type is written and
    /// whose value the page's handlers change.
    fn signal_stmt(&mut self) {
        self.start(K::LetStmt);
        self.bump(); // signal
        self.not_a_statement_keyword("a signal");
        self.name("a signal's name");
        if self.expect(
            Kind::Colon,
            "and the signal's type: `signal name: Type = value`",
        ) {
            self.type_ref();
        }
        if self.expect(
            Kind::Eq,
            "and the signal's first value: `signal name: Type = value`",
        ) {
            self.expr(0);
        }
        self.finish();
    }

    /// `provide name = e` (ADR-0144): an assignment of the signal a module
    /// declares, for everything the body it is written in contains.
    fn provide_stmt(&mut self) {
        self.start(K::LetStmt);
        self.bump(); // provide
        self.name("the name of the signal provided");
        if self.expect(
            Kind::Eq,
            "and the value it is provided with: `provide name = value`",
        ) {
            self.expr(0);
        }
        self.finish();
    }

    fn let_stmt(&mut self) {
        self.start(K::LetStmt);
        self.bump(); // let
        let mutable = self.eat_kw("mut");
        self.not_a_statement_keyword("a binding");
        // **`let _ = e`: an explicit discard** (ADR-0250, ruling 0099-a):
        // the value is computed and bound to nothing. It was "expected a
        // binding name", and a program discarded by naming a binding it
        // never read, `let _ignored = e`.
        if self.at(Kind::Underscore) {
            // `let mut _`: nothing is bound to change. Rust refuses it too.
            if mutable {
                self.error_help(
                    "PW0001",
                    "`let mut` binds a name to change, and `_` binds nothing",
                    "discard the value with `let _ = ..`, or name it: `let mut x = ..`",
                );
            }
            self.start(K::WildcardPat);
            self.bump();
            self.finish();
        } else {
            self.name("a binding name");
        }
        if self.eat(Kind::Colon) {
            self.type_ref();
        }
        if self.eat(Kind::Eq) {
            self.expr(0);
        }
        self.finish();
    }

    fn if_expr(&mut self) {
        self.start(K::IfExpr);
        self.bump(); // if
        self.expr(0);
        if self.at(Kind::LBrace) {
            self.block_expr();
        }
        while self.at_kw("elif") || self.at_kw("else") {
            let is_else = self.at_kw("else");
            self.bump();
            if is_else && self.at_kw("if") {
                self.bump();
                self.expr(0);
            } else if !is_else {
                self.expr(0);
            }
            if self.at(Kind::LBrace) {
                self.block_expr();
            }
        }
        self.finish();
    }

    /// `for x in xs { .. }`, `for (i, v) in xs.enumerate() { .. }`.
    ///
    /// The binder is a PATTERN, so the tuple form binds two names and every
    /// consumer that collects bindings finds them. Written as its own node
    /// rather than left as a call: a loop is control flow, and
    /// `resolve::local_bindings` cannot introduce a scope from an argument
    /// list.
    fn for_expr(&mut self) {
        self.start(K::ForExpr);
        self.bump(); // for
        self.pattern();
        // `in` is a contextual keyword here; without it the loop is still a
        // loop with a missing iterable, which is a better tree to report on
        // than a call.
        if self.at_kw("in") {
            self.bump();
        } else {
            self.error("PW0018", "expected `in` after the loop binding");
        }
        self.expr(0);
        if self.at(Kind::LBrace) {
            self.block_expr();
        }
        self.finish();
    }

    fn match_expr(&mut self) {
        self.start(K::MatchExpr);
        self.bump(); // match
        self.expr(0);
        if self.eat(Kind::LBrace) {
            let mut guard = 0;
            while !self.at(Kind::RBrace) && !self.at_eof() {
                guard += 1;
                if guard > 5_000 {
                    self.error("PW0099", "match made no progress");
                    break;
                }
                let before = self.pos;
                self.start(K::MatchArm);
                self.pattern();
                // Until 2026-09-25 an arm with no `=>` was accepted without a
                // word, which is how `=> return Ok(())` became an arm whose
                // pattern was `Ok(())` (below).
                if self.expect(Kind::FatArrow, "after a match arm's pattern") {
                    self.arm_body();
                }
                self.finish();
                self.eat(Kind::Comma);
                if self.pos == before {
                    self.bump();
                }
            }
            if !self.eat(Kind::RBrace) {
                self.error("PW0006", "unclosed match, expected `}`");
            }
        }
        self.finish();
    }

    /// A match arm's body: one expression, or `return` and the value on its
    /// line.
    ///
    /// A block reads `return e` as two statements, `return` and then `e`. An
    /// arm holds one expression, so until 2026-09-25
    /// `Delivered(r) => return Ok(())` ended at `return`, and `Ok(())` was
    /// read as the next arm's pattern, with no body and no error. The
    /// exhaustiveness analysis read that phantom arm as a wildcard, which
    /// proved every match holding one exhaustive. Now the arm holds both
    /// statements, and the HIR reads them as a block.
    fn arm_body(&mut self) {
        let returns = self.at_kw("return");
        self.expr(0);
        if returns
            && !self.newline_ahead()
            && !self.at(Kind::Comma)
            && !self.at(Kind::RBrace)
            && !self.at_eof()
        {
            self.expr(0);
        }
    }

    fn pattern(&mut self) {
        self.eat_trivia();
        let cp = self.b.checkpoint();
        self.pattern_atom();
        while self.at(Kind::Pipe) {
            self.b.start_at(cp, K::OrPat);
            self.bump();
            self.pattern_atom();
            self.finish();
        }
    }

    fn pattern_atom(&mut self) {
        match self.cur() {
            Kind::Underscore => {
                self.start(K::WildcardPat);
                self.bump();
                self.finish();
            }
            Kind::Int | Kind::Float | Kind::Str => {
                self.start(K::LiteralPat);
                self.bump();
                self.finish();
            }
            // `-1`: a negative literal, one pattern (ADR-0060). It was a
            // parse error, so a match could not name a negative number.
            Kind::Minus if matches!(self.nth(1).kind, Kind::Int | Kind::Float) => {
                self.start(K::LiteralPat);
                self.bump();
                self.bump();
                self.finish();
            }
            Kind::LParen => {
                self.start(K::TuplePat);
                self.bump();
                loop {
                    if self.at(Kind::RParen) || self.at_eof() {
                        break;
                    }
                    self.pattern();
                    if !self.eat(Kind::Comma) {
                        break;
                    }
                }
                self.eat(Kind::RParen);
                self.finish();
            }
            Kind::Ident => {
                // A constructor pattern if it takes arguments, if it is
                // qualified by its type (`Shape.Empty`), or if its name is a
                // case's, uppercase (ADR-0195, ruling 2); `true` and `false`
                // are `Bool`'s cases. A binding otherwise, which is one name.
                // Until 2026-09-26 `Shape.Empty` was a binding of that dotted
                // name, so it matched everything, and `Shape.Circle(r)` did
                // not parse (ADR-0059). Until ADR-0197 a bare `Empty` was a
                // binding here, and each later reader guessed again.
                let mut after = 1;
                while self.nth_is(after, Kind::Dot) && self.nth_is(after + 1, Kind::Ident) {
                    after += 2;
                }
                let takes_args = self.nth_is(after, Kind::LParen);
                let is_ctor = takes_args
                    || after > 1
                    || pattern_kind(self.cur_text()) == PatternKind::Case
                    || matches!(self.cur_text(), "true" | "false");
                if !is_ctor {
                    self.not_a_statement_keyword("a binding");
                }
                self.start(if is_ctor { K::CtorPat } else { K::BindingPat });
                self.dotted_name("a pattern");
                if takes_args {
                    self.bump(); // (
                    loop {
                        if self.at(Kind::RParen) || self.at_eof() {
                            break;
                        }
                        self.pattern();
                        if !self.eat(Kind::Comma) {
                            break;
                        }
                    }
                    self.eat(Kind::RParen);
                }
                self.finish();
            }
            _ => {
                self.start(K::WildcardPat);
                let found = self.found();
                self.error("PW0011", format!("expected a pattern, found {found}"));
                self.bump();
                self.finish();
            }
        }
    }

    // --- declarations -------------------------------------------------------

    /// Does an identifier here start a clause the parser does not know?
    ///
    /// **The invariant this exists for:** Pleris may reject authored semantics,
    /// but it must never silently erase them. `partiton public` used to become
    /// `policies = []` — observationally identical to a declaration with no
    /// policy at all — and `impact` did exactly that in the platform library
    /// for a whole commit while every rule reading it got an empty answer.
    ///
    /// A policy head is a bare identifier followed by a value on the same line.
    /// It is never followed by `.`, `(`, `=`, `<`, `,` or `{`, which is what
    /// separates it from an expression: `Stores.get(1)` and `partiton public`
    /// are both identifier-led, and only the second is a clause. A declaration
    /// starter is excluded because a policy list ends where the next
    /// declaration begins.
    fn at_unknown_policy(&self) -> bool {
        if !self.at(Kind::Ident) {
            return false;
        }
        let head = self.cur_text();
        // A STATEMENT keyword begins a statement, never a policy. Needed once
        // UI declarations started parsing their policies inside the braces
        // (2026-08-11): `animate pulse { .. }` sits after `placement browser`
        // in `A-020`, and without this it was reported as an unknown policy.
        //
        // The three tables must not overlap in interpretation. That is the
        // same lesson as `measure` being both a phase keyword and a callable
        // name — `docs/RISK_QUEUE.md` — arriving from the other direction.
        if POLICY_KEYWORDS.contains(&head)
            || DECL_STARTERS.contains(&head)
            || STMT_KEYWORDS.contains(&head)
            || UI_NOUNS.contains(&head)
            || EXPR_KEYWORDS.contains(&head)
        {
            return false;
        }
        // **`user` before a declaration's keyword begins the declaration**
        // (ADR-0318's contextual visibility; ADR-0324): until 2026-10-12, after
        // a declaration with no body, `fn owner(..) host ".."`, it was read as
        // that declaration's unknown policy `user`, and the query it opened
        // as the function's clauses. No program had written one there: the
        // feed's queries said `private`, which is reserved.
        if self.at_visibility() {
            return false;
        }
        // A value must follow, on this line. An operator is not one: `a + b`
        // in a query's body is an expression. Until 2026-09-25 only `<` was
        // excluded, so `{ a + b }` was an unknown policy `a` with the value
        // `+ b`, and the body lost its only statement.
        !matches!(
            self.nth(1).kind,
            Kind::Dot
                | Kind::LParen
                | Kind::Eq
                | Kind::LAngle
                | Kind::Comma
                | Kind::LBrace
                | Kind::RBrace
                | Kind::Arrow
                | Kind::Colon
                | Kind::Eof
                | Kind::Plus
                | Kind::Minus
                | Kind::Star
                | Kind::Slash
                | Kind::Percent
                | Kind::Cmp
                | Kind::RAngle
                | Kind::Amp
                | Kind::Pipe
                | Kind::PipeGt
                | Kind::Question
                | Kind::LBracket
        ) && !self.nth_starts_line(1)
    }

    /// `in_block` — are we already inside the declaration's braces?
    ///
    /// It decides whether a bare `{` after a policy head opens a BLOCK POLICY
    /// or the declaration's own body. In header position it is always the
    /// body: `query Store(id) ..\n    offline\n{ .. }` ends its last policy at
    /// the brace, and reading that as `offline { .. }` swallowed the whole
    /// declaration body into a flag's value.
    ///
    /// The `( .. ) {` form is unambiguous in either position, because a
    /// declaration's body never follows a parameter list at a policy head.
    /// **A head the parser knows, here.** `layout` is one only where a name
    /// follows it on its line, `layout StoreLayout`: the word is also an
    /// effect family's, and `layout.measure` written first in a body is not
    /// a clause. Named heads require a name on their line (`NAMED_HEADS`),
    /// and `bounded` requires a count (`COUNTED_HEADS`). `set.size()` is a call.
    fn at_policy_head(&self) -> bool {
        self.at(Kind::Ident)
            && POLICY_KEYWORDS.contains(&self.cur_text())
            && (!NAMED_HEADS.contains(&self.cur_text())
                || (self.nth_is(1, Kind::Ident) && !self.nth_starts_line(1)))
            && (!COUNTED_HEADS.contains(&self.cur_text())
                || (self.nth_is(1, Kind::Int) && !self.nth_starts_line(1)))
            && (!OWNERSHIP_HEADS.contains(&self.cur_text()) || self.a_name_follows_on_its_line())
            && (self.cur_text() != "private" || self.private_heads_a_clause())
    }

    /// Is the token after this one a name, on this one's line?
    fn a_name_follows_on_its_line(&self) -> bool {
        self.nth_is(1, Kind::Ident) && !self.nth_starts_line(1)
    }

    /// **Does the `private` here head a table's clause** (track
    /// `row-ownership`, ADR-0384): `private sent, day`, a field's name after
    /// it on its line? Anywhere a declaration's word follows it, `private fn`,
    /// `private query`, `private user query`, it is that declaration's
    /// visibility, as it always was.
    fn private_heads_a_clause(&self) -> bool {
        if !self.a_name_follows_on_its_line() {
            return false;
        }
        let next = &self.src[self.nth(1).span.clone()];
        if DECL_STARTERS.contains(&next) || UI_NOUNS.contains(&next) {
            return false;
        }
        // `user` is a visibility only where a declaration's word follows it
        // (`at_visibility`); a field named `user` is a field.
        let after = self.nth(2);
        let after_text = &self.src[after.span.clone()];
        !(next == "user"
            && after.kind == Kind::Ident
            && (UI_NOUNS.contains(&after_text)
                || RESOURCE_NOUNS.contains(&after_text)
                || matches!(
                    after_text,
                    "type" | "opaque" | "fn" | "let" | "prelude" | "effect"
                )))
    }

    fn policies(&mut self, in_block: bool) {
        let known = self.at_policy_head();
        if !known && !self.at_unknown_policy() {
            return;
        }
        self.start(K::PolicyList);
        while self.at_policy_head() || self.at_unknown_policy() {
            // Recorded as what it is. `Policy` for a head the parser knows,
            // `UnknownPolicy` for one it does not — never nothing.
            let unknown = !POLICY_KEYWORDS.contains(&self.cur_text());
            if unknown {
                let head = self.cur_text().to_string();
                self.error(
                    "PW0005",
                    format!("unknown policy `{head}`; the compiler does not know this clause"),
                );
            }
            self.start(if unknown { K::UnknownPolicy } else { K::Policy });
            self.bump(); // keyword

            // **A BLOCK policy: `acquire { .. }`, `release(h) { .. }`,
            // `draw(ctx) { .. }`.**
            //
            // Architect ruling, 2026-08-11: a block policy's header introduces
            // real lexical binders, and downstream resolution should receive
            // them already lowered — nothing should recognise `"draw"` or
            // `"release"` by spelling to decide what a block binds.
            //
            // Detected from the TOKENS, not from a table: an optional parameter
            // list followed by `{`. That is the same rule the `measure` repair
            // established — a spelling selects a production only when the
            // tokens match it — so `retry bounded_exponential(max = 3)` is not
            // one, because no block follows.
            if (in_block && self.at(Kind::LBrace)) || self.at_block_policy_header() {
                if self.at(Kind::LParen) {
                    self.param_list();
                }
                if self.at(Kind::LBrace) {
                    self.block_expr();
                }
                self.finish();
                continue;
            }
            let mut depth = 0i32;
            // The last token the value took, for where its line ends.
            let mut last: Option<Kind> = None;
            while !self.at_eof() {
                let k = self.cur();
                if depth == 0 && k == Kind::LBrace {
                    break;
                }
                // **Inside a block, a value ends with its line, unless it
                // cannot have** (ADR-0273): a bracket still open, or a line
                // that ends wanting more, a comma or an operator, goes on to
                // the next. A materialization that derives its value has a
                // body after its clauses, and one that began with a name, a
                // literal or a constructor was the last clause's value:
                // `regenerate on_invalidation` took the body `n` as
                // `on_invalidation n`. A header's clauses end at the body's
                // `{`, and read on across a missing comma, which a check then
                // names (ADR-0237).
                if in_block
                    && depth == 0
                    && self.newline_ahead()
                    && !matches!(
                        last,
                        Some(
                            Kind::Comma
                                | Kind::Colon
                                | Kind::Dot
                                | Kind::Arrow
                                | Kind::FatArrow
                                | Kind::Cmp
                                | Kind::Pipe
                                | Kind::PipeGt
                                | Kind::Bang
                                | Kind::Eq
                                | Kind::Question
                                | Kind::Amp
                                | Kind::Plus
                                | Kind::Minus
                                | Kind::Star
                                | Kind::Slash
                                | Kind::Percent
                        )
                    )
                {
                    break;
                }
                // A policy list inside a `materialize` block ends at the
                // block's own `}`. Without this the last clause's value
                // swallowed the closing brace, so the declaration looked
                // unclosed to everything downstream.
                if depth == 0 && k == Kind::RBrace {
                    break;
                }
                // A policy value must take at least one token. `cache private`
                // and `scope component` would otherwise end immediately,
                // because `private` and `component` also start declarations.
                // A policy value ends where the next clause or declaration
                // begins — and one only begins at the START OF A LINE.
                //
                // Without that condition, `key store, session` ended after the
                // comma, because `session` also starts a `session query`
                // declaration. Everything after it stopped being a policy, so
                // `cache shared` was invisible and the shared-cache rule did
                // not run at all. Silently: the query simply had no cache
                // policy as far as any checker could tell.
                //
                // `newline_ahead` is the right test even though the name reads
                // backwards: `nth()` skips trivia WITHOUT advancing `pos`, so
                // the whitespace between the last consumed token and this one
                // is still ahead of `pos`. A hand-rolled backwards scan looked
                // before that whitespace, always answered false, and silently
                // disabled the rule for two corpus fixtures.
                // `took_value` is deliberately NOT required when the next
                // token starts a new LINE. A policy value never begins on the
                // line after its head, so `affine` followed by `acquire { .. }`
                // has no value — and requiring one made the flag swallow the
                // head that followed it, which deleted `acquire` and `release`
                // from `A-007` entirely. The guard exists for `cache private`,
                // where `private` is on the SAME line.
                if depth == 0
                    && k == Kind::Ident
                    && self.newline_ahead()
                    && (POLICY_KEYWORDS.contains(&self.cur_text())
                        || DECL_STARTERS.contains(&self.cur_text())
                        // A STATEMENT ends this clause too. Needed from
                        // 2026-08-11, when UI declarations began writing their
                        // policies inside their braces: `placement browser`
                        // is followed by `frame { .. }` in `A-017`, and
                        // without this the value swallowed `frame` and the
                        // phase block stopped being a phase block — so a
                        // component was rejected for effects its phases permit.
                        || STMT_KEYWORDS.contains(&self.cur_text())
                        || UI_NOUNS.contains(&self.cur_text())
                        || EXPR_KEYWORDS.contains(&self.cur_text())
                        // An UNKNOWN head ends this clause too. Without it the
                        // value loop swallows the next line — `capability none`
                        // followed by `impakt layout_write` became one policy
                        // whose value was `none impakt layout_write`, so the
                        // clause the parser was supposed to reject disappeared
                        // into the one it accepted.
                        || self.at_unknown_policy()
                        // And a declaration a contextual `user` opens
                        // (ADR-0324): `user query` after `host ".."`.
                        || self.at_visibility())
                {
                    break;
                }
                match k {
                    Kind::LParen | Kind::LBracket | Kind::LAngle => depth += 1,
                    Kind::RParen | Kind::RBracket | Kind::RAngle => depth -= 1,
                    _ => {}
                }
                last = Some(k);
                self.bump();
            }
            self.finish();
        }
        self.finish();
    }

    fn body(&mut self) {
        if !self.at(Kind::LBrace) {
            return;
        }
        self.start(K::Body);
        self.block_expr();
        self.finish();
    }

    /// A block whose POLICY CLAUSES are inside its braces.
    ///
    /// `materialize` has written them that way since E6 — corpus A-009 and
    /// R-017 are the specification — and UI declarations joined it on
    /// 2026-08-11, when policy values left the executable body tree (architect
    /// ruling). `page StorePage(id) { placement origin  cache private  .. }`
    /// used to lower `placement` and `origin` as two unrelated bare names, and
    /// `tests/policy_consumers.rs` measured what that cost: four of six
    /// analyses could not tell a policy value from a term.
    fn policy_block_body(&mut self) {
        if !self.at(Kind::LBrace) {
            return;
        }
        self.start(K::Body);
        self.block_expr_with(true);
        self.finish();
    }
    fn decl(&mut self) -> bool {
        if self.at_kw("module") || self.at_kw("import") {
            let is_module = self.at_kw("module");
            self.start(if is_module {
                K::ModuleDecl
            } else {
                K::ImportDecl
            });
            self.bump();
            self.dotted_name("a module name");
            if self.at(Kind::Dot) && self.nth_is(1, Kind::LBrace) {
                self.bump();
            }
            if self.at(Kind::LBrace) {
                self.bump();
                self.skip_balanced(Kind::LBrace, Kind::RBrace);
                self.eat(Kind::RBrace);
            }
            self.finish();
            return true;
        }

        // A visibility keyword may precede any declaration, not only the UI and
        // resource nouns. `private type Secretive` parsed as TWO declarations —
        // a bare word and an unqualified type — so the type looked public and a
        // module could import it.
        let vis_prefix = self.at_visibility()
            && matches!(
                &self.src[self.nth(1).span.clone()],
                "type" | "opaque" | "fn" | "let"
            );

        if vis_prefix {
            // Flush trivia, take a checkpoint, then consume the keyword. The
            // branch below re-parents both under the declaration's real kind
            // via `start_at`, so the visibility is the declaration's first
            // token — which is where `lower::visibility_of` looks.
            self.eat_trivia();
            self.pending_vis = Some(self.b.checkpoint());
            self.bump();
        }

        if self.at_kw("opaque") {
            self.start(K::OpaqueDecl);
            self.bump();
            self.eat_kw("type");
            self.name("a type name");
            // Type parameters, as `type` already accepts: `opaque type
            // Secret<C>`. Without them `Secret<Payments>` could not be
            // *declared*, only written — so a privacy label naming a capability
            // had to be invented by a checker instead of read from a signature.
            self.type_params();
            if self.eat(Kind::Eq) {
                self.type_ref();
            }
            // `where value >= 1`: what every value of the type holds
            // (ADR-0179), an expression the checker reads as bounds.
            if self.at_kw("where") {
                self.start(K::Invariant);
                self.bump();
                self.expr(0);
                self.finish();
            }
            self.finish();
            return true;
        }

        if self.at_kw("type") {
            let is_record = {
                // `type X = Name {` is a record; otherwise a union.
                let mut i = 1;
                while !matches!(self.nth(i).kind, Kind::Eq | Kind::Eof) {
                    i += 1;
                }
                self.nth(i + 1).kind == Kind::Ident && self.nth(i + 2).kind == Kind::LBrace
            };
            self.start(if is_record {
                K::RecordDecl
            } else {
                K::UnionDecl
            });
            self.bump();
            self.name("a type name");
            // **The same binder `opaque type` and `effect` use.** This was
            // `skip_balanced` until 2026-09-24, so `type Box<T> = Box { v: T }`
            // parsed, lowered with no type parameters, and left `T` a name that
            // resolved to nothing — a declaration whose generality the parser
            // accepted and then threw away without a diagnostic.
            self.type_params();
            if self.eat(Kind::Eq) {
                if is_record {
                    self.name("a record constructor");
                    if self.at(Kind::LBrace) {
                        self.start(K::FieldList);
                        self.bump();
                        loop {
                            if self.at(Kind::RBrace) || self.at_eof() {
                                break;
                            }
                            self.start(K::Field);
                            self.name("a field name");
                            if self.eat(Kind::Colon) {
                                self.type_ref();
                            }
                            self.finish();
                            if !self.eat(Kind::Comma) {
                                break;
                            }
                        }
                        self.eat(Kind::RBrace);
                        self.finish();
                    }
                } else {
                    self.start(K::VariantList);
                    loop {
                        self.eat(Kind::Pipe);
                        if !self.at(Kind::Ident) {
                            break;
                        }
                        self.start(K::Variant);
                        self.name("a variant name");
                        if self.at(Kind::LParen) {
                            self.bump();
                            loop {
                                if self.at(Kind::RParen) || self.at_eof() {
                                    break;
                                }
                                if !self.type_ref() {
                                    break;
                                }
                                if !self.eat(Kind::Comma) {
                                    break;
                                }
                            }
                            self.eat(Kind::RParen);
                        }
                        self.finish();
                        if !self.at(Kind::Pipe) {
                            break;
                        }
                    }
                    self.finish();
                }
            }
            self.finish();
            return true;
        }

        // **`signal drawer: Bool`: a signal a page or view provides**
        // (ADR-0144). A name and a type: a `provide` in a body gives it its
        // value, for what that body contains. A value written here is
        // refused, and kept in the tree, unless clauses follow it on the
        // lines below: **a signal the device keeps** (track `device-store`,
        // ADR-0366), whose value is the default the device starts with and
        // whose clauses say where it rests (`storage device "todos"`), whose
        // it is (`privacy Device`) and what two tabs' writes do (`conflict
        // last_write_wins`). What they must say is the checker's
        // (`device_store`, PW6805).
        if self.at_kw("signal") && self.nth_is(1, Kind::Ident) {
            self.start(K::LetDecl);
            self.bump(); // signal
            self.not_a_statement_keyword("a signal");
            self.name("a signal's name");
            if self.expect(Kind::Colon, "and the signal's type: `signal name: Type`") {
                self.type_ref();
            }
            if self.at(Kind::Eq) {
                let written = self.cur_span();
                self.bump();
                self.expr(0);
                if self.at_policy_head() && self.newline_ahead() {
                    self.policies(false);
                } else {
                    self.errors.push(SyntaxError {
                        code: "PW5306",
                        message: "a signal declared in a module has no value of its own".into(),
                        span: written,
                        help: Some(
                            "a page or view gives it one, for what it contains: `provide name =                              value`; or the device keeps it: `storage device \"name\"` on the                              line below"
                                .into(),
                        ),
                    });
                }
            }
            self.finish();
            return true;
        }

        if self.at_kw("let") {
            self.start(K::LetDecl);
            self.bump();
            self.eat_kw("mut");
            self.not_a_statement_keyword("a binding");
            self.name("a binding name");
            if self.eat(Kind::Colon) {
                self.type_ref();
            }
            if self.eat(Kind::Eq) {
                self.expr(0);
            }
            self.finish();
            return true;
        }

        if self.at_kw("fn") {
            self.start(K::FnDecl);
            self.bump();
            self.not_an_expression_keyword("a function");
            self.name("a function name");
            // **A callable may bind type parameters**: `fn map<T, U>(items:
            // List<T>, ..) -> List<U>`. E9-V2 (ADR-0031). Until 2026-09-24 this
            // was `PW0007`, so no callable was generic and `pw-std` declared
            // `List.map` over `List<decode.Unknown>` — a stand-in that a value
            // checker must either reject at every use or special-case by name.
            self.type_params();
            self.param_list();
            if self.eat(Kind::Arrow) {
                self.type_ref();
            }
            if self.at(Kind::Bang) {
                self.effect_row();
            }
            // **A `fn` may carry policies.**
            //
            // `host "pw:host/carts#add"` is how a declaration says the platform
            // supplies its implementation. Architect ruling, 2026-08-20:
            //
            // > Host implementation is explicit declaration metadata, never
            // > inferred from a missing/`todo` body or from the effect row.
            //
            // Read in HEADER position: a `fn`'s body is its implementation, and
            // a host-bound one has none, so there is no brace to be inside.
            self.policies(false);
            self.body();
            self.finish();
            return true;
        }

        let vis = self.at_visibility();
        let after_vis = if vis { self.nth(1) } else { self.nth(0) };
        let after_vis_text = &self.src[after_vis.span.clone()];

        // `layout` is a declaration only where a name follows it: the word is
        // also an effect family's (`layout.measure`).
        let named = self.nth(if vis { 2 } else { 1 }).kind == Kind::Ident;
        if after_vis.kind == Kind::Ident
            && UI_NOUNS.contains(&after_vis_text)
            && (after_vis_text != "layout" || named)
        {
            self.start(K::UiDecl);
            if vis {
                self.bump();
            }
            self.bump(); // noun
            self.not_an_expression_keyword("a declaration");
            self.name("a name");
            self.param_list();
            if self.at(Kind::Bang) {
                self.effect_row();
            }
            self.policies(false);
            // **A UI declaration's policies are inside its braces too.**
            //
            // `page StorePage(id) { placement origin  cache private  .. }`.
            // They were parsed as ordinary expressions until 2026-08-11, so
            // `placement` and `origin` were two unrelated bare names in the
            // executable body — and `tests/policy_consumers.rs` measured what
            // that cost: four of six analyses could not tell a policy value
            // from a term, and a page's declared AUTHORITY moved because of
            // what one spelled.
            //
            // Architect ruling, 2026-08-11: policy values leave the executable
            // body tree. Same mechanism `materialize` has used since E6.
            self.policy_block_body();
            self.finish();
            return true;
        }

        // E8 — `effect database.read<T> { capability .. host .. }`.
        //
        // Its own branch, ahead of the generic resource nouns, because it is
        // the ONE declaration whose name is a dotted path. Architect ruling,
        // 2026-08-07:
        //
        // > Give effect declarations their own path grammar […] That keeps the
        // > parser change local to the language feature that actually requires
        // > it. Given everything this project has discovered about syntax
        // > features accidentally widening unrelated grammar, I would strongly
        // > prefer that.
        //
        // So `type foo.bar` and `fn foo.bar()` stay invalid: `name()` is
        // untouched and only this branch reaches `dotted_name`.
        //
        // The type parameters reuse `type_params` — the same binder
        // `opaque type Secret<C>` uses — rather than an effect-specific one.
        // `prelude Effect` — the package saying which of its namespaces are
        // ambient. One keyword and one namespace name; no path, because the
        // module declaring it is the module whose declarations are exported.
        //
        // Architect ruling, 2026-08-07, sketched it as `prelude Effect from
        // web.effects` in a package manifest. Pleris has no manifest format —
        // a package is a directory of `.pw` files — so the declaration lives
        // in the module that owns the declarations. Same semantics for the one
        // case that exists, one fewer invention on the way, and it generalises
        // to `prelude Type` unchanged. What it cannot express is a package
        // electing a module it does not own; nothing needs that today, and a
        // manifest is where it belongs when packages get one.
        if after_vis.kind == Kind::Ident && after_vis_text == "prelude" {
            self.start(K::PreludeDecl);
            if vis {
                self.bump();
            }
            self.bump(); // `prelude`
            self.name("a namespace name");
            self.finish();
            return true;
        }

        if after_vis.kind == Kind::Ident && after_vis_text == "effect" {
            self.start(K::EffectDecl);
            if vis {
                self.bump();
            }
            self.bump(); // `effect`
            self.dotted_name("an effect name");
            self.type_params();
            self.policy_block_body();
            self.finish();
            return true;
        }

        if after_vis.kind == Kind::Ident && RESOURCE_NOUNS.contains(&after_vis_text) {
            let materialize = after_vis_text == "materialize";
            self.start(K::ResourceDecl);
            if vis {
                self.bump();
            }
            self.bump(); // noun
            self.not_an_expression_keyword("a declaration");
            self.name("a name");
            self.param_list();
            if self.eat(Kind::Arrow) {
                self.type_ref();
            }
            if self.at(Kind::Bang) {
                self.effect_row();
            }
            self.policies(false);
            // Every data-operation noun reads its policies inside its braces
            // too, since 2026-08-11. `resource StoreMap(..) { placement browser
            // affine  acquire { .. }  release(h) { .. } }` wrote four policies
            // there and every one of them was a bare name in the executable
            // body — `affine` a `Name`, `acquire` a call-shaped statement whose
            // block bound nothing.
            self.policy_block_body();
            let _ = materialize;
            self.finish();
            return true;
        }

        // `handler_policy { .. }` and similar bare named blocks.
        if self.at(Kind::Ident) && self.nth_is(1, Kind::LBrace) {
            self.start(K::ErrorDecl);
            self.name("a declaration name");
            self.body();
            self.finish();
            return true;
        }

        false
    }

    fn recover(&mut self) {
        while !self.at_eof() {
            if self.at(Kind::Ident) && DECL_STARTERS.contains(&self.cur_text()) {
                return;
            }
            if self.at(Kind::LBrace) {
                self.bump();
                self.skip_balanced(Kind::LBrace, Kind::RBrace);
                self.eat(Kind::RBrace);
                continue;
            }
            self.bump();
        }
    }

    fn run(mut self) -> Parse {
        self.b.start(K::SourceFile);
        while !self.at_eof() {
            self.fuel += 1;
            if self.fuel > 200_000 {
                self.error("PW0099", "parser made no progress");
                break;
            }
            let before = self.pos;
            if !self.decl() {
                self.start(K::ErrorDecl);
                let found = self.found();
                self.error_help(
                    "PW0007",
                    format!("expected a declaration, found {found}"),
                    format!(
                        "declarations start with one of: {}",
                        DECL_STARTERS.join(", ")
                    ),
                );
                self.bump();
                self.recover();
                self.finish();
            }
            if self.pos == before && !self.at_eof() {
                self.bump();
            }
        }
        // Trailing trivia must land inside the file node, or the tree loses it.
        self.eat_trivia();
        self.b.finish_node();

        Parse {
            green: SyntaxNode::new_root(self.b.finish()),
            errors: self.errors,
        }
    }
}

pub fn parse_tree(src: &str) -> Parse {
    P::new(src).run()
}

/// Parse a complete type fragment with the same grammar used in declarations.
/// This is for legacy HIR slots which retain source text, not for reparsing an
/// already resolved or serialized semantic type. Extra tokens are an error.
pub fn parse_type(src: &str) -> Parse {
    let mut p = P::standalone(src, "the end of the type".to_string());
    p.b.start(K::SourceFile);
    p.type_ref();
    p.rest_unread("a type annotation is one type", None);
    p.b.finish_node();
    Parse {
        green: SyntaxNode::new_root(p.b.finish()),
        errors: p.errors,
    }
}

/// **Parse an optimistic clause, standalone.**
///
/// ```text
/// Cart(current_session()) as cart => cart.add(item, quantity)
/// └─ the resource ENTRY ─┘    └ its ┘   └── the transition ──┘
///                          current value
/// ```
///
/// Architect ruling, 2026-08-11:
///
/// > An optimistic clause identifies a resource entry and binds its current
/// > value; its body is an ordinary Pleris transition expression.
///
/// The target is parsed at a binding power above `as` (which is 9), so the cast
/// operator does not absorb the binder — `Cart(..) as cart` is a resource
/// binding, not a cast to a type called `cart`.
///
/// A clause is arms of this shape, one for each entry the command speculates
/// on, separated by commas (ADR-0238).
///
/// Which policy heads take this shape is `pw_core::policy`'s answer. This is
/// the grammar; the table lives where the rest of the policy vocabulary does.
pub fn parse_transition_clause(src: &str) -> Parse {
    let mut p = P::standalone(src, "the end of the clause".to_string());
    p.b.start(K::SourceFile);
    p.b.start(K::TransitionClause);
    // **Each entry the command speculates on, an arm of its own** (ADR-0238):
    // `Timeline(..) as feed => liked(feed, post), Thread(post) as thread =>
    // liked_thread(thread, post)`, separated by commas as a clause's values
    // are (ADR-0237).
    p.comma_separated(
        |p| p.transition_arm(),
        "an optimistic clause's arms are separated by commas",
        "write a comma before this arm",
    );
    p.b.finish_node();
    p.b.finish_node();
    Parse {
        green: SyntaxNode::new_root(p.b.finish()),
        errors: p.errors,
    }
}

/// **Parse an `{#each}`'s head, standalone** (ADR-0242).
///
/// ```text
/// menu as item (item.id)
/// └list┘   └name┘ └─key──┘
/// ```
///
/// The list, `as` and the name each row binds, and the key that tells the
/// rows apart, in parentheses, where one is written. It was split at ` as `
/// and `(` in five places, each its own reading: `{#each xs ys as x (x)}`
/// checked, its list `xs ys`, and an unclosed key went unnoticed.
pub fn parse_each_head(src: &str) -> Parse {
    let mut p = P::standalone(src, "the end of the `{#each}`".to_string());
    p.b.start(K::SourceFile);
    p.b.start(K::EachHead);
    // The list. Above `as`'s power, as an optimistic clause's target is, so
    // the cast rule does not read the name as a type.
    if p.at_eof() {
        let found = p.found();
        p.error(
            "PW0019",
            format!("expected a list, `as` and a name for each row, found {found}"),
        );
    } else {
        p.expr(10);
        if !p.at_kw("as") {
            // What stands between the list and `as` is one error, and the
            // head is read on from `as`, so the name each row binds is
            // bound: `{#each xs ys as x}` is one mistake, not a second for
            // every `x` the rows read.
            let (start, found) = (p.cur_span().start, p.found());
            let mut end = start;
            p.start(K::ErrorExpr);
            while !p.at_eof() && !p.at_kw("as") {
                end = p.cur_span().end;
                p.bump();
            }
            p.finish();
            p.errors.push(SyntaxError {
                code: "PW0019",
                message: format!("expected `as` and a name for each row, found {found}"),
                span: start..end,
                help: None,
            });
        }
        if p.at_kw("as") {
            p.bump();
            if p.name("a name for each row") {
                // And what stands between the name and the key, an index
                // or a second name: one error, and the key read on, so a
                // keyed list is not refused for a key it has.
                if !p.at_eof() && !p.at(Kind::LParen) {
                    let start = p.cur_span().start;
                    let mut end = start;
                    p.start(K::ErrorExpr);
                    while !p.at_eof() && !p.at(Kind::LParen) {
                        end = p.cur_span().end;
                        p.bump();
                    }
                    p.finish();
                    p.errors.push(SyntaxError {
                        code: "PW0016",
                        message: "an `{#each}` is its list, `as` a name for each row, and a key \
                                  in parentheses"
                            .to_string(),
                        span: start..end,
                        help: None,
                    });
                }
                if p.eat(Kind::LParen) {
                    p.expr(0);
                    p.expect(Kind::RParen, "to close the key");
                }
            }
        }
    }
    p.rest_unread(
        "an `{#each}` is its list, `as` a name for each row, and a key in parentheses",
        None,
    );
    p.b.finish_node();
    p.b.finish_node();
    Parse {
        green: SyntaxNode::new_root(p.b.finish()),
        errors: p.errors,
    }
}

/// **Parse one expression, standalone.**
///
/// The same expression grammar `parse_tree` uses — there is one parser
/// (E6F), and a second reader of Pleris expressions is exactly what that
/// milestone existed to remove.
///
/// Written for an expression the declaration grammar keeps as text: a
/// string's hole, `{a}` in `"sum {a}"`, and the expression a block marker
/// carries, `c` in `{#if c}` (ADR-0237). `what` names it, for the error
/// reporting what follows the expression: "a hole is one expression".
///
/// The returned tree is rooted at `K::SourceFile` with the expression as its
/// only child, so every consumer that walks a tree works unchanged. Spans are
/// relative to `src`; a caller that pads `src` to the expression's place in
/// its file gets spans in that file.
pub fn parse_expr(src: &str, what: &str) -> Parse {
    let mut p = P::standalone(src, format!("the end of {what}"));
    p.b.start(K::SourceFile);
    if !p.at_eof() {
        p.expr(0);
    }
    // Anything after the first expression is a second value in a position
    // that takes one. Reported rather than dropped: `"sum {a b}"` rendered
    // `a`, and `{#if flag other}` was decided by `flag`, until ADR-0237.
    p.rest_unread(&format!("{what} is one expression"), None);
    p.b.finish_node();
    Parse {
        green: SyntaxNode::new_root(p.b.finish()),
        errors: p.errors,
    }
}

/// **Parse expressions separated by commas, standalone.**
///
/// A clause that names declarations and passes each its key:
/// `depends_on Store(id), Menu(id)`. The same expression grammar as
/// [`parse_expr`]; each expression is a child of the root, in order. What
/// each one must be — a name, or a name applied to its key — is the policy's
/// question, not the grammar's.
pub fn parse_expr_list(src: &str) -> Parse {
    let mut p = P::standalone(src, "the end of the clause".to_string());
    p.b.start(K::SourceFile);
    p.comma_separated(
        |p| p.expr(0),
        "a clause's values are separated by commas",
        "write a comma before this value",
    );
    p.b.finish_node();
    Parse {
        green: SyntaxNode::new_root(p.b.finish()),
        errors: p.errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::tree_text;

    fn parse_ok(src: &str) -> Parse {
        let p = parse_tree(src);
        assert!(p.ok(), "unexpected errors: {:?}", p.errors);
        p
    }

    /// Every test asserts losslessness too: a grammar that drops a token would
    /// otherwise pass every structural assertion.
    fn assert_lossless(src: &str, p: &Parse) {
        assert_eq!(tree_text(&p.green), src, "parse was not lossless");
    }

    fn kinds(node: &SyntaxNode) -> Vec<K> {
        node.descendants().map(|n| n.kind()).collect()
    }

    /// Text of the first node of `kind`, if any.
    fn first_text(p: &Parse, kind: K) -> Option<String> {
        p.green
            .descendants()
            .find(|n| n.kind() == kind)
            .map(|n| n.text().to_string())
    }

    /// Text of every node of `kind`, in document order.
    fn texts(p: &Parse, kind: K) -> Vec<String> {
        p.green
            .descendants()
            .filter(|n| n.kind() == kind)
            .map(|n| n.text().to_string())
            .collect()
    }

    #[test]
    fn a_block_left_open_ends_with_its_element() {
        // `{#if a}` with no `{/if}`: the block ends with `</main>`, and the
        // view and the file go on (ADR-0200). The checker says it is never
        // closed (PW5019). Until ADR-0200 it took `</main>` and every `}`
        // after it, and the file failed at its end, twice.
        let src = "view V(a: Bool) !{} {\n    <main>{#if a}<p>A</p></main>\n}\n\nview W() !{} {\n    <p>B</p>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let main = p
            .green
            .descendants()
            .find(|n| n.kind() == K::Element && n.text().to_string().starts_with("<main>"))
            .expect("main");
        assert_eq!(main.text().to_string(), "<main>{#if a}<p>A</p></main>");
        let block = main
            .children()
            .find(|c| c.kind() == K::MarkupBlock)
            .expect("the block, inside main");
        assert_eq!(block.text().to_string(), "{#if a}<p>A</p>");
        // A block closed by its marker is unchanged.
        let closed = "view V(a: Bool) !{} {\n    <main>{#if a}<p>A</p>{/if}</main>\n}\n";
        let p = parse_ok(closed);
        assert_lossless(closed, &p);
    }

    #[test]
    fn markup_nests_elements_rather_than_listing_them() {
        // The token-run version round-tripped and parsed and was still useless:
        // a renderer cannot read a token soup. Assert the NESTING, because a
        // flat list of Element nodes would satisfy every other check here.
        let src = "view V() !{} {\n    <main><h1>Title</h1><p>Body</p></main>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);

        let main = p
            .green
            .descendants()
            .find(|n| n.kind() == K::Element)
            .expect("an element");
        assert!(main.text().to_string().starts_with("<main>"));
        assert!(main.text().to_string().ends_with("</main>"));

        let inner: Vec<String> = main
            .children()
            .filter(|c| c.kind() == K::Element)
            .map(|c| c.text().to_string())
            .collect();
        assert_eq!(
            inner,
            ["<h1>Title</h1>", "<p>Body</p>"],
            "h1 and p must be CHILDREN of main, not siblings"
        );
    }

    #[test]
    fn a_self_closing_element_has_no_children() {
        let src = "view V() !{} {\n    <section><img />after</section>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let img = p
            .green
            .descendants()
            .find(|n| n.kind() == K::Element && n.text().to_string().starts_with("<img"))
            .expect("the img");
        assert_eq!(img.text().to_string(), "<img />");
        assert!(
            img.children().all(|c| c.kind() != K::Element),
            "a self-closing element must not swallow what follows it"
        );
    }

    #[test]
    fn an_event_s_modifiers_are_part_of_its_attribute() {
        // ADR-0138: until 2026-10-02 `on:submit|prevent={save}` was the
        // attribute `on:submit`, a stray `|`, and an attribute `prevent`.
        let src = "view V() !{} {\n    <form on:submit|prevent|stop={save}>x</form>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        assert_eq!(texts(&p, K::AttrName), ["on:submit|prevent|stop"]);
        assert_eq!(texts(&p, K::AttrValue), ["{save}"]);
    }

    #[test]
    fn an_arrow_lambda_s_parameters_may_be_typed() {
        // ADR-0138: `(e: InputEvent) => ..` writes the list `fn(e: InputEvent)
        // ..` does. It was an unclosed parenthesis.
        let src = "fn f() -> Int !{} {\n    let g = (e: InputEvent, n: Int) => n\n    1\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let lambda = p
            .green
            .descendants()
            .find(|n| n.kind() == K::LambdaExpr)
            .expect("a lambda");
        let params = lambda
            .children()
            .find(|c| c.kind() == K::ParamList)
            .expect("a parameter list");
        assert_eq!(
            params.children().filter(|c| c.kind() == K::Param).count(),
            2
        );
        // Control: a parenthesised expression is still one.
        let src = "fn f() -> Int !{} {\n    (1 + 2)\n}\n";
        let p = parse_ok(src);
        assert!(p.green.descendants().any(|n| n.kind() == K::ParenExpr));
    }

    #[test]
    fn attributes_keep_their_names_and_values_apart() {
        let src = "view V() !{} {\n    <button aria-label=\"Add\" on:press={handler} disabled>Go</button>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);

        assert_eq!(
            texts(&p, K::AttrName),
            ["aria-label", "on:press", "disabled"],
            "a namespaced or hyphenated name is ONE name"
        );
        let values = texts(&p, K::AttrValue);
        assert_eq!(
            values,
            ["\"Add\"", "{handler}"],
            "a bare attribute has no value"
        );

        // The interpolated handler must be a real expression, not text: an
        // effect checker has to be able to look inside it.
        assert!(
            p.green
                .descendants()
                .any(|n| n.kind() == K::NameExpr && n.text() == "handler"),
            "the attribute value must parse as an expression"
        );
        assert_eq!(texts(&p, K::Text), ["Go"]);
    }

    #[test]
    fn a_component_tag_keeps_its_qualified_name() {
        let src = "view V() !{} {\n    <store.Card id={x} />\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let name = p
            .green
            .descendants()
            .find(|n| n.kind() == K::Name && n.text().to_string().contains('.'))
            .expect("a dotted tag name");
        assert_eq!(name.text().to_string(), "store.Card");
    }

    #[test]
    fn an_unclosed_element_still_produces_a_well_formed_tree() {
        // Recovery that left nodes open would corrupt every ancestor's text
        // range, and the losslessness suite would then fail for a reason that
        // has nothing to do with the missing tag.
        let src = "view V() !{} {\n    <main><p>oops\n}\n";
        let p = parse_tree(src);
        assert_eq!(tree_text(&p.green), src, "still lossless");
        assert!(
            p.green
                .descendants()
                .filter(|n| n.kind() == K::Element)
                .count()
                >= 1,
            "the elements that did open must still be nodes"
        );
    }

    #[test]
    fn a_visibility_keyword_belongs_to_the_declaration_it_precedes() {
        // Permanent fixture, at the architect's request. `private type X`
        // parsed as TWO declarations — a bare word, then an unqualified type —
        // so the type came out with NO visibility and any module could import
        // it. That is not merely a parser bug: it silently widened a
        // module boundary, which is exactly the kind of claim the project makes.
        for src in [
            "private type Secretive = Secretive { y: Int }\n",
            "private type Alias = String\n",
            "public opaque type Id = String\n",
            "session fn f() -> Int !{} { 1 }\n",
            "private let x = 1\n",
        ] {
            let p = parse_ok(src);
            assert_lossless(src, &p);

            let decls: Vec<K> = p
                .green
                .children()
                .map(|c| c.kind())
                .filter(|k| *k != K::Whitespace)
                .collect();
            // ONE declaration, not two. Which flavour of declaration it is is
            // not what this test is about — that the visibility belongs to it
            // is.
            assert_eq!(
                decls.len(),
                1,
                "{src:?} must be ONE declaration, got {decls:?}"
            );
            assert!(
                is_decl_kind(decls[0]),
                "{src:?} produced {:?}, not a declaration",
                decls[0]
            );

            // ...and the visibility must be INSIDE it, or lowering cannot see it.
            let first = p
                .green
                .first_child()
                .expect("a declaration")
                .descendants_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| !t.kind().is_trivia())
                .expect("a first token");
            assert!(
                matches!(first.text(), "private" | "public" | "session"),
                "{src:?}: the declaration's first token is {:?}",
                first.text()
            );
        }

        // Control: a bare visibility word with no declaration after it must not
        // silently swallow whatever follows.
        let stray = parse_tree("private\n\ntype T = T { x: Int }\n");
        assert_eq!(
            tree_text(&stray.green),
            "private\n\ntype T = T { x: Int }\n"
        );
    }

    #[test]
    fn an_opaque_type_states_its_invariant_after_its_representation() {
        // ADR-0179: `where` and a predicate, inside the declaration, after
        // the representation. The predicate is an expression the checker
        // reads as bounds.
        let src = "opaque type Percent = Int where value >= 0 & value <= 100\n\
                   opaque type Plain = Int\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let clause = first_text(&p, K::Invariant).expect("an invariant");
        assert_eq!(clause.trim(), "where value >= 0 & value <= 100");
        assert_eq!(texts(&p, K::Invariant).len(), 1, "the second declares none");
        let decl = p
            .green
            .descendants()
            .find(|n| n.kind() == K::OpaqueDecl)
            .expect("an opaque type");
        assert!(
            kinds(&decl).contains(&K::Invariant),
            "inside its declaration"
        );
        assert!(kinds(&decl).contains(&K::BinaryExpr));
    }

    #[test]
    fn compound_comparisons_are_one_operator() {
        // Before `Cmp`, `opens >= closes` lexed as `>` then `=` and parsed as
        // `opens > (= closes)` — an error three tokens later.
        let src = "fn f() { if opens >= closes { 1 } else { 2 } }";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let bin = first_text(&p, K::BinaryExpr).expect("a comparison");
        assert_eq!(bin.trim(), "opens >= closes");

        // Negative control: `>` and `=` written apart must NOT join.
        let split = parse_tree("fn f() { a > = b }");
        assert!(!split.ok(), "`> =` must not lex as `>=`");
    }

    #[test]
    fn a_multi_line_string_is_a_single_token() {
        let src = "fn f() {\n    unsafe capability g\n        because \"\"\"one\n                  two\"\"\"\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let strs: Vec<_> = p
            .green
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == K::Str)
            .collect();
        assert_eq!(strs.len(), 1, "expected one string token, got {strs:?}");
        assert!(strs[0].text().contains('\n'), "it must span lines");

        // Negative control: a single-quoted string still ends at the newline,
        // which is the recovery property the triple-quoted form exists to keep.
        let one = parse_tree("fn f() { let s = \"unclosed\n }");
        assert!(
            one.green
                .descendants_with_tokens()
                .filter_map(|e| e.into_token())
                .any(|t| t.kind() == K::UnterminatedStr),
            "a single-quoted string must not swallow the newline"
        );
    }

    /// **Markup text is text** (ADR-0167): `//` and `/*` in it begin no
    /// comment. Until 2026-10-03 the lexer read `//` as one, which ran to the
    /// line's end, over the closing tag: `<p>http://example.com</p>` did not
    /// parse.
    #[test]
    fn a_slash_slash_in_markup_text_is_text() {
        for (markup, text) in [
            ("<p>http://example.com</p>", "http://example.com"),
            ("<p>a // b</p>", "a // b"),
            ("<p>a /* b */ c</p>", "a /* b */ c"),
            ("<p>// shown</p>", "// shown"),
            ("<p>// shown\n</p>", "// shown\n"),
        ] {
            let src = format!("view V() !{{}} {{\n    <main>{markup}<p>next</p></main>\n}}\n");
            let p = parse_ok(&src);
            assert_lossless(&src, &p);
            let paragraphs = texts(&p, K::Element);
            assert_eq!(
                paragraphs[1..],
                [markup.to_string(), "<p>next</p>".to_string()],
                "{markup}"
            );
            assert_eq!(texts(&p, K::Text)[0], text, "{markup}");
        }
        // Control: in code, `//` still begins a comment.
        let src = "view V() !{} {\n    <main><p>{f(1) // the one\n}</p></main>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        assert!(texts(&p, K::Text).iter().all(|t| !t.contains("the one")));
        assert!(
            p.green
                .descendants_with_tokens()
                .any(|t| t.kind() == K::LineComment && t.to_string() == "// the one")
        );
    }

    /// **`<!-- … -->` is a comment in markup** (ADR-0167): one token, which
    /// no node of the tree's markup holds. Until 2026-10-03 it was read as an
    /// element with no name, and broke what followed it.
    #[test]
    fn an_html_comment_in_markup_is_a_comment() {
        let src = "view V() !{} {\n    <ul><!-- a // <li> { here -->\n<li>one</li></ul>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let ul = p
            .green
            .descendants()
            .find(|n| n.kind() == K::Element)
            .expect("the list");
        let items: Vec<String> = ul
            .children()
            .filter(|c| c.kind() == K::Element)
            .map(|c| c.text().to_string())
            .collect();
        assert_eq!(items, ["<li>one</li>"]);
        let comments: Vec<String> = ul
            .children_with_tokens()
            .filter(|c| c.kind() == K::MarkupComment)
            .map(|c| c.to_string())
            .collect();
        assert_eq!(comments, ["<!-- a // <li> { here -->"]);
        // One that is not closed runs to the end, and says so.
        let src = "view V() !{} {\n    <ul><!-- open\n<li>one</li></ul>\n}\n";
        let p = parse_tree(src);
        assert_lossless(src, &p);
        assert!(
            p.errors
                .iter()
                .any(|e| e.code == "PW0006" && e.message == "a comment in markup is not closed"),
            "{:?}",
            p.errors
        );
    }

    /// **An unquoted attribute value is read as HTML reads it** (ADR-0167):
    /// up to a space, `>` or `/>`. Until 2026-10-03 it was read as code, and
    /// `href=http://x.y` built `href="http"`, its `//` a comment that took the
    /// rest of the line, the element after it included.
    #[test]
    fn an_unquoted_attribute_value_is_read_as_html_reads_it() {
        for (tag, values) in [
            ("<a href=http://x.y>y</a>", &["http://x.y"][..]),
            ("<a href=//cdn.x/y.js>y</a>", &["//cdn.x/y.js"][..]),
            ("<a href = /stores/48>y</a>", &["/stores/48"][..]),
            ("<input type=text aria-label=Name>", &["text", "Name"][..]),
            ("<img src=x.png alt=x/>", &["x.png", "x"][..]),
            ("<a href=\"/a\" title=b>y</a>", &["\"/a\"", "b"][..]),
        ] {
            let src = format!("view V() !{{}} {{\n    <main>{tag}<p>next</p></main>\n}}\n");
            let p = parse_ok(&src);
            assert_lossless(&src, &p);
            assert_eq!(texts(&p, K::AttrValue), values, "{tag}");
            // And the element after it is whole.
            assert!(
                texts(&p, K::Element).contains(&"<p>next</p>".to_string()),
                "{tag}"
            );
        }
    }

    #[test]
    fn markup_on_the_next_line_is_not_a_comparison() {
        // The bug this prevents is silent: `f(id)` and `<main>` parse as one
        // `<` comparison, and the error lands on `</main>` two lines later.
        let src = "view V() !{} {\n    let store = fetch(id)\n    <main><h1>{store.name}</h1></main>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        assert!(
            first_text(&p, K::TemplateRegion)
                .is_some_and(|t| t.starts_with("<main>") && t.ends_with("</main>")),
            "the markup must be its own region: {:?}",
            first_text(&p, K::TemplateRegion)
        );
        let comparisons: Vec<_> = p
            .green
            .descendants()
            .filter(|n| n.kind() == K::BinaryExpr)
            .map(|n| n.text().to_string())
            .collect();
        assert!(
            comparisons.is_empty(),
            "no comparison here: {comparisons:?}"
        );

        // Positive control: on the SAME line `<` is still a comparison.
        let cmp = parse_ok("fn f() { a < b }");
        assert_eq!(first_text(&cmp, K::BinaryExpr).unwrap().trim(), "a < b");
    }

    #[test]
    fn markup_written_on_one_line_after_a_root_is_one_region() {
        // Two roots on one line, as HTML writes them: until 2026-10-09 the
        // second `<` was a comparison with the first, PW0009 at its `/`.
        for line in [
            "<h1>a</h1><p>b</p>",
            "<h1>a</h1><hr />",
            "<hr /><hr />",
            "<p>a</p><!-- note --><p>b</p>",
            "<p>a</p>{#if x}<b>y</b>{/if}",
            "{#if x}<b>y</b>{/if}<p>a</p>",
        ] {
            let src = format!("view V() !{{}} {{\n    {line}\n}}\n");
            let p = parse_ok(&src);
            assert_lossless(&src, &p);
            assert_eq!(texts(&p, K::TemplateRegion), [line], "{line}");
            let found = p
                .green
                .descendants()
                .filter(|n| n.kind() == K::Element)
                .count();
            assert_eq!(found, 2, "{line}: each element its own");
            assert!(
                !kinds(&p.green).contains(&K::BinaryExpr),
                "{line}: no comparison"
            );
        }
        // The space between two on one line is text, as inside an element.
        let src = "view V() !{} {\n    <span>a</span> <span>b</span>\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let region = p
            .green
            .descendants()
            .find(|n| n.kind() == K::TemplateRegion)
            .expect("a region");
        let spaces: Vec<String> = region
            .children()
            .filter(|n| n.kind() == K::Text)
            .map(|n| n.text().to_string())
            .collect();
        assert_eq!(spaces, [" "], "the space between the two roots");
        // Controls: on the next line, a second region, as before; and what
        // follows markup on its line and is no markup is left to the
        // expression it is, an operator here, as before.
        let two = parse_ok("view V() !{} {\n    <h1>a</h1>\n    <p>b</p>\n}\n");
        assert_eq!(texts(&two, K::TemplateRegion), ["<h1>a</h1>", "<p>b</p>"]);
        let sum = parse_ok("view V() !{} {\n    <p>a</p> + 1\n}\n");
        assert_eq!(texts(&sum, K::TemplateRegion), ["<p>a</p>"]);
        assert!(kinds(&sum.green).contains(&K::BinaryExpr));
    }

    #[test]
    fn a_template_region_balances_over_block_markers() {
        let src = "view V() !{} {\n    {#each items as i (i.id)}<li>{i.name}</li>{/each}\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let region = first_text(&p, K::TemplateRegion).expect("a region");
        assert!(
            region.contains("{/each}"),
            "the region must reach its closing marker, got {region:?}"
        );
        // The keyed expression inside `(i.id)` is real syntax, not skipped text.
        assert!(
            p.green
                .descendants()
                .any(|n| n.kind() == K::FieldExpr && n.text() == "i.name"),
            "interpolated expressions must be parsed, not consumed as tokens"
        );
    }

    #[test]
    fn a_qualified_keyword_statement_owns_its_block() {
        let src = "fn f() {\n    acquire { unsafe.imperative { Sdk.mount(self) } }\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let stmt = p
            .green
            .descendants()
            .find(|n| n.kind() == K::LetStmt && n.text().to_string().starts_with("unsafe."))
            .expect("the unsafe statement");
        assert!(
            stmt.descendants().any(|n| n.kind() == K::BlockExpr),
            "its block must be nested inside it, not a sibling"
        );
        assert!(stmt.text().to_string().contains("Sdk.mount"));
    }

    #[test]
    fn a_property_transition_is_a_named_field_holding_a_chain() {
        let src = "fn f() {\n    animate pulse {\n        transform: scale(1.0) -> scale(1.08) -> scale(1.0)\n    }\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let field = p
            .green
            .descendants()
            .find(|n| n.kind() == K::Field)
            .expect("a property binding");
        assert!(field.text().to_string().starts_with("transform:"));
        // Left-associative: the outer step covers the whole chain.
        let chain = field
            .descendants()
            .find(|n| n.kind() == K::BinaryExpr)
            .expect("a transition chain");
        assert_eq!(
            chain.text().to_string().trim(),
            "scale(1.0) -> scale(1.08) -> scale(1.0)"
        );
    }

    #[test]
    fn a_nested_fn_is_a_declaration_not_a_lambda() {
        let src = "component C() {\n    placement browser\n    fn load() -> Store !{ database.read<Stores> } {\n        Stores.get(id)\n    }\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let f = p
            .green
            .descendants()
            .find(|n| n.kind() == K::FnDecl)
            .expect("a nested fn declaration");
        // The effect row must belong to the nested fn, with its type argument
        // intact — this is the row an effect checker will read.
        let row = f
            .descendants()
            .find(|n| n.kind() == K::EffectRow)
            .expect("its effect row");
        assert_eq!(row.text().to_string().trim(), "!{ database.read<Stores> }");
        assert!(
            row.descendants().any(|n| n.kind() == K::TypeArgList),
            "`<Stores>` must parse as a type argument list"
        );
        // Negative control: an unnamed `fn(` is still a lambda.
        let l = parse_ok("fn g() { h(fn(x: Int) x + 1) }");
        assert!(l.green.descendants().any(|n| n.kind() == K::LambdaExpr));
    }

    #[test]
    fn a_function_with_a_body_produces_expression_nodes() {
        let src = "fn f(x: Int) -> Int !{} {\n    g(x, 1) + 2\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let ks = kinds(&p.green);
        assert!(ks.contains(&K::FnDecl), "{ks:?}");
        assert!(ks.contains(&K::EffectRow), "{ks:?}");
        assert!(ks.contains(&K::BlockExpr), "{ks:?}");
        assert!(ks.contains(&K::CallExpr), "{ks:?}");
        assert!(ks.contains(&K::BinaryExpr), "{ks:?}");
    }

    #[test]
    fn operator_precedence_is_real_not_approximated() {
        // `a + b * c` must nest as `a + (b * c)`. An approximation that got this
        // wrong would still produce a tree that "parsed".
        let src = "fn f() { a + b * c }";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let outer = p
            .green
            .descendants()
            .find(|n| n.kind() == K::BinaryExpr)
            .expect("a binary expression");
        // The outer operator is `+`, and its right operand is another BinaryExpr.
        assert!(outer.text().to_string().contains('+'));
        let inner = outer
            .descendants()
            .filter(|n| n.kind() == K::BinaryExpr)
            .nth(1)
            .expect("a nested binary expression");
        assert_eq!(inner.text().to_string().trim(), "b * c");
    }

    #[test]
    fn a_lambda_inside_a_call_is_parsed_as_a_lambda() {
        // R-037's shape: the effect travels through a callback. If the lambda
        // body were misassociated, an effect checker would look at the wrong
        // expression and still report something plausible.
        let src = "fn f() { List.map(items, item => database.read(item.id)) }";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let lambda = p
            .green
            .descendants()
            .find(|n| n.kind() == K::LambdaExpr)
            .expect("a lambda");
        let body = lambda.text().to_string();
        assert!(body.contains("database.read"), "lambda body: {body}");
        // The lambda must be INSIDE the argument list, not a sibling of the call.
        assert!(
            lambda.ancestors().any(|a| a.kind() == K::ArgList),
            "lambda is not inside the call's arguments"
        );
    }

    #[test]
    fn match_arms_and_patterns_are_structured() {
        let src = "fn f(s: OrderState) -> String {\n    match s {\n        Draft => \"d\",\n        Confirmed(c) => c,\n        _ => \"other\",\n    }\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let ks = kinds(&p.green);
        assert!(ks.contains(&K::MatchExpr), "{ks:?}");
        assert_eq!(
            p.green
                .descendants()
                .filter(|n| n.kind() == K::MatchArm)
                .count(),
            3
        );
        assert!(ks.contains(&K::CtorPat), "{ks:?}");
        assert!(ks.contains(&K::WildcardPat), "{ks:?}");
    }

    #[test]
    fn a_return_in_an_arm_keeps_the_value_on_its_line() {
        // Until 2026-09-25 this was FOUR arms: the third was `Ok(())`, a
        // pattern with no `=>` and no body, and nothing reported it.
        let src = "fn f(s: S) -> Int {\n    match s {\n        A => 1\n        B(r) => return Ok(())\n        C(x) => return (x)\n        D => return\n        E => 2\n    }\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let arms: Vec<_> = p
            .green
            .descendants()
            .filter(|n| n.kind() == K::MatchArm)
            .collect();
        assert_eq!(arms.len(), 5, "{arms:#?}");
        // `B(r)`'s arm holds `return` and `Ok(())`; `C(x)`'s holds `return`
        // and `(x)`, not a call of `return`; a bare `return` holds itself.
        let bodies: Vec<usize> = arms.iter().map(|a| a.children().count() - 1).collect();
        assert_eq!(bodies, [1, 2, 2, 1, 1]);
    }

    #[test]
    fn return_is_a_statement_not_an_operand() {
        // Two statements each: `return`, then the value. Parsed as a name,
        // `return (x)` was a call and `return -1` a subtraction.
        let src =
            "fn f(x: Int) -> Int {\n    return (x)\n}\nfn g(x: Int) -> Int {\n    return -1\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let ks = kinds(&p.green);
        assert!(!ks.contains(&K::CallExpr), "{ks:?}");
        assert!(!ks.contains(&K::BinaryExpr), "{ks:?}");
        let returns = p
            .green
            .descendants()
            .filter(|n| n.kind() == K::NameExpr && n.text().to_string().trim() == "return")
            .count();
        assert_eq!(returns, 2);
    }

    #[test]
    fn a_statement_keyword_cannot_name_a_value() {
        // Every word that begins a statement or an expression in a body
        // (ADR-0195, ruling 3), wherever a name is bound: a `let`, a
        // parameter, a loop's binding, a pattern's. `let return = n` checked
        // until then, and its next use read as a `return`.
        for (word, src) in [
            (
                "query",
                "fn f() -> Int {\n    let query = 1\n    query\n}\n",
            ),
            ("query", "fn f(query: Int) -> Int {\n    query\n}\n"),
            (
                "return",
                "fn f(n: Int) -> Int {\n    let return = n\n    n\n}\n",
            ),
            (
                "match",
                "fn f(n: Int) -> Int {\n    let match = n\n    n\n}\n",
            ),
            ("if", "fn f(if: Int) -> Int {\n    0\n}\n"),
            (
                "frame",
                "fn f(xs: List<Int>) -> Int {\n    for frame in xs {\n    }\n    0\n}\n",
            ),
            (
                "use",
                "fn f(o: Option<Int>) -> Int {\n    match o {\n        Some(use) => 1,\n        None => 0,\n    }\n}\n",
            ),
            (
                "signal",
                "fn f(n: Int) -> Int {\n    let signal = n\n    n\n}\n",
            ),
        ] {
            let p = parse_tree(src);
            let found = p.errors.iter().find(|e| e.code == "PW0013");
            assert!(
                found.is_some_and(|e| e
                    .message
                    .contains(&format!("`{word}` begins a statement or an expression"))
                    && e.help.as_deref()
                        == Some(&*format!("name it `{word}_`, or after what it holds"))),
                "{src:?}: {:?}",
                p.errors
            );
        }
        // A declaration is used by a call, and a call by a statement word
        // reads as one: the platform's `query`, `measure` and `mutate`. An
        // expression word's does not.
        parse_ok("fn query(selector: String) -> Int {\n    0\n}\n");
        let p = parse_tree("fn return(n: Int) -> Int {\n    n\n}\n");
        assert!(
            p.errors.iter().any(|e| e.code == "PW0013"),
            "{:?}",
            p.errors
        );
        // Every other keyword names what the program likes; `true` and
        // `false` in a pattern are literals.
        parse_ok("fn f(term: Int) -> Int {\n    let queried = term\n    queried\n}\n");
        parse_ok(
            "fn f(session: Int, page: Int, cache: Int) -> Int {\n    let view = session\n    view\n}\n",
        );
        parse_ok(
            "fn f(b: Bool) -> Int {\n    match b {\n        true => 1,\n        false => 0,\n    }\n}\n",
        );
    }

    #[test]
    fn doubled_logical_operators_name_the_pleris_spelling() {
        for expression in ["a && b", "a || b", "a&&b", "a||b", "&& b", "|| b"] {
            let parsed = parse_expr(expression, "the expression");
            assert_lossless(expression, &parsed);
            assert!(
                parsed.errors.iter().any(|e| e.code == "PW0009"
                    && e.help.as_deref()
                        == Some("Pleris writes `&` for and and `|` for or; write one operator")),
                "{expression}: {:?}",
                parsed.errors
            );
        }
        // No textual ban: the valid spellings, strings, markup and comments
        // retain their own meaning. Separated stray operators get no advice
        // about a doubled token the programmer did not write.
        for expression in [
            "a & b",
            "a | b",
            "(a & b) | c",
            "\"&& ||\"",
            "<p>&& ||</p>",
            "a & // && ||\nb",
        ] {
            let parsed = parse_expr(expression, "the expression");
            assert_lossless(expression, &parsed);
            assert!(parsed.ok(), "{expression}: {:?}", parsed.errors);
            assert!(
                !parsed.errors.iter().any(|e| e
                    .help
                    .as_deref()
                    .is_some_and(|h| h.contains("Pleris writes"))),
                "{expression}: {:?}",
                parsed.errors
            );
        }
        for expression in ["a & & b", "a | | b", "a &\n& b", "a | // comment\n| b"] {
            let parsed = parse_expr(expression, "the expression");
            assert_lossless(expression, &parsed);
            assert!(!parsed.ok());
            assert!(
                !parsed.errors.iter().any(|e| e
                    .help
                    .as_deref()
                    .is_some_and(|h| h.contains("Pleris writes"))),
                "{expression}: {:?}",
                parsed.errors
            );
        }
    }

    #[test]
    fn or_and_and_mixed_without_parentheses_is_refused() {
        // ADR-0322: `|` and `&` have no precedence between them. Until
        // 2026-10-12 they were one power, left to right, and `a | b & c` was
        // `(a | b) & c` (W9's finding). Refused once a chain, where the mix
        // begins.
        let query = |body: &str| {
            format!(
                "public query Q(a: Bool, b: Bool, c: Bool, d: Bool, e: Bool) -> Bool {{ {body} }}\n"
            )
        };
        for (body, mix) in [
            ("a | b & c", " & "),
            ("a & b | c", " | "),
            ("a == b | c > d & e", " & "),
            ("a | b | c & d", " & "),
            ("a | b & c | d", " & "),
            ("a | b & c & d", " & "),
        ] {
            let src = query(body);
            let p = parse_tree(&src);
            assert_lossless(&src, &p);
            let found: Vec<_> = p.errors.iter().map(|e| (e.code, e.span.start)).collect();
            let at = src.find(mix).expect("the mix") + 1;
            assert_eq!(found, vec![("PW0031", at)], "{body}");
        }
        // Each grouping written out, a chain of one, a comparison inside one,
        // and a lambda's body of its own.
        for body in [
            "(a | b) & c",
            "a | (b & c)",
            "a | b | c",
            "a & b & c",
            "!(a & b) | c",
            "a == b | c",
            "List.any([a], x => x & b) | c",
        ] {
            let src = query(body);
            let p = parse_ok(&src);
            assert_lossless(&src, &p);
        }
    }

    #[test]
    fn a_user_declaration_after_one_with_no_body_is_its_own() {
        // ADR-0324: `user` before a declaration's keyword begins the
        // declaration, after a function with no body too. Until 2026-10-12 it
        // was that function's unknown policy `user`, and the query it opened
        // the `host` clause's value (the feed, once its queries said `user`).
        let src = "fn owner(post: Int) -> Int !{}\n    host \"feed:data/posts#author\"\n\n\
                   // the reader's\nuser query Mine(reader: Int) -> Int\n    freshness 0.seconds\n\
                   {\n    reader\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let decls: Vec<_> = p.green.children().map(|n| n.kind()).collect();
        assert_eq!(decls.len(), 2, "two declarations: {decls:?}");
        // Control: the same after `private`, which is reserved.
        let src = src.replace("user query", "private query");
        parse_ok(&src);
    }

    #[test]
    fn a_lambdas_parameters_may_be_a_parenthesised_list() {
        // `(a, b) => a + b`: a list in parentheses, then `=>`. Until
        // 2026-09-25 the parser read one expression inside parentheses, so
        // the comma was an unclosed parenthesis and this never parsed, though
        // the lambda grammar below described it.
        let src =
            "fn f(xs: List<Int>) -> Int {\n    List.fold(xs, 0, (total, x) => total + x)\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let lambda = p
            .green
            .descendants()
            .find(|n| n.kind() == K::LambdaExpr)
            .expect("a lambda");
        assert!(
            lambda.children().any(|c| c.kind() == K::TupleExpr),
            "{lambda:#?}"
        );
        // And nowhere else: the language has no tuple value.
        let p = parse_tree("fn f() -> Int {\n    let pair = (1, 2)\n    3\n}\n");
        assert!(
            p.errors
                .iter()
                .any(|e| e.message.contains("a lambda's parameters")),
            "{:?}",
            p.errors
        );
        // A single parenthesised expression is still just that.
        parse_ok("fn f(a: Int) -> Int {\n    (a + 1) * 2\n}\n");
    }

    #[test]
    fn a_query_body_may_begin_with_an_operand() {
        // A query's body is where its policies live, so a bare name at the
        // start of a line could be a policy's head. One followed by an
        // operator is an operand: until 2026-09-25 `{ a + b }` was an unknown
        // policy `a`, and the body had no statement.
        for body in [
            "a + b",
            "a * b - a / b",
            "a % b",
            "a == b",
            "b != 0 & a > 1",
            "b == 0 | a > b",
            "a > b",
            "a |> f",
        ] {
            let src = format!("public query Q(a: Int, b: Int) -> Int {{ {body} }}\n");
            let p = parse_ok(&src);
            assert_lossless(&src, &p);
        }
        // `items[0]` is no operand: the grammar has no index, and this was
        // `items` and then a list, `[0]`, the query's value, and parsed. Two
        // statements on one line, refused (ADR-0243); and not an unknown
        // policy `items`, since a `[` follows it.
        let src = "public query Q(items: List<Int>) -> Int { items[0] }\n";
        let p = parse_tree(src);
        assert_lossless(src, &p);
        let found: Vec<_> = p.errors.iter().map(|e| (e.code, e.span.start)).collect();
        assert_eq!(found, vec![("PW0030", src.find('[').expect("["))]);
        // A real unknown policy is still one.
        let p = parse_tree("public query Q(a: Int) -> Int {\n    cahce shared\n    a\n}\n");
        assert!(
            p.errors
                .iter()
                .any(|e| e.message.contains("unknown policy `cahce`")),
            "{:?}",
            p.errors
        );
    }

    #[test]
    fn an_arm_without_an_arrow_is_reported() {
        let p = parse_tree(
            "fn f(s: S) -> Int {\n    match s {\n        A => 1\n        B 2\n    }\n}\n",
        );
        assert!(
            p.errors
                .iter()
                .any(|e| e.message.contains("after a match arm's pattern")),
            "{:?}",
            p.errors
        );
    }

    #[test]
    fn unclosed_brackets_are_reported_not_swallowed() {
        // Before `expect` was wired in, each of these parsed silently: the
        // closer was consumed with a bare `eat` whose false was discarded.
        for (src, want) in [
            ("fn f() { [1, 2 }", "to close a list literal"),
            ("view v() { <p>{name</p> }", "to close an interpolation"),
        ] {
            let p = parse_tree(src);
            assert!(
                p.errors.iter().any(|e| e.message.contains(want)),
                "{src:?} should report {want:?}, got {:?}",
                p.errors
            );
            assert_lossless(src, &p);
        }
    }

    #[test]
    fn field_access_chains_left_to_right() {
        let src = "fn f() { a.b.c(1).d }";
        let p = parse_ok(src);
        assert_lossless(src, &p);

        // Asserting only that the node KINDS exist would pass even when each
        // wraps nothing — which is exactly the bug the precedence test caught.
        // So assert the nesting: the outermost postfix step must cover the
        // whole chain, and each inner one a strict prefix of it.
        let outer = p
            .green
            .descendants()
            .find(|n| matches!(n.kind(), K::FieldExpr | K::CallExpr))
            .expect("a postfix expression");
        assert_eq!(outer.text().to_string().trim(), "a.b.c(1).d");
        let inner: Vec<String> = outer
            .descendants()
            .filter(|n| matches!(n.kind(), K::FieldExpr | K::CallExpr))
            .map(|n| n.text().to_string())
            .collect();
        // Every prefix of the chain is its own node: `.c` and `.d` must have
        // the same shape even though a call intervenes between them.
        for want in ["a.b.c(1)", "a.b.c", "a.b"] {
            assert!(
                inner.contains(&want.to_string()),
                "missing {want:?} in {inner:?}"
            );
        }
    }

    /// **A table and its clauses** (track `tables`, ADR-0338): one
    /// declaration, each clause a policy of it, and the word heads clauses
    /// only where their value follows on the line.
    #[test]
    fn a_table_declaration_reads_each_clause() {
        let src = "module Todos\n\ntable Todos\n    row     Todo\n    key     owner, id\n    \
                   order   added\n    index   by_list on list\n    unique  by_slug on slug\n    \
                   refers  owner to Users\n    bounded 500\n    \
                   migrate title renamed from name\n\nfn after() -> Int !{} {\n    1\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let heads: Vec<String> = texts(&p, K::Policy)
            .iter()
            .map(|t| t.split_whitespace().next().unwrap_or_default().to_string())
            .collect();
        assert_eq!(
            heads,
            [
                "row", "key", "order", "index", "unique", "refers", "bounded", "migrate"
            ],
            "{:?}",
            texts(&p, K::Policy)
        );
        assert_eq!(
            p.green
                .descendants()
                .filter(|n| n.kind() == K::ResourceDecl)
                .count(),
            1,
            "one table"
        );
        assert_eq!(
            p.green
                .descendants()
                .filter(|n| n.kind() == K::FnDecl)
                .count(),
            1,
            "the function after it is a declaration of its own"
        );
    }

    /// **A table's ownership clauses** (track `row-ownership`, ADR-0384
    /// H1): `owned by`, `shares with` and `private` each head a clause of a
    /// table, a field named `user` among a private clause's; and `private`
    /// is still a declaration's visibility wherever a declaration's word
    /// follows it.
    #[test]
    fn a_tables_ownership_clauses_beside_privates_visibility() {
        let src = "module Notes\n\ntable Notes\n    row         Note\n    key         \
                   author, id\n    owned by    author\n    shares with to\n    private     \
                   draft, user\n\nprivate fn after() -> Int !{} {\n    1\n}\n\nprivate query \
                   Q() -> Int {\n    1\n}\n\ntable Users\n    row     User\n    key     id\n    \
                   owned by id\n    private user\n\nprivate type T = T { a: Int }\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        let heads: Vec<String> = texts(&p, K::Policy)
            .iter()
            .map(|t| t.split_whitespace().next().unwrap_or_default().to_string())
            .collect();
        assert_eq!(
            heads,
            [
                "row", "key", "owned", "shares", "private", "row", "key", "owned", "private"
            ],
            "{:?}",
            texts(&p, K::Policy)
        );
        for (kind, want) in [(K::FnDecl, 1), (K::ResourceDecl, 3), (K::RecordDecl, 1)] {
            assert_eq!(
                p.green.descendants().filter(|n| n.kind() == kind).count(),
                want,
                "{kind:?}"
            );
        }
    }

    /// The control: `owned` and `shares` are names a body may begin with, a
    /// clause only where a name follows on their line.
    #[test]
    fn owned_and_shares_are_still_names_in_a_policy_block() {
        for (head, rest) in [("owned", ".count"), ("shares", " + 1"), ("owned", "")] {
            let src = format!(
                "materialize M() {{\n    regenerate on_invalidation\n    {head}{rest}\n}}\n"
            );
            let p = parse_tree(&src);
            assert_lossless(&src, &p);
            assert!(
                !texts(&p, K::Policy).iter().any(|t| t.starts_with(head)),
                "`{head}{rest}` read as a clause: {:?}",
                texts(&p, K::Policy)
            );
        }
    }

    /// The control: a table's word heads are names a body may begin with,
    /// a clause only where a name (or, for `bounded`, a count) follows on
    /// its line (`NAMED_HEADS`, `COUNTED_HEADS`).
    #[test]
    fn a_tables_words_are_still_names_in_a_policy_block() {
        for (head, rest) in [
            ("row", ".word"),
            ("order", ".total"),
            ("index", " + 1"),
            ("unique", ""),
            ("bounded", ".count"),
        ] {
            let src = format!(
                "materialize M() {{\n    regenerate on_invalidation\n    {head}{rest}\n}}\n"
            );
            let p = parse_tree(&src);
            assert_lossless(&src, &p);
            assert!(
                !texts(&p, K::Policy).iter().any(|t| t.starts_with(head)),
                "`{head}{rest}` read as a clause: {:?}",
                texts(&p, K::Policy)
            );
        }
        // And each is a clause where its value follows.
        for clause in ["row Todo", "order added", "bounded 5"] {
            let src = format!("materialize M() {{\n    {clause}\n}}\n");
            let p = parse_tree(&src);
            assert!(
                texts(&p, K::Policy).iter().any(|t| t.trim() == clause),
                "`{clause}` is no clause: {:?}",
                texts(&p, K::Policy)
            );
        }
    }

    #[test]
    fn declarations_still_parse_as_they_did() {
        for src in [
            "module store.pricing\n",
            "opaque type StoreId = String\n",
            "type OrderState =\n    | Draft\n    | Confirmed(OrderConfirmation)\n",
            "public query Store(id: StoreId) -> Store\n    freshness 30.seconds\n    cache shared\n{\n    Stores.get(id)\n}\n",
            "view V(x: Int) !{} {\n    x\n}\n",
        ] {
            let p = parse_tree(src);
            assert!(p.ok(), "{src:?} -> {:?}", p.errors);
            assert_eq!(tree_text(&p.green), src, "not lossless: {src:?}");
        }
    }

    #[test]
    fn recovery_reports_more_than_one_error_and_keeps_later_declarations() {
        let src =
            "module m\n\n$$$\n\nfn good() -> Int !{} { 1 }\n\n%%%\n\nfn also() -> Int !{} { 2 }\n";
        let p = parse_tree(src);
        assert!(p.errors.len() >= 2, "{:?}", p.errors);
        assert_eq!(tree_text(&p.green), src, "recovery must stay lossless");
        assert_eq!(
            p.green
                .descendants()
                .filter(|n| n.kind() == K::FnDecl)
                .count(),
            2,
            "both good functions must survive recovery"
        );
    }

    #[test]
    fn malformed_input_never_panics_and_stays_lossless() {
        for src in [
            "",
            "{",
            "}",
            "((((",
            "fn",
            "fn f(",
            "fn f(x:",
            "type",
            "type T =",
            "opaque type",
            "public query",
            "!{",
            "fn f() -> Int !{ database.read<",
            "\"unterminated",
            "§§§",
            "fn f() { match",
            "fn f() { a => }",
            "let",
            "fn f() { [1, 2",
        ] {
            let p = parse_tree(src);
            assert_eq!(tree_text(&p.green), src, "not lossless: {src:?}");
        }
    }

    #[test]
    fn an_each_head_is_its_list_its_name_and_its_key() {
        // ADR-0242.
        let src = "menu.items as item (item.id)";
        let p = parse_each_head(src);
        assert_lossless(src, &p);
        assert!(p.ok(), "{:?}", p.errors);
        assert_eq!(first_text(&p, K::Name).as_deref(), Some("item"));
        assert_eq!(texts(&p, K::FieldExpr), ["menu.items", "item.id"]);
        // No key is a head too.
        assert!(parse_each_head("xs as x").ok());
        // Two words for a list: `as` was expected, the one error, and the
        // name after it read.
        let src = "xs ys as x (x)";
        let p = parse_each_head(src);
        assert_lossless(src, &p);
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0019");
        assert_eq!(&src[p.errors[0].span.clone()], "ys");
        assert_eq!(first_text(&p, K::Name).as_deref(), Some("x"));
        // An unclosed key.
        let p = parse_each_head("xs as x (x");
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(
            p.errors[0].message,
            "expected `)` to close the key, found the end of the `{#each}`"
        );
        // A second name, or an index, stands between the name and the key:
        // the one error, and the key read.
        let src = "xs as x, i (x)";
        let p = parse_each_head(src);
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0016");
        assert_eq!(&src[p.errors[0].span.clone()], ", i");
        assert_eq!(texts(&p, K::NameExpr).last().map(String::as_str), Some("x"));
    }

    #[test]
    fn a_let_may_discard_its_value() {
        // ADR-0250: `let _ = e` binds nothing, and parses.
        let src = "module m\nfn f() -> Int !{} {\n    let _ = g()\n    let _: Int = 1\n    0\n}\n";
        let p = parse_ok(src);
        assert_lossless(src, &p);
        assert_eq!(texts(&p, K::WildcardPat), vec!["_", "_"]);
        // The control: a name is a name.
        let named = parse_ok("module m\nfn f() -> Int !{} {\n    let x = g()\n    x\n}\n");
        assert!(texts(&named, K::WildcardPat).is_empty());
    }

    #[test]
    fn a_discard_is_neither_mutable_nor_a_use() {
        // ADR-0250: `let mut _` binds nothing to change, and `use _` nothing
        // to hold; each is one error, where it is written.
        for (src, at, says) in [
            (
                "module m\nfn f() -> Int !{} {\n    let mut _ = 1\n    0\n}\n",
                "_",
                "`let mut` binds a name to change, and `_` binds nothing",
            ),
            (
                "module m\nfn f() -> Int !{} {\n    use _ = g()\n    0\n}\n",
                "_",
                "`use` binds a name, and `_` binds nothing",
            ),
        ] {
            let p = parse_tree(src);
            assert_lossless(src, &p);
            assert_eq!(
                p.errors
                    .iter()
                    .map(|e| (e.code, &src[e.span.clone()], e.message.as_str()))
                    .collect::<Vec<_>>(),
                vec![("PW0001", at, says)],
                "{src}"
            );
        }
        // The controls: a name for each.
        parse_ok("module m\nfn f() -> Int !{} {\n    let mut x = 1\n    use h = g()\n    x\n}\n");
    }

    #[test]
    fn two_statements_on_one_line_are_separated() {
        // ADR-0243: by `;` or by a line.
        let f = |body: &str| format!("module m\nfn f(a: Int, b: Int) -> Int !{{}} {{ {body} }}\n");
        // Each is one error, at the second.
        for (body, second) in [
            ("a b", "b"),
            ("let x = a x", "x"),
            // A `return` takes one statement; a word's path is not the word.
            ("return a b", "b"),
            ("cache.a b", "b"),
            // After a block, the line takes nothing; a clause's value ends
            // with its line.
            ("acquire { a } b", "b"),
            ("release(a) { a } b", "b"),
            ("scope component\n    a b", "b"),
        ] {
            let src = f(body);
            let p = parse_tree(&src);
            assert_lossless(&src, &p);
            let found: Vec<_> = p.errors.iter().map(|e| (e.code, e.span.start)).collect();
            assert_eq!(
                found,
                vec![("PW0030", src.rfind(second).expect("second"))],
                "{body}"
            );
        }
        // The controls: a `;`, a `,`, a line, and a record's fields.
        for body in ["a; b", "a, b", "a\n    b", "R { a: 1, b: 2 }"] {
            assert!(parse_tree(&f(body)).ok(), "{body}");
        }
        // After a statement read with an error, and at a token no expression
        // begins with, the error is the parse's own: `g(a[0])` is an
        // argument list left open (PW0010) and a stray `)` (PW0009), and
        // never two statements.
        let src = f("g(a[0])");
        let p = parse_tree(&src);
        let codes: Vec<_> = p.errors.iter().map(|e| e.code).collect();
        assert!(!codes.contains(&"PW0030"), "{codes:?}");
        assert!(
            codes.contains(&"PW0010") && codes.contains(&"PW0009"),
            "{codes:?}"
        );
        // What a block's readers read as one: a `return` and its value, a
        // clause's head and the rest of its line, and a block after what
        // precedes it.
        for body in [
            "return Err(a)",
            "scope component",
            "respects prefers_reduced_motion",
            "impact layout_write when LayoutAffect",
            "view { a }",
            "acquire { a }",
            "release(a) { a }",
            "task.spawn(detached) { a }",
        ] {
            assert!(parse_tree(&f(body)).ok(), "{body}");
        }
    }

    #[test]
    fn the_grammar_can_accept_and_reject() {
        // docs/RISK_QUEUE.md negative control.
        assert!(parse_tree("module m\n").ok());
        assert!(!parse_tree("module m\n$$$\n").ok());
    }

    #[test]
    fn what_a_standalone_parse_leaves_is_one_error_over_it() {
        // ADR-0237: it was an error per token, and no one saw any.
        let src = "a b c";
        let p = parse_expr(src, "a hole");
        assert_lossless(src, &p);
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0016");
        assert_eq!(p.errors[0].message, "a hole is one expression");
        assert_eq!(&src[p.errors[0].span.clone()], "b c");
        // The control.
        assert!(parse_expr("a.b", "a hole").ok());
        // Its end is the hole's, not the file's.
        let p = parse_expr("a ==", "a hole");
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(
            p.errors[0].message,
            "expected an expression, found the end of a hole"
        );
    }

    #[test]
    fn a_value_with_no_comma_before_it_is_reported_and_read() {
        // ADR-0237, as rustc reads an omitted separator: two values, and one
        // error over the second.
        let src = "Liked(id) Posted(_)";
        let p = parse_expr_list(src);
        assert_lossless(src, &p);
        assert_eq!(texts(&p, K::CallExpr), ["Liked(id)", "Posted(_)"]);
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0016");
        assert_eq!(&src[p.errors[0].span.clone()], "Posted(_)");
        // The control.
        assert!(parse_expr_list("Liked(id), Posted(_)").ok());
        // What is not a value after a comma is that value's error alone.
        let p = parse_expr_list("Liked(id), )");
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0009");
        // What is not a value where a comma is missing is no value: the
        // comma is the error, over all that is left, as rustc reports it.
        let src = "Liked(id) ; ;";
        let p = parse_expr_list(src);
        assert_lossless(src, &p);
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0016");
        assert_eq!(&src[p.errors[0].span.clone()], "; ;");
        assert_eq!(p.errors[0].help, None);
    }

    #[test]
    fn an_optimistic_clause_has_an_arm_for_each_entry() {
        // ADR-0238: arms separated by commas, each a node of its own.
        let src =
            "Timeline(s, _) as feed => liked(feed, p), Thread(p) as thread => again(thread, p)";
        let p = parse_transition_clause(src);
        assert_lossless(src, &p);
        assert!(p.ok(), "{:?}", p.errors);
        assert_eq!(
            texts(&p, K::TransitionArm),
            [
                "Timeline(s, _) as feed => liked(feed, p)",
                "Thread(p) as thread => again(thread, p)"
            ]
        );
        // Without the comma: the comma is the one error, and the arm is read.
        let src =
            "Timeline(s, _) as feed => liked(feed, p) Thread(p) as thread => again(thread, p)";
        let p = parse_transition_clause(src);
        assert_lossless(src, &p);
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0016");
        assert_eq!(
            &src[p.errors[0].span.clone()],
            "Thread(p) as thread => again(thread, p)"
        );
        assert_eq!(texts(&p, K::TransitionArm).len(), 2);
        // An arm with no transition after its arrow keeps the comma after it.
        let p = parse_transition_clause("A(s) as a =>, B(s) as b => g(b)");
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].message, "expected an expression, found `,`");
        assert_eq!(texts(&p, K::TransitionArm).len(), 2);
    }

    #[test]
    fn an_optimistic_clause_without_its_arrow_reads_its_transition() {
        // ADR-0237: the arrow is the one error, and the transition is read.
        let src = "Cart(s) as cart add(cart)";
        let p = parse_transition_clause(src);
        assert_lossless(src, &p);
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0017");
        assert_eq!(texts(&p, K::CallExpr), ["Cart(s)", "add(cart)"]);
        // After the arrow, its absence is the error.
        let p = parse_transition_clause("Cart(s) as cart =>");
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(
            p.errors[0].message,
            "expected an expression, found the end of the clause"
        );
        // Text after the transition is the rest's error.
        let src = "Cart(s) as cart => add(cart) more";
        let p = parse_transition_clause(src);
        assert_lossless(src, &p);
        assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
        assert_eq!(p.errors[0].code, "PW0016");
        assert_eq!(&src[p.errors[0].span.clone()], "more");
    }
}
