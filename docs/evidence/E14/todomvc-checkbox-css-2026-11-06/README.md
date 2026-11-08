# TodoMVC's checkbox drawn in CSS: its whole check, 2026-11-06

ADR-0367's amendment of 2026-11-06, "a todo's checkbox drawn in CSS". Branch
`feat/todomvc-checkboxes-css-20261106`, from `master` at `5d4d4d55`:
- `d94b6ecd`: the checkbox drawn in CSS, its two tests and its four controls;
- `cefa613c`: what the first check, and the controls run alone after it,
  called for (below);
- the commit that adds this directory, which changes nothing else.

Every file here but this README is in `SHA256SUMS`. Long logs are gzipped,
and so are the scripts that produced them, beside them. Terminal colour codes
are stripped.

## The check, at `cefa613c` (`final-verify-tm.log`): passed

`final-verify-tm.sh` ran at the exact commit, with no uncommitted code, from
14:23 to 15:51 (UTC−5):
1. **Fallow's audit**, as CI runs it (fallow 3.23.0, `--gate new-only`,
   against `5d4d4d55`): pass, nothing introduced (`fallow-tm.json.gz`).
2. **The mutation scripts the change reaches** (`reached.py 5d4d4d55 3`, any
   anchor within 3 lines of a changed line): `page_policy`, `todomvc` and
   `todomvc_producer` (`tm-reached.txt`).
3. **The whole pre-push check** (`prepush-verify-tm.sh`, tag `tm2`; summary in
   `tm2-verify.log`):
   - `just preflight`;
   - `just evidence-gates`: 1,177 tests OK, 5 skipped;
   - `cargo test` of `pw-render`, `pw-render-wasm` and `pw-dev-server`: 867,
     8 and 8 passed;
   - every own-renderer app built;
   - the whole own-renderer suite in Chromium and Firefox: 1,044 tests,
     1,033 passed, 11 skipped by their own conditions, 0 failed;
   - the runtime-size gates: 3 passed.
4. **Each reached mutation script, run whole** (`tm-mut-*.log.gz`):
   `page_policy` 44 of 44 killed; `todomvc` 46 of 46; `todomvc_producer` 17
   of 17. None refused, none survived.
5. **The anchors restored:** 4,977 mutants in 324 scripts, each matching
   once, and no code left changed.

The checkbox's measures, in `final-verify-tm.log` and `tm2-specs.log.gz`:

| | Unchecked: mean, over an eighth; the ring's ink (colour, amount) | Checked: the same; the tick's ink |
|---|---|---|
| Chromium 145.0.7632.6 | 2.03, 21; 0.0 points, 13.6% | 3.19, 65; 0.8, 13.8%; 0.1, −0.3% |
| Firefox 146.0.1 | 1.02, 2; 0.0, 14.8% | 1.64, 19; 0.7, 15.2%; 0.0, −0.3% |
| Bound | 3, 35; 5, 23% | 4.75, 100; 5, 23%; 5, 23% |

## The first check, at `d94b6ecd`: failed (`run1-*`)

The same script, tag `tm1`, from 12:35 to 14:00. Fallow, `preflight`, cargo,
the builds, the suite (1,033 passed, 11 skipped) and the size gates passed.
Three things did not:
- **`just evidence-gates`** failed one test of 1,177,
  `test_todomvc_producers.Producers.test_browser_requires_fresh_receipt_and_native_exit`
  (`run1-tm1-evidence-gates.log.gz`). It faked a browser receipt of 50
  tests, a count it repeated from `todomvc_mutations.py`, where the change
  made it 52.
- **`todomvc_producer_mutations.py`** stopped at its baseline, for that test
  (`run1-tm-mut-todomvc_producer.log.gz`).
- **`todomvc_mutations.py`** refused "the ring is drawn off its centre"
  (`run1-tm-mut-todomvc.log.gz`: "missing complete browser receipt"). The
  pixel test failed, but with `toBeLessThan`, which does not name itself
  when it fails. So `assertion-expect.mjs` gave the failure no proof, and the
  run's receipt was refused. `matcher-names.log` (`matcher-names.mjs`) shows
  which of Playwright 1.58's matchers name themselves: `toBe`, `toEqual` and
  `toStrictEqual`; `toMatch`, `toBeLessThan` and the six others it tried do
  not. The script stops at a refusal, so 41 of the 46 ran: 40 killed, and
  that one refused.

`page_policy` killed 44 of 44.

## Between the two checks

- **`styles-mutant-effects.log`** (`styles-mutant-effects.py`): each styles
  mutant, applied to a copy, changes the sheet the script writes; none is
  equivalent.
- **`styles-only-1.log`**: the four styles mutants alone
  (`styles-only-mutations.py`, the script's own `main` over them), with the
  bounds compared by `toEqual`. Three were killed. "The checked ring keeps
  the unchecked colour" survived: on a line under a pixel wide, grey for
  green moves the box's pixels too little.
- **`ink-measure.log`** (`ink-measure.mjs`): the ink per region (255 less
  each channel, summed over the region's pixels), for the drawing and for
  drawings with a colour changed, in both engines. The ink bounds and the
  ADR's ink table come from it.
- **`styles-only.log`**: the four again, with the ink bounds: 4 of 4 killed.

`cefa613c` makes these fixes: the producers' test takes the count from
`BROWSER_COUNT`; the spec asserts only with matchers that name themselves;
and it bounds each region's ink, in colour and in amount.

## The alternatives, measured before the bounds were set

`checkbox-alternatives.log` (`checkbox-alternatives.mjs`), in Chromium and
Firefox: six ring profiles, the tick's box at sub-pixel offsets and rounded,
and a `path()` clip in place of `polygon()`.

## Reproduce

In the worktree, with `env-tm.sh`'s toolchain (Node 22.21.1, the Rust
toolchain the repository pins, port base 14880):

    SCRATCH=<directory holding these scripts> bash final-verify-tm.sh

or, by part:

    npx -y fallow@3.23.0 audit --root . --quiet --format json --changed-since 5d4d4d55 --gate new-only
    just preflight && just evidence-gates
    cd spikes/own-renderer && pnpm exec playwright test e2e/todomvc.spec.mjs --project=chromium --project=firefox
    CARGO_INCREMENTAL=0 python3 scripts/todomvc_mutations.py
    python3 scripts/todomvc_producer_mutations.py
    CARGO_INCREMENTAL=0 python3 scripts/page_policy_mutations.py

Not run here: WebKit, which cannot open a page on this PC; the recipes that
need PostgreSQL; and `verify.yml`'s planned recipes, `e14-todomvc` among
them, which CI runs on a PR into `master`.

## 2026-11-07: WebKit measured at one pixel a CSS pixel

#81's verify run 42315680977 failed the pixel test in WebKit only: checked,
122 pixels over an eighth against a bound of 100. The drawing was not off.
WebKit's project is "Desktop Safari", whose device scale factor is 2, so the
test's screenshots of the 40-pixel box were 80 pixels square, counted
against bounds of 1,600 pixels and filed on a 40-pixel grid.
- `webkit-tick-scale.log` (`webkit-tick-scale.mjs.gz`): at 2x, screenshots
  at device scale measure WebKit 2.10 and 122, as CI did, and Chromium 2.08
  and 117; at CSS scale both measure 3.18–3.19 and 65, as at 1x.
- `webkit-tick-placement.log` (`webkit-tick-placement.mjs.gz`): WebKit lays
  the label out 58 px tall (Chromium and Firefox 58.8), so every placement
  of the tick measures the same there.
- `webkit-tm-webkit-fix.log`: with `checkbox()` at `scale: "css"`, the
  TodoMVC spec in Chromium, Firefox and WebKit 26.0, 156 of 156 passed.
  WebKit: unchecked 2.03 and 21, checked 3.18 and 65.

## 2026-11-07: #81's review

`review-2026-11-07.log`: with the review's three items folded in, Fallow's audit as CI runs it
(failed once on `sheetOf` at cyclomatic 5, then passing with its shape check
moved to `shapedAsTodoMVCs`), the TodoMVC spec in Chromium, Firefox and
WebKit, 156 of 156, and the seven styles controls alone, 7 of 7 killed.
