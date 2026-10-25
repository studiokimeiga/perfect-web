# Parallel mutant-program verification: worker-count trial

The full Pleris mutant-program gate was benchmarked on 2026-10-24 at
commit dc1084704ed709633af1f768b3ca53c47ae1989a using the same
344 source mutants, 13 program roots and 560 checker invocations.
The comparator refused missing, duplicate or unequal census results.
All three variants completed with zero failures and equal provenance.

| Worker count | Wall time |
|---:|---:|
| 2 | 576.552s |
| 4 | **519.998s** |
| 6 (prior default) | 666.655s |

Four workers were 22.0% faster than six in this single experiment,
while preserving the same checked mutation identities and results.
That is not proof that four is the global optimum across hardware/load
conditions. Do not quote the ratio as an end-to-end CI speedup.

The only production change here is the WORKERS constant changing from
six to four inside scripts/mutant_programs.py. Test programs, checker
algorithm, manifest sources, mutation rules and counts remain unchanged.
The regular just mutant-programs command still invokes the entire
census and must remain required in CI. No success can be credited if
mutants are skipped, ignored, cancelled or incompatible.

Full x64/ARM CI, security, Fallow, failure census, CodeQL and PR-aware
verification must pass at the actual patch SHA before adoption; compare
subsequent CI run times against the prior default and monitor regressions.

Benchmark: https://github.com/studiokimeiga/perfect-web/actions/runs/40137262208
