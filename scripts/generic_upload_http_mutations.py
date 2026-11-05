#!/usr/bin/env python3
"""Generic public upload authority + real PG/local-file/HTTP composition faults.

Every native producer must complete its full named inventory. Compile errors,
panics outside native assertions and stopped producers earn no fault credit.
Run with the actual throwaway PW_TABLES_DATABASE_URL. Source always restores.
"""
from pathlib import Path
import hashlib,importlib.util,json,os,sys
ROOT=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT/'scripts'))
import cargo_assertion_results,producer_process,mutant_incremental
MUTANTS = [
    ('bypasses current generation authority', ROOT / 'spikes/own-renderer/server/src/uploads.rs',
     '        if let Some(read) = self.generations.get() {', '        if false && let Some(read) = self.generations.get() {'),
    ('serves an unreferenced hash as cache valid', ROOT / 'spikes/own-renderer/server/src/uploads.rs',
     '                Ok(None) => return Answer::text(404, "not found"),', '                Ok(None) => return Answer::new(304,"image/png",Vec::new()),'),
    ('turns a LIVE storage error into not found', ROOT / 'spikes/own-renderer/server/src/uploads.rs',
     '                    eprintln!("pw dev server: reading a current upload generation: {why}");\n                    return Answer::text(503, "unavailable");', '                    eprintln!("pw dev server: reading a current upload generation: {why}");\n                    return Answer::text(404, "not found");'),
    ('answers validators before authority and file verification', ROOT / 'spikes/own-renderer/server/src/uploads.rs',
     '        if let Some(read) = self.generations.get() {', '        if let Some(read) = self.generations.get() {\n            if header(headers,"if-none-match").is_some() { return Answer::new(304,"image/png",Vec::new()); }'),
    ('accepts file kind differing from its descriptor', ROOT / 'spikes/own-renderer/server/src/uploads.rs',
     '                    m.kind.word() == ext', '                    true'),
    ('accepts measured dimensions differing from its descriptor', ROOT / 'spikes/own-renderer/server/src/uploads.rs',
     '                        && m.width == descriptor.reference.width\n                        && m.height == descriptor.reference.height', '                        && true'),
    ('uses the first declarations authority for every serving route', ROOT / 'spikes/own-renderer/server/src/uploads.rs',
     'read(&declared.name, key.hex(), ext)', 'read("PostImage", key.hex(), ext)'),
    ('releases database exclusion before the real file checkpoint', ROOT / 'runtime/pw-table-postgres/src/lib.rs',
     '        let _ = assets::collect_committed(&inner, open);', '        if let Some(key) = open.lifetime.take() { if let Some(lease) = open.lease.as_mut() { inner.unfence(lease,key); } }\n        let _ = assets::collect_committed(&inner, open);'),
    ('unlinks a newer same hash physical generation', ROOT / 'spikes/own-renderer/server/src/blob/assets.rs',
     '    fn collect_generation(&self, namespace: &Namespace, descriptor: &Descriptor) -> io::Result<()> {\n        let dir = self.checked_generation(namespace, descriptor)?;', '    fn collect_generation(&self, namespace: &Namespace, descriptor: &Descriptor) -> io::Result<()> {\n        let mut newer=descriptor.clone(); newer.reference.generation="c".repeat(32); let descriptor=&newer;\n        let dir = self.checked_generation(namespace, descriptor)?;'),
]
def main():
    assert os.environ.get('PW_TABLES_DATABASE_URL'),'actual throwaway PostgreSQL required'
    spec=importlib.util.spec_from_file_location('legacy_uploads',ROOT/'scripts/uploads_mutations.py')
    legacy=importlib.util.module_from_spec(spec);spec.loader.exec_module(legacy)
    old=next(m for m in legacy.MUTANTS if m[0]=='a layer that cannot say whether a post names a blob is taken to say yes')
    mutants=MUTANTS+[(old[0],old[2],old[3],old[4])]
    filters=['tests::upload_generations::','uploads::tests::a_lease_is_claimed_once_and_ended_once']
    commands=[['cargo','test','--quiet','--locked','-p','pw-dev-server','--bin','pw-dev-server',f,'--','--format=pretty','--test-threads=2'] for f in filters]
    sources=sorted((ROOT/'spikes/own-renderer/server/src').rglob('*.rs'))+sorted((ROOT/'runtime').rglob('*.rs'))
    output=Path(os.environ.get('PW_GENERIC_UPLOAD_HTTP_EVIDENCE','/tmp/pw-protected-migration/upload-http-strict'))
    output.mkdir(parents=True,exist_ok=True)
    inventories={}
    def run(n,indices):
        passed=failed=0
        for i in indices:
            command=commands[i];child=producer_process.run(command,cwd=ROOT,timeout=600)
            path=output/f'run-{n:02}-{i:02}.log';path.write_text(child.stdout)
            result=cargo_assertion_results.validate(child.returncode,child.stdout,root=ROOT,sources=sources,expected=inventories.get(i))
            path.with_suffix('.json').write_text(json.dumps({'command':command,'exit':child.returncode,'valid':result.valid,'passed':result.passed,'failed':result.failed,'sha256':hashlib.sha256(child.stdout.encode()).hexdigest()},indent=2))
            if not result.valid: raise RuntimeError(f'invalid native producer {path}: '+child.stdout[-4000:])
            inventories.setdefault(i,cargo_assertion_results.names(child.stdout));passed+=result.passed;failed+=result.failed
            mutant_incremental.bounded(command)
        return passed,failed
    passed,failed=run(0,range(2));assert failed==0
    print(f'baseline {passed} native assertions, complete inventories',flush=True)
    for n,(label,path,before,after) in enumerate(mutants,1):
        original=path.read_bytes();s=original.decode();assert s.count(before)==1,(label,s.count(before))
        try:
            path.write_text(s.replace(before,after))
            passed,failed=run(n,[0] if n<len(mutants) else [1]);assert failed>0,f'survived: {label}'
            print(f'KILLED {n}/{len(mutants)}: {label}; {passed} pass/{failed} fail',flush=True)
        finally:path.write_bytes(original)
    print(f'{len(mutants)}/{len(mutants)} killed; no invalid or stopped producer',flush=True)
    return 0
if __name__=='__main__':sys.exit(main())
