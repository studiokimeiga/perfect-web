"""Bounded native diagnostic only: unchanged fifty-test Chromium inventory."""
import contextlib
import hashlib
import json
import os
import re
from pathlib import Path
import secrets
import subprocess
import sys
import time

ROOT = Path('/home/haki/code/pw-wt/session-followup-20261027')
sys.path.insert(0, str(ROOT / 'scripts'))
import todomvc_mutations as gate
import browser_test_results
import producer_process
import mutation_source

mode, directory = sys.argv[1:]
if mode not in ('before', 'after'):
    raise SystemExit('unknown mode')
out = Path(directory)
out.mkdir(exist_ok=False)
os.chdir(ROOT)
gate.BOUND = 600
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
paths = [gate.APP, gate.RUNTIME_FILE, ROOT / 'spikes/own-renderer/e2e/todomvc.spec.mjs',
         ROOT / 'scripts/todomvc_mutations.py', ROOT / 'spikes/own-renderer/assertion-result-reporter.mjs']
sha = lambda data: hashlib.sha256(data).hexdigest()
inputs = {str(p.relative_to(ROOT)): sha(p.read_bytes()) for p in paths}
(out / 'inputs-before.json').write_text(json.dumps({'head': head, 'mode': mode, 'inputs': inputs}, indent=2)+'\n')
inventory = None
runs = []

def browser(label, pages=False):
    global inventory
    start = time.time()
    with (out / (label + '.build.log')).open('x') as stream:
        with contextlib.redirect_stdout(stream), contextlib.redirect_stderr(stream):
            built = gate.build(pages)
    if not built:
        raise RuntimeError(label + ': build failed; no result credit')
    receipt = out / (label + '.receipt.json')
    nonce = secrets.token_hex(32)
    cmd = ['bash', str(ROOT / 'scripts/run_suite.sh'), '-C', '.', '--',
           'pnpm', 'exec', 'playwright', 'test', *gate.SPEC, '--retries=0',
           '--reporter=line,./assertion-result-reporter.mjs']
    reaped = []
    result = producer_process.run(cmd, timeout=600,
        cwd=ROOT / 'spikes/own-renderer',
        env={**os.environ, 'PW_ASSERTION_REPORT': str(receipt), 'PW_ASSERTION_NONCE': nonce},
        on_reaped=lambda pid, status, timed_out, cancelled: reaped.append({'pid': pid, 'status': status, 'timed_out': timed_out, 'cancelled': cancelled}))
    (out / (label + '.stdout.log')).write_text(result.stdout)
    raw = json.loads(receipt.read_text()) if receipt.exists() else None
    parsed = browser_test_results.parse(result.returncode, raw, nonce, inventory)
    row = {'label': label, 'head': head, 'cmd': cmd, 'cwd': str(ROOT / 'spikes/own-renderer'),
           'started': start, 'finished': time.time(), 'exit': result.returncode,
           'native_reaped': reaped, 'result': parsed._asdict()}
    (out / (label + '.result.json')).write_text(json.dumps(row, indent=2)+'\n')
    runs.append(row)
    print(label, 'valid='+str(parsed.valid), 'passed='+str(parsed.passed),
          'failed='+str(parsed.failed), parsed.reason, flush=True)
    if label.endswith('baseline'):
        if not parsed.valid or parsed.failed or parsed.passed != 50:
            raise RuntimeError(label + ': baseline is not complete green')
        inventory = parsed.inventory
    elif mode == 'after':
        if not parsed.valid or not parsed.failed or parsed.passed + parsed.failed != 50:
            raise RuntimeError(label + ': no complete fifty-test assertion result')
        target = 'the toggle-all, pressed, is not checked for a todo added once the completed are cleared'
        blocks = re.split(r'(?m)^  \d+\) ', result.stdout)[1:]
        intended = [block for block in blocks if target in block.splitlines()[0]]
        if (parsed.failed != 2 or len(intended) != 1
                or 'Error: expect(locator).toBeVisible() failed' not in intended[0]
                or 'Received: hidden' not in intended[0]
                or ('PW_ASSERTION:'+nonce) not in intended[0]):
            raise RuntimeError(label + ': intended existing visibility assertion did not fail')

mutant = [row for row in gate.MUTANTS if row[0] == 'the clear button counts what is left']
if len(mutant) != 1:
    raise RuntimeError('control must be unique')
_, kind, path, anchor, replacement = mutant[0]
original = path.read_text()
if kind != gate.PROGRAM or original.count(anchor) != 1:
    raise RuntimeError('control anchor mismatch')
try:
    browser(mode+'-baseline', pages=True)
    with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
        with (out / 'mutant-check.log').open('x') as stream:
            with contextlib.redirect_stdout(stream), contextlib.redirect_stderr(stream):
                checked = gate.checks()
        (out / 'mutant-check.json').write_text(json.dumps([checked[0], checked[1], sorted(checked[2])])+'\n')
        if checked != (True, True, set()):
            raise RuntimeError('mutant must pass native checker')
        browser(mode+'-mutant')
finally:
    if path.read_text() != original:
        raise RuntimeError('source restoration failed')
    browser(mode+'-restored-baseline', pages=True)
    after = {str(p.relative_to(ROOT)): sha(p.read_bytes()) for p in paths}
    (out / 'inputs-after.json').write_text(json.dumps({'head': head, 'inputs': after}, indent=2)+'\n')
    if inputs != after or head != subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip():
        raise RuntimeError('diagnostic changed source inputs or HEAD')
    (out / 'runs.json').write_text(json.dumps(runs, indent=2)+'\n')
print('completed diagnostic; whole 43-control recipe NOT RUN', flush=True)
