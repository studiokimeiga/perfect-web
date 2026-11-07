//! E6 — the resource dependency graph (charter §14 M6 task 1).
//!
//! ISR invalidates by route and by timer. This invalidates by **what a thing
//! actually depends on**, which is a different claim and a checkable one: the
//! graph is derived from declarations, so "changing one menu item regenerates
//! store 47's menu fragment and nothing else" is a property of the program
//! rather than a configuration someone remembered to write.
//!
//! # Compile time answers a different question from run time
//!
//! A key is a value, so at compile time there is no node for "store 47". What
//! there is:
//!
//! ```text
//! compile time   MenuFragment(id) --reads--> Menu(id)
//!                MenuChanged(store) --invalidates--> MenuFragment(store)
//!
//! run time       MenuFragment(47) --reads--> Menu(47)
//! ```
//!
//! The compiler produces the **schema** — nodes per declaration, edges labelled
//! with the key expressions that connect them. The materializer instantiates it
//! per key. Saying so matters, because a graph that claimed to know which
//! *instances* exist would be claiming to know the contents of the database.
//!
//! What the schema is nonetheless enough for is the gate: an event carries its
//! parameters, an edge records which of them flows into which key position, and
//! that is what makes `MenuChanged(store_47)` narrower than "the menu changed".
//!
//! # Serializable, because inspectable is a gate item
//!
//! Charter §14 M6 gate item 6 asks for the graph to be inspectable and
//! serializable. Both, and not one dressed as the other: a diagram is not
//! serializable and a blob is not inspectable. `pw emit-graph` prints JSON, and
//! `--plain` prints the edges as text a person reads.

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use crate::hir::{Decl, DeclKind, Hir, Span};

/// A dimension a materialization's cache key varies by (charter §14 M6 task 1).
///
/// Not the same thing as a dependency. A dependency is a *resource whose change
/// invalidates this*; a dimension *partitions* the cache so two readers who
/// should see different content never share an entry. Confusing them is how a
/// cache serves French copy to an English reader and calls it a hit.
///
/// # What is NOT here, and why
///
/// The **compatibility generation** — which build produced the artifact — was
/// `Dimension::CodeVersion`, declared by the author as `code_version
/// included_in_key`, and `PW5102` made omitting it an error.
///
/// Architect ruling, 2026-08-06:
///
/// > If every shared materialization must be separated across incompatible
/// > code generations, that is platform mechanism, not application policy.
///
/// So it is injected unconditionally instead (see [`Graph::compatibility`] and
/// `pw_materialize::Materializer::new`). The dimensions that remain are the
/// ones an application genuinely decides: who shares an entry with whom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Locale,
    Tenant,
    /// `shared` or `private`. Charter §14 M6 gate item 4 — private data must
    /// never be written into shared materialization storage — is this
    /// dimension being present and correct.
    PrivacyPartition,
    PolicyVersion,
}

impl Dimension {
    /// The policy keyword that includes this dimension in the key.
    pub fn keyword(self) -> &'static str {
        match self {
            Dimension::Locale => "locale",
            Dimension::Tenant => "tenant",
            Dimension::PrivacyPartition => "partition",
            Dimension::PolicyVersion => "policy_version",
        }
    }

    pub const ALL: [Dimension; 4] = [
        Dimension::Locale,
        Dimension::Tenant,
        Dimension::PrivacyPartition,
        Dimension::PolicyVersion,
    ];
}

/// What a node in the graph is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "node")]
pub enum NodeKind {
    /// A `query`, `subscription` or `resource` — something with a snapshot.
    Resource {
        /// `shared`, `private` or `none`, as written.
        partition: Option<String>,
        /// `public`, `session` or `private`.
        privacy: Option<String>,
        /// The `key` policy's components, in order. `key store, experiment`
        /// gives `["store", "experiment"]`.
        ///
        /// What a resource's entries are separated BY, which is the question a
        /// key audit asks of everything that reads it: a fragment keyed by
        /// less than its dependency is a fragment two readers share when they
        /// should not.
        #[serde(default)]
        key: Vec<String>,
    },
    /// A `materialize` declaration.
    Materialization {
        placement: Option<String>,
        partition: Option<String>,
        regenerate: Option<String>,
        stampede: Option<String>,
        fallback: Option<String>,
        /// Dimensions declared `included_in_key`, sorted.
        varies_by: Vec<Dimension>,
    },
    /// An `event` declaration.
    Event,
    /// A `command` — what emits events.
    Command,
    /// A `page`, which reads resources like a materialization but is not one.
    Page,
}

/// One node: a declaration, with the part of its policy the graph cares about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// `Module.Name` — the same identity `Signatures` uses, so a node can be
    /// joined against a signature without a by-name lookup.
    pub path: String,
    pub name: String,
    #[serde(flatten)]
    pub kind: NodeKind,
    /// Parameter names in declaration order, so an edge can say which
    /// parameter of an event flows into which position of a key.
    pub params: Vec<String>,
}

/// Why one node points at another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// A materialization or page reads a resource. `depends_on Menu(id)`, or a
    /// page's `let menu = query Menu(id)`.
    Reads,
    /// A materialization regenerates when this event arrives.
    /// `invalidates_on MenuChanged(id)`.
    InvalidatedBy,
    /// A command emits an event. `emits MenuChanged(id)`.
    Emits,
    /// A command invalidates a resource directly. `invalidates Cart(session)`.
    Invalidates,
}

/// One edge, with the key expression that labels it.
///
/// The label is the source text of the argument list — `MenuChanged(id)` gives
/// `["id"]`. It is what makes the edge narrow: an edge with no arguments means
/// "any change to this", and one with `id` means "only where the ids match".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    /// The arguments as written, in order.
    pub key: Vec<String>,
}

/// The whole graph.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    /// The compatibility generation every materialized entry is namespaced by.
    ///
    /// Platform mechanism, not application policy: a shared cache entry
    /// outlives the deployment that wrote it, so markup generated by one
    /// release would otherwise be hydrated by a client running another — the
    /// mismatch E7V refuses in a resume manifest, except that a cache hit
    /// compares nothing and so nothing refuses.
    ///
    /// V1 is the whole build's identity, which is safe and blunt: any
    /// deployment invalidates every fragment. V2 is a per-materialization
    /// contract hash over its typed IR, policies, output schema, dependency
    /// contracts and platform ABI, so an unrelated deployment does not. The
    /// field is here at V1 so the consumer never learns to live without it.
    pub compatibility: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Names an edge referred to that no declaration defines.
    ///
    /// Kept in the graph rather than dropped, because a dangling edge is the
    /// E6 failure that looks like success: a materialization listening for an
    /// event nobody declared simply never regenerates, and the page is not
    /// wrong, only permanently stale.
    pub dangling: Vec<Dangling>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dangling {
    pub from: String,
    pub name: String,
    pub kind: EdgeKind,
}

impl Graph {
    /// Build from the whole program.
    ///
    /// Every unit at once, because a materialization in one module depends on
    /// a query in another and a graph built per file would have an edge to
    /// nothing in exactly the case the milestone is about.
    pub fn build(hirs: &[&Hir], ws: &crate::resolve::Workspace) -> Graph {
        Graph::build_for(hirs, ws, crate::resume_artifacts::BUILD)
    }

    /// The graph for one build identity.
    ///
    /// Exposed so a test can produce two generations of the same program and
    /// show that their entries do not share storage — the property `PW5102`
    /// used to ask the author to guarantee.
    pub fn build_for(hirs: &[&Hir], ws: &crate::resolve::Workspace, compatibility: &str) -> Graph {
        let mut g = Graph {
            compatibility: compatibility.to_string(),
            ..Graph::default()
        };

        // `DefId` -> the node path, so a resolution can name a node. Built from
        // the workspace rather than by matching spellings: E2C deleted by-name
        // resolution from member lookup, and an edge is exactly the same
        // question — whether a dependency exists must not depend on which
        // spellings happen to be unique across the program.
        let mut by_def: std::collections::BTreeMap<crate::resolve::DefId, String> =
            std::collections::BTreeMap::new();
        for m in &ws.modules {
            for def in m.defines.values() {
                by_def.insert(*def, path_of(hirs[def.unit], crate::hir::DeclId(def.decl)));
            }
        }

        // Pass one: nodes. Every edge target must exist before any edge is
        // resolved, so a forward reference is not a dangling one.
        for hir in hirs {
            for (id, decl) in hir.all_decls() {
                let Some(kind) = node_kind(decl) else {
                    continue;
                };
                g.nodes.push(Node {
                    path: path_of(hir, id),
                    name: decl.name.clone(),
                    kind,
                    params: decl.params.iter().map(|p| p.name.clone()).collect(),
                });
            }
        }

        // Pass two: edges.
        for (unit, hir) in hirs.iter().enumerate() {
            for (id, decl) in hir.all_decls() {
                if node_kind(decl).is_none() {
                    continue;
                }
                let from = path_of(hir, id);
                // An edge's target is a query, a materialization, an event, a
                // command or a page — never a TYPE. Saying so is not an
                // optimisation, it is the difference between two right answers:
                // `store.page` declares `query Store` and imports the type
                // `domain.Store`, and the general `resolve` tries the type
                // namespace first, so every page's dependency on its own query
                // resolved to a record definition instead. The graph looked
                // full and every edge pointed at the wrong kind of thing.
                //
                // Local before imported, across all three, for the same reason
                // `resolve_in` prefers local within one: a declaration in this
                // module is what the author meant.
                let resolve = |name: &str| resolve_from(ws, &by_def, unit, name);

                for (policy, kind) in [
                    ("depends_on", EdgeKind::Reads),
                    ("invalidates_on", EdgeKind::InvalidatedBy),
                    ("emits", EdgeKind::Emits),
                    ("invalidates", EdgeKind::Invalidates),
                ] {
                    let Some(p) = decl.policy(policy) else {
                        continue;
                    };
                    // The keys lowering made, each as written (ADR-0240). The
                    // graph split the clause's text until then: a value read
                    // past a missing comma was not in it, and a comma inside
                    // a key's string split the key.
                    for k in &p.keys {
                        g.push_edge(&from, &k.name, kind, k.written.clone(), resolve(&k.name));
                    }
                }

                // A page declares its dependencies by reading them:
                // `let menu = query Menu(id)`. Same edge as `depends_on`,
                // discovered differently, because a page has no policy for it
                // and requiring one would make the graph a second place to
                // state a fact the body already states.
                // A materialization that derives its value states them so
                // too (ADR-0273).
                if (decl.kind == DeclKind::Page
                    || (decl.kind == DeclKind::Materialize && decl.ret.is_some()))
                    && let Some(body_id) = decl.body
                {
                    for (name, key) in queried(hir.body(body_id)) {
                        g.push_edge(&from, &name, EdgeKind::Reads, key, resolve(&name));
                    }
                }
                // **A page reads what its layout reads** (ADR-0303): the
                // layout's markup is rendered into the page's document, and
                // its bindings are the page's, each resolved where the layout
                // is written.
                if decl.kind == DeclKind::Page
                    && let Some(layout) = crate::layouts::of(hirs, ws, unit, decl)
                    && let Some(body_id) =
                        hirs[layout.unit].decl(crate::hir::DeclId(layout.decl)).body
                {
                    for (name, key) in queried(hirs[layout.unit].body(body_id)) {
                        let target = resolve_from(ws, &by_def, layout.unit, &name);
                        g.push_edge(&from, &name, EdgeKind::Reads, key, target);
                    }
                }
            }
        }

        g.edges.sort_by(|a, b| {
            (&a.from, &a.to, a.kind, &a.key).cmp(&(&b.from, &b.to, b.kind, &b.key))
        });
        g.edges.dedup();
        g.nodes.sort_by(|a, b| a.path.cmp(&b.path));
        g.dangling
            .sort_by(|a, b| (&a.from, &a.name, a.kind).cmp(&(&b.from, &b.name, b.kind)));
        g.dangling.dedup();
        g
    }

    fn push_edge(
        &mut self,
        from: &str,
        name: &str,
        kind: EdgeKind,
        key: Vec<String>,
        target: Option<String>,
    ) {
        match target {
            Some(to) => self.edges.push(Edge {
                from: from.to_string(),
                to,
                kind,
                key,
            }),
            None => self.dangling.push(Dangling {
                from: from.to_string(),
                name: name.to_string(),
                kind,
            }),
        }
    }

    /// The semantic identity of one entry of a resource.
    ///
    /// `logical_key` is supplied by the caller, because a key is a VALUE and
    /// the compiler knows only the shape. What the compiler does know — which
    /// declaration, which partition, whether the generation participates — is
    /// answered here rather than by each consumer reading the node again.
    ///
    /// The partition comes from the resource's own declaration: `session` and
    /// `private` name a principal, and a public resource names none. A caller
    /// supplies the principal's id, because the compiler cannot know whose
    /// session it is.
    pub fn entry_identity(
        &self,
        resource: &str,
        logical_key: &[String],
        principal: Option<&str>,
    ) -> Option<EntryIdentitySpec> {
        let node = self.node(resource)?;
        let NodeKind::Resource {
            privacy, partition, ..
        } = &node.kind
        else {
            return None;
        };
        let restricted = privacy.as_deref().is_some_and(|v| v != "public")
            || partition
                .as_deref()
                .is_some_and(|v| v.starts_with("private"));
        let partition = match (restricted, privacy.as_deref(), principal) {
            (false, _, _) => "public".to_string(),
            (true, Some("private"), Some(id)) => format!("user:{id}"),
            (true, _, Some(id)) => format!("session:{id}"),
            // Restricted and no principal named. The identity would be a
            // public one, which is the exact confusion `PW5101` exists to
            // prevent, so there is no identity to give.
            (true, _, None) => return None,
        };
        Some(EntryIdentitySpec {
            resource: node.path.clone(),
            logical_key: logical_key.to_vec(),
            partition,
            // ALWAYS, and never inferred from the partition.
            //
            // Architect ruling, 2026-08-07 — correcting a coupling this bridge
            // shipped with:
            //
            //   > Privacy partition must not decide whether an entry survives
            //   > a deployment. […] A public entry can absolutely become
            //   > incompatible after a deploy. A private entry can remain
            //   > perfectly compatible across a deploy.
            //
            // They are orthogonal questions: the partition asks WHO may share
            // an entry, the generation asks WHICH code generations may regard
            // it as the same entry. The first version answered the second with
            // the first, so a session-scoped entry silently claimed to be
            // build-stable and a public one silently claimed not to be.
            //
            // V1 is conservative: every entry carries the generation. V2
            // replaces the coarse whole-build value with a per-resource
            // contract hash, so an unrelated deployment stops invalidating
            // everything — and that will come from the resource's own
            // compatibility contract, never from `Partition::Public`.
            compatibility: Some(self.compatibility.clone()),
        })
    }

    pub fn node(&self, path: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.path == path)
    }

    /// Does `command` invalidate `resource`'s entries (ADR-0101)?
    ///
    /// It names the resource in `invalidates`, or emits an event the resource
    /// listens for with `invalidates_on`. Nothing else reaches an entry: the
    /// materializer drops what listens for a committed event, and no more.
    pub fn invalidates(&self, command: &str, resource: &str) -> bool {
        self.edges.iter().any(|e| {
            e.from == command
                && match e.kind {
                    EdgeKind::Invalidates => e.to == resource,
                    EdgeKind::Emits => self.edges.iter().any(|l| {
                        l.kind == EdgeKind::InvalidatedBy && l.from == resource && l.to == e.to
                    }),
                    _ => false,
                }
        })
    }

    /// Everything an event's arrival regenerates, transitively.
    ///
    /// The gate question in one call: `affected_by("Events.MenuChanged")` must
    /// name store 47's menu fragment and nothing belonging to another store —
    /// which at schema level means every materialization listening for that
    /// event, plus anything reading what those produce.
    pub fn affected_by(&self, event: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut queue: Vec<String> = vec![event.to_string()];
        let mut seen: Vec<String> = vec![event.to_string()];
        while let Some(cur) = queue.pop() {
            for e in &self.edges {
                let hit = match e.kind {
                    // An event points AT what it invalidates: the edge is
                    // written on the materialization, so it runs backwards.
                    EdgeKind::InvalidatedBy => e.to == cur,
                    EdgeKind::Reads => e.to == cur,
                    _ => false,
                };
                if !hit {
                    continue;
                }
                if seen.contains(&e.from) {
                    continue;
                }
                seen.push(e.from.clone());
                out.push(e.from.clone());
                queue.push(e.from.clone());
            }
        }
        out.sort();
        out
    }

    /// What a materialization's cache key separates, and what it does not.
    ///
    /// Charter §14 M6 task 9 — cache-key auditing. Reported from `pw explain`,
    /// as advice rather than as a diagnostic, because the compiler cannot
    /// always tell a deliberate omission from a mistake: two readers sharing an
    /// entry is sometimes exactly what a cache is for.
    ///
    /// The one case that is never deliberate — the build that produced the
    /// artifact — is not here at all. `PW5102` used to ask the author for it and
    /// was retired on the architect's ruling of 2026-08-06: injecting a
    /// compatibility generation is platform mechanism, not application policy.
    /// So this concerns the **application-semantic** dimensions the compiler
    /// cannot safely supply on anyone's behalf.
    pub fn key_audit(&self, path: &str) -> Option<KeyAudit> {
        let node = self.node(path)?;
        let NodeKind::Materialization {
            varies_by,
            partition,
            ..
        } = &node.kind
        else {
            return None;
        };

        // A dependency's key component that the fragment does not supply and
        // does not vary by. The fragment then has ONE entry for what the
        // resource has several of, so two readers who would receive different
        // data share a document.
        let mut gaps = Vec::new();
        for e in &self.edges {
            if e.from != path || e.kind != EdgeKind::Reads {
                continue;
            }
            let Some(read) = self.node(&e.to) else {
                continue;
            };
            let (dep, params) = (&read.name, &read.params);
            let key: Vec<String> = match &read.kind {
                NodeKind::Resource { key, .. } => key.clone(),
                // A materialization read (ADR-0255, ruling 10): its entries are
                // separated by its parameters and by what it varies by.
                NodeKind::Materialization { varies_by: by, .. } => params
                    .iter()
                    .cloned()
                    .chain(by.iter().map(|d| d.keyword().to_string()))
                    .collect(),
                _ => continue,
            };
            for component in &key {
                // Supplied positionally: `Menu(id)` gives the resource's first
                // parameter, so a key component naming that parameter is
                // covered by the fragment's own logical key.
                let supplied = params
                    .iter()
                    .position(|p| p == component)
                    .is_some_and(|i| i < e.key.len());
                let varied = varies_by.iter().any(|d| d.keyword() == component);
                if !supplied && !varied {
                    gaps.push(KeyGap {
                        resource: dep.clone(),
                        component: component.clone(),
                        why: format!(
                            "`{dep}` separates its entries by `{component}`, and \
                             `{}` neither passes it nor includes it in its key, so \
                             one document serves every value of it",
                            node.name
                        ),
                    });
                }
            }
        }
        gaps.sort_by(|a, b| (&a.resource, &a.component).cmp(&(&b.resource, &b.component)));
        gaps.dedup();

        Some(KeyAudit {
            fragment: node.name.clone(),
            logical_key: node.params.clone(),
            partition: partition.clone(),
            application: Dimension::ALL
                .into_iter()
                .filter(|d| *d != Dimension::PrivacyPartition)
                .map(|d| (d, varies_by.contains(&d)))
                .collect(),
            gaps,
        })
    }
}

/// A resource entry's semantic identity, as the compiler knows it.
///
/// Architect ruling, 2026-08-07:
///
/// > Do not let E7-P manually construct `EntryIdentity` from its own
/// > interpretation of graph nodes. There should be one bridge.
///
/// This is that bridge's output. It mirrors `pw_resource::EntryIdentity` by
/// FIELD NAME across ADR-0018's boundary — the compiler does not link a runtime
/// — so a rename on either side is a contract test failing rather than an
/// empty identity at run time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryIdentitySpec {
    pub resource: String,
    pub logical_key: Vec<String>,
    pub partition: String,
    pub compatibility: Option<String>,
}

/// What one materialization's key separates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAudit {
    pub fragment: String,
    /// The application's own key: the declaration's parameters.
    pub logical_key: Vec<String>,
    /// `public` or `private`, which is the privacy partition.
    pub partition: Option<String>,
    /// The dimensions an application decides, and whether this key includes
    /// each. The compatibility generation is not among them: it is injected.
    pub application: Vec<(Dimension, bool)>,
    pub gaps: Vec<KeyGap>,
}

/// A dependency that separates its entries by something this key does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyGap {
    pub resource: String,
    pub component: String,
    pub why: String,
}

/// The node a declaration is: `Module.Name`, as [`Graph::build`] names it.
pub(crate) fn path_of(hir: &Hir, id: crate::hir::DeclId) -> String {
    crate::contract::component_id(hir, id)
}

fn node_kind(decl: &Decl) -> Option<NodeKind> {
    let value = |k: &str| decl.policy(k).map(|p| p.value.trim().to_string());
    Some(match decl.kind {
        DeclKind::Query | DeclKind::Subscription | DeclKind::Resource => NodeKind::Resource {
            partition: value("cache").or_else(|| value("partition")),
            privacy: decl.visibility.clone(),
            key: value("key")
                .map(|v| {
                    v.split(',')
                        .map(|k| k.trim().to_string())
                        .filter(|k| !k.is_empty() && k != "()")
                        .collect()
                })
                .unwrap_or_default(),
        },
        DeclKind::Materialize => NodeKind::Materialization {
            placement: value("placement"),
            partition: value("partition"),
            regenerate: value("regenerate"),
            stampede: value("stampede"),
            fallback: value("fallback"),
            varies_by: varies_by(decl),
        },
        DeclKind::Event => NodeKind::Event,
        DeclKind::Command => NodeKind::Command,
        DeclKind::Page => NodeKind::Page,
        _ => return None,
    })
}

/// **What a name an edge is written with resolves to, from `unit`**: a
/// query, a materialization, an event, a command or a page, never a type;
/// local before imported (`Graph::build`).
fn resolve_from(
    ws: &crate::resolve::Workspace,
    by_def: &std::collections::BTreeMap<crate::resolve::DefId, String>,
    unit: usize,
    name: &str,
) -> Option<String> {
    use crate::resolve::{Namespace, Resolution};
    const USABLE: [Namespace; 3] = [Namespace::Term, Namespace::Ui, Namespace::Event];
    let mut imported = None;
    for ns in USABLE {
        match ws.resolve_in(unit, ns, name) {
            Resolution::Local(d) => return by_def.get(&d).cloned(),
            Resolution::Imported { def, .. } if imported.is_none() => {
                imported = by_def.get(&def).cloned();
            }
            // Ambiguous resolves to NOTHING, not to one of the
            // candidates. Two imports offering one name means
            // the program does not say which, and picking
            // either decides an invalidation boundary by
            // accident.
            _ => {}
        }
    }
    imported
}

/// The dimensions a materialization declares `included_in_key`.
///
/// `partition public` counts as including the privacy partition: naming the
/// partition IS varying by it. The others need the explicit
/// `<dimension> included_in_key`, because a policy that merely mentions a
/// locale is not the same as one that keys on it.
///
/// `code_version included_in_key` is still accepted and now means nothing: the
/// compatibility generation is injected whether it is written or not. Accepted
/// rather than rejected because a program that says something true about the
/// platform is not an error, and because rejecting it would break every file
/// written before the ruling.
fn varies_by(decl: &Decl) -> Vec<Dimension> {
    let mut out: Vec<Dimension> = Dimension::ALL
        .into_iter()
        .filter(|d| {
            decl.policy(d.keyword())
                .is_some_and(|p| p.value.trim() == "included_in_key")
        })
        .collect();
    if decl
        .policy("partition")
        .is_some_and(|p| !p.value.trim().is_empty())
        && !out.contains(&Dimension::PrivacyPartition)
    {
        out.push(Dimension::PrivacyPartition);
    }
    out.sort();
    out
}

/// One key argument as a reader would write it.
///
/// `path_of` answers "which declaration does this name" and returns nothing for
/// a call, so `query Cart(current_session())` labelled its edge with an empty
/// key — indistinguishable from `query Cart()`, which is an edge to every cart
/// rather than to one session's. The label is a description of the key, so it
/// renders the shape rather than resolving it.
fn key_text(body: &crate::hir::Body, id: crate::hir::ExprId) -> String {
    use crate::hir::Expr;
    match body.expr(id) {
        Expr::Name(n) => n.clone(),
        Expr::Field { base, name } => format!("{}.{name}", key_text(body, *base)),
        Expr::Call { callee, args } => {
            let inner: Vec<String> = args.iter().map(|a| key_text(body, a.value)).collect();
            format!("{}({})", key_text(body, *callee), inner.join(", "))
        }
        Expr::Literal(l) => format!("{l:?}"),
        _ => "_".to_string(),
    }
}

/// `let menu = query Menu(id)` in a page body -> `("Menu", ["id"])`.
/// The resources a body READS: `let menu = query Menu(id)`.
///
/// Shared with `contract.rs`, which needs the same list for a page's component
/// dependencies. A second finder would agree until one of them learned about a
/// new form.
pub(crate) fn queried(body: &crate::hir::Body) -> Vec<(String, Vec<String>)> {
    use crate::hir::Expr;
    let mut out = Vec::new();
    for id in body.walk() {
        let Expr::Keyword {
            keyword,
            modifiers,
            args,
            ..
        } = body.expr(id)
        else {
            continue;
        };
        if !matches!(keyword.as_str(), "query" | "subscription") {
            continue;
        }
        let Some(name) = modifiers.first() else {
            continue;
        };
        let key = args.iter().map(|a| key_text(body, *a)).collect();
        out.push((name.clone(), key));
    }
    out
}

// --- rules ---------------------------------------------------------------

use crate::codes;
use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};

/// E6's checks over one unit, against the whole program's graph.
///
/// Whole-program by construction: a fragment in one module depends on a query
/// in another, and every one of these rules is about the pair. The diagnostics
/// are attributed to the unit that *wrote the clause*, because that is the file
/// whose author can fix it.
///
/// Spans come from the declaration rather than from the graph. The graph is a
/// serialized artifact spanning many files, so a byte offset in it would be a
/// number with no file attached — and a diagnostic that points confidently at
/// the wrong place is worse than one that points at the clause.
pub fn check(
    hir: &Hir,
    g: &Graph,
    // Each resource's value label, by path, where it names a reader: what its
    // body reads, through what it calls (ADR-0118).
    resources: &std::collections::BTreeMap<String, crate::privacy::Label>,
    out: &mut Vec<Diagnostic>,
) {
    for (id, decl) in hir.all_decls() {
        if node_kind(decl).is_none() {
            continue;
        }
        let path = path_of(hir, id);
        let at = hir.decl_span(id);

        // 1. A clause that names nothing.
        //
        // The E6 failure that looks like success: a fragment listening for an
        // event nobody declares simply never regenerates. The page is not
        // wrong, only permanently stale, and no test of the page finds it —
        // the content is valid, it is just the content from before.
        for (policy, kind, what) in [
            ("depends_on", EdgeKind::Reads, "depends on"),
            (
                "invalidates_on",
                EdgeKind::InvalidatedBy,
                "is invalidated by",
            ),
            ("emits", EdgeKind::Emits, "emits"),
            ("invalidates", EdgeKind::Invalidates, "invalidates"),
        ] {
            let Some(p) = decl.policy(policy) else {
                continue;
            };
            for d in &g.dangling {
                if d.from != path || d.kind != kind {
                    continue;
                }
                // The key that names nothing, where it is written (ADR-0240).
                let Some(key) = p.keys.iter().find(|k| k.name == d.name) else {
                    continue;
                };
                out.push(Diagnostic {
                    code: codes::GRAPH_EDGE_UNRESOLVED.id,
                    invariant: codes::GRAPH_EDGE_UNRESOLVED.invariant,
                    reason: "graph_target_undeclared",
                    detector: Detector::ResourceGraph,
                    severity: Severity::Error,
                    message: format!(
                        "`{}` {what} `{}`, which nothing declares",
                        decl.name, d.name
                    ),
                    primary_span: key.name_span.clone(),
                    related: vec![Related::new(
                        at.clone(),
                        format!("`{}` is the node with the edge", decl.name),
                    )],
                    explanation: Some(format!(
                        "Invalidation is driven by this graph, so an edge to nothing is not \
                         an error at run time — it is silence. `{}` would never regenerate on \
                         `{}`, and the page it produces would stay valid, correct-looking and \
                         permanently out of date. Nothing downstream can distinguish that from \
                         a fragment whose inputs never changed.",
                        decl.name, d.name
                    )),
                    repairs: vec![Repair {
                        description: format!(
                            "declare `{}`, or import the module that does",
                            d.name
                        ),
                        replacement: None,
                    }],
                });
            }
        }

        // 1b. A clause that names a declaration of another kind (ADR-0088).
        //
        // `invalidates CartChanged(..)` resolved to the event, and `emits
        // Cart(..)` to the resource, and each was an edge like any other.
        for (policy, kind, what, wants_resource) in [
            ("depends_on", EdgeKind::Reads, "depends on", true),
            (
                "invalidates_on",
                EdgeKind::InvalidatedBy,
                "is invalidated by",
                false,
            ),
            ("emits", EdgeKind::Emits, "emits", false),
            ("invalidates", EdgeKind::Invalidates, "invalidates", true),
        ] {
            let Some(p) = decl.policy(policy) else {
                continue;
            };
            let wanted = match (wants_resource, policy) {
                (true, "depends_on") => "a resource or a materialization",
                (true, _) => "a resource",
                _ => "an event",
            };
            for e in g.edges.iter().filter(|e| e.from == path && e.kind == kind) {
                // A name that is a declaration and no node of the graph: a
                // function, or a view. Until ADR-0255 the clause was taken as
                // it was, and `depends_on someFn(id)` checked.
                let Some(node) = g.node(&e.to) else {
                    let name = &e.to;
                    out.push(Diagnostic {
                        code: codes::CLAUSE_NAMES_ANOTHER_KIND.id,
                        invariant: codes::CLAUSE_NAMES_ANOTHER_KIND.invariant,
                        reason: "clause_names_no_node",
                        detector: Detector::ResourceGraph,
                        severity: Severity::Error,
                        message: format!("`{}` {what} `{name}`, which is not {wanted}", decl.name),
                        primary_span: p.span.clone(),
                        related: vec![Related::new(
                            at.clone(),
                            format!("`{}` is the node with the edge", decl.name),
                        )],
                        explanation: Some(format!(
                            "`{policy}` names {wanted}: a node of the dependency graph, \
                             which invalidation runs through. `{name}` is a declaration of \
                             another kind, and an edge to it would carry nothing."
                        )),
                        repairs: vec![Repair {
                            description: format!("name {wanted} here"),
                            replacement: None,
                        }],
                    });
                    continue;
                };
                let is = match &node.kind {
                    NodeKind::Resource { .. } if wants_resource => continue,
                    // A materialization may read another (ADR-0255, ruling 10).
                    NodeKind::Materialization { .. } if policy == "depends_on" => continue,
                    NodeKind::Event if !wants_resource => continue,
                    NodeKind::Resource { .. } => "a resource",
                    NodeKind::Event => "an event",
                    NodeKind::Materialization { .. } => "a materialization",
                    NodeKind::Command => "a command",
                    NodeKind::Page => "a page",
                };
                out.push(Diagnostic {
                    code: codes::CLAUSE_NAMES_ANOTHER_KIND.id,
                    invariant: codes::CLAUSE_NAMES_ANOTHER_KIND.invariant,
                    reason: "clause_names_another_kind",
                    detector: Detector::ResourceGraph,
                    severity: Severity::Error,
                    message: format!(
                        "`{}` {what} `{}`, which is {is}, not {wanted}",
                        decl.name, node.name
                    ),
                    primary_span: p.span.clone(),
                    related: vec![Related::new(
                        at.clone(),
                        format!("`{}` is the node with the edge", decl.name),
                    )],
                    // Until 2026-09-26 the graph looked a name up wherever a name
                    // might be, and drew the edge to whatever it found.
                    explanation: Some(format!(
                        "`{policy}` names {wanted}. A command invalidates a resource's \
                         entries and emits events; a materialization depends on resources \
                         and materializations, and listens for events."
                    )),
                    repairs: vec![Repair {
                        description: format!("name {wanted} here"),
                        replacement: None,
                    }],
                });
            }
        }

        // 1c. A materialization that depends on itself (ADR-0255, ADR-0195's
        // ruling 10, PW5109). Reported once a cycle, by its first member.
        if matches!(
            g.node(&path).map(|n| &n.kind),
            Some(NodeKind::Materialization { .. })
        ) && let Some(cycle) = cycle_through(g, &path)
            && cycle.iter().all(|m| m.as_str() >= path.as_str())
        {
            let named =
                |m: &String| format!("`{}`", g.node(m).map_or(m.as_str(), |n| n.name.as_str()));
            let through: Vec<String> = cycle[1..].iter().map(named).collect();
            let message = match through.as_slice() {
                [] => format!("`{}` depends on itself", decl.name),
                [one] => format!("`{}` depends on itself, through {one}", decl.name),
                [rest @ .., last] => format!(
                    "`{}` depends on itself, through {} and {last}",
                    decl.name,
                    rest.join(", ")
                ),
            };
            out.push(Diagnostic {
                code: codes::MATERIALIZATION_CYCLE.id,
                invariant: codes::MATERIALIZATION_CYCLE.invariant,
                reason: "materialization_depends_on_itself",
                detector: Detector::ResourceGraph,
                severity: Severity::Error,
                message,
                primary_span: decl
                    .policy("depends_on")
                    .map(|p| p.span.clone())
                    .unwrap_or_else(|| at.clone()),
                related: vec![Related::new(
                    at.clone(),
                    format!("`{}` is rebuilt from what it reads", decl.name),
                )],
                explanation: Some(
                    "A materialization is rebuilt from what it reads, once what it reads \
                     is rebuilt (ADR-0195's ruling 10). Around a cycle nothing is first: \
                     each would wait for the other, or be rebuilt from the other's stale \
                     entry."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "let one of them read the resources the other reads, and \
                                  not the other"
                        .to_string(),
                    replacement: None,
                }],
            });
        }

        // 2 and 3 are about materializations whose entry is SHARED.
        let Some(Node {
            kind:
                NodeKind::Materialization {
                    partition: Some(partition),
                    ..
                },
            ..
        }) = g.node(&path)
        else {
            continue;
        };
        if partition.trim() != "public" {
            continue;
        }

        // 2. Charter §14 M6 gate item 4, at the graph rather than the cache.
        //
        // `PW5001` asks whether a QUERY's own partition admits its data. This
        // asks whether a FRAGMENT pulls private data into an entry that one
        // reader's request fills and every other reader is served from. The
        // two are independent: each resource here can be perfectly configured
        // and the fragment still wrong.
        for e in &g.edges {
            if e.from != path || e.kind != EdgeKind::Reads {
                continue;
            }
            // A materialization it reads (ADR-0255, ruling 10): shared only
            // where it is `partition public`, which holds of what that one
            // reads in turn, since the build refuses a cycle.
            if let Some(Node {
                kind: NodeKind::Materialization { partition: dp, .. },
                name: dep,
                ..
            }) = g.node(&e.to)
                && dp.as_deref().map(str::trim) != Some("public")
            {
                out.push(shared_reads_restricted(
                    decl,
                    dep,
                    "is not materialized `partition public`",
                    &at,
                ));
                continue;
            }
            let Some(Node {
                kind:
                    NodeKind::Resource {
                        partition: rp,
                        privacy,
                        ..
                    },
                name: dep,
                ..
            }) = g.node(&e.to)
            else {
                continue;
            };
            let restricted = privacy.as_deref().is_some_and(|v| v != "public")
                || rp.as_deref().is_some_and(|v| v.starts_with("private"));
            // Or it reads a reader's value, through whatever it calls
            // (ADR-0118). A query declared `public` that read the session made
            // one reader's value, and its edge looked public until 2026-09-26.
            let read = resources.get(&e.to);
            let why = match (restricted, read) {
                (false, None) => continue,
                (false, Some(label)) => format!("reads {label}"),
                (true, _) => privacy
                    .as_deref()
                    .filter(|v| *v != "public")
                    .map(|v| format!("is declared `{v}`"))
                    .unwrap_or_else(|| "is cached `private`".to_string()),
            };
            out.push(shared_reads_restricted(decl, dep, &why, &at));
        }
    }
}

/// **A shared materialization depends on `dep`, which `why`** (PW5101).
fn shared_reads_restricted(decl: &Decl, dep: &str, why: &str, at: &Span) -> Diagnostic {
    Diagnostic {
        code: codes::PRIVATE_IN_SHARED_MATERIALIZATION.id,
        invariant: codes::PRIVATE_IN_SHARED_MATERIALIZATION.invariant,
        reason: "restricted_dependency_of_shared_fragment",
        detector: Detector::ResourceGraph,
        severity: Severity::Error,
        message: format!(
            "`{}` is materialized into a shared entry and depends on `{dep}`, which {why}",
            decl.name
        ),
        primary_span: decl
            .policy("depends_on")
            .map(|p| p.span.clone())
            .unwrap_or_else(|| at.clone()),
        related: vec![Related::new(
            at.clone(),
            "`partition public` makes one entry serve every reader".to_string(),
        )],
        explanation: Some(format!(
            "A shared materialization is computed for whoever asks first and served \
             to everyone after. `{dep}` {why}, so the first reader's data would be \
             written into storage the rest of them read. Charter §14 M6 gate item 4 \
             states this as a property of the STORAGE; it is decided here, where the \
             dependency is written."
        )),
        repairs: vec![Repair {
            description: format!(
                "materialize `{}` with `partition private`, or drop the dependency on `{dep}` and read it per reader",
                decl.name
            ),
            replacement: None,
        }],
    }
}

/// **A materialization that depends on itself** (ADR-0255, ADR-0195's ruling
/// 10, PW5109): the cycle through `path`, as the paths of its members from
/// `path` back to it, where the reads from `path` reach `path` again.
fn cycle_through(g: &Graph, path: &str) -> Option<Vec<String>> {
    let reads = |from: &str| -> Vec<String> {
        g.edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Reads && e.from == from)
            .filter(|e| {
                g.node(&e.to)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Materialization { .. }))
            })
            .map(|e| e.to.clone())
            .collect()
    };
    // Breadth first, each node once, keeping how it was reached: the
    // shortest way back is the cycle a reader can follow.
    let mut came_from: BTreeMap<String, String> = BTreeMap::new();
    let mut queue: std::collections::VecDeque<String> = reads(path).into_iter().collect();
    for next in &queue {
        came_from.insert(next.clone(), path.to_string());
    }
    while let Some(cur) = queue.pop_front() {
        if cur == path {
            let mut cycle = vec![path.to_string()];
            let mut at = came_from.get(path).cloned();
            while let Some(p) = at {
                if p == path {
                    break;
                }
                cycle.push(p.clone());
                at = came_from.get(&p).cloned();
            }
            cycle[1..].reverse();
            return Some(cycle);
        }
        for next in reads(&cur) {
            if !came_from.contains_key(&next) {
                came_from.insert(next.clone(), cur.clone());
                queue.push_back(next);
            }
        }
    }
    None
}
