"""A mutation script's server test module is one its validator reads (2026-11-16).

`cargo_assertion_results` accepts a failure only at an assertion of a file the
script's `SOURCES` lists. #116 added `tests::owned_clock::` to
`row_ownership_mutations.py`'s `TESTS` and not `owned_clock.rs` to its
`SOURCES`, and its verify found three mutants INVALID. `mutation_anchors.py`,
which `just ci` runs on every change, now says so before any build.
"""
import pathlib
import sys
import types
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
import mutation_anchors  # noqa: E402 - after the path, as the tests beside it

SERVER = mutation_anchors.SERVER
OWNED_CLOCK = SERVER / "tests/owned_clock.rs"
MAIN = SERVER / "main.rs"
SCRIPT = pathlib.Path("example_mutations.py")


def script(tests, sources):
    return types.SimpleNamespace(TESTS=tests, SOURCES=sources)


SERVER_RUN = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin",
              "pw-dev-server", "tests::owned_clock::"]


class UnreadTests(unittest.TestCase):
    def test_a_server_module_the_sources_omit_is_named(self):
        found = mutation_anchors.unread_tests(SCRIPT, script([SERVER_RUN], [MAIN]))
        self.assertEqual(len(found), 1, found)
        self.assertIn("tests::owned_clock::", found[0])
        self.assertIn("owned_clock.rs", found[0])

    def test_a_server_module_the_sources_list_is_not(self):
        found = mutation_anchors.unread_tests(SCRIPT, script([SERVER_RUN], [MAIN, OWNED_CLOCK]))
        self.assertEqual(found, [])

    def test_filters_beside_their_command_are_read_too(self):
        # `durable_tables_mutations.py`'s shape: (command, filters).
        tests = [(["cargo", "test", "-p", "pw-dev-server", "--bin", "pw-dev-server"],
                  ["tests::owned_clock::", "tests::tables_pg::"])]
        found = mutation_anchors.unread_tests(SCRIPT, script(tests, [MAIN, OWNED_CLOCK]))
        self.assertEqual(len(found), 1, found)
        self.assertIn("tests::tables_pg::", found[0])

    def test_a_module_inline_in_its_parent_is_its_parents_file(self):
        # `identity::transition_tests` is a module inside `identity.rs`.
        self.assertEqual(mutation_anchors.module_file("identity::transition_tests::"),
                         SERVER / "identity.rs")

    def test_another_crates_command_and_a_script_without_sources_are_left(self):
        other = [["cargo", "test", "-p", "pw-core", "--test", "tables", "tests::owned_clock::"]]
        self.assertEqual(mutation_anchors.unread_tests(SCRIPT, script(other, [MAIN])), [])
        self.assertEqual(mutation_anchors.unread_tests(SCRIPT, script([SERVER_RUN], None)), [])

    def test_every_script_here_reads_its_server_modules(self):
        problems = []
        for path in sorted((mutation_anchors.ROOT / "scripts").glob("*_mutations.py")):
            _, found = mutation_anchors.problems_in(path)
            problems.extend(p for p in found if "SOURCES lists not" in p)
        self.assertEqual(problems, [])


class UnreadWordTests(unittest.TestCase):
    """A script that runs the server's tests by a word of their names
    (`uploads`, `blob`) lists every file holding one (L5's second review:
    `upload_metadata_mutations.py` ran seven files its `RUST_SOURCES` did not
    list)."""

    COMMAND = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin",
               "pw-dev-server", "--", "sniff", "--format=pretty"]

    def test_a_file_a_word_reaches_and_the_sources_omit_is_named(self):
        reached = mutation_anchors.reached("sniff")
        self.assertIn(SERVER / "uploads/sniff.rs", reached)
        found = mutation_anchors.unread_word_tests(
            SCRIPT, types.SimpleNamespace(SERVER=self.COMMAND, RUST_SOURCES=[SERVER / "uploads.rs"]))
        self.assertEqual(len(found), len(reached - {SERVER / "uploads.rs"}), found)
        self.assertTrue(any("uploads/sniff.rs" in f and "`sniff`" in f for f in found), found)

    def test_every_file_listed_is_none_and_a_suites_command_is_read_too(self):
        listed = sorted(mutation_anchors.reached("sniff"))
        self.assertEqual(mutation_anchors.unread_word_tests(
            SCRIPT, types.SimpleNamespace(SERVER=self.COMMAND, RUST_SOURCES=listed)), [])
        # `render_merge_mutations.py`'s shape: suites of (command, environment).
        suites = {"unit": (self.COMMAND, {"PW_STORE_TEST_LAYER": "memory"})}
        self.assertTrue(mutation_anchors.unread_word_tests(
            SCRIPT, types.SimpleNamespace(CARGO=suites, SOURCES={"host": MAIN})))

    def test_a_test_is_named_with_its_module_and_one_inline_in_its_file(self):
        names = {name for name, _ in mutation_anchors.server_tests()}
        self.assertTrue(any(n.startswith("uploads::sniff::tests::") for n in names))
        self.assertTrue(any(n.startswith("tests::uploads::") for n in names))
        self.assertEqual(
            {path for name, path in mutation_anchors.server_tests() if name.startswith("blob::assets::")},
            {SERVER / "blob/assets.rs"})

    def test_a_script_whose_sources_name_no_server_file_is_left(self):
        # Its server kills, if it has any, are read some other way.
        self.assertEqual(mutation_anchors.unread_word_tests(
            SCRIPT, types.SimpleNamespace(SERVER=self.COMMAND,
                                          SOURCES=[mutation_anchors.ROOT / "compiler/pw-core/src/lib.rs"])), [])
        self.assertEqual(mutation_anchors.unread_word_tests(
            SCRIPT, types.SimpleNamespace(SERVER=self.COMMAND)), [])

    def test_every_script_here_lists_what_its_words_run(self):
        problems = []
        for path in sorted((mutation_anchors.ROOT / "scripts").glob("*_mutations.py")):
            _, found = mutation_anchors.problems_in(path)
            problems.extend(p for p in found if "runs the server's tests by" in p)
        self.assertEqual(problems, [])


if __name__ == "__main__":
    unittest.main()
