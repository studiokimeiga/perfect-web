# ADR-XXXX: an input button names itself

Status: proposed, 2026-11-06, on branch `claude/competent-dirac-a03323`.
Not landed; not numbered (PARALLEL.md: the integrator numbers ADRs).
Milestone: E14. A correction to PW5014 (`control_without_label`, ADR-0143),
found on 2026-11-06 while adding PW5053, the `<img>` alt rule. It extends
PW5053 (`image_without_alt`, the image ADR,
`ADR-XXXX-an-image-says-what-it-shows.md` on `fix/img-alt-20261106`), so
the branch merges that one at `7bd477fa` and builds on it.

## Context

- **Two refusals of named buttons.** This page fails `pw check --plain`,
  with PW5014 "`<input>` has no associated label" at each button:

  ```
  <form method="get" action="/find">
      <input type="image" src="/search.png" alt="Search" />
      <input type="reset" />
  </form>
  ```

  HTML names both. An image button is named by its `alt`. A reset button
  has a label of its own: its `value`, or "Reset".
- **The defect beneath them.** PW5014 passed every input whose `type` was
  `hidden`, `submit` or `button`, and held every other input to a
  `<label>` or ARIA. It had no model of a button's own label, so it was
  wrong both ways:
  - it refused `reset` and `image` buttons, which HTML names;
  - it passed buttons nothing names: a `submit` whose `value` is empty,
    and a `button` with no `value` or an empty one. Each shows no text,
    and Chromium gives each no name (below).
- **`type` was read on any control.** So `<select type="hidden">` passed
  as a hidden input would.
- **No program in the repository writes** an `<input>` of type `submit`,
  `reset`, `button` or `image`. Only `labels.rs` wrote two, in one test.

## What the sources say

Read on 2026-11-06.

- **The HTML standard, the button states of `input`**
  (html.spec.whatwg.org, 4.10.5.1.18 to .21):
  - Submit and Reset: "If the element has a value attribute, the button's
    label must be the value of that attribute; otherwise, it must be an
    implementation-defined string that means "Submit"" (or "Reset") "or
    some such."
  - Button: "A label for the button must be provided in the value
    attribute, though it may be the empty string." With no `value`, the
    label "must be the empty string."
  - Image Button: "The alt attribute provides the textual label for the
    button for users and user agents who cannot use the image. The alt
    attribute must be present, and must contain a non-empty string giving
    the label that would be appropriate for an equivalent button if the
    image was unavailable." Where the image is not shown, "the element
    represents a submit button whose label is given by the value of the
    alt attribute".
- **HTML-AAM** (W3C Working Draft, 26 October 2026):
  - 4.1.2, `button`, `submit` and `reset`: `aria-label` or
    `aria-labelledby`, then the associated label, then the `value`. "For
    input type=submit and type=reset: if the accessible name is still
    empty, and the value attribute is unspecified", the name is the
    localized "submit" or "reset". Then `title`, else no name.
  - 4.1.3, `image`: `aria-label` or `aria-labelledby`, then the label,
    then a non-blank `alt`, then `title`, then a localized "submit" or
    "Submit Query".
- **WCAG 2.2**, both updated 12 January 2026:
  - **F65**: "Failure of Success Criterion 1.1.1 due to omitting the alt
    attribute or text alternative on img elements, area elements, and
    input elements of type "image"". The test fails an element with none
    of `alt`, `aria-labelledby`, `aria-label` or `title`, the last three
    where accessibility supported. "The alt attribute continues to be the
    preferred way to provide alternative text for images."
  - **H36**, "Using alt attributes on images used as submit buttons": the
    `alt` "is used to provide a functional label", which "indicates the
    button's function, but does not attempt to describe the image".
- **Chromium 145's accessibility tree** agrees with HTML-AAM on every case
  here (`ax-names.txt` in the evidence):
  - `reset` is "Reset" and `submit` is "Submit";
  - `reset` and `submit` with `value=""`, and `button` with none or an
    empty one, have no name;
  - a `button` its `<label>` wraps is named by the label;
  - an image button is its `alt`, and with none or an empty one
    "Submit".

## Decision

1. **An `<input>` button names itself** (PW5014). `named_controls`,
   ADR-0143's one place for what names a control, reads it:
   - a `submit` or `reset` by its `value`, or, with no `value` attribute,
     by the label HTML gives it;
   - a `button` by its `value` alone: it has no default;
   - a `value` that is blank, or written alone, names nothing. HTML makes
     it the button's label, so the button shows no text, and HTML-AAM
     gives the default only where `value` is not written;
   - a `value` the program computes is taken as a name, as an
     `aria-label` is (ADR-0143).

   What names any control names a button too: its `<label>`, an
   `aria-label`, an `aria-labelledby`. An icon button,
   `<input type="submit" value="" aria-label="Search">`, is named.
2. **An image button's `alt` is PW5053's.** An `<input type="image">` is
   refused when its `alt` is missing, blank or written alone, unless the
   program computes it:
   - "`<input type="image">` has no `alt`";
   - "has a blank `alt`", with a note at the `alt`. HTML-AAM reads the
     `alt` trimmed of white space, so one of spaces names nothing either.

   Unlike an `<img>`'s, the `alt` may not be empty: it is the button's
   label, and a button always does something a reader needs. A `title`,
   an `aria-label`, an `aria-labelledby` or a `<label>` does not stand in
   for it, for the image ADR's reasons: HTML requires the `alt` beside
   them, and none of them is shown where the image does not load.
3. **PW5014 does not report an image button.** Its name is its `alt`,
   which decision 2 holds: each image button is refused by PW5053 or named
   by its `alt`. One defect is reported once (ADR-0185).
4. **`type` is an `<input>`'s alone**, read whatever its case: HTML
   matches its keywords ASCII case-insensitively. On a `<select>` or a
   `<textarea>` it hides nothing and makes no button.
5. **A button is refused with a button's repair.**
   - PW5014's message is "`<input type="button">` has no label", with the
     button's own `type`: HTML calls a button's text its label. A field's
     stays "`<input>` has no associated label".
   - PW5014's reason is `button_without_label`.
   - The note names the blank `value`, or says a `button` input is named by
     its `value` and has none.
   - The first repair is "write `value="…"` saying what the button does".
   - PW5053's reason for an image button is `image_button_without_alt`,
     and its repair "write `alt="…"` saying what the button does".
   - PW5053's registry sentence now names the image button.

## How PW5014 and PW5053 relate

- **Each asks one question.** PW5014 asks of every form control whether
  something names it. PW5053 asks of every image whether its `alt` stands
  for it.
- **The image button is both.** Its name is its `alt`, so PW5053's
  stricter question answers PW5014's too. PW5014 leaves it alone.
- **They agree on what counts.** For an image, the `alt` alone: not an
  `aria-label` or a `title`.
- **They differ on emptiness, by HTML's own rules.** `alt=""` is right for
  a decorative `<img>` and wrong for an image button.
- **The image ADR deferred this.** Its alternatives say: "`<input
  type="image">` is PW5014's today, and PW5014 refuses one with an `alt`
  for having no `<label>`, as it does `<input type="reset">`. ... That
  defect is the next change, on its own." This ADR is that change. Its
  decision 3, "Only `<img>` is held to it", becomes `<img>` and the image
  button.

## Alternatives

- **Add `reset` and `image` to the types PW5014 skips.** Rejected. It
  ends the two refusals and keeps the defect beneath them: a list of types
  rather than what names each. A skipped `reset` with `value=""` would
  pass with no text and no name, as `submit` and `button` did.
- **Hold the image button under PW5014**, as a control named by its
  `alt`. Rejected:
  - an `aria-label` names a control, so `<input type="image"
    aria-label="Search">` would pass, though HTML requires the `alt` and
    nothing shows a label where the image does not load;
  - PW5053 refuses `<img aria-label>` for that reason, so the two rules
    would disagree;
  - PW5014's repairs, a `<label>` or an `aria-label`, are the wrong ones.
- **A code of its own for the image button.** Rejected. One requirement,
  an image's text is its `alt`, one WCAG failure, F65, and one ruling on
  what stands in for it. The symbol reads as the defect:
  `image_without_alt`.
- **WCAG F65's set.** Taking an `aria-label`, `aria-labelledby` or `title`
  in place of the `alt` is rejected, as in the image ADR.
- **Take `title` as a button's name**, HTML-AAM's last step. Rejected, as
  ADR-0143 rejected it for every control.
- **Name a `submit` or `reset` with an empty `value` "Submit" or
  "Reset"**, as Playwright's name computation does. Rejected: HTML makes
  the empty value the button's label, HTML-AAM gives the default only
  where `value` is not written, and Chromium gives such a button no name.

## Acceptance

- `compiler/pw-core/tests/labels.rs`, the input-button tests, each with
  controls, each refusal asserted whole: message and every note. Each of
  the eight new ones failed on the rule before this change;
  `a_hidden_input_is_no_control` keeps the old test's `hidden` half.
- `compiler/pw-core/tests/images.rs`, the image button: eight tests. Six
  failed on the rule before this change. Two are their controls:
  `only_an_input_in_the_image_state_is_held_to_its_alt` and
  `only_an_input_is_an_image_button`.
- Generality witnesses:
  - `control_without_label/buttons.pw`, a neighbour: the form above,
    clean;
  - `control_without_label/blank-submit.pw`: a submit button with
    `value=""`, refused;
  - `image_without_alt/image-button-empty-alt.pw`: an image button with
    `alt=""` beside an `aria-label`, refused;
  - `image_without_alt/image-button-alt.pw`, its neighbour.

  `control_without_label/DIMENSIONS.md` gains their rows. Its row for
  `labelledby-nothing.pw`, a witness ADR-0185 moved to
  `reference_names_nothing/`, is corrected.
- `scripts/labels_mutations.py`, 31 mutants: four re-anchored where
  ADR-0143's code now reads through `names_with` and `input_type`, and 15
  new. `scripts/images_mutations.py`, 24: 14 new.
- A separate reviewer read the change before it was committed. Its
  findings: too loose a test of why a field is refused (each is now
  asserted whole), no test that `type="image"` makes no image button of a
  `<textarea>` (one now, with its mutant), a button's message, and the
  two limits under "Not claimed" on quote marks and a label's text.
- `docs/guide/diagnostics.md`, written again from the registry.

## Not claimed

- **`<area>`.** F65 names it, and HTML requires its `alt` when it has an
  `href`. No program writes one, and it is not held.
- **A `<button>` element with no text.** PW5014 reads `input`, `select`
  and `textarea`. An empty `<button>`, such as an icon button with no
  `aria-label`, is not refused. That is a change of its own.
- **Whether a computed `value` or `alt` is empty**, as ADR-0143 does not
  claim it of a computed `aria-label`.
- **A `type` the program computes.** Such an input is held to a label, as
  a text field is.
- **A value of quote marks alone.** `static_attr` strips every quote mark
  at either end of a value, so `value="'"` reads as blank, as
  `aria-label="'"` did before. A fix belongs to that helper, which every
  attribute rule reads, and is a change of its own.
- **A label's text from a button inside it.** `has_text` reads a
  descendant's text, `aria-label` and `alt`, not a button's `value`. A
  `<label for>` whose only text is a nested button's `value` names
  nothing here, where accname would read the button's name. Unchanged.
- **The browser's half.** ADR-0143's parsed-tree suite asserts names
  through Playwright's `toHaveAccessibleName`. That is Playwright's own
  name computation, one JavaScript implementation run in each engine's
  DOM, not each engine's accessibility tree. On 2026-11-06 it named a
  `reset` and a `submit` with `value=""` "Reset" and "Submit", where
  Chromium's tree has no name, and gave a `button` its `<label>` wraps no
  name, where Chromium's tree has the label's. The render fixture and the
  suite are not changed here. `ax-names.txt` records Chromium's own tree.
  Firefox's and WebKit's own trees are not read.

## Questions for the integrator

- **The image ADR is amended here.** Its decision 3 and PW5053's registry
  sentence change on this branch, which merges `fix/img-alt-20261106` at
  `7bd477fa`. If that branch gains commits, merge it here again; if it is
  rewritten, this branch must be merged with it again. Folding this
  change into it is the other way.
- **For `KNOWN_LIMITATIONS.md`**, which is yours: PW5014's paragraph could
  say that an `<input>` button names itself by its `value` or the label
  HTML gives it. The two gaps above, `<area>` and an empty `<button>`,
  are not listed there yet.
- **The parsed-tree suite's claim**, "in all three engines", rests on one
  implementation. Reading each engine's own tree is a change of its own.
- **Three reached mutation scripts were not run here**: `page_policy`,
  `scheduled` and `guide` (see Evidence). They need a hosted run, or a
  session with an assigned port range.

## Evidence

`docs/evidence/E14/input-buttons-2026-11-06/`, each file from a command
recorded in `commands.txt`, at `5be06de4` unless it says otherwise:
- the page above and the new refusals, by `pw check --plain`: before, at
  `bdbdf6df`, and after;
- `ax-names.txt`, each engine's names for the buttons, with its script and
  page;
- `cargo test -p pw-core`, whole, and `cargo test -p pw-cli --test guide`;
- `just preflight`, `just evidence-gates` and
  `python3 scripts/mutation_anchors.py`;
- each mutation script below, run whole, with no survivor:
  - reached by the code and its tests: `labels` (31 of 31 killed), `ids`
    (13 of 13), whose tests include `labels.rs` and whose anchors sit in
    PW5014's block, and `images` (24 of 24);
  - reached by the generality witnesses alone: `shared_cache` (5 of 5),
    `effect_value` (4 of 4), `condition_truth` (6 of 6) and `affine`
    (7 of 7).

Reached and not run here:
- **`page_policy` and `scheduled`**, which run the generality suite among
  others. Their other suites need a browser suite on a port and
  pw-dev-server's tests against the shared PostgreSQL, and this session has
  no port range. Witnesses added to the generality suite can only add
  failures under a mutant, never remove one, so none of their kills can be
  lost; the four consumers run above agree.
- **`guide`**, reached through `diagnostics.md`'s PW5053 row, written again
  from the registry, which the guide's test holds equal to it. Its mutants
  are the guide's pages, its test, the generator and the tutorial's app,
  none of them changed here, and its browser mutants need the guestbook
  spec on a port. The image ADR's branch changes that script and the guide,
  and runs it.
