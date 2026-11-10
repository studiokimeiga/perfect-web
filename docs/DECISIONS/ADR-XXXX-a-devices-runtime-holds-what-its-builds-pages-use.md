# ADR-XXXX: a device's runtime holds what its build's pages use

Status: proposed 2026-11-09, on branch `perf/after-train-20261109`, under the
owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"), and the order of 2026-11-09 to work the measured
gaps with Svelte largest first. Milestone: E14. Amends ADR-0413,
which ruled out a runtime made from what an app's pages use.

## Context

The head-to-head at `3b0e7c4f` has TodoMVC sending 20,684 bytes compressed
against Svelte's 16,992, of which 16,506 is script. The device's runtime is
14.5 KB of that. It still holds everything any page without a server might use:
links with values in their addresses, `{#match}` arms, views, dialogs. TodoMVC
uses none of them, and the compiler knows it.

The device runtime's ADR refused "a runtime per app, from what its pages use"
because a build would have as many runtimes as it has combinations. That was
wrong: a build has one set of pages, so one union of what they use, and so one
device runtime. Its reason holds against a runtime per page, not per build.

## Decision

1. **The packaging reads what a build's device pages use** (`deviceFeatures`)
   from the same templates and plans the host reads: a value in a URL or a
   style, a `{#match}`, a view, a `<dialog>`. Inside loops, conditions, views
   and match arms too.
2. **What none of them uses is replaced by a flag and falls away**, as a
   server's code does: `PW_LINKS`, `PW_MATCH`, `PW_VIEWS`, `PW_DIALOGS`. The
   runtime is written once, and each feature's code is guarded at its top.
3. **A build it cannot read keeps everything.** A page whose template is not
   found, or a build with no templates, gets the whole device runtime: this is
   a size, and it is never a reason to run without a feature a page needs.
4. **Only pages that start the device's runtime count**: a page a reader decides
   or with a layout starts the full runtime and asks nothing of this one.
5. **The rest of a server's code leaves too.** The list comparison the renderer
   needs only when frames arrive, and a handler's `navigate`, are guarded by
   `PW_SERVER`.
6. **A page's head fetches its module beside its runtime.** Its document names
   `/bundle/<page>.mjs` with `modulepreload`, as it names the runtime, so the
   module's request starts with the document's parsing, not after the runtime
   has run.

## Why not the other ways

- **A runtime per page**, or code generated per page. A page's runtime would be
  cached alone and each navigation would fetch another. Per page may still pay
  for a site of few, large pages. It is a separate experiment.
- **Dynamic imports of each feature.** Each would be a request a page pays for
  on the way to its first press, which is what ADR-0412 removed.
- **The flag per feature set by hand in each app.** The compiler already knows,
  and a person would be wrong when an app changes.

## Result

The device's runtime is 12,366 bytes as sent for TodoMVC, from 14.5 KB. The tests
in `optimize-assets.test.mjs` hold each flag against a real build of each shape
and each word a feature leaves. `e2e/device-runtime.spec.mjs` holds the preload.
`scripts/device_runtime_mutations.py` removes each piece. The head-to-head's
figures go here when it has run at this branch's commit.

## Consequences

- A build's device runtime differs when its pages' features do. It is one file
  and it is versioned as every other is.
- A feature added to the runtime that a page can do without has to be guarded
  and read from the templates, or it ships to every page. The packaging's tests
  name each word that must fall away.
- Reading a build wrong is a broken page where reading it not at all is a larger
  one: which is why each shape a part can hide in has its own test and mutant.
