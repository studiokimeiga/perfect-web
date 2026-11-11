# ADR-0429: `pw build --dist` builds a whole app

Status: accepted 2026-11-11, to land with train-24. Proposed 2026-11-11, on
branch `dx/train-24-20261111`, under the owner's delegation of 2026-10-02 and
order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that every row of the head-to-head Svelte wins is a defect
until it is fixed or recorded. Milestone: E14. Builds on ADR-0149, ADR-0166
and ADR-0399.

## Context

An app's build was a shell script each app wrote again: `rows.sh`,
`todomvc.sh`, `guestbook.sh`, `feed.sh`. Each listed the standard packages
beside the app's sources, ran `pw check` and then `pw build --out
DIR/build`, copied the runtime the host serves it with, wrote what it was
built from as each source's digest with a few lines of Node
(`sources.json`, ADR-0166), and called the packaging for a production build
(`optimize-assets.mjs`, ADR-0399). Twenty-nine lines for the large list, its
build and configuration row of the head-to-head, where React's is 24 and
Svelte's 27; forty-three for TodoMVC, where Svelte's is 44.

None of it was the app's: every script did the same steps, and a developer
starting an app would have copied one. `pw diff` already finds the standard
packages beside the compiler (ADR-0149's `program_in`).

## Decision

`pw build --dist DIR <app>...` builds a whole app:

1. **Its sources**: the standard packages (`pw-std`, `pw-platform-web`),
   which every app is built with, then each path given, a directory as each
   `.pw` file under it. A file named twice, as an app that still names a
   package's files does, is one source. Each is named absolutely.
2. **Checked as `pw check` checks them**, through the same pass, its
   diagnostics printed as it prints them (`--plain` too); a program in error
   builds nothing.
3. **Written**: its artifacts in `DIR/build`, as `pw build --out DIR/build`
   writes them; the runtime its host serves it with, `DIR/pw-runtime.mjs`;
   each `--asset FILE=NAME` as `DIR/NAME`, a file name of the build's own and
   none it writes; with `--production`, packed as a production build is; and
   last `DIR/sources.json`, each source by its digest, so a build stopped
   part-way names no sources and the browser suite serves no such build.
4. **Only a build is replaced**: `DIR` is new, empty, or an app's build (its
   `build/templates.json` and its runtime). A directory holding anything else
   is refused, whatever it holds, as one with a `build/` of its own.
5. `--production` and `--asset` are a whole app's alone; `--dist` and `--out`
   are not asked together.

Each app's script is one command now: the large list's five lines, TodoMVC's
ten, the guestbook's and the feed's four. Each builds, byte for byte, what
its script built before, production builds too.

## Why not the other ways

- **The packages found by an import's name** (a module path resolved from
  `import List`): what a language's package manager does, and the larger
  answer for packages beyond the platform's. The platform's two are every
  web app's, so they are always added; resolving others is a design of its
  own.
- **The runtime and the packaging inside the binary** (`include_str!`, and
  a minifier in Rust): `pw` would be one file a developer could carry. The
  packaging is esbuild's today, run by Node, and `pw` is built in the
  repository; found beside the compiler as the packages are, until `pw` is
  shipped alone.
- **Adding the packages only where an app names none of them**: it read as
  more careful, and no test could tell it from adding them always, since a
  file named twice is one source; dropped for the plainer rule.
- **Keeping `pw check` a step of its own in each script**: a build checks
  anyway, and printing what the check finds is the build's to do.

## Consequences

- `rows.sh`, `todomvc.sh`, `guestbook.sh` and `feed.sh` call `pw build
  --dist`; the tutorial builds the guestbook so (`docs/guide/tutorial.md`).
- Tests: `compiler/pw-cli/tests/whole_app.rs`, eight, run against the
  binary. Controls: fourteen in `scripts/whole_app_mutations.py`, each
  killed by a failing test; `just e14-whole-app` records both.
- The head-to-head's build and configuration rows: the large list's 5 lines,
  where React's are 24 and Svelte's 27; TodoMVC's 10, where Svelte's are 44.
