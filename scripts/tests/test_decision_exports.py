"""`decision_exports.py`: every export a page's script asks of the decision's
Wasm, the Wasm has (ADR-0400; train-14's review). Read of the repository,
with a floor so that a reader that stopped finding scripts fails, and with
controls of each way a script asks."""

import importlib.util
import pathlib
import unittest

SCRIPTS = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("decision_exports", SCRIPTS / "decision_exports.py")
de = importlib.util.module_from_spec(spec)
spec.loader.exec_module(de)

RUST = """
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 { todo!() }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn decide_manifest(ptr: *const u8, len: usize) -> u32 { 0 }
#[no_mangle]
pub extern "C" fn last_recovery() -> u32 { 0 }
pub fn not_exported() {}
"""


class DecisionExports(unittest.TestCase):
    def test_the_repositorys_scripts_ask_only_what_the_decision_exports(self):
        gone, each = de.missing()
        self.assertEqual(gone, [])
        # The floor: the page runtime and both copies of the Marko spike's
        # are read, and each asks for the decision itself.
        asking = {p: names for p, names in each.items() if names}
        self.assertGreaterEqual(len(asking), 3, each)
        for path, names in asking.items():
            self.assertIn("decide_manifest", names, path)
            self.assertIn("memory", names, path)

    def test_the_decisions_exports_are_its_exported_functions_and_memory(self):
        self.assertEqual(de.exports(RUST), {"alloc", "decide_manifest", "last_recovery", "memory"})
        real = de.exports(de.WASM_SOURCE.read_text(encoding="utf-8"))
        self.assertTrue({"alloc", "decide_manifest", "last_recovery", "memory"} <= real, real)
        self.assertFalse({"last_trace_ptr", "last_trace_len"} & real, real)

    def test_a_member_the_decision_lacks_is_named(self):
        script = "const w = instance.exports;\nconst c = w.decide_manifest(p, n);\nconst t = w.last_trace_ptr();\n"
        self.assertEqual(de.asked(script), {"decide_manifest", "last_trace_ptr"})
        self.assertEqual(de.asked(script) - de.exports(RUST), {"last_trace_ptr"})

    def test_a_name_destructured_beside_the_decision_is_read_and_another_is_not(self):
        script = (
            "const { memory, alloc: take, decide_manifest, gone = null } = e.exports;\n"
            "const { out_ptr, out_len } = renderer.exports;\n"
        )
        self.assertEqual(de.asked(script), {"memory", "alloc", "decide_manifest", "gone"})


if __name__ == "__main__":
    unittest.main()
