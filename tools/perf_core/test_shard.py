"""Verify that test-binary sharding cannot omit targets or hide failures."""
import unittest
from shard import cargo_targets,balanced


def metadata(*names):
    return {"packages":[{"name":"pw-core","targets":[
        {"kind":["test"],"name":x} for x in names
    ]}]}


class Contracts(unittest.TestCase):
    def test_targets_equal_files(self):
        self.assertEqual(cargo_targets(metadata("a","b"),{"a","b"}),["a","b"])

    def test_files_and_cargo_must_match(self):
        for files in ({"a"},{"a","b","extra"}):
            with self.assertRaises(ValueError):
                cargo_targets(metadata("a","b"),files)

    def test_no_duplicate_targets(self):
        with self.assertRaises(ValueError):
            cargo_targets(metadata("a","a"),{"a"})

    def test_no_empty_cargo_metadata(self):
        with self.assertRaises(ValueError):
            cargo_targets({"packages":[]},{"a"})

    def test_all_targets_exactly_once(self):
        names=[f"t{i}" for i in range(200)]
        weights={"t0":200,"t1":120,"t2":60}
        plan=balanced(names,weights)
        self.assertEqual(sorted(t for part in plan for t in part),sorted(names))
        self.assertTrue(all(part for part in plan))

    def test_stable_partition_order(self):
        names=["a","b","c","d","e"]
        self.assertEqual(balanced(names,{"b":15}),balanced(names[::-1],{"b":15}))

    def test_fail_closed_invalid_weights(self):
        for value in (-1,float("nan"),float("inf")):
            with self.subTest(value=value),self.assertRaises(ValueError):
                balanced(["a"],{"a":value})

    def test_alternate_weights_can_change_allocation_without_missing_tests(self):
        names=["a","b","c","d","e","f"]
        alt={"a":100,"b":50,"c":25}
        a=balanced(names,{},3)
        b=balanced(names,alt,3)
        self.assertNotEqual(a,b)
        self.assertEqual(sorted(x for part in b for x in part),sorted(names))

    def test_duplicate_names_refused(self):
        with self.assertRaises(ValueError):
            balanced(["x","x"],{})
