# ADR-0404: an image says what it shows

Status: accepted 2026-11-07, landed with train-14 under the owner's delegation
of 2026-10-02. Proposed 2026-11-06, on branch `fix/img-alt-20261106` (a draft
PR against `master`). Asked for by the owner on 2026-11-06: "an `<img>`
without `alt` should be refused".

## Context

- **What the charter asks.** Its compile-time checks for semantic HTML
  include "missing labels" (§8.2), and "labels" is among its minimum
  accessibility checks (§17.4). PW5014 asks a form control for its name
  (ADR-0143). Nothing asked an image for its text.
- **What the guide showed.** Since ADR-0343 the guide's examples compile.
  Its one `gap=check`, an example the check passes and should refuse, was
  `<img src="/logo.png">` with no `alt`. `KNOWN_LIMITATIONS.md` lists the
  same: "An `<img>` with no `alt` checks clean".
- **What HTML requires** (the HTML standard, 4.8.4.4):
  - "Except where otherwise specified, the alt attribute must be specified
    and its value must not be empty".
  - For a purely decorative image, "the alt attribute must be present but
    its value must be the empty string".
  - A conformance checker may let an `<img>` go without `alt` in four
    cases: in a `<figure>` whose caption stands for it, with a non-empty
    `title`, in a document meant for one person known to see images, or
    with the non-conforming `generator-unable-to-provide-required-alt`.
- **What WCAG fails** (2.2, failure F65): an `img`, `area` or
  `input type="image"` with none of `alt`, `aria-labelledby`,
  `aria-label` or `title`. The last three count only where they are
  "accessibility supported"; `alt` is "the preferred way".
- **The repository.** Every `<img>` in its programs has an `alt`: the
  feed's, the origin fixtures' logos, the renderer's test page. Only the
  guide's gap example has none.

## Decision

1. **PW5053, `image_without_alt`** (Markup). An `<img>` with no `alt`
   attribute is refused wherever a declaration's markup writes it, a block
   or a loop included. Any `alt` stands for the image:
   - text;
   - `alt=""`, or the attribute alone, which HTML reads as `""`;
   - a value, `alt={photo.caption}`.
2. **Only `alt` counts.** A `title`, an `aria-label`, an
   `aria-labelledby` or a `role` does not stand in for it:
   - HTML requires `alt` beside them;
   - none of them is shown where the image does not load, as `alt` is;
   - a `title` is shown only to a pointer that rests on the image.

   A decorative image's repair is one attribute, `alt=""`.
3. **Only `<img>` is held to it.** A `<picture>`'s `<source>` offers
   another file for the `<img>` it holds, and that `<img>` has the `alt`.
4. **Where it is reported.** At the element, beside the declaration that
   renders it, as PW5014 is. The help names both repairs: `alt="…"`
   saying what the image shows, or `alt=""` if it shows nothing a reader
   needs.
5. **The corpus opens C31** (`docs/CORPUS.md`) with R-096, a page whose
   photo has no `alt`. `examples/generality/image_without_alt/` holds:
   - `caught.pw` (GENERAL): a view's loop of images from values, each with
     a `title` alone;
   - `neighbour.pw`: the same loop, each `alt` a value.

   The invariant is Tier 3 in `PROFILES.md`: a rule over one shape, whose
   failure is local.
6. **The guide follows.**
   - The tour's gap example is gone.
   - The tour's first section names the rule.
   - `errors.md` shows the refusal as `pw check` prints it.
   - `why.md` no longer says it is not refused.

## Alternatives

- **WCAG F65's set**: take an `aria-label`, `aria-labelledby` or `title`
  in place of `alt`. Rejected:
  - HTML requires `alt` regardless;
  - none of them is shown where the image fails to load;
  - F65 counts them only where "accessibility supported", which a compiler
    cannot know.
- **HTML's exceptions for conformance checkers**: a `<figure>` with a
  `<figcaption>`, or a `title`. They are for an image whose content its
  author cannot know, such as a photo a reader uploads. Rejected for now:
  - a Pleris page can say what it knows with a value,
    `alt={upload.caption}`, or `alt=""` beside a caption that says it;
  - the figure's case needs its caption to say something in every
    rendering, which the compiler would have to prove, for a case a
    one-attribute repair covers.
- **Refuse an empty `alt`, or require text.** Rejected: HTML requires
  `alt=""` of a decorative image, and what a value holds is not known at
  compile time.
- **Also `<input type="image">` and `<area>`**, which F65 names. Not here:
  - no program in the repository writes either;
  - `<input type="image">` is PW5014's today, and PW5014 refuses one with
    an `alt` for having no `<label>`, as it does `<input type="reset">`.
    An image button is named by its `alt`, and a reset button has a name of
    its own. That defect is the next change, on its own.

## Consequences

- A program with an `<img>` and no `alt` is refused by `pw check`, and by
  `pw build`, which runs the same checks first.
- No program in the repository changes: each `<img>` it writes has an
  `alt`.
- `KNOWN_LIMITATIONS.md`'s "An `<img>` with no `alt` checks clean"
  closes once this lands. That file is the integrator's.
- pleris.dev's `/e/PW5053` page appears once the site is built from a
  registry that has the code.

## Evidence

`docs/evidence/E14/img-alt-2026-11-06/`:
- the compiler's tests, among them `images.rs`;
- the guide's test;
- the corpus's formatting;
- the registry's generated page and its check;
- `just preflight` and `just evidence-gates`;
- `images_mutations.py`, whole, and every mutation script the change
  reaches.
