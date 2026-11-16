#!/usr/bin/env python3
"""Generic staging session/declaration/immutable-reader lifetime faults.

Every native producer must complete its full named inventory. Compile errors,
panics outside native assertions and stopped producers earn no fault credit.
The staging tests use no database and cannot skip on database availability. Source always restores.
"""
from pathlib import Path
import hashlib,importlib.util,json,os,sys
ROOT=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT/'scripts'))
import cargo_assertion_results,producer_process,mutant_incremental
MUTANTS = [
    ('collapses actual declaration keys within a session', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '    (session.to_string(), declaration.to_string())', '    (session.to_string(), { let _ = declaration; String::new() })'),
    ('selects the first declaration for every private preview', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '.get(&lease_key(session, &declared.name))', '.get(&lease_key(session, &self.declared[0].name))'),
    ('shows every attachments first declaration route', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     'src: format!("{}/{}", declared.route, lease.id),', 'src: format!("{}/{}", { let _ = declared; &self.declared[0].route }, lease.id),'),
    ('claims the first declaration instead of the requested one', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '.get_mut(&lease_key(session, declaration))', '.get_mut(&lease_key(session, &self.declared[0].name))'),
    ('ends claims using a different declaration key', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '.get(&lease_key(&claimed.session, &claimed.declaration))', '.get(&lease_key(&claimed.session, &self.declared[0].name))'),
    ('returns claims using a different declaration key', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '.get_mut(&lease_key(&claimed.session, &claimed.declaration))', '.get_mut(&lease_key(&claimed.session, &self.declared[0].name))'),
    ('prints private session bytes in claim diagnostics', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '.field("declaration", &self.declaration)', '.field("declaration", &self.declaration).field("session", &self.session)'),
    ('ignores expired unclaimed leases at lookup', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '        self.claimed || self.made.elapsed() < lease_for', '        let _ = lease_for; true'),
    ('expires a held claim before an explicit outcome', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '        self.claimed || self.made.elapsed() < lease_for', '        self.made.elapsed() < lease_for'),
    ('sweeps a held claim after its lifetime elapses', ROOT / "spikes/own-renderer/server/src/uploads.rs",
     '.filter(|(_, l)| !l.claimed && now.duration_since(l.made) >= self.lease_for)', '.filter(|(_, l)| now.duration_since(l.made) >= self.lease_for)'),
]

def main():
    spec=importlib.util.spec_from_file_location('legacy_uploads',ROOT/'scripts/uploads_mutations.py')
    legacy=importlib.util.module_from_spec(spec);spec.loader.exec_module(legacy)
    old=next(m for m in legacy.MUTANTS if m[0]=='a lease is shown to any session')
    mutants=MUTANTS+[(old[0],old[2],old[3],old[4])]
    filters=['uploads::generic_lease_tests::','uploads::tests::an_image_is_leased_to_its_session_and_shown_to_it_alone']
    commands=[['cargo','test','--quiet','--locked','-p','pw-dev-server','--bin','pw-dev-server',f,'--','--format=pretty','--test-threads=2'] for f in filters]
    sources=sorted((ROOT/'spikes/own-renderer/server/src').rglob('*.rs'))+sorted((ROOT/'runtime').rglob('*.rs'))
    output=Path(os.environ.get('PW_GENERIC_UPLOAD_LEASE_EVIDENCE','/tmp/pw-protected-migration/upload-lease-strict'))
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
