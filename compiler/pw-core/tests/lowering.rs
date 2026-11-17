//! Lowering the corpus into HIR.
//!
//! The properties here are the ones a checker depends on. Each has a control:
//! four of the measurement bugs in `docs/RISK_QUEUE.md` produced favourable
//! results while measuring nothing, and a lowering pass that silently drops a
//! subtree fails in exactly that shape — every checker downstream then reports
//! "no violations" on a body it never saw.

use pw_core::hir::*;
use pw_core::lower::lower_file;
use pw_syntax::kind::SyntaxKind as K;
use pw_syntax::parse_tree;

fn corpus() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .canonicalize()
        .expect("examples/ exists");
    let mut out = Vec::new();
    for dir in ["accepted", "rejected"] {
        for e in std::fs::read_dir(root.join(dir)).expect("corpus dir") {
            let p = e.expect("entry").path();
            if p.extension().is_some_and(|x| x == "pw") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn lower(src: &str) -> Hir {
    let p = parse_tree(src);
    assert!(p.ok(), "test source must parse: {:?}", p.errors);
    lower_file(src, &p.green)
}

/// Every expression in every body, with its span.
fn all_exprs(hir: &Hir) -> Vec<(&Expr, Span)> {
    hir.bodies
        .iter()
        .flat_map(|(_, b, _)| b.exprs().map(|(_, e, s)| (e, s.clone())))
        .collect()
}

#[test]
fn the_generic_callback_case_survives_lowering() {
    // docs/NEXT.md calls this load-bearing: an effect reaching the program
    // through a callback the callee knows nothing about. If the lambda body
    // is misassociated or dropped, an effect checker inspects the wrong
    // expression and still reports something plausible.
    let src = "fn f(items: List<Id>) -> List<Store> !{ database.read } {\n    List.map(items, item => database.read(item.id))\n}\n";
    let hir = lower(src);

    let (_, decl) = hir.all_decls().next().expect("one declaration");
    let body = hir.body(decl.body.expect("a body"));

    // The lambda must be an argument of the call, not a sibling of it.
    let (call_id, args) = body
        .walk()
        .into_iter()
        .find_map(|id| match body.expr(id) {
            Expr::Call { args, .. } if args.len() == 2 => Some((id, args.clone())),
            _ => None,
        })
        .expect("the two-argument call to List.map");
    let lambda_id = args[1].value;
    assert!(
        matches!(body.expr(lambda_id), Expr::Lambda { .. }),
        "the second argument must be the lambda, got {:?}",
        body.expr(lambda_id)
    );
    assert!(
        body.children(call_id).contains(&lambda_id),
        "the lambda must be reachable as a child of the call"
    );

    // And the effectful call inside the lambda must be reachable from it, or
    // an effect walk starting at the body root would never find it.
    let Expr::Lambda { body: inner, .. } = body.expr(lambda_id) else {
        unreachable!()
    };
    let inner_calls: Vec<String> = std::iter::once(*inner)
        .chain(body.children(*inner))
        .filter_map(|id| match body.expr(id) {
            Expr::Call { callee, .. } => Some(render_path(body, *callee)),
            _ => None,
        })
        .collect();
    assert!(
        inner_calls.iter().any(|c| c == "database.read"),
        "the lambda body must contain the database.read call, saw {inner_calls:?}"
    );

    // Control: the same walk on a body WITHOUT the callback must not find it.
    let clean = lower("fn f() -> Int !{} {\n    List.map(items, item => item.id)\n}\n");
    let (_, d2) = clean.all_decls().next().unwrap();
    let b2 = clean.body(d2.body.unwrap());
    assert!(
        !b2.walk().iter().any(|id| matches!(
            b2.expr(*id),
            Expr::Call { callee, .. } if render_path(b2, *callee) == "database.read"
        )),
        "the control body must not report a database call"
    );
}

/// `a.b.c` as written, reassembled from nested `Field` nodes.
fn render_path(body: &Body, id: ExprId) -> String {
    match body.expr(id) {
        Expr::Name(n) => n.clone(),
        Expr::Field { base, name } => format!("{}.{}", render_path(body, *base), name),
        _ => String::new(),
    }
}

#[test]
fn a_dotted_path_lowers_to_nested_fields_not_one_name() {
    // ADR-0014: lowering does not decide whether `Stores.get` is a module path
    // or a field access on a local. That is name resolution's call.
    let hir = lower("fn f() !{} {\n    Stores.get(id)\n}\n");
    let (_, d) = hir.all_decls().next().unwrap();
    let b = hir.body(d.body.unwrap());
    let field = b
        .walk()
        .into_iter()
        .find(|id| matches!(b.expr(*id), Expr::Field { .. }))
        .expect("a field access");
    assert_eq!(render_path(b, field), "Stores.get");
    let Expr::Field { base, .. } = b.expr(field) else {
        unreachable!()
    };
    assert!(matches!(b.expr(*base), Expr::Name(n) if n == "Stores"));
}

#[test]
fn a_resumable_handler_keeps_its_capture_descriptor() {
    // `resumable(captures = { item }) => ..` is call-shaped in parameter
    // position. It was silently dropped until the coverage property below
    // caught it — and it is exactly what R-010 (a non-serializable capture)
    // and R-030 (private data in a resume manifest) must read.
    let src = "view V() !{} {\n    <button on:press={resumable(captures = { item }) => add(item)} />\n}\n";
    let hir = lower(src);
    let (_, d) = hir.all_decls().next().unwrap();
    let b = hir.body(d.body.unwrap());

    let (desc, body_id) = b
        .walk()
        .into_iter()
        .find_map(|id| match b.expr(id) {
            Expr::Lambda {
                descriptor: Some(d),
                body,
                ..
            } => Some((*d, *body)),
            _ => None,
        })
        .expect("a lambda carrying a descriptor");

    // The span stops at the descriptor: trailing trivia belongs to the
    // lambda that follows, not to the call.
    assert_eq!(&src[b.expr_span(desc)], "resumable(captures = { item })");
    let Expr::Call { callee, args } = b.expr(desc) else {
        panic!("the descriptor must stay a call: {:?}", b.expr(desc))
    };
    assert_eq!(render_path(b, *callee), "resumable");
    assert_eq!(args[0].name.as_deref(), Some("captures"));
    // The handler body is still reachable — the descriptor did not replace it.
    assert!(matches!(b.expr(body_id), Expr::Call { .. }));

    // Control: an ordinary lambda has no descriptor.
    let plain = lower("fn f() !{} { g(x => x) }\n");
    let (_, pd) = plain.all_decls().next().unwrap();
    let pb = plain.body(pd.body.unwrap());
    assert!(pb.walk().iter().any(|id| matches!(
        pb.expr(*id),
        Expr::Lambda {
            descriptor: None,
            ..
        }
    )));
}

#[test]
fn every_span_points_inside_its_own_source() {
    // A span that is out of range, or belongs to a different file, produces a
    // diagnostic that underlines the wrong text — or panics the renderer.
    for path in corpus() {
        let src = std::fs::read_to_string(&path).expect("read");
        let p = parse_tree(&src);
        let hir = lower_file(&src, &p.green);
        let name = path.file_name().unwrap().to_string_lossy();

        for (_, body, _) in hir.bodies.iter() {
            for (id, expr, span) in body.exprs() {
                assert!(
                    span.end <= src.len() && span.start <= span.end,
                    "{name}: {expr:?} has span {span:?} outside a {}-byte file",
                    src.len()
                );
                // Charter §16.3: a diagnostic needs an origin span. An id that
                // cannot produce one is unusable, so the accessor must agree.
                assert_eq!(body.expr_span(id), *span);
            }
        }
    }
}

#[test]
fn no_call_lambda_or_match_in_the_tree_is_lost_in_lowering() {
    // The coverage property. A lowering that quietly skipped a node kind would
    // pass every test above — they all assert what IS there, never what is
    // missing. This asserts the converse across all 68 corpus files.
    let mut missing: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for path in corpus() {
        let src = std::fs::read_to_string(&path).expect("read");
        let p = parse_tree(&src);
        let hir = lower_file(&src, &p.green);
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let lowered: std::collections::HashSet<(usize, usize)> = all_exprs(&hir)
            .iter()
            .map(|(_, s)| (s.start, s.end))
            .collect();

        for node in p.green.descendants() {
            if !matches!(
                node.kind(),
                K::CallExpr | K::LambdaExpr | K::MatchExpr | K::FieldExpr
            ) {
                continue;
            }
            // Nodes inside a declaration signature (a default value in a
            // parameter list) are not part of any body.
            if node.ancestors().all(|a| a.kind() != K::Body) {
                continue;
            }
            checked += 1;
            let r = node.text_range();
            let key = (usize::from(r.start()), usize::from(r.end()));
            if !lowered.contains(&key) {
                missing.push(format!("{name}: {:?} at {key:?}", node.kind()));
            }
        }
    }

    assert!(checked > 200, "expected a real sample, checked {checked}");
    assert!(
        missing.is_empty(),
        "{} of {checked} expression nodes never reached HIR:\n{}",
        missing.len(),
        missing
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// **Every executable expression belongs to exactly one named execution root.**
///
/// Architect ruling, 2026-08-11, stating the invariant the `TermRoot` model
/// exists for:
///
/// > A `TermRoot` should own an expression tree, its lexical binders, and its
/// > initial `ExecutionContext`. […] Never: call-shaped thing → nobody owns it
/// > → analyses see nothing.
///
/// **Exactly one**, not at least one. A tree reachable from two roots would be
/// inferred twice and attributed to both contexts, which is the failure the
/// separation exists to prevent: an optimistic transition merged into a
/// command's row is indistinguishable from the command performing it.
#[test]
fn no_expression_belongs_to_two_named_roots() {
    for path in corpus() {
        let src = std::fs::read_to_string(&path).expect("read");
        let hir = lower_file(&src, &parse_tree(&src).green);
        let name = path.file_name().unwrap().to_string_lossy();

        for (_, d) in hir.all_decls() {
            let Some(b) = d.body else { continue };
            let body = hir.body(b);
            let mut owner: std::collections::HashMap<ExprId, &'static str> =
                std::collections::HashMap::new();
            for id in body.walk() {
                owner.insert(id, "the declaration body");
            }
            for (_, root) in d.term_roots() {
                for id in body.walk_from(root.root) {
                    if let Some(first) = owner.insert(id, root.context.name()) {
                        panic!(
                            "{name}: `{}` — an expression is reachable from both {first} and \
                             {}, so its effects would be attributed to both",
                            d.name,
                            root.context.name()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn every_allocated_expression_has_exactly_one_named_root() {
    // Orphans mean a subtree was lowered but never attached. A checker that
    // walks from the root would miss it, while a test that iterates the arena
    // would not notice — so this compares the two.
    //
    // **There is now one deliberate second root, and it is named here rather
    // than exempted.** A policy whose domain is executable code — `optimistic`,
    // `rollback` — lowers its value into the same arena and is reachable from
    // `Policy::term`, NOT from `Body::root`. Architect ruling, 2026-08-10: such
    // a value is a real term and must be resolved like one, and it runs in a
    // different execution context, so the declaration's effect row must not
    // absorb it. Reachable-from-root would have given the second for free with
    // the first.
    //
    // The invariant is unchanged in substance: nothing is allocated that no
    // named root reaches. Widening it to "or anywhere" would be the weakening
    // this test exists to prevent.
    for path in corpus() {
        let src = std::fs::read_to_string(&path).expect("read");
        let p = parse_tree(&src);
        let hir = lower_file(&src, &p.green);
        let name = path.file_name().unwrap().to_string_lossy();

        // Which body belongs to which declaration, so a policy term can be
        // attributed to the arena it was allocated in.
        let mut roots: std::collections::HashMap<usize, Vec<ExprId>> =
            std::collections::HashMap::new();
        for (_, d) in hir.all_decls() {
            let Some(b) = d.body else { continue };
            roots
                .entry(b.index())
                .or_default()
                .extend(d.term_roots().map(|(_, r)| r.root));
        }

        for (bi, body, _) in hir.bodies.iter() {
            let mut reachable: std::collections::HashSet<ExprId> =
                body.walk().into_iter().collect();
            for t in roots.get(&bi).into_iter().flatten() {
                reachable.extend(body.walk_from(*t));
            }
            let orphans: Vec<_> = body
                .exprs()
                .filter(|(id, _, _)| !reachable.contains(id))
                .map(|(_, e, s)| format!("{e:?} at {s:?}"))
                .collect();
            assert!(
                orphans.is_empty(),
                "{name} body {bi}: {} orphaned expressions:\n{}",
                orphans.len(),
                orphans
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }
}

/// **A policy term is NOT reachable from the body's root**, and that is the
/// property the execution-context separation rests on.
///
/// Without this, `every_allocated_expression_has_exactly_one_named_root` above
/// would pass just as happily for a lowering that attached policy terms to the
/// block — and every body-walking analysis would silently start charging a
/// command for code that runs on the client.
#[test]
fn an_embedded_policy_term_is_not_reachable_from_the_body_root() {
    let src = "\
module m

type Cart = Cart { n: Int }
type CartError = CartError { why: String }
opaque type MenuItemId = String

command add(item: MenuItemId) -> Result<Cart, CartError>
    requires   SignedIn
    optimistic Cart(item) as cart => cart
{
    todo
}
";
    let hir = lower(src);
    let (_, d) = hir.all_decls().find(|(_, d)| d.name == "add").expect("add");
    let body = hir.body(d.body.expect("body"));

    let terms: Vec<ExprId> = d.term_roots().map(|(_, r)| r.root).collect();
    assert_eq!(
        terms.len(),
        2,
        "the target and the transition are two NAMED ROOTS, not one tree with a \
         special case inside it (ADR-0025)"
    );
    let contexts: Vec<pw_core::hir::ExecutionContext> =
        d.term_roots().map(|(_, r)| r.context).collect();
    assert_eq!(
        contexts,
        [
            pw_core::hir::ExecutionContext::TargetSelection,
            pw_core::hir::ExecutionContext::OptimisticTransition
        ],
        "and each carries its own context"
    );

    let from_root: std::collections::HashSet<ExprId> = body.walk().into_iter().collect();
    for t in &terms {
        assert!(
            !from_root.contains(t),
            "a policy term is reachable from the body root, so every walk now \
             sees client-side code as part of the command"
        );
    }

    // And it IS reachable from the policy, so nothing is lost.
    assert!(
        body.walk_from(terms[0])
            .iter()
            .any(|id| matches!(body.expr(*id), pw_core::hir::Expr::Call { .. })),
        "the term is a real expression tree"
    );
}

#[test]
fn declared_effect_rows_reach_hir_with_their_spans() {
    let src = "fn load() -> Store !{ database.read<Stores>, trace } {\n    Stores.get(id)\n}\n";
    let hir = lower(src);
    let (_, d) = hir.all_decls().next().unwrap();
    let row = d.declared_effects.as_ref().expect("a declared row");
    assert_eq!(
        row.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
        ["database.read", "trace"],
        "the type argument is not part of the effect's identity"
    );
    // The span must underline the effect, not the whole row.
    assert_eq!(&src[row[0].span.clone()], "database.read<Stores>");

    // `!{}` is an explicit claim of purity and must not be confused with an
    // absent row: one is "I declare nothing", the other is "I declare none".
    let pure = lower("fn f() -> Int !{} { 1 }\n");
    let (_, pd) = pure.all_decls().next().unwrap();
    assert_eq!(pd.declared_effects.as_deref(), Some(&[][..]));

    let none = lower("component C() {\n    placement browser\n}\n");
    let (_, nd) = none.all_decls().next().unwrap();
    assert!(nd.declared_effects.is_none());
}

#[test]
fn a_nested_fn_becomes_a_child_declaration_with_its_own_body() {
    let src = "component C() {\n    placement browser\n\n    fn load() -> Store !{ database.read } {\n        Stores.get(id)\n    }\n}\n";
    let hir = lower(src);

    let (_, comp) = hir
        .all_decls()
        .find(|(_, d)| d.kind == DeclKind::Component)
        .expect("the component");
    assert_eq!(comp.children.len(), 1, "the nested fn is a child decl");

    let inner = hir.decl(comp.children[0]);
    assert_eq!(inner.name, "load");
    assert_eq!(inner.kind, DeclKind::Fn);
    assert_eq!(
        inner
            .declared_effects
            .as_ref()
            .map(|r| r.iter().map(|e| e.path.clone()).collect::<Vec<_>>()),
        Some(vec!["database.read".to_string()]),
        "the row belongs to the nested fn, not the component"
    );
    assert!(inner.body.is_some(), "and it has its own body");
}

#[test]
fn named_arguments_keep_their_names() {
    // `retry transport_only(max = 2, jitter = true)` — the rule that rejects an
    // unbounded retry reads `max`, so losing the name loses the rule.
    let hir = lower("fn f() !{} {\n    g(1, max = 2, jitter: true)\n}\n");
    let (_, d) = hir.all_decls().next().unwrap();
    let b = hir.body(d.body.unwrap());
    let args = b
        .walk()
        .into_iter()
        .find_map(|id| match b.expr(id) {
            Expr::Call { args, .. } if args.len() == 3 => Some(args.clone()),
            _ => None,
        })
        .expect("the three-argument call");
    let names: Vec<Option<&str>> = args.iter().map(|a| a.name.as_deref()).collect();
    assert_eq!(names, [None, Some("max"), Some("jitter")]);
}

#[test]
fn lowering_is_total_on_a_body_it_cannot_fully_parse() {
    // ADR-0014 property 4. A file with one broken expression must still yield
    // a walkable body for everything else, or one typo blinds every checker.
    let src = "fn f() !{} {\n    let a = @@@\n    database.read(id)\n}\n";
    let p = parse_tree(src);
    assert!(!p.ok(), "this source is meant to be broken");
    let hir = lower_file(src, &p.green);

    let (_, d) = hir.all_decls().next().expect("the fn still lowers");
    let b = hir.body(d.body.expect("with a body"));
    let paths: Vec<String> = b
        .walk()
        .into_iter()
        .filter_map(|id| match b.expr(id) {
            Expr::Call { callee, .. } => Some(render_path(b, *callee)),
            _ => None,
        })
        .collect();
    assert!(
        paths.iter().any(|p| p == "database.read"),
        "the good expression after the broken one must survive, saw {paths:?}"
    );
    assert!(
        b.walk().iter().any(|id| matches!(b.expr(*id), Expr::Error)),
        "and the broken one must be recorded as an error, not dropped"
    );
}

#[test]
fn the_whole_corpus_lowers_without_panicking() {
    let mut bodies = 0;
    let mut exprs = 0;
    for path in corpus() {
        let src = std::fs::read_to_string(&path).expect("read");
        let p = parse_tree(&src);
        let hir = lower_file(&src, &p.green);
        assert!(
            hir.modules.len() == 1,
            "{:?} must yield one module",
            path.file_name().unwrap()
        );
        bodies += hir.bodies.len();
        exprs += all_exprs(&hir).len();
    }
    // Asserted, not printed: a lowering that produced almost nothing would
    // otherwise satisfy every property above vacuously.
    assert!(
        bodies >= 68,
        "expected at least one body per file, got {bodies}"
    );
    assert!(
        exprs >= 1000,
        "expected a substantial HIR, got {exprs} expressions"
    );
}

#[test]
fn markup_lowers_to_a_nested_node_tree_with_its_attributes() {
    // E3 lowers this to a renderer and E5 reads its attributes, so the tree
    // has to survive lowering as a tree — not as the flat list of interpolated
    // expressions that was all HIR carried before.
    let src = "view V() !{} {\n    <main class=\"page\"><button on:press={add(item)}>Add {label}</button></main>\n}\n";
    let hir = lower(src);
    let (_, d) = hir.all_decls().next().unwrap();
    let b = hir.body(d.body.unwrap());

    let (parts, roots) = b
        .walk()
        .into_iter()
        .find_map(|id| match b.expr(id) {
            Expr::Template { parts, roots } => Some((parts.clone(), roots.clone())),
            _ => None,
        })
        .expect("a template");

    assert_eq!(roots.len(), 1, "one root element");
    let Node::Element {
        tag,
        attrs,
        children,
        ..
    } = b.node(roots[0])
    else {
        panic!("expected an element, got {:?}", b.node(roots[0]))
    };
    assert_eq!(tag, "main");
    assert!(matches!(&attrs[0].value, AttrValue::Static(v) if v == "\"page\""));

    // The button must be a CHILD of main, and its handler a real expression.
    let Node::Element { tag, attrs, .. } = b.node(children[0]) else {
        panic!("expected the button")
    };
    assert_eq!(tag, "button");
    assert_eq!(attrs[0].name, "on:press");
    assert_eq!(attrs[0].namespace(), Some(("on", "press")));
    let AttrValue::Expr(handler) = attrs[0].value else {
        panic!("the handler must be an expression, not text")
    };
    assert!(matches!(b.expr(handler), Expr::Call { .. }));

    // Both interpolations reach `parts`, in source order, exactly once each.
    assert_eq!(parts.len(), 2, "the handler and {{label}}, no duplicates");
    assert_eq!(parts[0], handler, "source order: the attribute comes first");

    // Every markup node is reachable from the roots.
    assert!(b.walk_markup(&roots).len() >= 4);
}

#[test]
fn two_template_regions_in_one_body_do_not_duplicate_each_others_parts() {
    // The first version collected `parts` by scanning the whole node arena, so
    // a second region re-collected the first's expressions and `walk()` visited
    // them twice — which is how one defect gets reported twice.
    let src = "component C() {\n    view { <p>{a}</p> }\n    view { <p>{b}</p> }\n}\n";
    let hir = lower(src);
    let (_, d) = hir.all_decls().next().unwrap();
    let b = hir.body(d.body.unwrap());

    let templates: Vec<usize> = b
        .walk()
        .into_iter()
        .filter_map(|id| match b.expr(id) {
            Expr::Template { parts, .. } => Some(parts.len()),
            _ => None,
        })
        .collect();
    assert_eq!(templates, [1, 1], "each region owns exactly its own part");

    let walked = b.walk();
    let unique: std::collections::HashSet<_> = walked.iter().collect();
    assert_eq!(
        walked.len(),
        unique.len(),
        "walk() must not repeat an expression"
    );
}

#[test]
fn policies_reach_hir_with_their_values_and_spans() {
    // Charter §14 M4's gate says *all* query and command policies appear in
    // `pw explain`. A policy the compiler dropped would not, so it has to
    // survive lowering before anything can display it.
    let src = "module m\n\
               public query store(id: StoreId) -> Store\n\
               \x20   cache          shared\n\
               \x20   freshness      30.seconds\n\
               \x20   invalidates_on StoreChanged(id)\n\
               \x20   offline\n\
               {\n    Stores.get(id)\n}\n";
    let hir = lower(src);
    let (_, d) = hir.all_decls().next().unwrap();

    let got: Vec<(&str, &str)> = d
        .policies
        .iter()
        .map(|p| (p.name.as_str(), p.value.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("cache", "shared"),
            ("freshness", "30.seconds"),
            ("invalidates_on", "StoreChanged(id)"),
            ("offline", ""),
        ],
        "in source order, alignment collapsed, a bare policy keeping an empty value"
    );

    assert_eq!(
        d.policy("freshness").map(|p| p.value.as_str()),
        Some("30.seconds")
    );
    assert!(
        d.policy("retry").is_none(),
        "a policy not written is absent"
    );

    // The span must underline the policy so a diagnostic can point at it.
    let p = d.policy("invalidates_on").expect("declared");
    assert_eq!(
        &src[p.span.clone()].trim(),
        &"invalidates_on StoreChanged(id)"
    );
}

#[test]
fn a_declaration_with_no_policy_block_has_none() {
    // Control: the accessor must distinguish "declared nothing" from "declared
    // something the lowering could not read".
    let hir = lower("module m\nfn f() -> Int !{} { 1 }\n");
    let (_, d) = hir.all_decls().next().unwrap();
    assert!(d.policies.is_empty());
}

#[test]
fn every_policy_in_the_accepted_corpus_survives_lowering() {
    // The coverage property, again: assert the converse. A policy keyword the
    // lowering skipped would leave every test above passing.
    let mut total = 0usize;
    let mut generated = 0usize;
    for path in corpus() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.starts_with("A-") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read");
        let hir = lower(&src);
        for (id, d) in hir.all_decls() {
            for p in &d.policies {
                if let Some(origin) = hir.generated.get(&id) {
                    // A derived host binding points to the table clause that
                    // produces it; no `host` keyword was written there.
                    let span = hir.decl_span(origin.table);
                    assert_eq!(hir.decl(origin.table).kind, DeclKind::Table);
                    assert_eq!(p.name, "host");
                    assert!(span.start <= p.span.start && p.span.end <= span.end);
                    assert!(!src[p.span.clone()].is_empty());
                    generated += 1;
                    continue;
                }
                if let Some(origin) = hir.generated_uploads.get(&id) {
                    // An upload's generated operation's host binding points
                    // to the upload's name (corpus C34's A-061, ADR-0440):
                    // no `host` keyword was written there either.
                    let span = hir.decl_span(origin.upload);
                    assert_eq!(hir.decl(origin.upload).kind, DeclKind::Upload);
                    assert_eq!(p.name, "host");
                    assert!(span.start <= p.span.start && p.span.end <= span.end);
                    assert!(!src[p.span.clone()].is_empty());
                    generated += 1;
                    continue;
                }
                total += 1;
                assert!(!p.name.is_empty(), "{name}: a policy with no name");
                assert!(
                    src[p.span.clone()].starts_with(&p.name),
                    "{name}: {:?} does not start its own span",
                    p.name
                );
            }
        }
    }
    assert!(total >= 20, "expected a real sample, saw {total} policies");
    assert!(generated > 0, "the corpus exercises derived host policies");
}
