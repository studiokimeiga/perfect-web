//! Syntax tree → HIR.
//!
//! **This is the only module in `pw-core` permitted to see a syntax node**
//! (ADR-0014 property 3, enforced by `hir::tests::only_lower_touches_syntax_nodes`).
//!
//! Lowering is total: anything unrecognised becomes [`Expr::Error`] carrying
//! its span, so one bad expression does not cost a checker the rest of the
//! body. It performs no name resolution and no meaning-changing desugaring —
//! see ADR-0014 for why `a.b` stays a field access and `a |> f` stays a
//! binary node.

use pw_syntax::kind::{SyntaxKind as K, SyntaxNode};

use crate::hir::*;

/// Lower a parsed source file.
pub fn lower_file(src: &str, root: &SyntaxNode) -> Hir {
    let mut lo = Lowerer {
        hir: Hir::default(),
        src,
    };
    let mut decls = Vec::new();
    let mut module_name = String::new();
    let mut module_span = 0..0;

    for node in root.children() {
        if node.kind() == K::ModuleDecl {
            module_name = first_name(&node).unwrap_or_default();
            module_span = span_of(&node);
            continue;
        }
        if let Some(id) = lo.decl(&node) {
            decls.push(id);
        }
    }

    let m = Module {
        name: module_name,
        decls,
    };
    lo.hir.modules.alloc(m, module_span);
    // A table's reads and writes are its module's declarations (track
    // `tables`, ADR-0338): written here, so every way a file is read has them.
    crate::uploads::generate(&mut lo.hir);
    crate::tables::generate(&mut lo.hir);
    lo.hir
}

struct Lowerer<'a> {
    hir: Hir,
    src: &'a str,
}

/// Per-body arenas, filled while walking one body (ADR-0014 property 1).
#[derive(Default)]
struct BodyBuilder {
    exprs: Arena<Expr>,
    pats: Arena<Pattern>,
    types: Arena<TypeRef>,
    nodes: Arena<Node>,
    signals: std::collections::BTreeSet<ExprId>,
    param_types: std::collections::BTreeMap<PatternId, TypeRefId>,
    bound: std::collections::BTreeSet<ExprId>,
    provides: std::collections::BTreeSet<ExprId>,
}

impl BodyBuilder {
    fn expr(&mut self, e: Expr, span: Span) -> ExprId {
        ExprId(self.exprs.alloc(e, span))
    }
    fn pat(&mut self, p: Pattern, span: Span) -> PatternId {
        PatternId(self.pats.alloc(p, span))
    }
    fn ty(&mut self, t: TypeRef, span: Span) -> TypeRefId {
        TypeRefId(self.types.alloc(t, span))
    }
    fn node(&mut self, n: Node, span: Span) -> NodeId {
        NodeId(self.nodes.alloc(n, span))
    }
}

fn span_of(n: &SyntaxNode) -> Span {
    let r = n.text_range();
    usize::from(r.start())..usize::from(r.end())
}

fn text(src: &str, n: &SyntaxNode) -> String {
    let s = span_of(n);
    src[s].to_string()
}

/// Is `n` a name, or a path of names: `Cart`, `store.page.Menu`?
fn names_a_path(n: &SyntaxNode) -> bool {
    match n.kind() {
        K::NameExpr => true,
        K::FieldExpr => n.children().next().is_some_and(|base| names_a_path(&base)),
        _ => false,
    }
}

/// The first `Name` node's text, which is how the grammar records identifiers.
fn first_name(n: &SyntaxNode) -> Option<String> {
    n.children()
        .find(|c| c.kind() == K::Name)
        .map(|c| c.text().to_string())
}

fn first_name_span(n: &SyntaxNode) -> Option<Span> {
    n.children()
        .find(|c| c.kind() == K::Name)
        .map(|c| span_of(&c))
}

/// Significant (non-trivia) tokens of a node, excluding those inside children.
fn own_tokens(n: &SyntaxNode) -> Vec<pw_syntax::SyntaxToken> {
    n.children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| !t.kind().is_trivia())
        .collect()
}

fn decl_kind_of(node: &SyntaxNode, src: &str) -> DeclKind {
    match node.kind() {
        K::FnDecl => DeclKind::Fn,
        // `signal drawer: Bool` at module level (ADR-0144) is a `let`'s node,
        // told apart by its first word, as a body's `signal` is.
        K::LetDecl
            if own_tokens(node)
                .first()
                .is_some_and(|t| t.text() == "signal") =>
        {
            DeclKind::Signal
        }
        K::LetDecl => DeclKind::Let,
        K::ImportDecl => DeclKind::Import,
        K::OpaqueDecl => DeclKind::Opaque,
        K::TypeDecl | K::RecordDecl | K::UnionDecl => DeclKind::Type,
        // `view`/`component`/`page` and the resource nouns are distinguished by
        // their leading keyword, which the grammar keeps as a bare token.
        K::EffectDecl => DeclKind::Effect,
        K::PreludeDecl => DeclKind::Prelude,

        K::UiDecl | K::ResourceDecl => {
            let kw = own_tokens(node)
                .iter()
                .map(|t| t.text().to_string())
                .find(|t| {
                    !matches!(
                        t.as_str(),
                        "public" | "session" | "private" | "user" | "internal" | "local"
                    )
                })
                .unwrap_or_default();
            match kw.as_str() {
                "view" => DeclKind::View,
                "component" => DeclKind::Component,
                "page" => DeclKind::Page,
                "layout" => DeclKind::Layout,
                "query" => DeclKind::Query,
                "command" => DeclKind::Command,
                "subscription" => DeclKind::Subscription,
                "resource" => DeclKind::Resource,
                "materialize" => DeclKind::Materialize,
                "event" => DeclKind::Event,
                "source" => DeclKind::Source,
                "table" => DeclKind::Table,
                "upload" => DeclKind::Upload,
                "predicate" => DeclKind::Predicate,
                "task" => DeclKind::Task,
                _ => {
                    let _ = src;
                    DeclKind::Other
                }
            }
        }
        _ => DeclKind::Other,
    }
}

impl Lowerer<'_> {
    /// **An interface's clause terms, in an arena of their own** (ADR-0240).
    ///
    /// A declaration with no body is an interface (`effects.rs`), and a
    /// clause's terms are lowered into the body's arena, so an interface's
    /// were not lowered at all. ADR-0237 parsed them for their errors alone;
    /// their keys were resolved, counted and typed by nothing, and
    /// `invalidates_on Changed(nosuch)` checked. They are lowered as a body's
    /// are now, into a body whose root is an empty block, which says nothing
    /// is implemented. `None` when no clause has terms.
    fn interface_terms(
        &mut self,
        owner: DeclId,
        policies: &mut [Policy],
        span: Span,
    ) -> Option<BodyId> {
        let has_terms = policies.iter().any(|p| {
            crate::policy::keyed(&p.name).is_some()
                || crate::policy::domain_of(&p.name) == Some(crate::policy::Domain::Transition)
        });
        if !has_terms {
            return None;
        }
        let mut b = BodyBuilder::default();
        let root = b.expr(Expr::Block { stmts: Vec::new() }, span.clone());
        self.policy_terms(&mut b, policies);
        let body = Body {
            owner,
            exprs: b.exprs,
            pats: b.pats,
            types: b.types,
            nodes: b.nodes,
            root,
            signals: b.signals,
            param_types: b.param_types,
            bound: b.bound,
            provides: b.provides,
        };
        Some(BodyId(self.hir.bodies.alloc(body, span)))
    }

    /// **The default of a signal the device keeps** (track `device-store`,
    /// ADR-0366): `signal todos: List<Todo> = []` at module level, its clauses
    /// below it. Its value is a body of its own, whose root is the value, so
    /// it is resolved, typed and checked as data where it is written, and
    /// the build writes it as a page's signal's first value is written
    /// (`backend::signals`). A module's signal with a value and no clause is
    /// the grammar's PW5306, and has none.
    fn signal_default(
        &mut self,
        owner: DeclId,
        kind: DeclKind,
        node: &SyntaxNode,
        policies: &mut [Policy],
    ) -> Option<BodyId> {
        if kind != DeclKind::Signal || policies.is_empty() {
            return None;
        }
        let value = node.children().find(|c| is_expr(c.kind()))?;
        let mut b = BodyBuilder::default();
        let root = self.expr(&mut b, &value);
        self.policy_terms(&mut b, policies);
        let body = Body {
            owner,
            exprs: b.exprs,
            pats: b.pats,
            types: b.types,
            nodes: b.nodes,
            root,
            signals: b.signals,
            param_types: b.param_types,
            bound: b.bound,
            provides: b.provides,
        };
        Some(BodyId(self.hir.bodies.alloc(body, span_of(&value))))
    }

    /// **A standalone parse's errors, each moved to its place in the file**
    /// (ADR-0237). Until then they were dropped, and what such a parse could
    /// not read was simply not there: `invalidates Cart(s) Order(s)`
    /// invalidated no order.
    fn read_errors(&mut self, parsed: &pw_syntax::Parse, offset: usize) {
        self.hir
            .syntax
            .extend(parsed.errors.iter().map(|e| pw_syntax::SyntaxError {
                span: (e.span.start + offset)..(e.span.end + offset),
                ..e.clone()
            }));
    }

    /// The `{expr}` holes in a string literal, lowered with the REAL grammar.
    ///
    /// The alternative — a small parser for what may appear in a hole — is the
    /// second-mini-language mistake `docs/NEXT.md` rejected for effect parsing,
    /// and it fails the same way: the two grammars disagree about something and
    /// the checker quietly analyses a different program from the one that runs.
    ///
    /// So the hole is handed to `pw_syntax::parse_expr`, space-padded to
    /// exactly the hole's offset in the real source. Every span that comes
    /// back is therefore already correct, with no arithmetic to get wrong.
    fn interpolations(&mut self, b: &mut BodyBuilder, text: &str, at: usize) -> Vec<ExprId> {
        let mut out = Vec::new();
        let mut rest = text;
        let mut base = at;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else { break };
            if let Some(id) = self.expression_at(b, &after[..close], base + open + 1, "a hole") {
                out.push(id);
            }
            base = base + open + 1 + close + 1;
            rest = &after[close + 1..];
        }
        out
    }

    /// One expression written at `at` in the real file: a string's hole, or
    /// the expression a block marker carries (`c` in `{#if c}`). Lowered with
    /// the real grammar, like every other expression, and read whole (ADR-0237):
    /// what follows the expression is an error, which `what` names. It was a
    /// statement after it, in a synthetic function's body, and dropped.
    fn expression_at(
        &mut self,
        b: &mut BodyBuilder,
        hole: &str,
        at: usize,
        what: &str,
    ) -> Option<ExprId> {
        if hole.trim().is_empty() {
            return None;
        }
        let mut padded = String::with_capacity(at + hole.len());
        padded.push_str(&" ".repeat(at));
        padded.push_str(hole);

        let parsed = pw_syntax::parse_expr(&padded, what);
        self.read_errors(&parsed, 0);
        let mut sub = Lowerer {
            hir: std::mem::take(&mut self.hir),
            src: &padded,
        };
        let id = parsed
            .green
            .children()
            .find(|c| is_expr(c.kind()))
            .map(|e| sub.expr(b, &e));
        self.hir = sub.hir;
        id
    }

    /// The expression a block marker carries after one of `prefixes`: `c` in
    /// `{#if c}` or `{:else if c}`, `e` in `{#match e}`. `what` names it.
    fn marker_expression(
        &mut self,
        b: &mut BodyBuilder,
        node: &SyntaxNode,
        prefixes: &[&str],
        what: &str,
    ) -> Option<ExprId> {
        let raw = text(self.src, node);
        let lead = raw.len() - raw.trim_start().len();
        let t = raw.trim_start();
        let prefix = prefixes.iter().find(|p| {
            t.strip_prefix(**p)
                .is_some_and(|r| r.starts_with(char::is_whitespace))
        })?;
        let inner = t[prefix.len()..].trim_end().strip_suffix('}')?;
        self.expression_at(b, inner, span_of(node).start + lead + prefix.len(), what)
    }

    /// **An `{#each}`'s head** (ADR-0242), parsed once by the grammar and
    /// padded to its place, so its parts' spans and its errors are the
    /// file's. `None` for another block, or a head with no list and name.
    ///
    /// Its list and its key are lowered into the body's arena (ADR-0345),
    /// as a block marker's condition is: until 2026-10-15 they were kept as
    /// text, and no walker of the body's expressions saw them.
    fn each_head(
        &mut self,
        b: &mut BodyBuilder,
        node: &SyntaxNode,
    ) -> Option<crate::hir::EachHead> {
        let raw = text(self.src, node);
        let lead = raw.len() - raw.trim_start().len();
        let rest = raw.trim_start().strip_prefix("{#each")?;
        if !(rest.starts_with(char::is_whitespace) || rest.starts_with('}')) {
            return None;
        }
        let inner = rest.trim_end().strip_suffix('}')?;
        let at = span_of(node).start + lead + "{#each".len();
        let mut padded = String::with_capacity(at + inner.len());
        padded.push_str(&" ".repeat(at));
        padded.push_str(inner);

        let parsed = pw_syntax::parse_each_head(&padded);
        self.read_errors(&parsed, 0);
        // A head whose list and name were read is the block's, its errors
        // reported: the rows' names are bound, and one mistake is one error.
        let head = parsed.green.children().find(|c| c.kind() == K::EachHead)?;
        // By where each is, around the name: what a recovery skipped is an
        // error node, and neither the list nor the key.
        let read = |c: &SyntaxNode| is_expr(c.kind()) && c.kind() != K::ErrorExpr;
        let name = head.children().find(|c| c.kind() == K::Name)?;
        let list = head
            .children()
            .take_while(|c| c.kind() != K::Name)
            .find(read)?;
        let key = head
            .children()
            .skip_while(|c| c.kind() != K::Name)
            .skip(1)
            .find(read);
        let written =
            |n: &SyntaxNode| pw_syntax::collapse_policy_whitespace(text(&padded, n).trim());
        let shown = crate::hir::EachWritten {
            list: written(&list),
            key: key.as_ref().map(written),
        };
        let (binder, binder_span) = (written(&name), span_of(&name));
        // The list before the key, as written, with the real grammar's
        // lowering over the head padded to its place: every span the file's.
        let mut sub = Lowerer {
            hir: std::mem::take(&mut self.hir),
            src: &padded,
        };
        let list = sub.expr(b, &list);
        let key = key.as_ref().map(|k| sub.expr(b, k));
        self.hir = sub.hir;
        Some(crate::hir::EachHead {
            list,
            binder,
            binder_span,
            key,
            written: shown,
        })
    }

    /// `{:else}`, `{:else if c}`, `{:Some(x)}`: a block's branch marker, kept
    /// at its place (ADR-0042).
    fn branch(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> NodeId {
        let marker = text(self.src, node);
        let condition = self.marker_expression(b, node, &["{:else if"], "a branch's condition");
        let inner = marker
            .trim()
            .trim_start_matches("{:")
            .trim_end_matches('}')
            .trim();
        b.node(
            Node::Branch {
                arm: constructor_arm(inner),
                marker,
                condition,
            },
            span_of(node),
        )
    }

    fn decl(&mut self, node: &SyntaxNode) -> Option<DeclId> {
        if !matches!(
            node.kind(),
            K::FnDecl
                | K::UiDecl
                | K::ResourceDecl
                | K::EffectDecl
                | K::PreludeDecl
                | K::LetDecl
                | K::TypeDecl
                | K::RecordDecl
                | K::UnionDecl
                | K::OpaqueDecl
                | K::ImportDecl
                | K::ErrorDecl
        ) {
            return None;
        }

        let kind = decl_kind_of(node, self.src);
        // A module's signal is assigned by the handlers of what it is
        // provided to (ADR-0144).
        let mutable = kind == DeclKind::Signal
            || kind == DeclKind::Let && own_tokens(node).iter().any(|t| t.text() == "mut");
        let name = first_name(node).unwrap_or_default();
        let name_span = first_name_span(node).unwrap_or_else(|| span_of(node));
        let declared_effects = node
            .children()
            .find(|c| c.kind() == K::EffectRow)
            .map(|row| self.effect_row(&row));

        // Reserve the id before lowering the body, so a nested declaration can
        // name its parent and the body can name its owner.
        let id = DeclId(self.hir.decls.alloc(
            Decl {
                name,
                name_span,
                kind,
                params: self.params(node),
                // A predicate the program gives a body answers whether it
                // holds (ADR-0115 as amended by ADR-0384's H2): a `Bool`,
                // which its body is checked against.
                ret: self.return_type(node).or_else(|| {
                    (kind == DeclKind::Predicate && node.children().any(|c| c.kind() == K::Body))
                        .then(|| crate::hir::DeclaredType::new("Bool", Vec::new()))
                }),
                variants: self.variants(node),
                fields: self.record_fields(node),
                policies: self.policies(node),
                imports: imported_names(node),
                visibility: visibility_of(node),
                opaque_of: self.opaque_of(node),
                invariant: invariant(self.src, node),
                type_params: self.type_params(node),
                declared_effects,
                body: None,
                terms: None,
                children: Vec::new(),
                mutable,
            },
            span_of(node),
        ));

        // The policies are taken back out of the declaration, lowered against
        // the body's arena, and put back. A policy whose domain is executable
        // code needs an `ExprId`, and that arena does not exist until the body
        // is built.
        let mut policies = std::mem::take(
            &mut self
                .hir
                .decls
                .get_mut(id.index())
                .expect("just allocated")
                .policies,
        );
        let (body, children) = match node.children().find(|c| c.kind() == K::Body) {
            Some(b) => self.body(id, &b, &mut policies),
            None => (
                self.signal_default(id, kind, node, &mut policies),
                Vec::new(),
            ),
        };
        // An interface's clause terms, in an arena of their own (ADR-0240).
        let terms = match body {
            Some(_) => None,
            None => self.interface_terms(id, &mut policies, span_of(node)),
        };
        // Internal invariant, not a claim about the program: `id` was
        // returned by `alloc` four lines up and arenas do not shrink. No `.pw`
        // source can make this absent.
        let d = self.hir.decls.get_mut(id.index()).expect("just allocated");
        d.policies = policies;
        d.body = body;
        d.terms = terms;
        d.children = children;
        Some(id)
    }

    fn params(&self, node: &SyntaxNode) -> Vec<Param> {
        node.children()
            .find(|c| c.kind() == K::ParamList)
            .map(|l| {
                l.children()
                    .filter(|c| c.kind() == K::Param)
                    .map(|p| {
                        let t = p.children().find(|c| c.kind() == K::TypeRef);
                        Param {
                            name: first_name(&p).unwrap_or_default(),
                            // The complete type, built once. `DeclaredType`
                            // keeps the head and its arguments together, so a
                            // consumer has to ask for the head alone by name.
                            ty: t.as_ref().map(declared_type),
                            span: span_of(&p),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The type after `->`. It is the `TypeRef` that is a direct child of the
    /// declaration, so it cannot be confused with a parameter's type (those sit
    /// inside `Param`) or a field's.
    fn return_type(&self, node: &SyntaxNode) -> Option<crate::hir::DeclaredType> {
        node.children()
            .find(|c| c.kind() == K::TypeRef)
            .map(|t| declared_type(&t))
    }

    fn record_fields(&self, node: &SyntaxNode) -> Option<Vec<Param>> {
        let list = node.children().find(|c| c.kind() == K::FieldList)?;
        Some(
            list.children()
                .filter(|c| c.kind() == K::Field)
                .map(|f| {
                    let t = f.children().find(|c| c.kind() == K::TypeRef);
                    Param {
                        name: first_name(&f).unwrap_or_default(),
                        ty: t.as_ref().map(declared_type),
                        span: span_of(&f),
                    }
                })
                .collect(),
        )
    }

    /// A declaration's policy block, values kept as written.
    /// A declaration's policy clauses.
    ///
    /// Two positions, because the language has two. Most declarations write
    /// their policies between the signature and the body, where the list is a
    /// direct child. A `materialize` block writes them INSIDE its braces —
    /// corpus A-009 and R-017 are the specification — so the list is a
    /// grandchild through `Body > BlockExpr`. Looking only at direct children
    /// found none of a fragment's policies, and an empty policy list reads as
    /// "declared nothing" rather than "not found".
    fn policies(&self, node: &SyntaxNode) -> Vec<Policy> {
        node.children()
            .find(|c| c.kind() == K::PolicyList)
            .or_else(|| {
                node.children()
                    .find(|c| c.kind() == K::Body)?
                    .children()
                    .find(|c| c.kind() == K::BlockExpr)?
                    .children()
                    .find(|c| c.kind() == K::PolicyList)
            })
            .map(|list| {
                list.children()
                    .filter(|c| c.kind() == K::Policy)
                    .map(|p| {
                        let whole = text(self.src, &p);
                        let trimmed = whole.trim_end();
                        // The grammar keeps a policy keyword as a bare token,
                        // so the name is the first word and the value is the
                        // rest — with the run of alignment spaces collapsed,
                        // because `freshness      30.seconds` and
                        // `freshness 30.seconds` declare the same policy.
                        // The head ends at whitespace OR at `(`. A block policy
                        // is written `draw(ctx) { .. }` with no space, and
                        // splitting on whitespace alone made the head
                        // `draw(ctx)` — a name `crate::policy` does not know,
                        // so the clause had no domain and no execution context.
                        let head_end = trimmed
                            .find(|c: char| c.is_whitespace() || c == '(')
                            .unwrap_or(trimmed.len());
                        let (name, rest) = trimmed.split_at(head_end);
                        let (name, value) = (
                            name.to_string(),
                            pw_syntax::collapse_policy_whitespace(rest.trim_start()),
                        );
                        Policy {
                            name,
                            value,
                            roots: Vec::new(),
                            keys: Vec::new(),
                            span: span_of(&p),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn variants(&self, node: &SyntaxNode) -> Option<Vec<VariantDef>> {
        let list = node.children().find(|c| c.kind() == K::VariantList)?;
        Some(
            list.children()
                .filter(|c| c.kind() == K::Variant)
                .map(|v| VariantDef {
                    name: first_name(&v).unwrap_or_default(),
                    fields: v
                        .children()
                        .filter(|c| c.kind() == K::TypeRef)
                        .map(|t| declared_type(&t))
                        .collect(),
                    span: span_of(&v),
                })
                .collect(),
        )
    }

    fn opaque_of(&self, node: &SyntaxNode) -> Option<crate::hir::DeclaredType> {
        if node.kind() != K::OpaqueDecl {
            return None;
        }
        node.children()
            .find(|c| c.kind() == K::TypeRef)
            .map(|t| declared_type(&t))
    }

    fn effect_row(&self, row: &SyntaxNode) -> Vec<EffectRef> {
        row.children()
            .filter(|c| c.kind() == K::EffectRef)
            .map(|e| {
                // `database.read<Stores>` — the path is the name, and the type
                // arguments come from the `TypeArgList` the grammar already
                // built. Taken from the TREE rather than sliced back out of
                // the text: the parser has answered this question once, and a
                // second answer is how the two come to disagree.
                let head = e.children().find(|c| c.kind() == K::TypeRef);
                let path = head
                    .as_ref()
                    .and_then(|t| t.children().find(|c| c.kind() == K::Name))
                    .map(|n| n.text().to_string())
                    .unwrap_or_else(|| text(self.src, &e).trim().to_string());
                let args = head
                    .as_ref()
                    .and_then(|t| t.children().find(|c| c.kind() == K::TypeArgList))
                    .map(|list| {
                        list.children()
                            .filter(|c| c.kind() == K::TypeRef)
                            .map(|a| declared_type(&a).written())
                            .collect()
                    })
                    .unwrap_or_default();
                EffectRef {
                    path,
                    written: text(self.src, &e).trim().to_string(),
                    args,
                    span: span_of(&e),
                }
            })
            .collect()
    }

    /// The type parameters a declaration binds: the `TypeArgList` that is the
    /// declaration's OWN child, as `type_params()` in the grammar builds it.
    ///
    /// Its own child, deliberately. A `TypeArgList` nested inside a `TypeRef`
    /// is an argument at a use site — `-> Result<Store, StoreError>` — and
    /// reading one of those as a parameter binding would give every function
    /// returning a generic type a set of parameters it never declared.
    fn type_params(&self, node: &SyntaxNode) -> Vec<String> {
        node.children()
            .find(|c| c.kind() == K::TypeArgList)
            .map(|list| {
                list.children()
                    .filter(|c| c.kind() == K::TypeRef)
                    .map(|p| type_path(&p))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Lower a `Body` node, hoisting any nested declarations out of it.
    ///
    /// `policies` are lowered into the SAME arena, so a policy whose domain is
    /// executable code gets a real expression with real spans — see
    /// [`Self::policy_terms`].
    fn body(
        &mut self,
        owner: DeclId,
        node: &SyntaxNode,
        policies: &mut [Policy],
    ) -> (Option<BodyId>, Vec<DeclId>) {
        let Some(block) = node.children().find(|c| c.kind() == K::BlockExpr) else {
            return (None, Vec::new());
        };

        // Nested declarations are declarations, not expressions: a `fn` inside
        // a `component` gets its own DeclId and its own body.
        let mut children = Vec::new();
        for c in block.children() {
            if let Some(child) = self.decl(&c) {
                children.push(child);
            }
        }

        let mut b = BodyBuilder::default();
        let root = self.block(&mut b, &block);
        self.policy_terms(&mut b, policies);
        self.policy_blocks(&mut b, &block, policies);
        let body = Body {
            owner,
            exprs: b.exprs,
            pats: b.pats,
            types: b.types,
            nodes: b.nodes,
            root,
            signals: b.signals,
            param_types: b.param_types,
            bound: b.bound,
            provides: b.provides,
        };
        (
            Some(BodyId(self.hir.bodies.alloc(body, span_of(node)))),
            children,
        )
    }

    /// **A block policy's contents become a named root with its own binders.**
    ///
    /// `draw(ctx) { ctx.rect(w) }`, `release(handle) { Maps.destroy(handle) }`.
    /// Architect ruling, 2026-08-11: the header introduces real lexical
    /// binders, and downstream resolution receives them already lowered.
    ///
    /// The block lives in the declaration's own syntax tree, so its spans are
    /// already file-relative — unlike a transition, which is parsed from the
    /// policy's value text and has to be shifted back.
    fn policy_blocks(&mut self, b: &mut BodyBuilder, block: &SyntaxNode, policies: &mut [Policy]) {
        let Some(list) = block.children().find(|c| c.kind() == K::PolicyList) else {
            return;
        };
        let nodes: Vec<SyntaxNode> = list.children().filter(|c| c.kind() == K::Policy).collect();
        for (p, node) in policies.iter_mut().zip(nodes) {
            let Some(context) = crate::policy::execution_context(&p.name) else {
                continue;
            };
            let Some(body_node) = node.children().find(|c| c.kind() == K::BlockExpr) else {
                continue;
            };
            // The header's parameters ARE the binders. Read from the
            // `ParamList` the grammar built, not from the value's text.
            let binders: Vec<(String, crate::hir::Span)> = node
                .children()
                .find(|c| c.kind() == K::ParamList)
                .map(|l| {
                    l.children()
                        .filter(|c| c.kind() == K::Param)
                        .filter_map(|param| {
                            let name = param.children().find(|c| c.kind() == K::Name)?;
                            Some((text(self.src, &name).trim().to_string(), span_of(&name)))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let root = self.expr(b, &body_node);
            p.roots.push(crate::hir::TermRoot {
                context,
                binders,
                root,
            });
        }
    }

    /// **A policy value that is a transition becomes one.**
    ///
    /// Architect ruling, 2026-08-11 (ADR-0025):
    ///
    /// > An optimistic clause identifies a resource entry and binds its current
    /// > value; its body is an ordinary Pleris transition expression.
    ///
    /// Which heads take this shape is `crate::policy`'s answer — one table, not
    /// a second list here. Both expressions are allocated in the declaration's
    /// own arena and are **not reachable from `root`**, deliberately: they are
    /// terms, so name resolution must see them, and they run in a different
    /// execution context, so the declaration's effect row must not absorb them.
    /// `Decl::transitions` is how a consumer asks for them by name rather than
    /// finding them by walking.
    ///
    /// Spans are offset back into the file. The sub-parse sees only the value
    /// text, so without this a diagnostic would underline column 3 of whatever
    /// line happened to be there.
    fn policy_terms(&mut self, b: &mut BodyBuilder, policies: &mut [Policy]) {
        for p in policies.iter_mut() {
            if crate::policy::keyed(&p.name).is_some() {
                self.clause_keys(b, p);
                continue;
            }
            if crate::policy::domain_of(&p.name) != Some(crate::policy::Domain::Transition) {
                continue;
            }
            // The value as the source writes it: `Policy::value` collapses
            // runs of whitespace, and a span must point into the file.
            let (value, offset) = self.written_value(p);
            if value.is_empty() {
                continue;
            }
            let parsed = pw_syntax::parse_transition_clause(&value);
            self.read_errors(&parsed, offset);
            let Some(clause) = parsed
                .green
                .children()
                .find(|c| c.kind() == K::TransitionClause)
            else {
                continue;
            };
            // Each arm its target, its name and its transition, in order
            // (ADR-0238). An arm missing one is its parse's error, reported,
            // and lowers to nothing.
            let arms: Vec<(SyntaxNode, SyntaxNode, SyntaxNode)> = clause
                .children()
                .filter(|c| c.kind() == K::TransitionArm)
                .filter_map(|arm| {
                    let name = arm.children().find(|c| c.kind() == K::Name)?;
                    let target = arm.children().find(|c| is_expr(c.kind()))?;
                    let body = arm.children().filter(|c| is_expr(c.kind())).nth(1)?;
                    (target.text_range().end() <= name.text_range().start()
                        && name.text_range().end() <= body.text_range().start())
                    .then_some((target, name, body))
                })
                .collect();
            if arms.is_empty() {
                continue;
            }

            // Lowered by a `Lowerer` over the VALUE's text, because `span_of`
            // reads a node's range and the sub-parse's ranges start at zero.
            // Then every span it allocated is moved back into the file — one
            // shift over the arena suffix, rather than a second span convention
            // every node kind would have to honour.
            let before = (b.exprs.len(), b.pats.len(), b.types.len(), b.nodes.len());
            let mut sub = Lowerer {
                hir: std::mem::take(&mut self.hir),
                src: &value,
            };
            let lowered: Vec<(ExprId, SyntaxNode, ExprId)> = arms
                .iter()
                .map(|(target, name, body)| (sub.expr(b, target), name.clone(), sub.expr(b, body)))
                .collect();
            self.hir = std::mem::take(&mut sub.hir);
            b.exprs.shift_spans_from(before.0, offset);
            b.pats.shift_spans_from(before.1, offset);
            b.types.shift_spans_from(before.2, offset);
            b.nodes.shift_spans_from(before.3, offset);

            // **Two roots, not one tree with a special case inside it.**
            //
            // Architect ruling, 2026-08-11: the target selector and the state
            // transformation are different computations with different
            // contexts. The target may read invocation state to name the entry;
            // the transformation may not perform anything at all.
            // Each arm two roots, its target's and then its transition's.
            p.roots = lowered
                .into_iter()
                .flat_map(|(target, name_node, body)| {
                    let bs = span_of(&name_node);
                    let binder = text(&value, &name_node).trim().to_string();
                    [
                        crate::hir::TermRoot {
                            context: crate::hir::ExecutionContext::TargetSelection,
                            binders: Vec::new(),
                            root: target,
                        },
                        crate::hir::TermRoot {
                            context: crate::hir::ExecutionContext::OptimisticTransition,
                            binders: vec![(binder, (bs.start + offset)..(bs.end + offset))],
                            root: body,
                        },
                    ]
                })
                .collect();
        }
    }

    /// **A policy's value as the source writes it, and where it starts in
    /// the file.** `Policy::value` collapses runs of whitespace, so it is not
    /// text a span can point into: until 2026-09-26 an optimistic clause
    /// written with extra spaces had every diagnostic inside it underline
    /// the wrong columns (ADR-0088).
    fn written_value(&self, p: &Policy) -> (String, usize) {
        let whole = self.src[p.span.clone()].trim_end();
        let head = whole.find(char::is_whitespace).unwrap_or(whole.len());
        let rest = &whole[head..];
        let value = rest.trim_start();
        (
            value.to_string(),
            p.span.start + head + (rest.len() - value.len()),
        )
    }

    /// **The declarations a clause names, each with its key** (ADR-0088).
    ///
    /// `depends_on Store(id), Menu(id)` names two resources and passes each
    /// the key `id`. The names are the clause's, looked up by its kind; the
    /// arguments are terms, lowered into the declaration's arena like an
    /// optimistic clause's, and each is a root in the `Key` context. Until
    /// 2026-09-26 the value was text, and nothing resolved, counted or typed
    /// a key: `invalidates Cart(nosuch)` checked.
    ///
    /// An item that is neither a name nor a name applied to arguments names
    /// no declaration. The graph reports what it names (PW5100).
    fn clause_keys(&mut self, b: &mut BodyBuilder, p: &mut Policy) {
        let (value, offset) = self.written_value(p);
        if value.is_empty() {
            return;
        }
        let parsed = pw_syntax::parse_expr_list(&value);
        self.read_errors(&parsed, offset);
        let items: Vec<SyntaxNode> = parsed
            .green
            .children()
            .filter(|c| is_expr(c.kind()))
            .collect();
        let before = (b.exprs.len(), b.pats.len(), b.types.len(), b.nodes.len());
        let mut sub = Lowerer {
            hir: std::mem::take(&mut self.hir),
            src: &value,
        };
        let shift = |s: Span| (s.start + offset)..(s.end + offset);
        let mut keys = Vec::new();
        for item in &items {
            let (name_node, list) = match item.kind() {
                K::CallExpr => (
                    item.children().next(),
                    item.children().find(|c| c.kind() == K::ArgList),
                ),
                _ => (Some(item.clone()), None),
            };
            let Some(name_node) = name_node.filter(names_a_path) else {
                continue;
            };
            let args = list.as_ref().map(|l| sub.args(b, l)).unwrap_or_default();
            let written = list
                .iter()
                .flat_map(|l| l.children().filter(|c| is_expr(c.kind())))
                .map(|a| pw_syntax::collapse_policy_whitespace(text(&value, &a).trim()))
                .collect();
            keys.push(crate::hir::ClauseKey {
                name: text(&value, &name_node).trim().to_string(),
                name_span: shift(span_of(&name_node)),
                span: shift(span_of(item)),
                args,
                written,
            });
        }
        self.hir = std::mem::take(&mut sub.hir);
        b.exprs.shift_spans_from(before.0, offset);
        b.pats.shift_spans_from(before.1, offset);
        b.types.shift_spans_from(before.2, offset);
        b.nodes.shift_spans_from(before.3, offset);
        // A listener's arguments bind; a key's are evaluated (ADR-0091), an
        // event's and an invalidated entry's by the command (ADR-0208,
        // ADR-0209).
        let context = match crate::policy::domain_of(&p.name) {
            Some(crate::policy::Domain::Listener) => crate::hir::ExecutionContext::Listener,
            Some(crate::policy::Domain::EventRef) => crate::hir::ExecutionContext::Emitted,
            _ if p.name == "invalidates" => crate::hir::ExecutionContext::Invalidated,
            _ => crate::hir::ExecutionContext::Key,
        };
        p.roots = keys
            .iter()
            .flat_map(|k| &k.args)
            .map(|a| crate::hir::TermRoot {
                context,
                binders: Vec::new(),
                root: a.value,
            })
            .collect();
        p.keys = keys;
    }

    fn block(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> ExprId {
        // A nested declaration already has its own DeclId and body; it is not
        // also a statement of the enclosing one.
        let stmts = node
            .children()
            .filter(|c| is_expr(c.kind()))
            .map(|c| self.expr(b, &c))
            .collect();
        b.expr(Expr::Block { stmts }, span_of(node))
    }

    fn expr(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> ExprId {
        let span = span_of(node);
        match node.kind() {
            K::BlockExpr => self.block(b, node),

            K::LiteralExpr => {
                let t = own_tokens(node);
                // A number's value is its digits: a `_` between two of them
                // groups them, and is no part of it (ADR-0276).
                let lit = match t.first().map(|t| (t.kind(), t.text().to_string())) {
                    Some((K::Int, s)) => Literal::Int(s.replace('_', "")),
                    Some((K::Float, s)) => Literal::Float(s.replace('_', "")),
                    Some((K::Str, s)) => {
                        // A string with `{..}` holes is not a literal: the holes
                        // are expressions, and a checker that has to search the
                        // characters for them cannot see into `{token.value}`.
                        // Where the holes are is the string decoder's answer
                        // (ADR-0049): `\{` opens none, and a `"""` string has
                        // none.
                        if let Ok(pieces) = pw_syntax::strings::pieces(&s) {
                            let mut parts = Vec::new();
                            for p in &pieces {
                                if let pw_syntax::strings::Piece::Hole { source, at } = p
                                    && let Some(id) =
                                        self.expression_at(b, source, span.start + at, "a hole")
                                {
                                    parts.push(id);
                                }
                            }
                            if !parts.is_empty() {
                                return b.expr(Expr::Interpolated { text: s, parts }, span);
                            }
                        }
                        Literal::Str(s)
                    }
                    Some((K::UnterminatedStr, s)) => Literal::UnterminatedStr(s),
                    _ => return b.expr(Expr::Error, span),
                };
                b.expr(Expr::Literal(lit), span)
            }

            K::NameExpr => b.expr(Expr::Name(text(self.src, node).trim().to_string()), span),

            K::FieldExpr => {
                // `base . name` — the base is the only child expression.
                let base = match node.children().next() {
                    Some(c) => self.expr(b, &c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                let name = own_tokens(node)
                    .iter()
                    .rev()
                    .find(|t| t.kind() == K::Ident)
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                b.expr(Expr::Field { base, name }, span)
            }

            K::CallExpr => {
                let mut kids = node.children();
                let callee = match kids.next() {
                    Some(c) => self.expr(b, &c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                let args = node
                    .children()
                    .find(|c| c.kind() == K::ArgList)
                    .map(|l| self.args(b, &l))
                    .unwrap_or_default();
                b.expr(Expr::Call { callee, args }, span)
            }

            K::TryExpr => {
                let value = match node.children().next() {
                    Some(c) => self.expr(b, &c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                b.expr(Expr::Try { value }, span)
            }

            K::LambdaExpr => {
                // `x => e`: the body is the last child; everything before it is
                // either a parameter or a call-shaped descriptor.
                let kids: Vec<_> = node.children().collect();
                let Some((last, init)) = kids.split_last() else {
                    let body = b.expr(Expr::Error, span.clone());
                    return b.expr(
                        Expr::Lambda {
                            descriptor: None,
                            params: Vec::new(),
                            body,
                        },
                        span,
                    );
                };
                let mut descriptor = None;
                let mut params = Vec::new();
                for p in init {
                    // `() => e` takes no parameter: an empty list is no
                    // pattern. It lowered to one, a unit or an error pattern,
                    // until 2026-10-02 (ADR-0134), and a handler written that
                    // way was refused as one that takes the event.
                    if matches!(p.kind(), K::ParenExpr | K::TupleExpr | K::ParamList)
                        && p.children().next().is_none()
                    {
                        continue;
                    }
                    if is_param_shaped(p.kind()) {
                        params.push(self.param_pattern(b, p));
                    } else {
                        descriptor = Some(self.expr(b, p));
                    }
                }
                let body = self.expr(b, last);
                b.expr(
                    Expr::Lambda {
                        descriptor,
                        params,
                        body,
                    },
                    span,
                )
            }

            K::BinaryExpr => {
                let kids: Vec<_> = node.children().collect();
                let op = own_tokens(node)
                    .first()
                    .map(|t| bin_op(t.kind(), t.text()))
                    .unwrap_or(BinOp::Cmp("?".into()));
                let lhs = match kids.first() {
                    Some(c) => self.expr(b, c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                let rhs = match kids.get(1) {
                    Some(c) => self.expr(b, c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                b.expr(Expr::Binary { op, lhs, rhs }, span)
            }

            K::CastExpr => {
                let kids: Vec<_> = node.children().collect();
                let value = match kids.first() {
                    Some(c) => self.expr(b, c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                let ty = match kids.iter().find(|c| c.kind() == K::TypeRef) {
                    Some(t) => self.type_ref(b, t),
                    None => b.ty(
                        crate::hir::TypeRef {
                            path: String::new(),
                            args: vec![],
                        },
                        span.clone(),
                    ),
                };
                b.expr(Expr::Cast { value, ty }, span)
            }

            K::UnaryExpr => {
                let op = match own_tokens(node).first().map(|t| t.kind()) {
                    Some(K::Bang) => UnOp::Not,
                    _ => UnOp::Neg,
                };
                let operand = match node.children().next() {
                    Some(c) => self.expr(b, &c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                b.expr(Expr::Unary { op, operand }, span)
            }

            // Parentheses group; the tree already records that (ADR-0014).
            // Empty, they are the unit value (ADR-0200).
            K::ParenExpr => match node.children().next() {
                Some(c) => self.expr(b, &c),
                None => b.expr(Expr::Literal(Literal::Unit), span),
            },

            K::IfExpr => {
                // `if a { x } elif b { y } else { z }` is one node holding
                // `a`, `{ x }`, `b`, `{ y }`, `{ z }`, and `else if` the same.
                // Until 2026-09-26 the third child was taken for the `else`,
                // so the chain lowered to `if a { x } else b`: `b` was the
                // value when `a` was false, and `{ y }` and `{ z }` were
                // dropped (ADR-0068). The chain is nested ifs.
                let kids: Vec<_> = node.children().collect();
                let cond = match kids.first() {
                    Some(c) => self.expr(b, c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                let then = match kids.get(1) {
                    Some(c) => self.expr(b, c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                let els = self.if_chain(b, kids.get(2..).unwrap_or_default(), &span);
                b.expr(Expr::If { cond, then, els }, span)
            }

            K::ForExpr => {
                // `pattern`, `iterable`, `body` — the pattern first, because
                // the grammar emits it first and a consumer that guessed by
                // shape would confuse `for x in xs` with `for (i, v) in xs`.
                let kids: Vec<_> = node.children().collect();
                let pat = kids
                    .first()
                    .filter(|c| is_pattern(c.kind()))
                    .map(|c| self.pattern(b, c));
                let rest: Vec<&SyntaxNode> = kids.iter().filter(|c| is_expr(c.kind())).collect();
                let iterable = match rest.first() {
                    Some(c) => self.expr(b, c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                let body = match rest.get(1) {
                    Some(c) => self.expr(b, c),
                    None => b.expr(Expr::Error, span.clone()),
                };
                b.expr(
                    Expr::For {
                        pat,
                        iterable,
                        body,
                    },
                    span,
                )
            }

            K::MatchExpr => {
                let mut kids = node.children().peekable();
                let scrutinee = match kids.peek().filter(|c| c.kind() != K::MatchArm) {
                    Some(_) => {
                        // Internal invariant: `peek` returned `Some` on the
                        // line above and nothing consumed the iterator between
                        // the two.
                        let c = kids.next().expect("peeked");
                        self.expr(b, &c)
                    }
                    None => b.expr(Expr::Error, span.clone()),
                };
                let arms = node
                    .children()
                    .filter(|c| c.kind() == K::MatchArm)
                    .map(|arm| {
                        let kids: Vec<_> = arm.children().collect();
                        let pat = match kids.first() {
                            Some(p) => self.pattern(b, p),
                            None => b.pat(Pattern::Error, span_of(&arm)),
                        };
                        let body = match kids.get(1..).unwrap_or_default() {
                            [] => b.expr(Expr::Error, span_of(&arm)),
                            [e] => self.expr(b, e),
                            // `=> return e`: the two statements a block reads
                            // it as, and so a block (the parser's `arm_body`).
                            stmts => {
                                let at =
                                    span_of(&stmts[0]).start..span_of(&stmts[stmts.len() - 1]).end;
                                let stmts = stmts.iter().map(|s| self.expr(b, s)).collect();
                                b.expr(Expr::Block { stmts }, at)
                            }
                        };
                        MatchArm { pat, body }
                    })
                    .collect();
                b.expr(Expr::Match { scrutinee, arms }, span)
            }

            K::RecordExpr => {
                let name = node
                    .children()
                    .find(|c| c.kind() == K::NameExpr)
                    .map(|n| n.text().to_string().trim().to_string());
                let fields = node
                    .children()
                    .filter(|c| c.kind() == K::Field)
                    .map(|f| self.field_init(b, &f))
                    .collect();
                b.expr(Expr::Record { name, fields }, span)
            }

            K::ListExpr | K::TupleExpr => {
                let items = node.children().map(|c| self.expr(b, &c)).collect();
                b.expr(Expr::List { items }, span)
            }

            // A labelled statement (`transform: scale(1.0) -> scale(1.08)`) uses
            // the same node kind as a record field, because it is the same shape.
            K::Field => {
                let f = self.field_init(b, node);
                let fields = vec![f];
                b.expr(Expr::Record { name: None, fields }, span)
            }

            K::LetStmt => self.let_or_keyword(b, node, span),

            K::TemplateRegion => {
                // The element tree, plus a flat list of interpolated
                // expressions. Both are needed: a renderer walks the tree, and
                // an effect or privacy walk wants the expressions without
                // having to descend markup it does not understand.
                // Only the nodes THIS region allocates. Scanning the whole
                // arena would re-collect an earlier region's expressions in a
                // body that has two, and `walk()` would then visit them twice —
                // which is how a checker ends up reporting one defect twice.
                let first = b.nodes.len();
                let roots: Vec<NodeId> = node
                    .children()
                    .filter(|c| is_markup(c.kind()))
                    .map(|c| self.markup(b, &c))
                    .collect();
                // Every expression each node holds, at every position, as the
                // one reading of them says (ADR-0345). Until 2026-10-15 this
                // was a `match` that named the positions it knew and dropped
                // the rest, and an `{#each}`'s list and key were none of them.
                let mut parts: Vec<ExprId> = b
                    .nodes
                    .iter()
                    .skip(first)
                    .flat_map(|(_, n, _)| n.exprs())
                    .map(|(_, e)| e)
                    .collect();
                parts.sort_by_key(|e| b.exprs.span_at(e.index()).map(|s| s.start).unwrap_or(0));
                b.expr(Expr::Template { parts, roots }, span)
            }

            K::Interpolation => match node.children().find(|c| is_expr(c.kind())) {
                Some(e) => self.expr(b, &e),
                None => b.expr(
                    Expr::Template {
                        parts: vec![],
                        roots: vec![],
                    },
                    span,
                ),
            },

            _ => b.expr(Expr::Error, span),
        }
    }

    /// `LetStmt` covers both `let x = e` and the keyword-statement family; they
    /// are told apart by their first token.
    fn let_or_keyword(&mut self, b: &mut BodyBuilder, node: &SyntaxNode, span: Span) -> ExprId {
        let toks = own_tokens(node);
        let head = toks
            .first()
            .map(|t| t.text().to_string())
            .unwrap_or_default();

        if head == "let" || head == "signal" {
            let pat = node.children().find(|c| c.kind() == K::Name).map(|n| {
                // A signal is assigned by its handlers (ADR-0130); where it
                // may be is `signals::check`'s question.
                let mutable = head == "signal" || toks.iter().any(|t| t.text() == "mut");
                b.pat(
                    Pattern::Bind {
                        name: n.text().to_string(),
                        mutable,
                    },
                    span_of(&n),
                )
            });
            // `let _ = e`, an explicit discard (ADR-0250).
            let pat = pat.or_else(|| {
                node.children()
                    .find(|c| c.kind() == K::WildcardPat)
                    .map(|w| b.pat(Pattern::Wild, span_of(&w)))
            });
            let ty = node
                .children()
                .find(|c| c.kind() == K::TypeRef)
                .map(|t| self.type_ref(b, &t));
            let init = node
                .children()
                .find(|c| is_expr(c.kind()))
                .map(|e| self.expr(b, &e));
            let id = b.expr(Expr::Let { pat, ty, init }, span);
            if head == "signal" {
                b.signals.insert(id);
            }
            return id;
        }

        // `provide drawer = e` (ADR-0144): the assignment of the signal the
        // module declares, so its name resolves and its value is checked as
        // any assignment's is, marked as the `provide` it is.
        if head == "provide" {
            let target = node
                .children()
                .find(|c| c.kind() == K::Name)
                .map(|n| b.expr(Expr::Name(n.text().to_string()), span_of(&n)));
            let value = node
                .children()
                .find(|c| is_expr(c.kind()))
                .map(|e| self.expr(b, &e));
            let (Some(lhs), Some(rhs)) = (target, value) else {
                // The parser reported what is missing.
                return b.expr(Expr::Error, span);
            };
            let id = b.expr(
                Expr::Binary {
                    op: BinOp::Assign,
                    lhs,
                    rhs,
                },
                span,
            );
            b.provides.insert(id);
            return id;
        }

        // `unsafe.imperative`, `observe resize`, `use key: Secret<T> = ..`.
        // The keyword absorbs `.ident` while the tokens stay dotted; every
        // bare identifier after that is a modifier word.
        let mut keyword = head;
        let mut modifiers = Vec::new();
        let mut i = 1;
        while i + 1 < toks.len() && toks[i].kind() == K::Dot && toks[i + 1].kind() == K::Ident {
            keyword.push('.');
            keyword.push_str(toks[i + 1].text());
            i += 2;
        }
        while i < toks.len() {
            if toks[i].kind() == K::Ident {
                modifiers.push(toks[i].text().to_string());
            }
            i += 1;
        }

        let args: Vec<ExprId> = node
            .children()
            .find(|c| c.kind() == K::ArgList)
            .map(|l| self.args(b, &l).into_iter().map(|a| a.value).collect())
            .unwrap_or_default();
        let block = node
            .children()
            .find(|c| c.kind() == K::BlockExpr)
            .map(|blk| self.expr(b, &blk));
        // An initialiser (`use key: T = expr`) is an argument to the statement.
        let mut args = args;
        if let Some(init) = node
            .children()
            .find(|c| is_expr(c.kind()) && c.kind() != K::BlockExpr)
        {
            let id = self.expr(b, &init);
            args.push(id);
        }

        // The grammar keeps a `because "…"` string as a bare token, so it is
        // recovered from the statement's own tokens rather than from `args`.
        let justification = toks
            .iter()
            .find(|t| matches!(t.kind(), K::Str | K::UnterminatedStr))
            .map(|t| t.text().to_string());

        // `attributes_forced_layout_to VendorMap` — the word ending in `_to`
        // names the clause, and the identifier after it names the owner. Read
        // from the tokens rather than a fixed keyword so a new attribution
        // clause does not need a parser change to be visible.
        let attribution = toks
            .iter()
            .position(|t| {
                t.kind() == K::Ident && (t.text().ends_with("_to") || t.text() == "attributed")
            })
            .and_then(|i| toks.get(i + 1))
            .filter(|t| t.kind() == K::Ident)
            .map(|t| t.text().to_string());

        b.expr(
            Expr::Keyword {
                keyword,
                justification,
                attribution,
                modifiers,
                args,
                block,
            },
            span,
        )
    }

    /// What follows an `if`'s first block: nothing, an `else` block, or
    /// another condition and its block, and what follows that.
    fn if_chain(
        &mut self,
        b: &mut BodyBuilder,
        rest: &[SyntaxNode],
        span: &Span,
    ) -> Option<ExprId> {
        match rest {
            [] => None,
            [only] => Some(self.expr(b, only)),
            [cond, block, more @ ..] => {
                let cond = self.expr(b, cond);
                let then = self.expr(b, block);
                let els = self.if_chain(b, more, span);
                let at = match (
                    b.exprs.span_at(cond.index()),
                    els.and_then(|e| b.exprs.span_at(e.index())),
                ) {
                    (Some(c), Some(e)) => c.start..e.end,
                    (Some(c), None) => c.start..span.end,
                    _ => span.clone(),
                };
                Some(b.expr(Expr::If { cond, then, els }, at))
            }
        }
    }

    fn field_init(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> FieldInit {
        let name = first_name(node).unwrap_or_default();
        let value = node
            .children()
            .find(|c| is_expr(c.kind()))
            .map(|e| self.expr(b, &e));
        FieldInit {
            name,
            value,
            span: span_of(node),
        }
    }

    fn args(&mut self, b: &mut BodyBuilder, list: &SyntaxNode) -> Vec<Arg> {
        // The grammar consumes `max =` / `jitter:` as bare tokens before the
        // value, so a name is recovered by looking at what precedes each value.
        let mut named: Vec<String> = Vec::new();
        let mut pending: Option<String> = None;
        for e in list.children_with_tokens() {
            if let Some(t) = e.as_token() {
                match t.kind() {
                    K::Ident => pending = Some(t.text().to_string()),
                    K::Comma => pending = None,
                    _ => {}
                }
            } else if e.as_node().is_some_and(|n| is_expr(n.kind())) {
                named.push(pending.take().unwrap_or_default());
            }
        }
        list.children()
            .filter(|c| is_expr(c.kind()))
            .enumerate()
            .map(|(i, c)| Arg {
                name: named.get(i).filter(|s| !s.is_empty()).cloned(),
                value: self.expr(b, &c),
            })
            .collect()
    }

    /// A lambda parameter, which the grammar emits as an expression node.
    fn param_pattern(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> PatternId {
        let span = span_of(node);
        match node.kind() {
            // `x => e` writes the parameter as an expression; `fn(x) e`
            // writes it as a declaration-shaped `Param` holding a `Name`.
            // Both are parameters, and only the first was handled: `fn(el) ..`
            // lowered its parameter to `Pattern::Error`, so `el` was bound to
            // nothing and every rule that needed its type saw an untyped
            // receiver.
            //
            // Nothing complained, because the effect checker had a by-spelling
            // fallback that found `getBoundingClientRect` without needing a
            // receiver at all. R-037 was caught for a reason unrelated to what
            // it tests — see `docs/RISK_QUEUE.md`.
            K::NameExpr | K::Name => {
                let name = text(self.src, node).trim().to_string();
                if name == "_" {
                    b.pat(Pattern::Wild, span)
                } else {
                    b.pat(
                        Pattern::Bind {
                            name,
                            mutable: false,
                        },
                        span,
                    )
                }
            }
            K::Param => {
                // `el` or `el: T` — the binding is the first child, and a
                // written type a sibling, kept beside the pattern (ADR-0138).
                let pat = match node.children().next() {
                    Some(c) => self.param_pattern(b, &c),
                    None => b.pat(Pattern::Error, span),
                };
                if let Some(t) = node.children().find(|c| c.kind() == K::TypeRef) {
                    let ty = self.type_ref(b, &t);
                    b.param_types.insert(pat, ty);
                }
                pat
            }
            // `(x) => ..`: one parameter in parentheses. It was read as a
            // descriptor, like `resumable(..)`, so `x` was bound to nothing
            // and its use did not resolve (PW0021) until 2026-09-26
            // (ADR-0066).
            K::ParenExpr => match node.children().next() {
                Some(c) => self.param_pattern(b, &c),
                None => b.pat(Pattern::Error, span),
            },
            K::ParamList | K::TupleExpr | K::ListExpr => {
                let args = node.children().map(|c| self.param_pattern(b, &c)).collect();
                b.pat(
                    Pattern::Ctor {
                        path: String::new(),
                        args,
                    },
                    span,
                )
            }
            _ => b.pat(Pattern::Error, span),
        }
    }

    /// Lower one markup node.
    fn markup(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> NodeId {
        let span = span_of(node);
        match node.kind() {
            K::Element => {
                let open = node.children().find(|c| c.kind() == K::OpenTag);
                let tag = open.as_ref().and_then(first_name).unwrap_or_default();
                // A canonical link's `href` is a page's address (ADR-0311).
                let canonical = tag == "link"
                    && open.as_ref().is_some_and(|o| {
                        o.children()
                            .filter(|c| c.kind() == K::Attr)
                            .any(|a| self.static_rel_names_canonical(&a))
                    });
                let attrs = open
                    .as_ref()
                    .map(|o| {
                        o.children()
                            .filter(|c| c.kind() == K::Attr)
                            .flat_map(|a| self.attrs(b, &a, canonical))
                            .collect()
                    })
                    .unwrap_or_default();
                let children: Vec<NodeId> = node
                    .children()
                    .filter(|c| is_markup(c.kind()))
                    .map(|c| self.markup(b, &c))
                    .collect();
                let self_closing = node.children().all(|c| c.kind() != K::CloseTag);
                b.node(
                    Node::Element {
                        tag,
                        attrs,
                        children,
                        self_closing,
                    },
                    span,
                )
            }
            K::Text => {
                let t = text(self.src, node);
                b.node(Node::Text(t), span)
            }
            K::MarkupBlock => {
                // The opening marker is the block's first interpolation, and
                // the closing one its last, when it was closed. Every `{:..}`
                // marker between them stays at its place, as a `Branch`: until
                // 2026-09-25 they were dropped here, and `{#if a}A{:else}B{/if}`
                // rendered A and B together (ADR-0042).
                let open = node.children().find(|c| c.kind() == K::Interpolation);
                let close = node
                    .children()
                    .filter(|c| c.kind() == K::Interpolation)
                    .last()
                    .filter(|c| Some(c) != open.as_ref())
                    .filter(|c| text(self.src, c).trim_start().starts_with("{/"));
                let directive = open.as_ref().map(|i| text(self.src, i)).unwrap_or_default();
                let subject = open.as_ref().and_then(|o| {
                    self.marker_expression(b, o, &["{#if", "{#match"], "a block's subject")
                });
                let each = open.as_ref().and_then(|o| self.each_head(b, o));
                let mut children = Vec::new();
                for c in node.children().filter(|c| is_markup(c.kind())) {
                    if Some(&c) == open.as_ref() || Some(&c) == close.as_ref() {
                        continue;
                    }
                    children.push(match is_block_marker(self.src, &c) {
                        true => self.branch(b, &c),
                        false => self.markup(b, &c),
                    });
                }
                b.node(
                    Node::Block {
                        directive,
                        children,
                        subject,
                        each,
                        close: close.map(|c| text(self.src, &c)).unwrap_or_default(),
                    },
                    span,
                )
            }
            K::Interpolation => {
                // A bare `{/each}` or `{:else}` outside a block: keep it, so
                // recovery still yields something walkable, and the markup
                // rules refuse it (PW5019).
                let raw = text(self.src, node);
                if raw.starts_with("{:") {
                    return self.branch(b, node);
                }
                if raw.starts_with("{#") || raw.starts_with("{/") {
                    return b.node(
                        Node::Block {
                            directive: raw,
                            children: vec![],
                            subject: None,
                            each: None,
                            close: String::new(),
                        },
                        span,
                    );
                }
                match node.children().find(|c| is_expr(c.kind())) {
                    Some(e) => {
                        let id = self.expr(b, &e);
                        b.node(Node::Interpolation(id), span)
                    }
                    None => b.node(Node::Text(raw), span),
                }
            }
            _ => b.node(Node::Text(text(self.src, node)), span),
        }
    }

    /// An attribute, or the two `bind:value={s}` is short for (ADR-0142):
    /// `value={s}`, and `on:input` with a handler setting `s` to what was
    /// typed. Lowered to them, so every rule reads them as it reads any
    /// attribute and handler. A binding of anything but a name stays as
    /// written, for the rule on what a binding binds to refuse.
    fn attrs(&mut self, b: &mut BodyBuilder, node: &SyntaxNode, canonical: bool) -> Vec<Attr> {
        let attr = self.attr(b, node, canonical);
        if attr.name != "bind:value" {
            return vec![attr];
        }
        let AttrValue::Expr(e) = attr.value else {
            return vec![attr];
        };
        let Some(Expr::Name(signal)) = b.exprs.get(e.index()).cloned() else {
            return vec![attr];
        };
        let span = attr.span.clone();
        let event = b.pat(
            Pattern::Bind {
                name: BOUND_EVENT.to_string(),
                mutable: false,
            },
            span.clone(),
        );
        let target = b.expr(Expr::Name(signal), span.clone());
        let read = b.expr(Expr::Name(BOUND_EVENT.to_string()), span.clone());
        let typed = b.expr(
            Expr::Field {
                base: read,
                name: "value".to_string(),
            },
            span.clone(),
        );
        let set = b.expr(
            Expr::Binary {
                op: BinOp::Assign,
                lhs: target,
                rhs: typed,
            },
            span.clone(),
        );
        let handler = b.expr(
            Expr::Lambda {
                descriptor: None,
                params: vec![event],
                body: set,
            },
            span.clone(),
        );
        b.bound.insert(handler);
        vec![
            Attr {
                name: "value".to_string(),
                value: AttrValue::Expr(e),
                span: span.clone(),
            },
            Attr {
                name: "on:input".to_string(),
                value: AttrValue::Expr(handler),
                span,
            },
        ]
    }

    /// Whether an attribute is `rel`, written as text that names `canonical`
    /// among its relations, as HTML compares keywords, ignoring ASCII case.
    fn static_rel_names_canonical(&self, node: &SyntaxNode) -> bool {
        let named_rel = node
            .children()
            .find(|c| c.kind() == K::AttrName)
            .is_some_and(|n| n.text() == "rel");
        let Some(v) = node.children().find(|c| c.kind() == K::AttrValue) else {
            return false;
        };
        named_rel
            && v.children().all(|c| c.kind() != K::Interpolation)
            && crate::canonical::names_canonical(
                text(self.src, &v).trim_matches(|c| c == '"' || c == '\''),
            )
    }

    /// **A page's address** (ADR-0311): a call of a path, `StorePage(id)`, in
    /// a canonical link's `href`, lowered as an address and not a call. Its
    /// page stays the path as written, never an expression a name check
    /// would read as a value. `None` for anything else, which is lowered as
    /// written and refused by PW5051.
    fn address(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> Option<ExprId> {
        if node.kind() != K::CallExpr {
            return None;
        }
        let callee = node.children().next().filter(names_a_path)?;
        let page = text(self.src, &callee)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let args = node
            .children()
            .find(|c| c.kind() == K::ArgList)
            .map(|l| self.args(b, &l))
            .unwrap_or_default();
        Some(b.expr(Expr::Address { page, args }, span_of(node)))
    }

    fn attr(&mut self, b: &mut BodyBuilder, node: &SyntaxNode, canonical: bool) -> Attr {
        let name = node
            .children()
            .find(|c| c.kind() == K::AttrName)
            .map(|n| n.text().to_string())
            .unwrap_or_default();
        let value = match node.children().find(|c| c.kind() == K::AttrValue) {
            None => AttrValue::None,
            Some(v) => match v.children().find(|c| c.kind() == K::Interpolation) {
                Some(i) => match i.children().find(|c| is_expr(c.kind())) {
                    Some(e) if canonical && name == "href" => match self.address(b, &e) {
                        Some(address) => AttrValue::Expr(address),
                        None => AttrValue::Expr(self.expr(b, &e)),
                    },
                    Some(e) => AttrValue::Expr(self.expr(b, &e)),
                    None => AttrValue::None,
                },
                // `href="/stores/{id}"`: a string with holes, lowered as one,
                // so what reads a string's holes reads these (ADR-0042). Until
                // 2026-09-25 it stayed static text with braces in it.
                None => {
                    let raw = text(self.src, &v);
                    let parts = match raw.contains('{') {
                        true => self.interpolations(b, &raw, span_of(&v).start),
                        false => Vec::new(),
                    };
                    match parts.is_empty() {
                        true => AttrValue::Static(v.text().to_string()),
                        false => AttrValue::Expr(
                            b.expr(Expr::Interpolated { text: raw, parts }, span_of(&v)),
                        ),
                    }
                }
            },
        };
        Attr {
            name,
            value,
            span: span_of(node),
        }
    }

    fn pattern(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> PatternId {
        let span = span_of(node);
        match node.kind() {
            K::WildcardPat => b.pat(Pattern::Wild, span),
            K::BindingPat => b.pat(
                Pattern::Bind {
                    name: text(self.src, node).trim().to_string(),
                    mutable: false,
                },
                span,
            ),
            K::CtorPat => {
                let path =
                    first_name(node).unwrap_or_else(|| text(self.src, node).trim().to_string());
                let args = node
                    .children()
                    .filter(|c| is_pattern(c.kind()))
                    .map(|c| self.pattern(b, &c))
                    .collect();
                b.pat(Pattern::Ctor { path, args }, span)
            }
            K::LiteralPat => {
                let t = own_tokens(node);
                // `-1`: the sign, then the number (ADR-0060).
                let (sign, t) = match t.split_first() {
                    Some((m, rest)) if m.kind() == K::Minus => ("-", rest),
                    _ => ("", t.as_slice()),
                };
                let lit = match t.first().map(|t| (t.kind(), t.text().to_string())) {
                    Some((K::Int, s)) => Literal::Int(format!("{sign}{}", s.replace('_', ""))),
                    Some((K::Float, s)) => Literal::Float(format!("{sign}{}", s.replace('_', ""))),
                    Some((K::Str, s)) if sign.is_empty() => Literal::Str(s),
                    _ => return b.pat(Pattern::Error, span),
                };
                b.pat(Pattern::Literal(lit), span)
            }
            K::OrPat | K::TuplePat => {
                let args: Vec<_> = node
                    .children()
                    .filter(|c| is_pattern(c.kind()))
                    .map(|c| self.pattern(b, &c))
                    .collect();
                if node.kind() == K::OrPat {
                    b.pat(Pattern::Or(args), span)
                } else {
                    b.pat(
                        Pattern::Ctor {
                            path: String::new(),
                            args,
                        },
                        span,
                    )
                }
            }
            _ => b.pat(Pattern::Error, span),
        }
    }

    fn type_ref(&mut self, b: &mut BodyBuilder, node: &SyntaxNode) -> TypeRefId {
        let span = span_of(node);
        let path = first_name(node).unwrap_or_else(|| text(self.src, node).trim().to_string());
        let args = node
            .children()
            .find(|c| matches!(c.kind(), K::TypeArgList | K::FnTypeArgs))
            .map(|l| {
                l.children()
                    .filter(|c| c.kind() == K::TypeRef)
                    .map(|c| self.type_ref(b, &c))
                    .collect()
            })
            .unwrap_or_default();
        b.ty(TypeRef { path, args }, span)
    }
}

/// The names an `import ... { A, B }` brings into scope.
///
/// The grammar skips the brace list as a balanced range, so the names are read
/// from the declaration's own tokens. That is enough for resolution and avoids
/// a grammar change; E2B's gate is about the *graph*, not about how the list is
/// spelled.
fn imported_names(node: &SyntaxNode) -> Vec<String> {
    if node.kind() != K::ImportDecl {
        return Vec::new();
    }
    let toks = node
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| !t.kind().is_trivia())
        .collect::<Vec<_>>();
    let Some(open) = toks.iter().position(|t| t.kind() == K::LBrace) else {
        return Vec::new();
    };
    toks[open + 1..]
        .iter()
        .take_while(|t| t.kind() != K::RBrace)
        .filter(|t| t.kind() == K::Ident)
        .map(|t| t.text().to_string())
        .collect()
}

/// The visibility keyword a declaration opens with, if any.
///
/// The grammar keeps it as a bare token, so it is the declaration's first
/// significant token when it is one of the four. `user` (track
/// `store-accounts`) is the grammar's only where a declaration's keyword
/// follows it, so as a declaration's first token it is its visibility.
fn visibility_of(node: &SyntaxNode) -> Option<String> {
    let first = own_tokens(node).first()?.text().to_string();
    matches!(first.as_str(), "public" | "session" | "private" | "user").then_some(first)
}

/// Convert a remaining source-text type slot through the authoritative parser.
/// Effects still expose written fragments in HIR; variant payloads (ADR-0059)
/// and opaque representations (ADR-0054) are trees since 2026-09-26. All
/// semantic consumers receive the resulting recursive tree, never a splitter.
pub(crate) fn type_fragment(src: &str) -> Option<crate::hir::DeclaredType> {
    let parsed = pw_syntax::parse_type(src);
    if !parsed.ok() {
        return None;
    }
    parsed
        .green
        .children()
        .find(|n| n.kind() == K::TypeRef)
        .map(|n| declared_type(&n))
}

/// Preserve the parser's complete type structure. No consumer reparses a
/// nested argument to recover information lost at the syntax/HIR boundary.
fn declared_type(t: &SyntaxNode) -> crate::hir::DeclaredType {
    // `fn(A, B) -> R` keeps its parameter types then its result, in order, as
    // the `fn` constructor's arguments.
    let args = t
        .children()
        .find(|c| matches!(c.kind(), K::TypeArgList | K::FnTypeArgs))
        .map(|l| {
            l.children()
                .filter(|c| c.kind() == K::TypeRef)
                .map(|a| declared_type(&a))
                .collect()
        })
        .unwrap_or_default();
    crate::hir::DeclaredType::new(type_path(t), args)
}

/// **An opaque type's invariant, as bounds on its `value`** (ADR-0179):
/// each comparison of `value` with an integer literal, joined by `&`; or on
/// its length, `String.length(value)` compared with one (ADR-0225). Any other
/// part is kept, with why, for PW0623 to refuse.
fn invariant(src: &str, node: &SyntaxNode) -> Option<crate::hir::Invariant> {
    if node.kind() != K::OpaqueDecl {
        return None;
    }
    let clause = node.children().find(|c| c.kind() == K::Invariant)?;
    let predicate = clause.children().find(|c| is_expr(c.kind()));
    let mut out = crate::hir::Invariant {
        measure: crate::hir::Measure::Value,
        at_least: None,
        at_most: None,
        written: predicate
            .as_ref()
            .map(|p| text(src, p).trim().to_string())
            .unwrap_or_default(),
        span: span_of(&clause),
        unread: Vec::new(),
    };
    let mut measured = None;
    match predicate {
        Some(p) => bounds(src, &p, &mut out, &mut measured),
        None => out
            .unread
            .push((span_of(&clause), "`where` states no predicate".to_string())),
    }
    out.measure = measured.unwrap_or_default();
    Some(out)
}

/// The bounds `e` states on `value`, or on its length, into `out`; and what
/// they are on, into `measured`, which every bound states alike.
fn bounds(
    src: &str,
    e: &SyntaxNode,
    out: &mut crate::hir::Invariant,
    measured: &mut Option<crate::hir::Measure>,
) {
    let kids: Vec<SyntaxNode> = e.children().filter(|c| is_expr(c.kind())).collect();
    let unread = |out: &mut crate::hir::Invariant, why: &str| {
        out.unread.push((span_of(e), why.to_string()));
    };
    match e.kind() {
        K::ParenExpr if kids.len() == 1 => bounds(src, &kids[0], out, measured),
        K::BinaryExpr if kids.len() == 2 => {
            let Some(op) = own_tokens(e).into_iter().next() else {
                return unread(out, "a bound needs its comparison");
            };
            if op.kind() == K::Amp {
                bounds(src, &kids[0], out, measured);
                bounds(src, &kids[1], out, measured);
                return;
            }
            let compared = matches!(op.kind(), K::Cmp | K::LAngle | K::RAngle)
                && matches!(op.text(), "<" | "<=" | ">" | ">=");
            if !compared {
                return unread(
                    out,
                    "a bound compares `value` with `<`, `<=`, `>` or `>=`, and bounds are \
                     joined by `&`",
                );
            }
            // What a side bounds: `value`, or its length (ADR-0225), as
            // written, whatever the spacing.
            let subject = |n: &SyntaxNode| -> Option<crate::hir::Measure> {
                let written: String = text(src, n).split_whitespace().collect();
                match written.as_str() {
                    "value" if n.kind() == K::NameExpr => Some(crate::hir::Measure::Value),
                    "String.length(value)" => Some(crate::hir::Measure::Length),
                    _ => None,
                }
            };
            // `value >= 1`, or `1 <= value` read the other way round.
            let (measure, op, k) = match (subject(&kids[0]), integer(&kids[1])) {
                (Some(m), Some(k)) => (m, op.text().to_string(), k),
                _ => match (integer(&kids[0]), subject(&kids[1])) {
                    (Some(k), Some(m)) => {
                        let flipped = match op.text() {
                            "<" => ">",
                            "<=" => ">=",
                            ">" => "<",
                            _ => "<=",
                        };
                        (m, flipped.to_string(), k)
                    }
                    _ => {
                        return unread(
                            out,
                            "a bound compares `value`, or `String.length(value)`, with an \
                             integer",
                        );
                    }
                },
            };
            match measured {
                Some(m) if *m != measure => {
                    return unread(
                        out,
                        "an invariant bounds one thing: `value`, or `String.length(value)`",
                    );
                }
                _ => *measured = Some(measure),
            }
            match op.as_str() {
                ">=" => out.at_least = Some(out.at_least.map_or(k, |lo| lo.max(k))),
                ">" => out.at_least = Some(out.at_least.map_or(k + 1, |lo| lo.max(k + 1))),
                "<=" => out.at_most = Some(out.at_most.map_or(k, |hi| hi.min(k))),
                _ => out.at_most = Some(out.at_most.map_or(k - 1, |hi| hi.min(k - 1))),
            }
        }
        _ => unread(
            out,
            "an invariant is bounds on `value`: `value >= 1`, joined by `&`",
        ),
    }
}

/// An integer literal, or one negated: `1`, `-5`.
fn integer(n: &SyntaxNode) -> Option<i128> {
    match n.kind() {
        K::LiteralExpr => {
            let t = own_tokens(n).into_iter().next()?;
            (t.kind() == K::Int)
                .then(|| t.text().replace('_', "").parse::<i128>().ok())
                .flatten()
        }
        K::UnaryExpr => {
            let op = own_tokens(n).into_iter().next()?;
            let inner = n.children().find(|c| is_expr(c.kind()))?;
            (op.kind() == K::Minus)
                .then(|| integer(&inner).map(|k| -k))
                .flatten()
        }
        K::ParenExpr => integer(&n.children().find(|c| is_expr(c.kind()))?),
        _ => None,
    }
}

fn type_path(t: &SyntaxNode) -> String {
    t.children()
        .find(|c| c.kind() == K::Name)
        .map(|n| n.text().to_string())
        .unwrap_or_else(|| t.text().to_string().trim().to_string())
}

fn bin_op(kind: K, text: &str) -> BinOp {
    match kind {
        K::Plus => BinOp::Add,
        K::Minus => BinOp::Sub,
        K::Star => BinOp::Mul,
        K::Slash => BinOp::Div,
        K::Percent => BinOp::Rem,
        K::Amp => BinOp::And,
        K::Pipe => BinOp::Or,
        K::PipeGt => BinOp::Pipe,
        K::Arrow => BinOp::Transition,
        K::Eq => BinOp::Assign,
        K::Cmp | K::LAngle | K::RAngle => BinOp::Cmp(text.to_string()),
        _ => BinOp::Cmp(text.to_string()),
    }
}

fn is_expr(k: K) -> bool {
    matches!(
        k,
        K::BlockExpr
            | K::LiteralExpr
            | K::NameExpr
            | K::FieldExpr
            | K::CallExpr
            | K::LambdaExpr
            | K::LetStmt
            | K::IfExpr
            | K::MatchExpr
            | K::ForExpr
            | K::BinaryExpr
            | K::CastExpr
            | K::UnaryExpr
            | K::ParenExpr
            | K::RecordExpr
            | K::TupleExpr
            | K::ListExpr
            | K::TryExpr
            | K::ErrorExpr
            | K::TemplateRegion
            | K::Field
    )
}

/// `Some(x)` in `{:Some(x)}`: a constructor and at most one name it binds.
/// `None` for anything else, `else` included, which the markup rules judge.
fn constructor_arm(inner: &str) -> Option<crate::hir::TemplateArm> {
    let ident = |s: &str| {
        let mut c = s.chars();
        c.next().is_some_and(|f| f.is_alphabetic() || f == '_')
            && c.all(|x| x.is_alphanumeric() || x == '_')
    };
    let (name, fields) = match inner.split_once('(') {
        Some((n, rest)) => (n.trim(), Some(rest.strip_suffix(')')?.trim())),
        None => (inner, None),
    };
    // `Circle`, or `Shape.Circle` through its type (ADR-0061): a case by
    // its capital, and each field a binding by its own (ADR-0195, ruling 2).
    // `{:Some(None)}` bound a name `None` until ADR-0197; an arm takes one
    // case apart, and a field that is a case is a nested pattern no arm has.
    let case = name.rsplit('.').next().unwrap_or(name);
    if pw_syntax::pattern_kind(case) != pw_syntax::PatternKind::Case || !name.split('.').all(ident)
    {
        return None;
    }
    let bindings = match fields {
        None => Vec::new(),
        Some(f) => {
            let names: Vec<&str> = f.split(',').map(str::trim).collect();
            if !names
                .iter()
                .all(|n| ident(n) && pw_syntax::pattern_kind(n) == pw_syntax::PatternKind::Binding)
            {
                return None;
            }
            names.into_iter().map(str::to_string).collect()
        }
    };
    Some(crate::hir::TemplateArm {
        case: name.to_string(),
        bindings,
    })
}

/// Is this node one of a block's own `{#..}` / `{/..}` / `{:..}` markers?
fn is_block_marker(src: &str, n: &SyntaxNode) -> bool {
    n.kind() == K::Interpolation && {
        let t = text(src, n);
        let t = t.trim_start();
        t.starts_with("{#") || t.starts_with("{/") || t.starts_with("{:")
    }
}

/// One markup node and everything under it.
fn is_markup(k: K) -> bool {
    matches!(k, K::Element | K::Text | K::Interpolation | K::MarkupBlock)
}

/// Can this node stand in a lambda's parameter position as a binding?
/// Anything else there is an annotation, kept as an expression.
fn is_param_shaped(k: K) -> bool {
    matches!(
        k,
        K::NameExpr | K::ParamList | K::TupleExpr | K::ListExpr | K::ParenExpr
    )
}

fn is_pattern(k: K) -> bool {
    matches!(
        k,
        K::WildcardPat | K::BindingPat | K::CtorPat | K::LiteralPat | K::OrPat | K::TuplePat
    )
}
