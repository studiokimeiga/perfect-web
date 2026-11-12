//! **One program, every artifact** — what `pw build` writes.
//!
//! E10's gate: "Store application builds from source to browser artifacts and
//! Wasm Components without Koka or Marko." Until 2026-09-25 the store's
//! artifacts came from four commands, and three of its five components were
//! compiled only inside tests. This composes the stages that already own each
//! artifact; it decides nothing itself:
//!
//! ```text
//! the template IR          template_ir::build_with, with the handler
//!                          identities resume_artifacts derived
//! the handler modules      backend::js::compile
//! the components           backend::component::compile_all, each audited
//! the contracts, the WIT   contract::contracts, wit::package
//! the resource graph       graph::Graph::build (since ADR-0123)
//! the speculations         backend::speculation::compile (ADR-0122)
//! ```
//!
//! Nothing here reaches Koka or Marko. `pw emit-koka` and `pw emit-marko` are
//! separate commands, and this module calls neither backend.

use crate::backend::component::Built;
use crate::backend::js;
use crate::check::Unit;

/// Every artifact of one program.
#[derive(Debug, Clone)]
pub struct Build {
    /// What the renderer renders, with each event part's handler identity.
    pub templates: Vec<crate::template_ir::Template>,
    /// Each resumable handler, compiled or refused.
    pub handlers: Vec<js::Compiled>,
    /// Each contract, in contract order: a component, a declaration with no
    /// component body, or a refusal.
    pub components: Vec<(String, Built)>,
    /// Each query that reaches no host, as an ES module (ADR-0044), by
    /// component id.
    pub modules: Vec<(String, String)>,
    pub contracts: Vec<crate::contract::ComponentContract>,
    pub wit: String,
    /// What each command emits and each entry listens for (ADR-0123): what
    /// a host's materializer consumes. Until 2026-10-02 the development
    /// server compiled a committed copy into itself.
    pub graph: crate::graph::Graph,
    /// Each page's optimistic speculation module (ADR-0122).
    pub speculations: Vec<crate::backend::speculation::Compiled>,
    /// **What each page computes from its signals** (ADR-0227), as the
    /// browser computes it: a module per page that computes any.
    pub computed: Vec<crate::backend::computed::Compiled>,
    /// What each page shows, as reads of what its queries return (ADR-0125).
    pub pages: Vec<crate::page_values::Planned>,
    /// **What each data source states it guarantees** (ADR-0207), which a
    /// host compares with what the database it opens provides (ADR-0246).
    pub sources: Vec<crate::check::SourceClauses>,
    /// Track `uploads` (ADR-0260): each upload the program declares, which
    /// a host holds a browser's file to.
    pub uploads: Vec<crate::uploads::UploadClauses>,
    /// **The words each predicate the program declares is told in**
    /// (ADR-0302), which a host answers a refusal by it with.
    pub predicates: std::collections::BTreeMap<String, String>,
    /// **Each table the program declares** (track `tables`, ADR-0338): its
    /// schema, its functions as a host binds their imports, and the
    /// migration from the schema released.
    pub tables: crate::tables::TablesJson,
    /// **The schema the build declares**, which `schema.lock` holds once the
    /// build is released.
    pub schema: crate::tables::Schema,
}

impl Build {
    /// **Write every artifact under `dir`** (ADR-0123), and say what was
    /// written, one line each. What `pw build --out DIR` writes, in one place,
    /// so a host's tests can serve a program they built (ADR-0145):
    ///
    /// ```text
    /// DIR/templates.json            the template IR, with handler identities
    /// DIR/handlers/<identity>.mjs   each resumable handler's compiled body
    /// DIR/components/<id>.wasm      each command and query, audited
    /// DIR/modules/<id>.mjs          each query that reaches no host (ADR-0044)
    /// DIR/contracts.json            what the host admits each component by
    /// DIR/app.wit                   the worlds the components implement
    /// DIR/pages/<page>.json         what each page shows (ADR-0125)
    /// DIR/graph.json                what a materializer consumes (ADR-0123)
    /// DIR/uploads.json              each upload's route and limits, which a
    ///                               host holds a browser's file to; none
    ///                               where the program declares no upload
    /// DIR/sources.json              what each data source guarantees, which
    ///                               a host holds its database to (ADR-0246)
    /// DIR/predicates.json           the words a refusal by each predicate the
    ///                               program declares is told in (ADR-0302);
    ///                               none where it declares none
    /// DIR/tables.json               each table: its schema, its functions as a
    ///                               host binds their imports, and the
    ///                               migration from the schema released
    ///                               (ADR-0338); none where it declares none
    /// DIR/read-plans.json           candidate keyed/set read pairs with provenance;
    ///                               not an executed runtime optimization
    /// DIR/schema.lock               the schema the build declares: committed
    ///                               beside the program when it is released
    /// DIR/speculations/<page>.*     each page's speculations (ADR-0122)
    /// DIR/computed/<page>.mjs       what each page computes from its
    ///                               signals, in the browser (ADR-0227)
    /// DIR/build-id                  the build's name, from all of the above
    ///                               (ADR-0300), written last
    /// ```
    pub fn write(&self, dir: &std::path::Path) -> Result<Vec<String>, String> {
        let refused = self.refusals();
        if !refused.is_empty() {
            return Err(format!("nothing written: {}", refused.join("; ")));
        }
        // **The build is named by what it built** (ADR-0300): FNV-1a, the
        // resume artifacts' own hash, over each file written, its path and
        // its bytes, in the order written.
        let named = std::cell::Cell::new(FNV_OFFSET);
        let write = |rel: &str, bytes: &[u8]| -> Result<(), String> {
            named.set(fnv1a(fnv1a(named.get(), rel.as_bytes()), bytes));
            let path = dir.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
        };
        let mut lines = Vec::new();
        let templates = serde_json::to_string_pretty(&self.templates).map_err(|e| e.to_string())?;
        write("templates.json", format!("{templates}\n").as_bytes())?;
        let contracts = serde_json::to_string_pretty(&self.contracts).map_err(|e| e.to_string())?;
        write("contracts.json", format!("{contracts}\n").as_bytes())?;
        write("app.wit", self.wit.as_bytes())?;
        for h in &self.handlers {
            if let crate::backend::wasm::Encoding::Encoded(m) = &h.module {
                write(&format!("handlers/{}.mjs", m.identity), m.source.as_bytes())?;
                lines.push(format!(
                    "  handler    {}  `{}` calls `{}`  {} bytes",
                    m.identity,
                    m.name,
                    m.commands.join("`, `"),
                    m.source.len()
                ));
            }
        }
        for (id, built) in &self.components {
            match built {
                Built::Component { compiled, audited } => {
                    let c = &compiled.component;
                    write(&format!("components/{id}.wasm"), &c.bytes)?;
                    lines.push(format!(
                        "  component  {id}  {} bytes ({} core), {audited} function(s) audited, \
                         imports [{}]",
                        c.bytes.len(),
                        c.core.len(),
                        c.imports.join(", ")
                    ));
                }
                Built::NoBody { kind } => {
                    lines.push(format!("  no body    {id}  a {kind:?}"));
                }
                Built::Placeholder => {
                    lines.push(format!(
                        "  todo       {id}  a placeholder body; nothing built depends on it"
                    ));
                }
                Built::Refused(_) => {}
            }
        }
        for (id, source) in &self.modules {
            write(&format!("modules/{id}.mjs"), source.as_bytes())?;
            lines.push(format!("  module     {id}  {} bytes", source.len()));
        }
        // What each page shows, as a host computes it (ADR-0125).
        for page in &self.pages {
            if let Ok(plan) = &page.plan {
                let text = serde_json::to_string_pretty(plan).map_err(|e| e.to_string())?;
                write(
                    &format!("pages/{}.json", plan.page),
                    format!("{text}\n").as_bytes(),
                )?;
                lines.push(format!(
                    "  page       {}  {} binding(s), {} part(s){}{}",
                    plan.page,
                    plan.bindings.len(),
                    plan.parts.len(),
                    // And what fills a region in the same response (ADR-0148).
                    match plan.streams.len() {
                        0 => String::new(),
                        n => format!(", {n} stream(s)"),
                    },
                    // And whether a host renders it once (ADR-0409).
                    if plan.prerender { ", prerendered" } else { "" }
                ));
            }
        }
        // What a host's materializer consumes (ADR-0123).
        let graph = serde_json::to_string_pretty(&self.graph).map_err(|e| e.to_string())?;
        write("graph.json", format!("{graph}\n").as_bytes())?;
        // What each source guarantees, which a host compares with what its
        // database provides before it serves (ADR-0246).
        let sources = serde_json::to_string_pretty(&self.sources).map_err(|e| e.to_string())?;
        write("sources.json", format!("{sources}\n").as_bytes())?;
        // Each upload's route and limits (track `uploads`), which a host
        // holds a browser's file to before it keeps a byte. A program that
        // declares none has no file, and its host serves no upload.
        if !self.uploads.is_empty() {
            let uploads = serde_json::to_string_pretty(&self.uploads).map_err(|e| e.to_string())?;
            write("uploads.json", format!("{uploads}\n").as_bytes())?;
        }
        // The words a refusal by each predicate the program declares is told
        // in (ADR-0302). A program that declares none has no file, and its
        // host tells each in the deployment's words.
        if !self.predicates.is_empty() {
            let words =
                serde_json::to_string_pretty(&self.predicates).map_err(|e| e.to_string())?;
            write("predicates.json", format!("{words}\n").as_bytes())?;
        }
        // Each table, and the schema the build declares (ADR-0338). A program
        // that declares none has neither, and its host serves no table.
        if !self.tables.tables.is_empty() {
            let tables = serde_json::to_string_pretty(&self.tables).map_err(|e| e.to_string())?;
            write("tables.json", format!("{tables}\n").as_bytes())?;
            // An inspectable, non-executing inventory of legal read-plan pairs.
            // Runtime plan selection requires separately verified semantic context.
            let plans = serde_json::to_string_pretty(&crate::read_plans::inventory(&self.tables))
                .map_err(|e| e.to_string())?;
            write("read-plans.json", format!("{plans}\n").as_bytes())?;
            write("schema.lock", self.schema.written().as_bytes())?;
            lines.push(format!(
                "  tables     {} table(s), schema {}, {} migration step(s) from {}",
                self.tables.tables.len(),
                self.schema.hash,
                self.tables.migration.steps.len(),
                self.tables
                    .migration
                    .from
                    .as_deref()
                    .map_or("none released".to_string(), |h| format!("schema {h}"))
            ));
        } else {
            // Rebuilding into an existing directory must not leave a prior
            // program's tables available to the host.
            for name in ["tables.json", "read-plans.json", "schema.lock"] {
                match std::fs::remove_file(dir.join(name)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        for s in &self.speculations {
            if let crate::backend::wasm::Encoding::Encoded(m) = &s.module {
                let manifest = serde_json::json!({
                    "page": m.page,
                    "module": format!("{}.mjs", m.page),
                    "bindings": m.bindings,
                    "commands": m.commands,
                    // Each part the browser renders again with it (ADR-0172).
                    "regions": m.regions,
                });
                write(&format!("speculations/{}.mjs", m.page), m.source.as_bytes())?;
                write(
                    &format!("speculations/{}.json", m.page),
                    format!(
                        "{}\n",
                        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?
                    )
                    .as_bytes(),
                )?;
                lines.push(format!(
                    "  speculate  {}  {} on {}  {} bytes",
                    m.page,
                    m.commands.join(", "),
                    m.bindings
                        .iter()
                        .map(|b| b.binding.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    m.source.len()
                ));
            }
        }
        for c in &self.computed {
            if let Ok(source) = &c.module {
                write(&format!("computed/{}.mjs", c.page), source.as_bytes())?;
                lines.push(format!("  computed   {}  {} bytes", c.page, source.len()));
            }
        }
        // Its name, last, from everything above.
        let id = build_id(named.get());
        write("build-id", format!("{id}\n").as_bytes())?;
        lines.push(format!("  build      {id}"));
        Ok(lines)
    }

    /// Everything that was refused, one line each. A build with any is not
    /// finished: a page would render a button whose handler cannot load, or a
    /// host would be asked for a component that does not exist.
    pub fn refusals(&self) -> Vec<String> {
        self.support_refusals()
            .into_iter()
            .map(|r| r.message)
            .collect()
    }

    pub(crate) fn support_refusals(&self) -> Vec<crate::program::Refusal> {
        use crate::program::{Refusal, Stage};
        let mut out = Vec::new();
        for h in &self.handlers {
            if !matches!(h.module, crate::backend::wasm::Encoding::Encoded(_)) {
                out.push(Refusal::new(
                    Stage::Handler,
                    &h.declaration,
                    format!("a handler in `{}`: {}", h.declaration, h.module),
                ));
            }
        }
        let placeholders: Vec<&str> = self
            .components
            .iter()
            .filter(|(_, b)| matches!(b, Built::Placeholder))
            .map(|(id, _)| id.as_str())
            .collect();
        for (id, built) in &self.components {
            if let Built::Refused(why) = built {
                out.push(Refusal::new(Stage::Component, id, format!("`{id}`: {why}")));
            }
        }
        for c in &self.contracts {
            for target in c
                .imports
                .iter()
                .filter(|i| i.kind == crate::contract::ImportKind::Component)
                .filter_map(|i| i.interface.strip_prefix("pw:app/"))
                .filter(|target| placeholders.contains(target))
            {
                out.push(Refusal::new(
                    Stage::Placeholder,
                    &c.component_id,
                    format!(
                        "`{}` depends on `{target}`, whose body is a placeholder (`todo`)",
                        c.component_id
                    ),
                ));
            }
        }
        for s in &self.speculations {
            if !matches!(s.module, crate::backend::wasm::Encoding::Encoded(_)) {
                out.push(Refusal::new(
                    Stage::Speculation,
                    &s.page,
                    format!("`{}`'s speculations: {}", s.page, s.module),
                ));
            }
        }
        for c in &self.computed {
            if let Err(why) = &c.module {
                out.push(Refusal::new(
                    Stage::Computed,
                    &c.page,
                    format!("`{}`'s computed values: {why}", c.page),
                ));
            }
        }
        for d in &self.graph.dangling {
            out.push(Refusal::new(
                Stage::Graph,
                &d.from,
                format!("a graph edge from `{}` names nothing: `{}`", d.from, d.name),
            ));
        }
        for t in &self.templates {
            for p in t
                .manifest()
                .into_iter()
                .filter(|p| p.kind == "event" && p.value.is_empty())
            {
                out.push(Refusal::new(Stage::Event, &t.path, format!(
                    "`{}`: element {} handles an event with no code to run: write its handler as a lambda, `on:press={{() => ..}}` (ADR-0134)",
                    t.path, p.owner.map(|o| o.0).unwrap_or_default())));
            }
        }
        for p in &self.pages {
            if let Err(why) = &p.plan {
                out.push(Refusal::new(
                    Stage::Page,
                    &p.page,
                    format!("`{}`'s values: {why}", p.page),
                ));
            }
        }
        out
    }

    /// The contracts whose bodies are placeholders nothing depends on: listed
    /// by a build, never written as components.
    pub fn placeholders(&self) -> Vec<&str> {
        self.components
            .iter()
            .filter(|(_, b)| matches!(b, Built::Placeholder))
            .map(|(id, _)| id.as_str())
            .collect()
    }
}

/// **Build a checked program.** A program that does not check builds nothing.
pub fn build(units: &[Unit]) -> Result<Build, String> {
    build_with(units, None)
}

/// Historical inspection API with the same accepted schema plan as full
/// executable validation. A partial plan retains its backend refusals;
/// CLI build requires the complete Validation::into_build proof.
pub fn build_with(
    units: &[Unit],
    released: Option<&crate::tables::Schema>,
) -> Result<Build, String> {
    crate::program::validate_with(units, released).inspect()
}

pub(crate) fn prepare(
    checked: &crate::backend::lower::Checked<'_>,
    tables: crate::tables::TablesJson,
    schema: crate::tables::Schema,
) -> Result<Build, Vec<crate::program::Refusal>> {
    use crate::program::{Refusal, Stage};
    fn stage<T: Default>(result: Result<T, String>, kind: Stage, refused: &mut Vec<Refusal>) -> T {
        result.unwrap_or_else(|why| {
            refused.push(Refusal::new(kind, "", why));
            T::default()
        })
    }
    let cx = checked.context();
    let (hirs, ws, sigs, contracts) = (cx.hirs, cx.ws, cx.sigs, cx.contracts);
    let mut early = Vec::new();
    let handlers = stage(
        crate::backend::js::compile_checked(checked),
        Stage::Handler,
        &mut early,
    );
    let sources = checked.sources();
    let templates = templates(hirs, &sources, sigs);
    let layouts = crate::template_ir::layouts(hirs, sigs, &crate::template_ir::Handlers::new());
    for t in templates.iter().chain(&layouts) {
        for blocked in t.blocked() {
            if let crate::template_ir::Part::Blocked { reason, at } = blocked {
                early.push(
                    Refusal::new(
                        Stage::Template,
                        &t.path,
                        format!(
                            "a template part the renderer cannot render: {}: `{at}`: {reason}",
                            t.path
                        ),
                    )
                    .at(at),
                );
            }
        }
    }
    // A failed WIT or template stage must not erase independent page, handler,
    // speculation or signal refusals. Only WIT-dependent encoders are skipped.
    let prepared =
        match crate::wit::package_prepared(hirs, ws, contracts, sigs, &checked.pages().derived) {
            Ok((wit, worlds)) => {
                match crate::backend::component::with_package(checked, &wit, &worlds, |p| {
                    Ok((
                        crate::backend::component::compile_prepared(p),
                        crate::backend::js_pure::modules_prepared(p),
                        crate::backend::computed::compile_prepared(checked, p),
                    ))
                }) {
                    Ok((components, modules, computed)) => {
                        Some((wit, components, modules, computed))
                    }
                    Err(why) => {
                        let owner = why
                            .declaration(&wit, &worlds)
                            .map(|def| {
                                crate::contract::component_id(
                                    hirs[def.unit],
                                    crate::hir::DeclId(def.decl),
                                )
                            })
                            .unwrap_or_default();
                        early.push(Refusal::new(Stage::Wit, &owner, why.message));
                        None
                    }
                }
            }
            Err(error) => {
                early.push(Refusal::wit(error, hirs));
                None
            }
        };
    let (wit, components, modules, computed) = match prepared {
        Some((wit, components, modules, computed)) => (
            wit,
            stage(components, Stage::Component, &mut early),
            modules.unwrap_or_else(|why| {
                early.push(why);
                Vec::new()
            }),
            stage(computed, Stage::Computed, &mut early),
        ),
        None => (
            String::new(),
            Vec::new(),
            Vec::new(),
            stage(
                crate::backend::computed::compile_checked(checked),
                Stage::Computed,
                &mut early,
            ),
        ),
    };
    let graph = crate::graph::Graph::build(hirs, ws);
    let speculations = stage(
        crate::backend::speculation::compile_with_handlers(checked, &handlers),
        Stage::Speculation,
        &mut early,
    );
    let mut pages = checked.pages().pages.clone();
    for compiled in stage(
        crate::backend::signals::compile_checked(checked),
        Stage::Signal,
        &mut early,
    ) {
        let Some(planned) = pages.iter_mut().find(|p| p.page == compiled.page) else {
            if let Err(why) = compiled.signals {
                early.push(Refusal::new(
                    Stage::Signal,
                    &compiled.page,
                    format!("`{}`'s signals: {why}", compiled.page),
                ));
            }
            continue;
        };
        match (compiled.signals, &mut planned.plan) {
            (Ok(signals), Ok(plan)) => plan.signals = signals,
            (Err(why), plan @ Ok(_)) => *plan = Err(format!("its signals: {why}")),
            (Err(why), Err(_)) => early.push(Refusal::new(
                Stage::Signal,
                &compiled.page,
                format!("`{}`'s signals: {why}", compiled.page),
            )),
            (Ok(_), Err(_)) => {}
        }
    }
    // Which pages no reader decides, now that what each handler sends is
    // known (ADR-0409).
    crate::prerender::mark(&mut pages, &templates, &handlers);
    let build = Build {
        templates,
        handlers,
        components,
        modules,
        contracts: contracts.to_vec(),
        wit,
        graph,
        speculations,
        computed,
        pages,
        sources: crate::check::source_clauses(hirs),
        uploads: crate::uploads::upload_clauses(hirs),
        predicates: crate::predicates::words_of(hirs),
        tables,
        schema,
    };
    if early.is_empty() {
        Ok(build)
    } else {
        early.extend(build.support_refusals());
        Err(early)
    }
}

/// **The template IR, with every event part's handler identity**: what
/// `pw emit-template` prints and `pw build` writes, from one derivation.
///
/// Per unit, because `resume_artifacts::located` walks one HIR against the
/// whole program's signatures.
pub fn templates(
    hirs: &[&crate::hir::Hir],
    sources: &[&str],
    sigs: &crate::signatures::Signatures,
) -> Vec<crate::template_ir::Template> {
    let mut identities = crate::template_ir::Handlers::new();
    for (unit, (hir, src)) in hirs.iter().zip(sources).enumerate() {
        for (decl, lambda, m, _) in
            crate::resume_artifacts::located(src, hir, sigs, crate::resume_artifacts::BUILD)
        {
            identities.insert((unit, decl, lambda), (m.handler, m.capture_paths));
        }
    }
    crate::template_ir::build_with(hirs, sigs, &identities)
}

/// FNV-1a's offset basis, where a build's name starts.
pub(crate) const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a, 64 bits, `bytes` folded into `h`, and then a zero byte, so that
/// `"ab", "c"` and `"a", "bc"` name two things.
pub(crate) fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes.iter().chain(std::iter::once(&0u8)) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// **A build's name** (ADR-0300): `b` and sixteen hex digits.
fn build_id(h: u64) -> String {
    format!("b{h:016x}")
}
