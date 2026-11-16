"""Actual PG row/storage lifetime faults and retained old fence/unbound guards."""
from pathlib import Path
import importlib.util,sys,json,hashlib,os
root=Path('/home/haki/code/pw-wt/data-path');sys.path.insert(0,str(root/'scripts'))
assert os.environ.get('PW_TABLES_DATABASE_URL'),'actual throwaway PostgreSQL required'
import cargo_assertion_results,producer_process,mutant_incremental
def module(name):
 spec=importlib.util.spec_from_file_location(name,root/f'scripts/{name}_mutations.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
d3=module('d3_postgres');durable=module('durable_tables')
mutants=[m for m in d3.MUTANTS if m[0].startswith('upload lifecycle ')];assert len(mutants)==26
mutants += [next(m for m in durable.MUTANTS if 'platform::still_fenced' in m[2]),next(m for m in d3.MUTANTS if m[0]=='closed cases PostgreSQL serves assets without a binding')]
tests=[['cargo','test','--quiet','--locked','-p','pw-table-postgres','--test','asset_rows'],['cargo','test','--quiet','--locked','-p','pw-table-postgres','--test','compiled_assets']]
sources=sorted((root/'runtime').rglob('*.rs'));inventories={}
out=Path('/tmp/pw-protected-migration/upload-pg-rows-strict');out.mkdir(exist_ok=True)
def run(n,indices):
 passed=failed=0
 for i in indices:
  cmd=tests[i]+['--','--format=pretty','--test-threads=2'];child=producer_process.run(cmd,cwd=root,timeout=1200)
  path=out/f'run-{n:02}-{i:02}.log';path.write_text(child.stdout)
  result=cargo_assertion_results.validate(child.returncode,child.stdout,root=root,sources=sources,expected=inventories.get(i))
  path.with_suffix('.json').write_text(json.dumps({'command':cmd,'exit':child.returncode,'valid':result.valid,'passed':result.passed,'failed':result.failed,'sha256':hashlib.sha256(child.stdout.encode()).hexdigest()},indent=2))
  if not result.valid:raise RuntimeError(f'invalid native producer {path}: '+child.stdout[-3000:])
  inventories.setdefault(i,cargo_assertion_results.names(child.stdout));passed+=result.passed;failed+=result.failed
  mutant_incremental.bounded(cmd)
 return passed,failed
passed,failed=run(0,range(2));assert failed==0;print(f'baseline {passed} native assertions, complete inventories',flush=True)
for n,(label,path,before,after) in enumerate(mutants,1):
 original=path.read_bytes();s=original.decode();assert s.count(before)==1,(label,s.count(before))
 try:
  path.write_text(s.replace(before,after))
  passed,failed=run(n,[0] if n<len(mutants) else [1]);assert failed>0,f'survived: {label}'
  print(f'KILLED {n}/{len(mutants)}: {label}; {passed} pass/{failed} fail',flush=True)
 finally:path.write_bytes(original)
print(f'{len(mutants)}/{len(mutants)} killed; no invalid or stopped producer',flush=True)
