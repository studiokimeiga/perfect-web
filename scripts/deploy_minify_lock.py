#!/usr/bin/env python3
"""The image's minifier, locked from the workspace's lockfile (track
`deploy`, ADR-XXXX, a production host).

`deploy/Containerfile` minifies the runtime with esbuild, installed by
`npm ci` from `deploy/minify/package-lock.json`. That lockfile is not
written by hand: this script writes it from `pnpm-lock.yaml`, the esbuild
`spikes/own-renderer` declares, each package with the integrity pnpm
recorded, so the image installs exactly the bytes the workspace audited
(`npm ci` refuses a tarball whose SHA-512 differs).

    python3 scripts/deploy_minify_lock.py           write it
    python3 scripts/deploy_minify_lock.py --check   fail where it differs

`just e14-deploy` runs the check, so a lockfile that moved on is said.
"""

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PNPM_LOCK = ROOT / "pnpm-lock.yaml"
OWN_RENDERER = ROOT / "spikes/own-renderer/package.json"
OUT = ROOT / "deploy/minify"
NAME = "pleris-runtime-minifier"


def version_declared():
    """The esbuild version spikes/own-renderer declares, exactly."""
    declared = json.loads(OWN_RENDERER.read_text())["devDependencies"]["esbuild"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", declared):
        sys.exit(f"spikes/own-renderer declares esbuild {declared!r}, not an exact version")
    return declared


def packages(lock, version):
    """Each package's block in pnpm-lock.yaml's `packages:` section, for
    esbuild at `version` and each `@esbuild/<platform>` at it: name to its
    fields (integrity, cpu, os, engines)."""
    section = lock.split("\npackages:\n", 1)[1].split("\nsnapshots:\n", 1)[0]
    found = {}
    for block in section.split("\n\n"):
        head = block.strip().split("\n", 1)[0].strip()
        match = re.fullmatch(r"'?((?:@esbuild/[a-z0-9-]+)|esbuild)@([0-9.]+)'?:", head)
        if not match or match.group(2) != version:
            continue
        fields = {}
        integrity = re.search(r"integrity: (sha512-[A-Za-z0-9+/=]+)", block)
        if not integrity:
            sys.exit(f"{match.group(1)}@{version} has no sha512 integrity in pnpm-lock.yaml")
        fields["integrity"] = integrity.group(1)
        for key in ("cpu", "os"):
            listed = re.search(rf"\n\s+{key}: \[([^\]]*)\]", block)
            if listed:
                fields[key] = [v.strip() for v in listed.group(1).split(",")]
        engines = re.search(r"engines: \{node: '([^']*)'\}", block)
        if engines:
            fields["engines"] = {"node": engines.group(1)}
        found[match.group(1)] = fields
    if "esbuild" not in found:
        sys.exit(f"esbuild@{version} is not in pnpm-lock.yaml's packages")
    return found


def optional_of(lock, version):
    """esbuild's optional dependencies, as pnpm-lock.yaml's snapshot says."""
    snapshots = lock.split("\nsnapshots:\n", 1)[1]
    block = snapshots.split(f"\n  esbuild@{version}:\n", 1)[1].split("\n\n", 1)[0]
    return sorted(re.findall(r"'(@esbuild/[a-z0-9-]+)': " + re.escape(version), block))


def tarball(name, version):
    """The registry's address of a package's tarball."""
    leaf = name.split("/")[-1]
    return f"https://registry.npmjs.org/{name}/-/{leaf}-{version}.tgz"


def written():
    """The package.json and package-lock.json the image's minifier is
    installed from, as text."""
    version = version_declared()
    lock = PNPM_LOCK.read_text()
    found = packages(lock, version)
    optional = optional_of(lock, version)
    missing = [name for name in optional if name not in found]
    if missing:
        sys.exit(f"pnpm-lock.yaml lacks {', '.join(missing)} at {version}")
    manifest = {"name": NAME, "private": True, "devDependencies": {"esbuild": version}}
    entries = {"": {"name": NAME, "devDependencies": {"esbuild": version}}}
    for name in optional:
        fields = found[name]
        entry = {
            "version": version,
            "resolved": tarball(name, version),
            "integrity": fields["integrity"],
        }
        for key in ("cpu", "os"):
            if key in fields:
                entry[key] = fields[key]
        entry["dev"] = True
        entry["optional"] = True
        if "engines" in fields:
            entry["engines"] = fields["engines"]
        entries[f"node_modules/{name}"] = entry
    esbuild = found["esbuild"]
    entries["node_modules/esbuild"] = {
        "version": version,
        "resolved": tarball("esbuild", version),
        "integrity": esbuild["integrity"],
        "dev": True,
        "hasInstallScript": True,
        "bin": {"esbuild": "bin/esbuild"},
        "engines": esbuild.get("engines", {"node": ">=18"}),
        "optionalDependencies": {name: version for name in optional},
    }
    lockfile = {"name": NAME, "lockfileVersion": 3, "requires": True, "packages": entries}
    return (
        json.dumps(manifest, indent=2) + "\n",
        json.dumps(lockfile, indent=2) + "\n",
    )


def main():
    manifest, lockfile = written()
    pairs = [(OUT / "package.json", manifest), (OUT / "package-lock.json", lockfile)]
    if "--check" in sys.argv[1:]:
        stale = [p for p, text in pairs if not p.exists() or p.read_text() != text]
        if stale:
            print("FAIL: " + ", ".join(str(p.relative_to(ROOT)) for p in stale)
                  + " differ from pnpm-lock.yaml: run python3 scripts/deploy_minify_lock.py")
            return 1
        print(f"deploy/minify: esbuild {version_declared()}, as pnpm-lock.yaml holds it")
        return 0
    OUT.mkdir(parents=True, exist_ok=True)
    for path, text in pairs:
        path.write_text(text)
    print(f"wrote {', '.join(str(p.relative_to(ROOT)) for p, _ in pairs)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
