//! `pw` — the developer command surface (charter §14 M2 task 12).
//!
//! ```text
//! pw check <path>...     parse and report diagnostics
//! pw explain <path>...   print the semantic facts a developer would otherwise infer
//! ```
//!
//! `pw fmt` is deliberately absent. The lexer is lossless so a formatter is
//! *possible*, but an idempotent formatter needs a decision about the
//! declaration layout the corpus should be normalised to, and shipping one that
//! reflows the corpus into a shape nobody agreed on would be worse than not
//! having it. Recorded as open in `docs/milestones/E2.md`.
//!
//! Charter §14 M2 gate: "the compiler can print an effect summary without
//! exposing generated-file paths to the user". `explain` asserts that in tests.

use std::path::Path;
use std::process::ExitCode;

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
use pw_core::diagnostics::{Diagnostic, Severity};

fn render(source: &str, path: &str, errors: &[pw_syntax::SyntaxError], styled: bool) -> String {
    let renderer = if styled {
        Renderer::styled()
    } else {
        Renderer::plain()
    };
    let mut out = String::new();
    for e in errors {
        let snippet = Snippet::source(source).path(path).line_start(1).annotation(
            AnnotationKind::Primary
                .span(e.span.clone())
                .label(&e.message),
        );
        let mut group = Level::ERROR
            .primary_title(format!("[{}] {}", e.code, e.message))
            .element(snippet);
        if let Some(h) = &e.help {
            group = group.element(Level::HELP.message(h.as_str()));
        }
        out.push_str(&renderer.render(&[group]));
        out.push('\n');
    }
    out
}

/// **Is `span` a range of `source` a renderer can read?** Within it, at
/// character boundaries. A span that is not is a checker's defect, rendered
/// as a note rather than allowed to stop `pw check` (ADR-0321): until
/// 2026-10-12 one past a file's end did, as an internal error.
fn readable(source: &str, span: &std::ops::Range<usize>) -> bool {
    span.start <= span.end
        && span.end <= source.len()
        && source.is_char_boundary(span.start)
        && source.is_char_boundary(span.end)
}

/// `found`, each diagnostic of the file `path`, whose text is `source`.
/// `files` is every file checked, by the name it is shown by, so a related
/// note in another file is shown in that file (ADR-0321).
fn render_findings(
    source: &str,
    path: &str,
    found: &[Diagnostic],
    files: &[(String, String)],
    styled: bool,
) -> String {
    let renderer = if styled {
        Renderer::styled()
    } else {
        Renderer::plain()
    };
    let mut out = String::new();
    for f in found {
        let level = if f.severity == Severity::Error {
            Level::ERROR
        } else {
            Level::WARNING
        };
        let mut snippet = Snippet::source(source).path(path).line_start(1);
        // Whether the file's own snippet marks anything: one that marks
        // nothing is not shown.
        let mut marked = false;
        let mut unread = Vec::new();
        if readable(source, &f.primary_span) {
            marked = true;
            snippet = snippet.annotation(
                AnnotationKind::Primary
                    .span(f.primary_span.clone())
                    .label(&f.message),
            );
        } else {
            unread.push(format!(
                "{path}, bytes {}..{}: {}",
                f.primary_span.start, f.primary_span.end, f.message
            ));
        }
        // Charter §16.3: name where the conflicting property originated, not
        // only where the rule fired. A note in another file is shown in it.
        let mut elsewhere = Vec::new();
        for r in &f.related {
            match &r.file {
                None if readable(source, &r.span) => {
                    marked = true;
                    snippet = snippet
                        .annotation(AnnotationKind::Context.span(r.span.clone()).label(&r.label));
                }
                None => unread.push(format!(
                    "{path}, bytes {}..{}: {}",
                    r.span.start, r.span.end, r.label
                )),
                Some(other) => match files.iter().find(|(name, _)| name == other) {
                    Some((name, text)) if readable(text, &r.span) => elsewhere.push(
                        Snippet::source(text.as_str())
                            .path(name.as_str())
                            .line_start(1)
                            .annotation(
                                AnnotationKind::Context.span(r.span.clone()).label(&r.label),
                            ),
                    ),
                    _ => unread.push(format!(
                        "{other}, bytes {}..{}: {}",
                        r.span.start, r.span.end, r.label
                    )),
                },
            }
        }
        let mut group =
            Group::with_title(level.primary_title(format!("[{}] {}", f.code, f.message)));
        if marked {
            group = group.element(snippet);
        }
        for other in elsewhere {
            group = group.element(other);
        }
        for note in &unread {
            group = group.element(Level::NOTE.message(note.as_str()));
        }
        if let Some(n) = &f.explanation {
            group = group.element(Level::NOTE.message(n.as_str()));
        }
        for r in &f.repairs {
            group = group.element(Level::HELP.message(r.description.as_str()));
        }
        out.push_str(&renderer.render(&[group]));
        out.push('\n');
    }
    out
}

/// The `pw explain` surface: the semantic facts a developer would otherwise
/// have to infer by reading the whole file.
///
/// Nothing here may name a generated artifact — no Koka module, no Marko
/// template, no build directory. That is a charter gate item, and
/// `explain_never_leaks_generated_paths` asserts it.
///
/// E6F: reads the HIR. It used to read a second declaration tree from a second
/// parser, and E6 showed what two parsers cost — a `materialize` block had
/// policies to one of them and none to the other, in the same build, for the
/// same file. There is now one parser that decides what a `.pw` program means.
/// `explain`, with the whole-program graph when the caller has one.
///
/// A materialization's key audit is a whole-program question — what a fragment
/// separates depends on what its dependencies separate — so a single-file
/// `explain` cannot answer it and says so rather than printing a smaller
/// answer that looks complete.
fn explain_with(
    hir: &pw_core::hir::Hir,
    source: &str,
    graph: Option<&pw_core::graph::Graph>,
) -> String {
    use pw_core::hir::DeclKind;
    use std::fmt::Write as _;
    let mut s = String::new();

    let module = hir
        .modules
        .iter()
        .next()
        .map(|(_, m, _)| m.name.clone())
        .unwrap_or_else(|| "(none)".to_string());
    let _ = writeln!(s, "module       {module}");

    // `@id:` and `@corpus:` are doc attributes in comments. Read from the
    // source, because neither tree carries trivia into its declarations.
    let attr = |k: &str| {
        source
            .lines()
            .find_map(|l| l.trim().strip_prefix(&format!("// @{k}:")))
            .map(str::trim)
    };
    if let Some(id) = attr("id") {
        let _ = writeln!(s, "corpus       {} ({})", id, attr("corpus").unwrap_or("?"));
    }
    let _ = writeln!(s);

    for (id, d) in hir.all_decls() {
        let name = if d.name.is_empty() { "?" } else { &d.name };
        let effects: Option<Vec<&str>> = d
            .declared_effects
            .as_ref()
            .map(|row| row.iter().map(|e| e.written.as_str()).collect());

        match d.kind {
            DeclKind::Import => continue,
            DeclKind::Effect => {
                let _ = writeln!(s, "effect       {name}");
            }
            // ADR-0207: a database, what it holds and what it guarantees.
            DeclKind::Source => {
                let _ = writeln!(s, "source       {name}");
                for p in &d.policies {
                    let _ = writeln!(s, "             {} {}", p.name, p.value);
                }
            }
            // Track `tables` (ADR-0338): an app's data, declared. Its reads
            // and writes are its module's functions, shown as functions are.
            DeclKind::Table => {
                let _ = writeln!(s, "table        {name}");
                for p in &d.policies {
                    let _ = writeln!(s, "             {} {}", p.name, p.value);
                }
            }
            // Track `uploads` (ADR-0260): a file a form posts, and its limits.
            DeclKind::Upload => {
                let _ = writeln!(s, "upload       {name}");
                for p in &d.policies {
                    let _ = writeln!(s, "             {} {}", p.name, p.value);
                }
            }
            // ADR-0302: a predicate a command requires, and its words.
            DeclKind::Predicate => {
                let _ = writeln!(s, "predicate    {name}");
                for p in &d.policies {
                    let _ = writeln!(s, "             {} {}", p.name, p.value);
                }
            }
            DeclKind::Opaque => {
                let _ = writeln!(s, "opaque type  {name}");
                let _ = writeln!(
                    s,
                    "             represented as {} — distinct from it everywhere in the checker",
                    d.opaque_of
                        .as_ref()
                        .map_or_else(|| "?".to_string(), |t| t.written())
                );
            }
            DeclKind::Type if d.variants.is_some() => {
                let variants = d.variants.as_deref().unwrap_or_default();
                let _ = writeln!(s, "union        {name} ({} variants)", variants.len());
                for v in variants {
                    let _ = writeln!(
                        s,
                        "             | {}{}",
                        v.name,
                        if v.fields.is_empty() {
                            String::new()
                        } else {
                            let fields: Vec<String> =
                                v.fields.iter().map(|f| f.written()).collect();
                            format!("({})", fields.join(", "))
                        }
                    );
                }
                let _ = writeln!(
                    s,
                    "             a match on this must cover every variant, whatever its effect row"
                );
            }
            DeclKind::Type => {
                let _ = writeln!(s, "record       {name}");
                for f in d.fields.as_deref().unwrap_or_default() {
                    let _ = writeln!(
                        s,
                        "             {}: {}",
                        f.name,
                        f.ty.as_ref().map(|t| t.written()).unwrap_or("?".into())
                    );
                }
            }
            DeclKind::Fn => {
                let _ = writeln!(s, "fn           {name}");
                let ps: Vec<String> = d
                    .params
                    .iter()
                    .map(|p| {
                        let ty = p.ty.as_ref().map(|t| t.written());
                        format!("{}: {}", p.name, ty.as_deref().unwrap_or("?"))
                    })
                    .collect();
                let _ = writeln!(s, "             ({})", ps.join(", "));
                if let Some(r) = &d.ret {
                    let _ = writeln!(s, "             -> {r}");
                }
                match &effects {
                    None => {
                        let _ = writeln!(s, "             effects: (unannotated — claims nothing)");
                    }
                    Some(row) if row.is_empty() => {
                        let _ =
                            writeln!(s, "             effects: !{{}}  <- explicit purity claim");
                    }
                    Some(row) => {
                        let _ = writeln!(s, "             effects: !{{{}}}", row.join(", "));
                        let _ = writeln!(s, "             placement: {}", derive_placement(row));
                    }
                }
            }
            DeclKind::Query
            | DeclKind::Command
            | DeclKind::Subscription
            | DeclKind::Resource
            | DeclKind::Materialize
            | DeclKind::Event
            | DeclKind::Task => {
                let noun = noun_of(d.kind);
                let _ = writeln!(s, "{noun:<12} {name}");
                let label = match d.visibility.as_deref() {
                    Some("public") => "Public",
                    Some("session") => "Session<SessionId>",
                    Some("private") | Some("user") => "User<UserId>",
                    _ => "(unspecified)",
                };
                let _ = writeln!(s, "             privacy: {label}");
                for p in &d.policies {
                    let _ = writeln!(s, "             {:<14} {}", p.name, p.value);
                }
                if let Some(c) = d.policies.iter().find(|p| p.name == "cache")
                    && c.value.starts_with("shared")
                    && matches!(
                        d.visibility.as_deref(),
                        Some("session") | Some("private") | Some("user")
                    )
                {
                    let _ = writeln!(
                        s,
                        "             WARNING: a {label} value in a shared cache is rejected at E5"
                    );
                }
                if d.kind == DeclKind::Materialize {
                    let module = hir.module_of(id).unwrap_or_default();
                    let path = if module.is_empty() {
                        name.to_string()
                    } else {
                        format!("{module}.{name}")
                    };
                    s.push_str(&key_audit(graph, &path));
                }
            }
            DeclKind::View | DeclKind::Component | DeclKind::Page | DeclKind::Layout => {
                let noun = noun_of(d.kind);
                let _ = writeln!(s, "{noun:<12} {name}");
                for p in &d.policies {
                    let _ = writeln!(s, "             {:<14} {}", p.name, p.value);
                }
                if let Some(row) = &effects {
                    if row.is_empty() {
                        let _ = writeln!(s, "             effects: !{{}}  <- pure render");
                    } else {
                        let _ = writeln!(s, "             effects: !{{{}}}", row.join(", "));
                    }
                }
            }
            // `prelude Effect` — what this package makes ambient. Shown,
            // because a reader asking "why does this file resolve
            // `database.read` without importing anything" needs the answer to
            // be visible somewhere.
            DeclKind::Prelude => {
                let _ = writeln!(s, "prelude      {name} namespace exported by this module");
            }
            // A signal a page or view provides (ADR-0144).
            DeclKind::Signal => {
                let _ = writeln!(
                    s,
                    "signal       {name}: {}",
                    d.ret
                        .as_ref()
                        .map_or_else(|| "?".to_string(), |t| t.written())
                );
            }
            DeclKind::Let | DeclKind::Other => {
                let _ = writeln!(s, "declaration  {name}");
            }
        }

        // **Every policy, whatever the declaration kind.**
        //
        // Charter §14 M4's gate is that a policy the compiler knows about is
        // visible here, and the arms above cover the kinds that had policies
        // when it was written. `replicated` and `paint` gained theirs on
        // 2026-08-11, when data-operation declarations began parsing their
        // clauses inside their braces — and `A-011`'s six and `A-021`'s two
        // became invisible the moment they became real.
        //
        // Kept as a sweep after the match rather than a ninth arm: a new
        // declaration kind should not be able to hide its policies by being
        // new. The `printed` guard is what stops the arms above double-printing.
        if !matches!(
            d.kind,
            DeclKind::Query
                | DeclKind::Command
                | DeclKind::Subscription
                | DeclKind::Resource
                | DeclKind::Materialize
                | DeclKind::Task
                | DeclKind::View
                | DeclKind::Component
                | DeclKind::Page
                | DeclKind::Layout
        ) {
            for p in &d.policies {
                let _ = writeln!(s, "             {:<14} {}", p.name, p.value);
            }
        }

        let span = hir.decl_span(id);
        let _ = writeln!(s, "             at bytes {}..{}", span.start, span.end);
        let line = source[..span.start.min(source.len())]
            .lines()
            .count()
            .max(1);
        let _ = writeln!(s, "             line {line}");
        let _ = writeln!(s);
    }
    s
}

/// Charter §14 M6 task 9 — cache-key auditing.
///
/// Reported as advice, not as a diagnostic. The compiler cannot always tell a
/// deliberate omission from a mistake: two readers sharing an entry is
/// sometimes exactly what a cache is for. What it CAN do is say which
/// dimensions the key separates and which a dependency separates that this key
/// does not, and let a reader decide.
///
/// The build that produced the artifact is deliberately absent from the
/// application section. `PW5102` used to ask the author for it and was retired
/// — injecting a compatibility generation is platform mechanism.
fn key_audit(graph: Option<&pw_core::graph::Graph>, path: &str) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let Some(g) = graph else {
        // Said rather than omitted. A smaller answer that looks complete is
        // the failure mode this whole milestone was about.
        let _ = writeln!(
            s,
            "             key audit: needs the whole program; pass every file"
        );
        return s;
    };
    let Some(a) = g.key_audit(path) else {
        return s;
    };

    let _ = writeln!(s, "             logical key");
    if a.logical_key.is_empty() {
        let _ = writeln!(s, "               (none — one entry for every reader)");
    }
    for k in &a.logical_key {
        let _ = writeln!(s, "               {k}");
    }

    let _ = writeln!(s, "             automatic platform dimensions");
    let _ = writeln!(s, "               build generation    included");
    let _ = writeln!(
        s,
        "               privacy partition   {}",
        a.partition.as_deref().unwrap_or("NOT DECLARED")
    );

    let _ = writeln!(s, "             application dimensions");
    for (d, present) in &a.application {
        let _ = writeln!(
            s,
            "               {:<19} {}",
            d.keyword(),
            if *present { "included" } else { "absent" }
        );
    }

    if !a.gaps.is_empty() {
        let _ = writeln!(s, "             POTENTIAL KEY GAP");
        for gap in &a.gaps {
            let _ = writeln!(
                s,
                "               `{}` separates by `{}`, this key does not",
                gap.resource, gap.component
            );
            let _ = writeln!(s, "                 {}", gap.why);
        }
    }
    s
}

/// The declaration keyword, so `explain` names what the author wrote.
fn noun_of(kind: pw_core::hir::DeclKind) -> &'static str {
    use pw_core::hir::DeclKind::*;
    match kind {
        Query => "query",
        Command => "command",
        Subscription => "subscription",
        Resource => "resource",
        Materialize => "materialize",
        Event => "event",
        Task => "task",
        View => "view",
        Component => "component",
        Page => "page",
        Layout => "layout",
        _ => "declaration",
    }
}

/// Placement derived from effects alone — charter §7.9's claim, in miniature.
fn derive_placement(effects: &[&str]) -> &'static str {
    let has = |p: &str| effects.iter().any(|e| e.starts_with(p));
    if has("secret") || has("database.write") {
        "Origin (secret or write capability required)"
    } else if has("database") {
        "Origin or Edge (database read)"
    } else if has("device") || has("layout") || has("dom") || has("observe") || has("paint") {
        "Browser (browser-only capability required)"
    } else if has("clock.wall") || has("random") {
        "not Build (nondeterministic)"
    } else {
        "unconstrained by effects"
    }
}

/// `pw fmt` — rewrite in place, or with `--check` report and exit non-zero.
///
/// `--check` never writes. It is the CI form: a repository is either canonical
/// or the build fails, with no third state where a tool silently edits code.
fn fmt_command(paths: &[&String], check_only: bool) -> ExitCode {
    let mut changed = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        // **Never overwrite source the parser has said it does not
        // understand.** Architect ruling, 2026-08-07:
        //
        // > Formatting to stdout for inspection is one thing; overwriting
        // > source whose meaning the parser has explicitly said it does not
        // > understand is another.
        //
        // A file containing an `UnknownPolicy` or an error node has been
        // rejected, and rewriting it in place would canonicalise a program
        // whose semantics the compiler never established — turning "I could not
        // read this" into "here is my version of it".
        //
        // `--check` still reports, because reading is safe and a formatting
        // difference is worth knowing about either way.
        let parsed = pw_syntax::parse_tree(&src);
        if !check_only && !parsed.ok() {
            eprintln!(
                "pw fmt: refusing to rewrite {path}: it does not parse cleanly, \
                 and formatting it would canonicalise a program whose meaning \
                 this compiler has not established. Run `pw check {path}` first."
            );
            refused.push((*path).clone());
            continue;
        }
        let out = pw_syntax::format_source(&src);
        if out == src {
            continue;
        }
        // **Never write a program that says something else** (ADR-0276):
        // the same tokens, and a policy's value its gaps as written. A
        // formatter's bug is refused here, by name, before it reaches a
        // file; `--check` reports it the same way.
        if let Err(why) = pw_syntax::meaning_kept(&src, &out) {
            eprintln!(
                "pw fmt: refusing to rewrite {path}: formatting it would change what \
                 it says, at {why}. This is a bug in `pw fmt`; the file is left as written."
            );
            refused.push((*path).clone());
            continue;
        }
        changed.push((*path).clone());
        if let Some(Err(e)) = (!check_only).then(|| std::fs::write(path, &out)) {
            eprintln!("pw: cannot write {path}: {e}");
            return ExitCode::from(2);
        }
    }

    if !refused.is_empty() {
        println!(
            "pw fmt: refused {} file(s) that do not parse cleanly, or that formatting would change",
            refused.len()
        );
        return ExitCode::FAILURE;
    }
    if changed.is_empty() {
        println!("pw fmt: {} file(s) already formatted", paths.len());
        return ExitCode::SUCCESS;
    }
    if check_only {
        for c in &changed {
            println!("would reformat: {c}");
        }
        println!(
            "pw fmt: {} of {} file(s) need formatting",
            changed.len(),
            paths.len()
        );
        return ExitCode::FAILURE;
    }
    println!(
        "pw fmt: reformatted {} of {} file(s)",
        changed.len(),
        paths.len()
    );
    ExitCode::SUCCESS
}

/// `pw emit-koka` — print the generated Koka, and every skipped declaration.
///
/// Skips go to stderr rather than being dropped: a backend that covers a subset
/// has to say which subset, or its output looks like a complete translation.
fn emit_koka_command(paths: &[&String]) -> ExitCode {
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        let hir = pw_core::lower::lower_file(&src, &parsed.green);
        let module = hir
            .modules
            .iter()
            .next()
            .map(|(_, m, _)| m.name.clone())
            .unwrap_or_else(|| "main".to_string());
        let out = pw_core::koka::lower_module(&hir, &module);
        print!("{}", out.source);
        for s in &out.skipped {
            eprintln!("skipped {}: {}", s.name, s.reason);
        }
    }
    ExitCode::SUCCESS
}

/// `pw emit-marko` — write generated templates under `--out DIR` (ADR-0017).
///
/// Requires an explicit output directory. Defaulting to the source directory
/// would put build output next to authoring source, which is the exact thing
/// charter §14 M3 task 3 forbids.
fn emit_marko_command(paths: &[&String], out_dir: Option<String>) -> ExitCode {
    let Some(out_dir) = out_dir else {
        eprintln!("pw emit-marko: --out DIR is required");
        eprintln!("  generated templates are build output and must not be written");
        eprintln!("  beside authoring source (charter §14 M3 task 3, ADR-0017)");
        return ExitCode::from(2);
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("pw: cannot create {out_dir}: {e}");
        return ExitCode::from(2);
    }

    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut units = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        let hir = pw_core::lower::lower_file(&src, &parsed.green);
        units.push((path, hir));
    }
    // Each name a module declares a signal, as each file resolves it
    // (ADR-0144): what the adapter refuses rather than drops.
    let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|(_, h)| h).collect();
    let ws = pw_core::resolve::Workspace::build(&hirs);
    for (unit, (path, hir)) in units.iter().enumerate() {
        let mut signals = std::collections::BTreeSet::new();
        for (_, d) in hir.all_decls() {
            let names: Vec<&String> = match d.kind {
                pw_core::hir::DeclKind::Signal => vec![&d.name],
                pw_core::hir::DeclKind::Import => d.imports.iter().collect(),
                _ => Vec::new(),
            };
            for n in names {
                if let pw_core::resolve::Resolution::Local(def)
                | pw_core::resolve::Resolution::Imported { def, .. } = ws.resolve(unit, n)
                    && pw_core::resolve::declaration(&hirs, def)
                        .is_some_and(|x| x.kind == pw_core::hir::DeclKind::Signal)
                {
                    signals.insert(n.clone());
                }
            }
        }
        let name = Path::new(path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| (**path).clone());
        let out = pw_core::marko::render_module(hir, &name, &signals);

        for (file, text) in &out.files {
            let dest = Path::new(&out_dir).join(file);
            if let Err(e) = std::fs::write(&dest, text) {
                eprintln!("pw: cannot write {}: {e}", dest.display());
                return ExitCode::from(2);
            }
            println!("{}", dest.display());
            written += 1;
        }
        for s in &out.skipped {
            eprintln!("skipped {}: {}", s.name, s.reason);
            skipped += 1;
        }
    }

    println!("pw emit-marko: {written} file(s) written, {skipped} skipped");
    if written == 0 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `pw emit-manifest` — the artifact that connects a declaration to the runtime.
///
/// Policies the schema could not interpret go to stderr and set a failing exit
/// code. A manifest with a silently defaulted freshness is worse than none: the
/// runtime would serve stale data and nothing would say why.
fn emit_manifest_command(paths: &[&String]) -> ExitCode {
    let mut all = Vec::new();
    let mut unparsed = 0usize;

    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        let hir = pw_core::lower::lower_file(&src, &parsed.green);
        let built = pw_core::manifest::build(&hir);
        for u in &built.unparsed {
            eprintln!(
                "{path}: `{}` declares `{} {}` which is {}",
                u.declaration, u.policy, u.value, u.reason
            );
            unparsed += 1;
        }
        all.extend(built.manifests);
    }

    match serde_json::to_string_pretty(&all) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("pw: cannot serialize manifests: {e}");
            return ExitCode::from(2);
        }
    }
    if unparsed > 0 {
        eprintln!("pw emit-manifest: {unparsed} policy value(s) not understood");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `pw emit-template` — the checked template IR (charter §14 M7 task 2).
///
/// The artifact the E7 renderer consumes. Printed as JSON for the same reason
/// the manifest and the graph are: a runtime that the compiler depends on is
/// the only runtime there can ever be (ADR-0018).
fn emit_template_command(paths: &[&String], plain: bool) -> ExitCode {
    let mut hirs = Vec::new();
    let mut sources = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        hirs.push(pw_core::lower::lower_file(&src, &parsed.green));
        sources.push(src);
    }
    let refs: Vec<&pw_core::hir::Hir> = hirs.iter().collect();

    // Handler identities, derived once by the resume artifacts and read by the
    // template IR: `pw_core::build::templates`, which `pw build` writes too.
    let ws = pw_core::resolve::Workspace::build(&refs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &refs);
    let texts: Vec<&str> = sources.iter().map(String::as_str).collect();
    let templates = pw_core::build::templates(&refs, &texts, &sigs);

    let mut blocked = 0usize;
    for t in &templates {
        for b in t.blocked() {
            if let pw_core::template_ir::Part::Blocked { reason, at } = b {
                eprintln!("{}: blocked at `{at}`: {reason}", t.path);
                blocked += 1;
            }
        }
    }

    if plain {
        for t in &templates {
            println!("{}  ({})  schema {}", t.path, t.params.join(", "), t.schema);
            let manifest = t.manifest();
            if !manifest.is_empty() {
                println!("  parts manifest — dynamic regions only:");
                for e in &manifest {
                    let owner = e.owner.map(|o| format!(" element {o}")).unwrap_or_default();
                    println!(
                        "    {:>3}  {:<18} {:?}{owner}  {}",
                        e.id.0, e.kind, e.anchor, e.value
                    );
                }
            }
            for c in &t.chunks {
                match c {
                    pw_core::template_ir::Chunk::Static(s) => {
                        println!("    static  {:?}", s);
                    }
                    pw_core::template_ir::Chunk::Dynamic(p) => {
                        println!("    part    {p:?}");
                    }
                }
            }
            println!();
        }
    } else {
        match serde_json::to_string_pretty(&templates) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("pw: cannot serialize the template IR: {e}");
                return ExitCode::from(2);
            }
        }
    }

    // A blocked part is a failing exit code. The renderer refuses it too; a
    // build that emitted it as a warning would let a page ship with a region
    // silently missing.
    if blocked > 0 {
        eprintln!("pw emit-template: {blocked} blocked part(s)");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `pw emit-graph` — the resource dependency graph (charter §14 M6 gate 6).
///
/// Every path at once, never per file. A materialization in one module depends
/// on a query in another, and a graph built one file at a time would report a
/// dangling edge for exactly the case E6 is about.
///
/// `--plain` prints the edges for a person; the default prints JSON. Both, and
/// not one dressed as the other: the gate asks for inspectable AND
/// serializable, and a blob is not inspectable.
/// E8-0 — the compiler→host `ComponentContract`, as JSON.
///
/// A DATA artifact (ADR-0018). The compiler emits it; the host deserializes it
/// and mirrors the types by field name. Neither links the other, which is what
/// keeps "the compiler decides what authority code needs, the host decides
/// whether it exists" a boundary rather than a slogan.
/// **The WIT package for a checked program: one world per contract.**
///
/// Refuses rather than emits when a signature names a type it cannot map — a
/// world containing a guessed type parses, links, and decodes a value into
/// something it never was.
fn emit_wit_command(paths: &[&String]) -> ExitCode {
    let mut hirs = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        hirs.push(pw_core::lower::lower_file(&src, &parsed.green));
    }
    let refs: Vec<&pw_core::hir::Hir> = hirs.iter().collect();
    let ws = pw_core::resolve::Workspace::build(&refs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &refs);
    let contracts = pw_core::contract::contracts(&refs, &sigs, &ws);
    match pw_core::wit::package(&refs, &ws, &contracts) {
        Ok((text, _)) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("pw: cannot generate WIT: {e}");
            ExitCode::FAILURE
        }
    }
}

fn emit_contracts_command(paths: &[&String], plain: bool) -> ExitCode {
    let mut hirs = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        hirs.push(pw_core::lower::lower_file(&src, &parsed.green));
    }
    let refs: Vec<&pw_core::hir::Hir> = hirs.iter().collect();
    let ws = pw_core::resolve::Workspace::build(&refs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &refs);
    let contracts = pw_core::contract::contracts(&refs, &sigs, &ws);

    if plain {
        for c in &contracts {
            println!("{}  abi {}", c.component_id, c.abi_schema);
            println!("  runs at   {}", c.allowed_placements.join(", "));
            for cap in &c.required_capabilities {
                println!("  needs     {}", cap.name());
            }
            for i in &c.imports {
                match i.kind {
                    pw_core::contract::ImportKind::HostCapability => {
                        println!("  host      {}  ({})", i.key(), i.capability)
                    }
                    pw_core::contract::ImportKind::Component => {
                        println!("  component {}", i.key())
                    }
                }
            }
            for e in &c.exports {
                println!("  exports   {} {}", e.kind, e.name);
                if let Some(component) = &e.component {
                    for requirement in &component.authorization {
                        let arguments = requirement
                            .arguments
                            .iter()
                            .map(|index| format!("arg {index}"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        if arguments.is_empty() {
                            println!("  requires  {}", requirement.predicate);
                        } else {
                            println!("  requires  {}({})", requirement.predicate, arguments);
                        }
                    }
                }
            }
            println!();
        }
    } else {
        match serde_json::to_string_pretty(&contracts) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("pw: cannot serialize contracts: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn emit_graph_command(paths: &[&String], plain: bool) -> ExitCode {
    let mut hirs = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        hirs.push(pw_core::lower::lower_file(&src, &parsed.green));
    }
    let refs: Vec<&pw_core::hir::Hir> = hirs.iter().collect();
    let ws = pw_core::resolve::Workspace::build(&refs);
    let g = pw_core::graph::Graph::build(&refs, &ws);

    if plain {
        for n in &g.nodes {
            println!("{}  {}", node_label(&n.kind), n.path);
        }
        println!();
        for e in &g.edges {
            let key = if e.key.is_empty() {
                String::new()
            } else {
                format!("({})", e.key.join(", "))
            };
            println!("{}  --{}-->  {}{key}", e.from, edge_label(e.kind), e.to);
        }
        if !g.dangling.is_empty() {
            println!();
            for d in &g.dangling {
                println!(
                    "DANGLING  {} --{}--> {} (nothing declares it)",
                    d.from,
                    edge_label(d.kind),
                    d.name
                );
            }
        }
    } else {
        match serde_json::to_string_pretty(&g) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("pw: cannot serialize the graph: {e}");
                return ExitCode::from(2);
            }
        }
    }

    // A dangling edge is a failing exit code, not a note. A materialization
    // listening for an event nobody declares never regenerates, and the page
    // is not wrong — only permanently stale, which no test of the page finds.
    if !g.dangling.is_empty() {
        eprintln!("pw emit-graph: {} dangling edge(s)", g.dangling.len());
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn node_label(k: &pw_core::graph::NodeKind) -> &'static str {
    use pw_core::graph::NodeKind::*;
    match k {
        Resource { .. } => "resource     ",
        Materialization { .. } => "materialize  ",
        Event => "event        ",
        Command => "command      ",
        Page => "page         ",
    }
}

fn edge_label(k: pw_core::graph::EdgeKind) -> &'static str {
    use pw_core::graph::EdgeKind::*;
    match k {
        Reads => "reads",
        InvalidatedBy => "invalidated-by",
        Emits => "emits",
        Invalidates => "invalidates",
    }
}

/// Turn an internal panic into a conspicuous, reproducible report.
///
/// Charter §3.1 and the architect's compiler-robustness gate: for every
/// syntactically representable program the compiler must produce output,
/// ordinary diagnostics, or a **clearly marked internal compiler error**. It
/// must not terminate without a report.
///
/// The reason is not politeness. When `exhaust.rs` panicked on a constructor
/// arity mismatch, every rule in that invocation reported nothing for every
/// file — a compiler that reports nothing is indistinguishable from a compiler
/// that found nothing, and the user's next move is to assume their code is
/// fine.
///
/// Containment is for the user and for the evidence. It is NOT a way of
/// tolerating the defect: the exit code is still a failure, the message says
/// this is a bug in `pw`, and `just ci` runs the robustness suite which fails
/// hard on any panic at all.
/// **What the value relations decided**, as counts — E9-V's evidence.
///
/// A checker that reports nothing and a checker that decided nothing look the
/// same from `pw check`. This prints the second question's answer: how many
/// relations were decided, how many disagreed, and why each undecided one was
/// not decided.
fn audit_values_command(paths: &[&String]) -> ExitCode {
    use pw_core::values::{Outcome, RelationKind};
    let mut units = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.errors.is_empty() {
            eprintln!("pw: {path} does not parse; nothing about it is counted");
            return ExitCode::from(1);
        }
        let hir = pw_core::lower::lower_file(&src, &parsed.green);
        units.push(pw_core::check::Unit {
            path: path.to_string(),
            src,
            hir,
        });
    }
    let mut table: std::collections::BTreeMap<(RelationKind, String), usize> =
        std::collections::BTreeMap::new();
    let mut disagreements = Vec::new();
    for (path, relations) in pw_core::values::analysis(&units) {
        for r in relations {
            let outcome = match &r.outcome {
                Outcome::Agree => "agree".to_string(),
                Outcome::Disagree { expected, actual } => {
                    disagreements.push(format!(
                        "{path}: {} {:?} {}: declared {expected}, found {actual}",
                        r.declaration, r.kind, r.target
                    ));
                    "disagree".to_string()
                }
                Outcome::Undecided(why) => format!("undecided: {why:?}"),
            };
            *table.entry((r.kind, outcome)).or_default() += 1;
        }
    }
    println!("{} file(s)", units.len());
    for ((kind, outcome), n) in &table {
        println!("  {:<11} {:<32} {n}", format!("{kind:?}"), outcome);
    }
    for d in &disagreements {
        println!("  {d}");
    }
    match disagreements.is_empty() {
        true => ExitCode::SUCCESS,
        false => ExitCode::from(1),
    }
}

/// **Compile one component to a Wasm component.** E10-I.
///
/// Writes the component to `out`, and prints what it imports and whether its
/// component-level types agree with its world — read back through
/// `wit-component`'s decoder, not through anything that wrote it.
/// `pw build --out DIR`: every artifact of one checked program.
///
/// ```text
/// DIR/templates.json            the template IR, with handler identities
/// DIR/handlers/<identity>.mjs   each resumable handler's compiled body
/// DIR/components/<id>.wasm      each command and query, audited
/// DIR/modules/<id>.mjs          each query that reaches no host, as an ES
///                               module (ADR-0044)
/// DIR/contracts.json            what the host admits each component by
/// DIR/app.wit                   the worlds the components implement
/// ```
///
/// All or nothing, like `emit-handlers`: one refusal writes nothing and fails
/// the build, naming what was refused and why. No Koka and no Marko: neither
/// backend is called (E10 gate item 1).
fn build_command(
    paths: &[&String],
    out: &str,
    released: Option<&pw_core::tables::Schema>,
) -> ExitCode {
    match build_program(paths, out, released) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

/// [`build_command`]'s work, its failure the code it exits with.
fn build_program(
    paths: &[&String],
    out: &str,
    released: Option<&pw_core::tables::Schema>,
) -> Result<(), ExitCode> {
    let mut sources = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(src) => src,
            Err(error) => {
                eprintln!("pw: cannot read {path}: {error}");
                return Err(ExitCode::from(2));
            }
        };
        sources.push((path.to_string(), src));
    }
    let build = match pw_core::program::validate_sources_with(&sources, released).into_build() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("pw: {e}");
            return Err(ExitCode::FAILURE);
        }
    };
    let refused = build.refusals();
    if !refused.is_empty() {
        for r in &refused {
            eprintln!("pw: refused: {r}");
        }
        eprintln!("pw: {} refusal(s); nothing written", refused.len());
        return Err(ExitCode::FAILURE);
    }

    let result = build.write(std::path::Path::new(out));
    let lines = match result {
        Ok(lines) => lines,
        Err(e) => {
            eprintln!("pw: cannot write the build: {e}");
            return Err(ExitCode::from(2));
        }
    };
    let handlers = build.handlers.len();
    let components = build
        .components
        .iter()
        .filter(|(_, b)| matches!(b, pw_core::backend::component::Built::Component { .. }))
        .count();
    println!(
        "pw build: {} template(s), {handlers} handler(s), {components} component(s), {} contract(s) -> {out}",
        build.templates.len(),
        build.contracts.len()
    );
    for l in lines {
        println!("{l}");
    }
    Ok(())
}

/// A flag's value: what follows its first use, where it has one.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Each value of a flag given more than once, in order.
fn flag_values(args: &[String], flag: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].clone())
        .collect()
}

/// The repository this `pw` was built in: its standard packages, and the
/// runtime and the packaging a host serves an app with (ADR-0149's
/// `program_in` finds the packages the same way).
fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// **A whole app's sources** (ADR-0429): the standard packages (`pw-std`,
/// `pw-platform-web`), which every app is built with, then each path as
/// given, a directory as each `.pw` file under it; a file named twice, as an
/// app that names a package's files does, is one source. Each absolute, as
/// the record of what a build was built from names it.
fn app_sources_of(paths: &[&String]) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("give the app's sources: its .pw files, or a directory of them".to_string());
    }
    let packages = repository()
        .join("packages")
        .canonicalize()
        .map_err(|e| format!("the standard packages: {e}"))?;
    let mut given = Vec::new();
    for path in paths {
        let path = std::path::Path::new(path.as_str());
        if path.is_dir() {
            let before = given.len();
            pw_files_under(path, &mut given).map_err(|e| format!("{}: {e}", path.display()))?;
            if given.len() == before {
                return Err(format!("{} holds no .pw file", path.display()));
            }
        } else {
            given.push(path.to_path_buf());
        }
    }
    let mut given: Vec<std::path::PathBuf> = given
        .into_iter()
        .map(|p| {
            p.canonicalize()
                .map_err(|e| format!("{}: {e}", p.display()))
        })
        .collect::<Result<_, _>>()?;
    let mut sources = Vec::new();
    for package in ["pw-std", "pw-platform-web"] {
        pw_files_under(&packages.join(package), &mut sources)
            .map_err(|e| format!("{package}: {e}"))?;
    }
    sources.append(&mut given);
    let mut seen = std::collections::BTreeSet::new();
    Ok(sources
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .map(|p| p.display().to_string())
        .collect())
}

/// **`pw build --dist DIR`: a whole app, built to be served** (ADR-0429):
/// its artifacts in `DIR/build`, as `pw build --out DIR/build` writes them;
/// the runtime its host serves it with (`DIR/pw-runtime.mjs`); each
/// `--asset FILE=NAME` beside it as `DIR/NAME`; with `--production`, packed
/// as a production build is (`optimize-assets.mjs`, ADR-0399); and last,
/// what it was built from, each source by its digest (`DIR/sources.json`,
/// ADR-0166). A build stopped part-way names no sources, and the browser
/// suite serves no such build. `DIR` is replaced: it must be new, empty or
/// an app's build (its artifacts and its runtime).
fn dist_command(
    sources: &[&String],
    dist: &str,
    production: bool,
    assets: &[String],
    released: Option<&pw_core::tables::Schema>,
) -> ExitCode {
    use sha2::Digest as _;
    let fail = |what: String| {
        eprintln!("pw build: {what}");
        ExitCode::from(2)
    };
    let dir = std::path::Path::new(dist);
    // Each asset's file and the name it is served at, read before anything
    // is touched: a name is a file of the build's own directory.
    let mut copies = Vec::new();
    for asset in assets {
        let Some((file, name)) = asset.split_once('=') else {
            return fail(format!("--asset {asset}: give FILE=NAME"));
        };
        let plain = !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains(['/', '\\'])
            && !["build", "sources.json", "pw-runtime.mjs"].contains(&name);
        if !plain {
            return fail(format!(
                "--asset {asset}: NAME is a file name of the build's own, not one it writes"
            ));
        }
        if !std::path::Path::new(file).is_file() {
            return fail(format!("--asset {asset}: {file} is no file"));
        }
        copies.push((file.to_string(), name.to_string()));
    }
    if dir.exists() {
        let mut entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => return fail(format!("{dist}: {e}")),
        };
        let empty = entries.next().is_none();
        // An app's build is replaced; nothing else is: a directory that holds
        // a `build/` of its own is no build of this command's.
        let built =
            dir.join("build/templates.json").is_file() && dir.join("pw-runtime.mjs").is_file();
        if !empty && !built {
            return fail(format!(
                "{dist} holds files that are no app's build; give a new or empty directory"
            ));
        }
        // The record first: a build stopped from here on names no sources.
        let _ = std::fs::remove_file(dir.join("sources.json"));
        if let Err(e) = std::fs::remove_dir_all(dir) {
            return fail(format!("{dist} cannot be replaced: {e}"));
        }
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        return fail(format!("{dist}: {e}"));
    }
    let out = dir.join("build").display().to_string();
    if let Err(code) = build_program(sources, &out, released) {
        return code;
    }
    let spike = repository().join("spikes/own-renderer");
    if let Err(e) = std::fs::copy(
        spike.join("public/pw-runtime.mjs"),
        dir.join("pw-runtime.mjs"),
    ) {
        return fail(format!("the runtime: {e}"));
    }
    for (file, name) in &copies {
        if let Err(e) = std::fs::copy(file, dir.join(name)) {
            return fail(format!("{file}: {e}"));
        }
    }
    if production {
        let packed = std::process::Command::new("node")
            .arg(spike.join("optimize-assets.mjs"))
            .arg(dir)
            .status();
        match packed {
            Ok(status) if status.success() => {}
            Ok(status) => return fail(format!("its packaging failed: {status}")),
            Err(e) => return fail(format!("its packaging needs node: {e}")),
        }
    }
    let mut files = Vec::new();
    for source in sources {
        let bytes = match std::fs::read(source.as_str()) {
            Ok(bytes) => bytes,
            Err(e) => return fail(format!("{source}: {e}")),
        };
        let sha256: String = sha2::Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        files.push(serde_json::json!({ "path": source.as_str(), "sha256": sha256 }));
    }
    let record =
        serde_json::to_string_pretty(&serde_json::json!({ "files": files })).unwrap_or_default();
    if let Err(e) = std::fs::write(dir.join("sources.json"), format!("{record}\n")) {
        return fail(format!("{dist}/sources.json: {e}"));
    }
    println!(
        "pw build: an app of {} source(s) -> {dist}{}",
        sources.len(),
        if production {
            ", packed for production"
        } else {
            ""
        }
    );
    ExitCode::SUCCESS
}

/// `pw emit-handlers --out DIR`: every resumable handler, as `DIR/<identity>.mjs`.
///
/// All or nothing. A page whose handlers were partly written would render
/// buttons that cannot work, so any refusal writes no file and fails the
/// build, naming each refused handler and why.
/// **Each page's optimistic speculations** (ADR-0122): `DIR/<page>.mjs`, the
/// module the runtime loads on a press, and `DIR/<page>.json`, the bindings
/// whose values the server sends it.
fn emit_speculations_command(paths: &[&String], out: &str) -> ExitCode {
    let mut units = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        let hir = pw_core::lower::lower_file(&src, &parsed.green);
        units.push(pw_core::check::Unit {
            path: path.to_string(),
            src,
            hir,
        });
    }
    let compiled = match pw_core::backend::speculation::compile(&units) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pw: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut modules = Vec::new();
    for c in &compiled {
        match &c.module {
            pw_core::backend::wasm::Encoding::Encoded(m) => modules.push(m),
            other => {
                eprintln!("pw: `{}`'s speculations were not compiled: {other}", c.page);
                return ExitCode::FAILURE;
            }
        }
    }
    if let Err(e) = std::fs::create_dir_all(out) {
        eprintln!("pw: cannot create {out}: {e}");
        return ExitCode::from(2);
    }
    for m in &modules {
        let dir = std::path::Path::new(out);
        let manifest = serde_json::json!({
            "page": m.page,
            "module": format!("{}.mjs", m.page),
            "bindings": m.bindings,
            "commands": m.commands,
            // Each part the browser renders again with it (ADR-0172).
            "regions": m.regions,
        });
        for (file, text) in [
            (dir.join(format!("{}.mjs", m.page)), m.source.clone()),
            (
                dir.join(format!("{}.json", m.page)),
                serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
            ),
        ] {
            if let Err(e) = std::fs::write(&file, text) {
                eprintln!("pw: cannot write {}: {e}", file.display());
                return ExitCode::from(2);
            }
        }
        println!(
            "speculation `{}`: {} on {} ({} bytes)",
            m.page,
            m.commands.join(", "),
            m.bindings
                .iter()
                .map(|b| b.binding.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            m.source.len()
        );
    }
    ExitCode::SUCCESS
}

fn emit_handlers_command(paths: &[&String], out: &str) -> ExitCode {
    let mut units = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        let hir = pw_core::lower::lower_file(&src, &parsed.green);
        units.push(pw_core::check::Unit {
            path: path.to_string(),
            src,
            hir,
        });
    }
    let compiled = match pw_core::backend::js::compile(&units) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pw: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut modules = Vec::new();
    let mut refused = 0;
    for c in &compiled {
        match &c.module {
            pw_core::backend::wasm::Encoding::Encoded(m) => modules.push((c, m)),
            other => {
                refused += 1;
                eprintln!(
                    "pw: a handler in `{}` was not compiled: {other}",
                    c.declaration
                );
            }
        }
    }
    if refused > 0 {
        eprintln!("pw: {refused} handler(s) refused; nothing written");
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::create_dir_all(out) {
        eprintln!("pw: cannot create {out}: {e}");
        return ExitCode::from(2);
    }
    for (c, m) in &modules {
        let file = std::path::Path::new(out).join(format!("{}.mjs", m.identity));
        if let Err(e) = std::fs::write(&file, &m.source) {
            eprintln!("pw: cannot write {}: {e}", file.display());
            return ExitCode::from(2);
        }
        println!(
            "handler {} `{}` in `{}` calls `{}` ({} bytes)",
            m.identity,
            m.name,
            c.declaration,
            m.commands.join("`, `"),
            m.source.len()
        );
    }
    println!("{} handler(s) written to {out}", modules.len());
    ExitCode::SUCCESS
}

fn emit_component_command(paths: &[&String], component_id: &str, out: &str) -> ExitCode {
    let mut units = Vec::new();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        let parsed = pw_syntax::parse_tree(&src);
        if !parsed.ok() {
            eprintln!("pw: {path} does not parse; nothing emitted");
            return ExitCode::FAILURE;
        }
        let hir = pw_core::lower::lower_file(&src, &parsed.green);
        units.push(pw_core::check::Unit {
            path: path.to_string(),
            src,
            hir,
        });
    }
    let compiled = match pw_core::backend::component::compile(&units, component_id) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pw: {e}");
            return ExitCode::FAILURE;
        }
    };
    let audit = pw_core::backend::component::audit(
        &compiled.component.bytes,
        &compiled.wit,
        &compiled.component.world,
    );
    if let Err(e) = std::fs::write(out, &compiled.component.bytes) {
        eprintln!("pw: cannot write {out}: {e}");
        return ExitCode::from(2);
    }
    println!(
        "{component_id}: world {} — {} bytes ({} bytes of core module)",
        compiled.component.world,
        compiled.component.bytes.len(),
        compiled.component.core.len()
    );
    for i in &compiled.component.imports {
        println!("  imports {i}");
    }
    match audit {
        Ok(n) => {
            println!("  component-level audit: {n} functions agree with the world");
            ExitCode::SUCCESS
        }
        Err(wrong) => {
            for w in wrong {
                eprintln!("  audit: {w}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Every `.pw` file under `dir`, recursively, in order.
fn pw_files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            pw_files_under(&p, out)?;
        } else if p.extension().is_some_and(|x| x == "pw") {
            out.push(p);
        }
    }
    Ok(())
}

/// **A directory's program** (ADR-0149): every `.pw` file under it, and the
/// standard packages.
fn program_in(dir: &str) -> Result<Vec<pw_core::check::Unit>, String> {
    let packages = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages");
    let mut paths = Vec::new();
    for p in ["pw-std", "pw-platform-web"] {
        pw_files_under(&packages.join(p), &mut paths).map_err(|e| format!("{p}: {e}"))?;
    }
    let packaged = paths.len();
    pw_files_under(std::path::Path::new(dir), &mut paths).map_err(|e| format!("{dir}: {e}"))?;
    if paths.len() == packaged {
        return Err(format!("{dir} holds no .pw file"));
    }
    // Named as the program names them, relative to where it is: a report
    // reads the same wherever the two programs were put.
    let root = packages
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    paths
        .into_iter()
        .map(|p| {
            let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let named = p
                .strip_prefix(dir)
                .or_else(|_| p.strip_prefix(&root))
                .unwrap_or(&p);
            Ok(pw_core::check::Unit {
                hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                path: named.display().to_string(),
                src,
            })
        })
        .collect()
}

/// `pw diff OLD NEW`: what the change from one program to the other means,
/// for review, in charter §19.2's sections (ADR-0149).
fn diff_command(old: &str, new: &str, json: bool) -> ExitCode {
    let model = |dir: &str| program_in(dir).and_then(|units| pw_core::semantic::Model::of(&units));
    let (a, b) = match (model(old), model(new)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) => {
            eprintln!("pw diff: {old}: {e}");
            return ExitCode::from(1);
        }
        (_, Err(e)) => {
            eprintln!("pw diff: {new}: {e}");
            return ExitCode::from(1);
        }
    };
    let report = pw_core::semantic::diff(&a, &b);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        print!("{}", report.text());
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!();
        eprintln!("internal compiler error: this is a bug in `pw`, not in your code.");
        eprintln!("  {info}");
        eprintln!();
        eprintln!("  what to do: the arguments below reproduce it. Please file them");
        eprintln!("  with the source file. Nothing was checked, so the absence of");
        eprintln!("  other diagnostics means nothing.");
        eprintln!(
            "  reproduce: pw {}",
            std::env::args().skip(1).collect::<Vec<_>>().join(" ")
        );
        let _ = &previous;
    }));

    match std::panic::catch_unwind(run) {
        Ok(code) => code,
        // Distinct from an ordinary diagnostic failure, so a script can tell
        // "your program is wrong" from "the compiler is wrong".
        Err(_) => ExitCode::from(101),
    }
}

fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let plain = args.iter().any(|a| a == "--plain");
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    // `pw diff OLD NEW [--json]` (ADR-0149): two directories, each a whole
    // program, with the standard packages added to both.
    if cmd == "diff" {
        let dirs: Vec<&String> = args
            .iter()
            .skip(1)
            .filter(|a| !a.starts_with("--"))
            .collect();
        let [old, new] = dirs.as_slice() else {
            eprintln!("usage: pw diff OLD_DIR NEW_DIR [--json]");
            return ExitCode::from(2);
        };
        return diff_command(old, new, args.iter().any(|a| a == "--json"));
    }
    // `--out DIR` takes a value, so DIR must not also be read as a source
    // path. Without this the output directory is opened as a `.pw` file and the
    // command fails after already having written its files.
    let app_sources: Vec<String>;
    let mut paths: Vec<&String> = Vec::new();
    let mut skip_next = false;
    for (i, a) in args.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--out"
            || a == "--component"
            || a == "--schema-lock"
            || a == "--dist"
            || a == "--asset"
        {
            skip_next = true;
            continue;
        }
        if a.starts_with("--") {
            continue;
        }
        let _ = i;
        paths.push(a);
    }
    // **A whole app** (`pw build --dist DIR`, ADR-0429): its sources, each
    // directory given read for its `.pw` files and the standard packages
    // added where it names none of them, checked as `pw check` checks them
    // and then built.
    let dist = (cmd == "build")
        .then(|| flag_value(&args, "--dist"))
        .flatten();
    if dist.is_some() {
        app_sources = match app_sources_of(&paths) {
            Ok(sources) => sources,
            Err(e) => {
                eprintln!("pw build: {e}");
                return ExitCode::from(2);
            }
        };
        paths = app_sources.iter().collect();
    }

    if !matches!(
        cmd,
        "check"
            | "explain"
            | "fmt"
            | "emit-koka"
            | "emit-marko"
            | "emit-manifest"
            | "emit-graph"
            | "emit-contracts"
            | "emit-template"
            | "emit-wit"
            | "audit-values"
            | "emit-component"
            | "emit-handlers"
            | "emit-speculations"
            | "build"
    ) || paths.is_empty()
    {
        eprintln!(
            "usage: pw <check|explain|fmt|emit-koka|emit-marko|emit-manifest|emit-graph|\
             emit-contracts|emit-template|emit-wit|audit-values|emit-component|emit-handlers|\
             emit-speculations|build> <path.pw>... [--plain]"
        );
        eprintln!();
        eprintln!("  check          parse and report diagnostics");
        eprintln!("  explain        print types, effects, privacy and derived placement");
        eprintln!("  fmt            rewrite files canonically; --check reports instead");
        eprintln!("  emit-koka      print Koka for the pure subset (ADR-0015)");
        eprintln!("  emit-marko     write Marko templates to --out DIR (ADR-0017)");
        eprintln!("  emit-manifest  print the resource manifests as JSON");
        eprintln!("  emit-graph     print the resource dependency graph (--plain for text)");
        eprintln!("  emit-template  print the checked template IR (--plain for text)");
        eprintln!("  emit-wit       print a WIT world per component contract");
        eprintln!("  audit-values   count the value relations decided, and why the rest were not");
        eprintln!(
            "  emit-component --component ID --out FILE  compile one component to a Wasm component"
        );
        eprintln!(
            "  emit-handlers --out DIR  compile every resumable handler to DIR/<identity>.mjs"
        );
        eprintln!(
            "  build --out DIR          every artifact: templates, handlers, components, contracts, WIT"
        );
        eprintln!(
            "  build --dist DIR <app>   a whole app to serve: the standard packages added, its artifacts,"
        );
        eprintln!(
            "                           the runtime, each --asset FILE=NAME, and its sources' digests;"
        );
        eprintln!("                           --production packs it as a production build");
        eprintln!(
            "  --schema-lock FILE       (check, build) hold the program's tables to the schema released"
        );
        return ExitCode::from(2);
    }

    // **The schema released** (track `tables`, ADR-0338): the `schema.lock`
    // a deployment holds the program's tables to, read whole or refused.
    let release_path = args
        .iter()
        .position(|a| a == "--schema-lock")
        .map(|i| args.get(i + 1));
    if release_path == Some(None) {
        eprintln!("pw: --schema-lock needs a file");
        return ExitCode::from(2);
    }
    let released = match release_path.flatten() {
        Some(path) => match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| pw_core::tables::Schema::read(&text))
        {
            Ok(schema) => Some(schema),
            Err(e) => {
                eprintln!("pw: {path}: {e}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };

    // What only a whole app's build takes.
    let production = args.iter().any(|a| a == "--production");
    let assets = flag_values(&args, "--asset");
    if cmd == "build" && dist.is_none() && (production || !assets.is_empty()) {
        eprintln!("pw build: --production and --asset build a whole app: give --dist DIR");
        return ExitCode::from(2);
    }
    if cmd == "build" && dist.is_some() && flag_value(&args, "--out").is_some() {
        eprintln!(
            "pw build: --dist DIR builds a whole app, --out DIR its artifacts alone: one of them"
        );
        return ExitCode::from(2);
    }

    if cmd == "build" && dist.is_none() {
        let out = args
            .iter()
            .position(|a| a == "--out")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let Some(out) = out else {
            eprintln!("usage: pw build --out DIR [--schema-lock FILE] <path.pw>...");
            eprintln!(
                "       pw build --dist DIR [--production] [--asset FILE=NAME]... <app path>..."
            );
            return ExitCode::from(2);
        };
        let sources: Vec<&String> = paths.iter().copied().filter(|p| **p != out).collect();
        return build_command(&sources, &out, released.as_ref());
    }

    if cmd == "emit-speculations" {
        let out = args
            .iter()
            .position(|a| a == "--out")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let Some(out) = out else {
            eprintln!("usage: pw emit-speculations --out DIR <path.pw>...");
            return ExitCode::from(2);
        };
        let sources: Vec<&String> = paths.iter().copied().filter(|p| **p != out).collect();
        return emit_speculations_command(&sources, &out);
    }
    if cmd == "emit-handlers" {
        let out = args
            .iter()
            .position(|a| a == "--out")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let Some(out) = out else {
            eprintln!("usage: pw emit-handlers --out DIR <path.pw>...");
            return ExitCode::from(2);
        };
        let sources: Vec<&String> = paths.iter().copied().filter(|p| **p != out).collect();
        return emit_handlers_command(&sources, &out);
    }

    if cmd == "emit-component" {
        let flag = |name: &str| {
            args.iter()
                .position(|a| a == name)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let (Some(id), Some(out)) = (flag("--component"), flag("--out")) else {
            eprintln!("usage: pw emit-component --component ID --out FILE <path.pw>...");
            return ExitCode::from(2);
        };
        let sources: Vec<&String> = paths.iter().copied().filter(|p| **p != id).collect();
        return emit_component_command(&sources, &id, &out);
    }

    if cmd == "audit-values" {
        return audit_values_command(&paths);
    }

    if cmd == "emit-manifest" {
        return emit_manifest_command(&paths);
    }

    if cmd == "emit-graph" {
        return emit_graph_command(&paths, args.iter().any(|a| a == "--plain"));
    }

    if cmd == "emit-contracts" {
        return emit_contracts_command(&paths, args.iter().any(|a| a == "--plain"));
    }

    if cmd == "emit-template" {
        return emit_template_command(&paths, args.iter().any(|a| a == "--plain"));
    }

    if cmd == "emit-wit" {
        return emit_wit_command(&paths);
    }

    if cmd == "emit-koka" {
        return emit_koka_command(&paths);
    }

    if cmd == "emit-marko" {
        let out_dir = args
            .iter()
            .position(|a| a == "--out")
            .and_then(|i| args.get(i + 1))
            .cloned();
        return emit_marko_command(&paths, out_dir);
    }

    if cmd == "fmt" {
        return fmt_command(&paths, args.iter().any(|a| a == "--check"));
    }

    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut files = 0usize;

    // Two passes. Body-level checks need a whole-program type environment: a
    // `match` in one file can be on a type declared in another, so a per-file
    // loop cannot check it at all (assumption A-009).
    struct Input {
        /// The path as given, and the name diagnostics call the file by.
        path: String,
        display: String,
        src: String,
        parsed: pw_syntax::Parse,
        /// `None` when the file did not parse cleanly. Recovery invents
        /// plausible declarations, and lowering those would put fictional types
        /// in the environment for every other file (E0 finding F-4).
        hir: Option<pw_core::hir::Hir>,
    }
    let mut inputs = Vec::new();
    // What a whole app's build is built from once it is checked.
    let checking = paths.clone();
    for path in paths {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pw: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        };
        files += 1;
        let display = Path::new(path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        let parsed = pw_syntax::parse_tree(&src);
        let hir = parsed
            .errors
            .is_empty()
            .then(|| pw_core::lower::lower_file(&src, &parsed.green));
        inputs.push(Input {
            path: path.to_string(),
            display,
            src,
            parsed,
            hir,
        });
    }

    // A file is named by its file name, and by the path it was given as
    // when another file has that name too (ADR-0106): `store/app.pw` and
    // `kiokun/app.pw` are two files.
    let mut named: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for i in &inputs {
        *named.entry(i.display.clone()).or_default() += 1;
    }
    for i in &mut inputs {
        if named[&i.display] > 1 {
            i.display = i.path.clone();
        }
    }

    // Only files that parse cleanly enter the program. Recovery invents
    // plausible-looking declarations, and lowering those would put fictional
    // types in the environment for every other file (E0 finding F-4).
    let clean: Vec<(String, String)> = inputs
        .iter()
        .filter(|i| i.parsed.errors.is_empty())
        .map(|i| (i.display.clone(), i.src.clone()))
        .collect();
    // Each file's diagnostics by its place, in the order the checker was
    // given them (ADR-0106). A map keyed by name kept the last file's for
    // every file of that name: `pw check a/app.pw b/app.pw` passed with an
    // error in `a/app.pw` until 2026-09-26.
    let all: Vec<_> = inputs
        .iter()
        .map(|i| (i.display.clone(), i.src.clone()))
        .collect();
    let checked = pw_core::check::check_program_sources_with(&all, released.as_ref());
    let body_diags: Vec<Vec<Diagnostic>> = inputs
        .iter()
        .zip(checked)
        .map(|(i, (_, diagnostics))| {
            if i.parsed.errors.is_empty() {
                diagnostics
            } else {
                Vec::new()
            }
        })
        .collect();

    // One graph for every file given, so `explain`'s key audit is a
    // whole-program answer. Built once rather than per file: a fragment's key
    // is only auditable against the resources it reads, and those are usually
    // in other files.
    let program_hirs: Vec<&pw_core::hir::Hir> =
        inputs.iter().filter_map(|i| i.hir.as_ref()).collect();
    let program_ws = pw_core::resolve::Workspace::build(&program_hirs);
    let program_graph = pw_core::graph::Graph::build(&program_hirs, &program_ws);

    for (input, checked) in inputs.iter().zip(&body_diags) {
        let Input {
            display,
            src,
            parsed,
            hir,
            ..
        } = input;

        if !parsed.errors.is_empty() {
            print!("{}", render(src, display, &parsed.errors, !plain));
            errors += parsed.errors.len();
        } else {
            // Semantic rules only run on a clean parse, for the same reason.
            // `hir` is `Some` exactly when `parsed.errors` is empty, three
            // lines up. The `else` branch is unreachable and says so rather
            // than silently reporting a clean file.
            // One checker (ADR-0090): the declaration rules run inside
            // `check_sources`, as they do for `pw build`.
            let mut found = checked.clone();
            found.sort_by_key(|d| d.primary_span.start);
            if !found.is_empty() {
                print!("{}", render_findings(src, display, &found, &clean, !plain));
                errors += found.iter().filter(|f| f.is_error()).count();
                warnings += found.iter().filter(|f| !f.is_error()).count();
            }
        }
        if cmd == "explain" && parsed.errors.is_empty() {
            println!("── {display}");
            if let Some(hir) = hir {
                print!("{}", explain_with(hir, src, Some(&program_graph)));
            }
        }
    }

    if cmd == "check" {
        if errors == 0 && warnings == 0 {
            println!("pw check: {files} file(s), no diagnostics");
        } else {
            println!("pw check: {files} file(s), {errors} error(s), {warnings} warning(s)");
        }
    }

    // A whole app, its sources checked as `pw check` checks them: what they
    // say is said first, and nothing is built from a program in error.
    if let Some(dist) = &dist {
        if errors > 0 {
            println!(
                "pw build: {files} file(s), {errors} error(s), {warnings} warning(s); nothing built"
            );
            return ExitCode::from(1);
        }
        return dist_command(&checking, dist, production, &assets, released.as_ref());
    }

    if errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A diagnostic of `code` at `primary` in its file, with `related`.
    fn finding(
        primary: std::ops::Range<usize>,
        related: Vec<pw_core::diagnostics::Related>,
    ) -> Diagnostic {
        let mut d = Diagnostic::error(
            "PW5046",
            "a page is declared for at least its layout's eyes",
            pw_core::diagnostics::Detector::DeclarationRule,
            "the message",
            primary,
        );
        d.related = related;
        d
    }

    /// **A span outside its file is shown as a note, never a stop**
    /// (ADR-0321): a checker's defect, told as one. Until 2026-10-12 a span
    /// past a file's end stopped `pw check` with an internal error.
    #[test]
    fn a_span_outside_its_file_is_a_note_and_no_panic() {
        let src = "module page\n";
        let past = pw_core::diagnostics::Related::new(3307..3312, "declared here");
        let text = render_findings(src, "page.pw", &[finding(0..6, vec![past])], &[], false);
        assert!(text.contains("[PW5046]"), "{text}");
        assert!(
            text.contains("page.pw, bytes 3307..3312: declared here"),
            "{text}"
        );
        // A primary span outside the file too: no snippet, the note alone.
        let text = render_findings(src, "page.pw", &[finding(90..99, vec![])], &[], false);
        assert!(
            text.contains("page.pw, bytes 90..99: the message"),
            "{text}"
        );
        // And a note naming a file that was not checked.
        let mut elsewhere = pw_core::diagnostics::Related::new(0..5, "declared here");
        elsewhere.file = Some("gone.pw".to_string());
        let text = render_findings(
            src,
            "page.pw",
            &[finding(0..6, vec![elsewhere])],
            &[],
            false,
        );
        assert!(
            text.contains("gone.pw, bytes 0..5: declared here"),
            "{text}"
        );
    }

    /// **A note in another file is shown in it**, by its own line.
    #[test]
    fn a_note_in_another_file_is_shown_in_it() {
        // The page's text runs past the note's span, so a renderer that read
        // the span in the page's file would mark the page's own text.
        let page = "module page\n\nlayout Shell\n// the page's own text, past the note's span\n";
        let shell = "module shell\n\nsession layout Shell {}\n";
        let at = shell.find("Shell {").expect("the layout's name");
        let mut note = pw_core::diagnostics::Related::new(at..at + 5, "declared here");
        note.file = Some("shell.pw".to_string());
        let files = [
            ("shell.pw".to_string(), shell.to_string()),
            ("page.pw".to_string(), page.to_string()),
        ];
        let text = render_findings(
            page,
            "page.pw",
            &[finding(13..25, vec![note])],
            &files,
            false,
        );
        assert!(text.contains("page.pw:3:1"), "{text}");
        assert!(text.contains("shell.pw:3:16"), "{text}");
        assert!(text.contains("declared here"), "{text}");
    }

    /// Parse and lower, the one way the compiler does it.
    fn hir_of(src: &str) -> pw_core::hir::Hir {
        let p = pw_syntax::parse_tree(src);
        assert!(p.ok(), "parse errors: {:?}", p.errors);
        pw_core::lower::lower_file(src, &p.green)
    }

    #[test]
    fn explain_never_leaks_generated_paths() {
        // Charter §14 M2 gate: the compiler prints an effect summary "without
        // exposing generated-file paths to the user".
        let src = "module store.pricing\n\npublic query Store(id: StoreId) -> Store\n    cache shared\n{\n    Stores.get(id)\n}\n\nfn subtotal(c: Cart) -> Money !{} { 0 }\n";
        let text = explain_with(&hir_of(src), src, None);
        for leak in [
            "koka",
            "Koka",
            ".kki",
            "marko",
            "Marko",
            ".pw-build",
            "generated/",
            "target/",
        ] {
            assert!(!text.contains(leak), "explain leaked {leak:?}:\n{text}");
        }
        assert!(text.contains("store.pricing"));
        assert!(text.contains("explicit purity claim"), "{text}");
    }

    #[test]
    fn explain_shows_every_policy_the_compiler_knows_about() {
        // Charter §14 M4 gate: *all* query and command policies are visible in
        // `pw explain`. Checked against HIR rather than against a hand-written
        // list, so a policy the language gains cannot quietly stop being shown.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/accepted");
        let mut checked = 0usize;
        let mut missing = Vec::new();

        for entry in std::fs::read_dir(&root).expect("accepted/") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|e| e != "pw") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read");
            let hir = hir_of(&src);
            let text = explain_with(&hir, &src, None);

            for (_, decl) in hir.all_decls() {
                for policy in &decl.policies {
                    checked += 1;
                    // The name must appear, and so must the value — showing
                    // `retry` without `bounded_exponential(max = 3)` tells a
                    // reader a policy exists but not what it says.
                    let shown = text.contains(&policy.name)
                        && (policy.value.is_empty() || text.contains(&policy.value));
                    if !shown {
                        missing.push(format!(
                            "{}: {} {}",
                            path.file_name().unwrap().to_string_lossy(),
                            policy.name,
                            policy.value
                        ));
                    }
                }
            }
        }

        assert!(
            checked >= 20,
            "expected a real sample, saw {checked} policies"
        );
        assert!(
            missing.is_empty(),
            "{} of {checked} policies are not visible in `pw explain`:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }

    /// The other half of ADR-0018's boundary for E6.
    ///
    /// `runtime/pw-materialize/tests/store-graph.json` is real `pw emit-graph`
    /// output, checked in and read by a crate that shares no type with the
    /// compiler. That is what makes the boundary testable, and it is also how
    /// a checked-in fixture goes stale: rename a field here and the runtime's
    /// tests keep passing against last week's bytes, until an empty list at run
    /// time becomes a fragment nothing ever invalidates.
    ///
    /// So the fixture is regenerated and compared. A rename fails a test.
    #[test]
    fn the_committed_graph_matches_what_the_compiler_emits() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut paths: Vec<std::path::PathBuf> = vec![root.join("examples/domain.pw")];
        for dir in ["examples/lib", "examples/store"] {
            let mut found: Vec<_> = std::fs::read_dir(root.join(dir))
                .unwrap_or_else(|e| panic!("{dir}: {e}"))
                .map(|e| e.expect("entry").path())
                .filter(|p| p.extension().is_some_and(|x| x == "pw"))
                .collect();
            found.sort();
            paths.extend(found);
        }

        let hirs: Vec<pw_core::hir::Hir> = paths
            .iter()
            .map(|p| {
                let src = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p:?}: {e}"));
                let parsed = pw_syntax::parse_tree(&src);
                assert!(parsed.ok(), "{p:?} does not parse");
                pw_core::lower::lower_file(&src, &parsed.green)
            })
            .collect();
        let refs: Vec<&pw_core::hir::Hir> = hirs.iter().collect();
        let ws = pw_core::resolve::Workspace::build(&refs);
        let g = pw_core::graph::Graph::build(&refs, &ws);
        let fresh = serde_json::to_string_pretty(&g).expect("serialize");

        let committed =
            std::fs::read_to_string(root.join("runtime/pw-materialize/tests/store-graph.json"))
                .expect("store-graph.json");

        assert_eq!(
            fresh.trim(),
            committed.trim(),
            "the committed graph is stale. Regenerate it with `just materialize` \
             and say in the commit message what changed about the graph's shape."
        );
    }

    #[test]
    fn explain_flags_a_session_value_in_a_shared_cache() {
        // The charter §7.8 rule, surfaced as a forward-looking warning until E5
        // makes it an error.
        let src = "module cart\n\nsession query Cart(s: SessionId) -> Cart\n    cache shared\n{\n    Carts.current(s)\n}\n";
        let text = explain_with(&hir_of(src), src, None);
        assert!(text.contains("Session<SessionId>"), "{text}");
        assert!(text.contains("rejected at E5"), "{text}");
    }

    #[test]
    fn explain_derives_placement_from_effects_alone() {
        let src = "module m\nfn f() -> () !{ secret<Payments> } { 0 }\nfn g() -> () !{ layout.measure } { 0 }\n";
        let text = explain_with(&hir_of(src), src, None);
        assert!(text.contains("Origin (secret"), "{text}");
        assert!(text.contains("Browser (browser-only"), "{text}");
    }

    #[test]
    fn check_renders_a_diagnostic_with_a_real_span() {
        let src = "module m\n\n$$$\n";
        let parsed = pw_syntax::parse_tree(src);
        assert!(!parsed.ok());
        let text = render(src, "m.pw", &parsed.errors, false);
        // Syntax errors live in PW00xx; PW01xx and above are semantic
        // invariants. They shared a range until the collision surfaced.
        assert!(text.contains("PW0007"), "{text}");
        assert!(text.contains("m.pw"), "{text}");
        assert!(!text.contains('\u{1b}'), "plain output must be ANSI-free");
    }

    #[test]
    fn unannotated_and_empty_effect_rows_read_differently() {
        let src = "module m\nfn a() -> Int { 0 }\nfn b() -> Int !{} { 0 }\n";
        let text = explain_with(&hir_of(src), src, None);
        assert!(text.contains("unannotated — claims nothing"), "{text}");
        assert!(text.contains("explicit purity claim"), "{text}");
    }
}
