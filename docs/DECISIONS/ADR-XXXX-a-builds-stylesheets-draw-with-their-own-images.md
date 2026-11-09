# ADR-XXXX: a build's stylesheets draw with their own images

Status: proposed 2026-11-09, on branch `perf/inline-styles-20261109`, under the
owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"), and the order of 2026-11-09 to work the measured
gaps with Svelte largest first. Milestone: E14. Amends ADR-0333 and undoes
ADR-0367's amendment of 2026-11-06.

## Context

TodoMVC's official stylesheet draws a todo's checkbox as two `data:` SVG images.
A generated policy allows images of the page's own origin (`img-src 'self'`,
ADR-0333), so the sheet could not be used as it ships. ADR-0367 first served the
two drawings as files, then wrote them in CSS: 215 lines of `todomvc-styles.mjs`
that read the official sheet and rebuild its drawings as gradients, a third
stylesheet in every TodoMVC page, 150 lines of tests holding that drawing to the
official one pixel for pixel, and seven mutation controls of it. The head-to-head
counts those 215 lines against Pleris: 183 of "build and configuration" against
Svelte's 44.

ADR-0333's rule is that what a page may do is generated from what its build
needs. A `data:` image in a stylesheet the build ships is something its build
needs, and the policy simply did not read stylesheets.

## Decision

A build whose own stylesheets draw with a `data:` image gets `img-src 'self'
data:`. Any other build keeps `img-src 'self'`. It is read from the build's
files, when the host starts, as a `url(data:image/...)` in a `.css` beside its
documents; a mention of the words elsewhere in a sheet is not one.

TodoMVC uses the official sheet as it ships, and `todomvc-styles.mjs`,
`todomvc-pleris.css` and their tests are removed.

## Why not the other ways

- **Allow `data:` for every build.** A page that draws nothing that way would be
  allowed what it does not need, against ADR-0333.
- **Serve the drawings as files again.** Two more requests, each after the frame
  that needs it: why ADR-0367 stopped doing that.

## Why this is safe

An image cannot run a script, and `data:` has no address to send anything to.
`script-src` is untouched. A value a reader controls cannot reach this: the
policy reads the build's own files when the host starts.

## Result

To be recorded from `just e14-head-to-head` before this is accepted.

## Consequences

- TodoMVC's checkbox is exactly the official drawing, because it is the official
  drawing.
- A stylesheet with a `data:` image widens the policy of every page of its build,
  which is also what an image of the page's own origin does.
