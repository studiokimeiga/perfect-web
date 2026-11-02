#!/usr/bin/env python3
"""Run complete CLI validation over the repository's source programs.

Reference apps have their normal program roots. Rejected and generality
witnesses remain separate programs, with the dependencies their existing
Rust corpus suites use. Historical texts use today's library and are listed
separately; a refusal there is not current-corpus conformance. No artifact
is written. PW6601 can only follow a successful semantic prerequisite gate,
so those findings are the programs newly refused by complete checking.
"""
import hashlib
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

def paths(pattern):
    return sorted(ROOT.glob(pattern))

def imported(fixture, accepted):
    src = fixture.read_text()
    heads = {m.group(1).split('.')[0] for m in re.finditer(r'^\s*import\s+([\w.]+)', src, re.M)}
    out = []
    for path in accepted:
        module = re.search(r'^\s*module\s+([\w.]+)', path.read_text(), re.M)
        if module and module[1].split('.')[0] in heads:
            out.append(path)
    return out

def programs():
    std = paths('packages/pw-std/*.pw') + paths('packages/pw-platform-web/*.pw')
    base = std + paths('examples/lib/*.pw') + paths('examples/domain.pw')
    accepted = paths('examples/accepted/*.pw')
    out = [('accepted', base + accepted), ('store', base + paths('examples/store/*.pw')),
        ('feed', std + paths('examples/feed/*.pw')), ('kiokun', std + paths('examples/kiokun/*.pw')),
        ('todomvc', std + paths('examples/todomvc/*.pw')),
        ('kiokun-site', std + paths('examples/kiokun/Shards.pw') + paths('examples/kiokun/dictionary.pw') + paths('examples/kiokun-site/*.pw')),
        ('demo', base + paths('examples/demo/*.pw')),
        # Track `tables` (ADR-0338): the program tables.rs builds, the
        # platform's packages and its four sources.
        ('tables', std + paths('examples/tables/*.pw')),
        # Track `row-ownership` (ADR-0384's H1 and H2): the owned table's
        # fixture, the program its tests build from the platform's packages.
        ('owned', std + paths('examples/owned/*.pw')),
        ('rules (the program of rule_fixtures.rs)', base + paths('examples/rules/**/*.pw'))]
    for group in ['counter', 'guestbook', 'hello-static', 'koka', 'render', 'streamed']:
        out.append((group, base + paths(f'examples/{group}/*.pw')))
    # Keep standalone compiler test fixtures in the complete source census.
    # The duplicate-index-fields witness is rejected by semantic checking;
    # it must still be checked as an executable program in isolation.
    for fixture in paths('compiler/pw-core/tests/fixtures/*.pw'):
        out.append((str(fixture.relative_to(ROOT)), std + [fixture]))
    for fixture in paths('examples/rejected/*.pw'):
        out.append((str(fixture.relative_to(ROOT)), base + imported(fixture, accepted) + [fixture]))
    for fixture in paths('examples/generality/**/*.pw'):
        out.append((str(fixture.relative_to(ROOT)), base + accepted + [fixture]))
    for fixture in paths('examples/robustness/**/*.pw') + paths('examples/history/**/*.pw'):
        out.append((str(fixture.relative_to(ROOT)), base + imported(fixture, accepted) + [fixture]))
    out.append(('historical benchmark baseline', std + paths('benchmarks/baselines/pleris/domain.pw') + paths('benchmarks/baselines/pleris/lib/*.pw') + paths('benchmarks/baselines/pleris/store/*.pw')))
    return out

def main():
    binary = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / 'target/debug/pw').resolve()
    print('Complete check program census; no artifacts')
    print('commit:', subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip())
    print('binary SHA256:', hashlib.sha256(binary.read_bytes()).hexdigest())
    print('source diff SHA256:', hashlib.sha256(subprocess.check_output(['git','diff','HEAD'],cwd=ROOT)).hexdigest())
    covered = set()
    totals = {'clean': 0, 'semantic-or-syntax-refusal': 0, 'new-support-refusal': 0}
    census = programs()
    for name, source in census:
        files = list(dict.fromkeys(source))
        covered.update(p.relative_to(ROOT).as_posix() for p in files)
        result = subprocess.run([str(binary), 'check', '--plain', *map(str,files)], cwd=ROOT, text=True, capture_output=True, timeout=180)
        text = result.stdout + result.stderr
        errors = re.findall(r'^error: \[(PW\d{4})\] (.*)', text, re.M)
        if result.returncode not in (0,1) or (result.returncode == 1 and not errors):
            raise RuntimeError(f'{name}: exit {result.returncode}\n{text}')
        kind = 'new-support-refusal' if any(c == 'PW6601' for c,_ in errors) else ('semantic-or-syntax-refusal' if errors else 'clean')
        totals[kind] += 1
        print(f'{name}: {kind}; {len(files)} files; codes={sorted({c for c,_ in errors})}',flush=True)
        for code, message in errors:
            if code == 'PW6601': print(f'  [{code}] {message}',flush=True)
    tracked = set(subprocess.check_output(['git','ls-files','*.pw'],cwd=ROOT,text=True).splitlines())
    missing = sorted(tracked - covered)
    print('program census:',len(census),'program assemblies;',len(covered & tracked),'of',len(tracked),'tracked source files covered;',totals)
    if missing: raise RuntimeError(f'uncovered tracked source files: {missing}')
    return 0

if __name__ == '__main__':
    sys.exit(main())
