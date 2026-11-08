# `control_without_label` — challenge dimensions

A form control must have something that names it.

| axis | file | why |
|---|---|---|
| indirect invalid | `caught.pw` | a `<select>` rather than an `<input>` |
| neighbour | `neighbour.pw` | the same control, with a `<label for>` |
| indirect invalid | `id-alone.pw` | an `id` and a placeholder, and no label points at the `id` |
| indirect invalid | `label-for-another.pw` | a label, for a different `id` |
| neighbour | `wrapped.pw` | the label wraps its control, with no `for` and no `id` |
| neighbour | `labelledby-heading.pw` | `aria-labelledby` names a heading with text |
| neighbour | `buttons.pw` | an image button named by its `alt`, and a reset button by the label HTML gives it |
| indirect invalid | `blank-submit.pw` | a submit button whose `value` is empty, which HTML makes its label |

The rows from `id-alone.pw` to `labelledby-heading.pw` were added on
2026-10-02 (ADR-0143). Until then an `id` counted as a name and was never
checked, so each caught row passed. `wrapped.pw` was refused, because the
rule read only the control's own attributes. A row for
`labelledby-nothing.pw`, an `aria-labelledby` naming an element that is not
there, stood here too: since ADR-0185 that reference is PW5032's, and the
witness is `reference_names_nothing/labelledby-nothing.pw`.

The last two rows were added on 2026-11-06 (ADR-0405). Until
then PW5014 refused both of `buttons.pw`'s buttons for having no `<label>`,
and passed every `submit` input, `blank-submit.pw`'s included. An image
button's `alt` is held by PW5053: `image_without_alt/image-button-empty-alt.pw`.
