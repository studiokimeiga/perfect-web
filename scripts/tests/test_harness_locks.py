"""Each harness lockfile pins what its baseline's package.json asks for.

The benchmark harness installs a framework sandbox from its own lockfile,
offline and frozen (`benchmarks/harness/locks/`, ADR-0124; `bench.mjs`), and
pnpm refuses a frozen install whose lockfile's specifiers are not the
package.json's. When the Next.js baseline moved to 16.4.1 (2026-10-22), its
harness lockfile stayed at 16.3.8, and nothing noticed for 26 days: the
sandbox's install failed (`just e14-harness` runs locally only), until the
baselines' patches of 2026-11-17 and their review. Each lockfile's importer
is held here to its baseline's dependencies and overrides, exactly.
"""

from __future__ import annotations

import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
LOCKS = ROOT / "benchmarks/harness/locks"
BASELINES = ROOT / "benchmarks/baselines"


def unquoted(text: str) -> str:
    text = text.strip()
    return text[1:-1] if text[:1] in ("'", '"') and text[-1:] == text[:1] else text


def importer(lock: str) -> dict[str, dict[str, str]]:
    """The lockfile's root importer (`.`): each section's specifiers by name."""
    body = lock.split("\nimporters:\n", 1)[1]
    root = body.split("\n  .:\n", 1)[1]
    root = re.split(r"(?m)^(?:  \S|\S)", root, maxsplit=1)[0]
    sections: dict[str, dict[str, str]] = {}
    section = name = None
    for line in root.splitlines():
        if re.fullmatch(r"    (dependencies|devDependencies|optionalDependencies):", line):
            section = line.strip().rstrip(":")
            sections[section] = {}
        elif section and re.fullmatch(r"      \S.*:", line):
            name = unquoted(line.strip()[:-1])
        elif section and name and line.startswith("        specifier: "):
            sections[section][name] = unquoted(line.split(":", 1)[1])
    return sections


def overrides(lock: str) -> dict[str, str]:
    """The lockfile's `overrides:` section, by selector."""
    found = re.search(r"(?m)^overrides:\n((?:  .*\n)+)", lock)
    if not found:
        return {}
    pairs = (line.strip().split(": ", 1) for line in found.group(1).splitlines())
    return {unquoted(k): unquoted(v) for k, v in pairs}


class HarnessLocksMatchTheirBaselines(unittest.TestCase):
    def test_each_lockfile_specifies_its_baselines_dependencies_exactly(self):
        stacks = sorted(p.name.removesuffix(".pnpm-lock.yaml") for p in LOCKS.glob("*.pnpm-lock.yaml"))
        self.assertEqual(stacks, ["next-react", "sveltekit"])
        for stack in stacks:
            with self.subTest(stack):
                lock = (LOCKS / f"{stack}.pnpm-lock.yaml").read_text()
                pkg = json.loads((BASELINES / stack / "package.json").read_text())
                found = importer(lock)
                for section in ("dependencies", "devDependencies"):
                    self.assertEqual(found.get(section, {}), pkg.get(section, {}), section)
                self.assertEqual(overrides(lock), pkg.get("pnpm", {}).get("overrides", {}))

    def test_the_reading_sees_a_specifier_that_drifted(self):
        lock = (
            "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n"
            "      next:\n        specifier: 16.3.8\n        version: 16.3.8(react@19.3.0)\n"
            "    devDependencies:\n      '@types/node':\n        specifier: 22.20.5\n"
            "        version: 22.20.5\n\npackages:\n\n  next@16.3.8:\n    resolution: {}\n"
        )
        self.assertEqual(
            importer(lock),
            {"dependencies": {"next": "16.3.8"}, "devDependencies": {"@types/node": "22.20.5"}},
        )
        self.assertNotEqual(importer(lock)["dependencies"], {"next": "16.4.1"})
        self.assertEqual(
            overrides("overrides:\n  cookie@<0.7.0: 0.7.2\n  deepmerge: file:vendor/deepmerge\n\nimporters:\n"),
            {"cookie@<0.7.0": "0.7.2", "deepmerge": "file:vendor/deepmerge"},
        )


if __name__ == "__main__":
    unittest.main()
