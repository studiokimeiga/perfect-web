"""Strict native provider controls; no crash/compile-failure credit."""
from pathlib import Path
import importlib.util,sys,json,hashlib
root=Path('/home/haki/code/pw-wt/data-path');sys.path.insert(0,str(root/'scripts'))
import cargo_assertion_results,producer_process,mutant_incremental
spec=importlib.util.spec_from_file_location('row',root/'scripts/row_ownership_mutations.py')
row=importlib.util.module_from_spec(spec);spec.loader.exec_module(row)
mutants=[m for m in row.MUTANTS if m[0].startswith('upload storage ')];assert len(mutants)==11
cmd=['cargo','test','--quiet','--locked','-p','pw-dev-server','--bin','pw-dev-server','blob::','--','--format=pretty','--test-threads=2']
sources=sorted((root/'spikes/own-renderer/server/src').rglob('*.rs'));inventory=None
out=Path('/tmp/pw-protected-migration/upload-storage-strict');out.mkdir(exist_ok=True)
def run(n):
 global inventory
 child=producer_process.run(cmd,cwd=root,timeout=1200)
 path=out/f'run-{n:02}.log';path.write_text(child.stdout)
 result=cargo_assertion_results.validate(child.returncode,child.stdout,root=root,sources=sources,expected=inventory)
 path.with_suffix('.json').write_text(json.dumps({'command':cmd,'exit':child.returncode,'valid':result.valid,'passed':result.passed,'failed':result.failed,'sha256':hashlib.sha256(child.stdout.encode()).hexdigest()},indent=2))
 if not result.valid:raise RuntimeError(f'invalid native producer {path}: '+child.stdout[-3000:])
 if inventory is None:inventory=cargo_assertion_results.names(child.stdout)
 mutant_incremental.bounded(cmd)
 return result
baseline=run(0);assert baseline.failed==0;print(f'baseline {baseline.passed} native assertions, complete inventory',flush=True)
for n,(label,path,before,after) in enumerate(mutants,1):
 original=path.read_bytes();s=original.decode();assert s.count(before)==1,(label,s.count(before))
 try:
  path.write_text(s.replace(before,after))
  result=run(n);assert result.failed>0,f'survived: {label}'
  print(f'KILLED {n}/{len(mutants)}: {label}; {result.passed} pass/{result.failed} fail',flush=True)
 finally:path.write_bytes(original)
print('11/11 killed; no invalid or stopped producer',flush=True)
