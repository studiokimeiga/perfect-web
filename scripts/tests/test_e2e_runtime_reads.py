"""A browser test reads the runtime's report only once the runtime is ready (2026-11-16).

`window.__pw` is the runtime's report: `public/pw-runtime.mjs` assigns it
when its module evaluates, and sets `data-pw-ready="1"` when it has booted.
A page's server HTML is visible before either. `optimistic.spec.mjs` reloaded
the store page, waited for a count its HTML already showed, and read
`window.__pw.log`: on PR #119's CI, Firefox evaluated it first ("window.__pw
is undefined"). This check fails any spec that reads `window.__pw.` (not
`window.__pw?.`) within a few lines after a navigation, with no wait for the
runtime between them. It would have found that read when it was written.
"""
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
RENDERER = ROOT / "spikes/own-renderer"

NAVIGATION = re.compile(r"\.(goto|reload|goBack|goForward)\(")
# What waits for the runtime: its ready mark, a function the page must
# answer, or a spec's own `ready(...)` helper (each waits for the mark).
WAITED = re.compile(r"pwReady|waitForFunction\(|\bready\(")
TEST = re.compile(r"^\s*test(\.describe)?\(")
# A read that throws where the report is not there yet.
READ = re.compile(r"window\.__pw\.")
# How far after a navigation a read is held to it.
WITHIN = 15


def unwaited_reads(lines):
    """The 1-based lines of `lines` that read the runtime's report after a
    navigation and before any wait for the runtime."""
    found, navigated = [], None
    for number, line in enumerate(lines, start=1):
        if TEST.search(line):
            navigated = None
        if NAVIGATION.search(line):
            navigated = number
        if WAITED.search(line):
            navigated = None
        if navigated is not None and number - navigated <= WITHIN and READ.search(line):
            found.append(number)
    return found


def specs():
    return sorted(p for d in RENDERER.glob("e2e*") if d.is_dir() for p in d.rglob("*.mjs"))


class RuntimeReads(unittest.TestCase):
    def test_no_spec_reads_the_runtimes_report_before_it_is_ready(self):
        found = [
            f"{path.relative_to(ROOT)}:{n}"
            for path in specs()
            for n in unwaited_reads(path.read_text(encoding="utf-8").splitlines())
        ]
        self.assertEqual(found, [], "each reads window.__pw before waiting for pwReady")

    def test_the_specs_are_found(self):
        self.assertTrue(any(p.name == "optimistic.spec.mjs" for p in specs()))

    def test_the_read_after_a_reload_is_found(self):
        # The test as it was before 2026-11-16: the negative control.
        before = [
            "  await page.reload();",
            '  await expect(page.locator("#cart-count")).toHaveText("1");',
            '  const log = await page.evaluate(() => window.__pw.log.join("\\n"));',
        ]
        self.assertEqual(unwaited_reads(before), [3])

    def test_a_read_after_the_ready_mark_is_not(self):
        after = [
            "  await page.reload();",
            '  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");',
            '  const log = await page.evaluate(() => window.__pw.log.join("\\n"));',
        ]
        self.assertEqual(unwaited_reads(after), [])

    def test_an_optional_read_and_a_helpers_wait_are_not(self):
        self.assertEqual(
            unwaited_reads([
                "  await page.goto(PAGE);",
                "  await expect.poll(() => page.evaluate(() => (window.__pw?.log ?? []).length)).toBe(1);",
            ]),
            [],
        )
        self.assertEqual(
            unwaited_reads([
                "  await ready(page);",
                "  await page.reload();",
                "  await ready(page);",
                "  const log = await page.evaluate(() => window.__pw.log);",
            ]),
            [],
        )

    def test_a_new_test_starts_unnavigated(self):
        self.assertEqual(
            unwaited_reads([
                "  await page.goto(PAGE);",
                "});",
                'test("another", async ({ page }) => {',
                "  const log = await page.evaluate(() => window.__pw.log);",
            ]),
            [],
        )


if __name__ == "__main__":
    unittest.main()
