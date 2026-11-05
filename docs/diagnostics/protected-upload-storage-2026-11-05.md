# Generation storage providers — 2026-11-05

Frozen source `20511c86822cd72b2716b0ac3e10f1847862bcfc`, clean before/after all gates. See the accepted
[storage decision](../DECISIONS/ADR-XXXX-protected-feed-store.md) and
[raw native output/receipts](../evidence/E14/protected-upload-storage-2026-11-05).

The shared Namespace/Storage boundary and concrete LocalBlobs/MemoryBlobs
implementations isolate physical generations. Fresh enrollment is exclusive;
existing enrollment verifies its retained marker. keep_new exclusively reserves
the generation directory before bytes/descriptor publication. Captured cleanup
removes its payload and retains reservation metadata, so orphan/collected or
even incomplete identities cannot be reused. Reads check namespace/descriptor,
canonical length/hash, and bound consumed bytes if a payload grows after its
metadata check. Legacy flat counting ignores generic storage.

Native physical-byte witnesses cover independent handles, missing/wrong-root
enrollment, namespace isolation, concurrent generation reservation, malformed
components/bytes, corruption/missing reads, retained incomplete/orphan/collected
identities, and stale captured cleanup versus a newer same-hash generation.
Review requested an independent work bound witness: the actual production read
helper consumes <=9 bytes from a 1MiB counting reader when expected length8;
an exact-length neighbor succeeds and relaxing its cap fails the native test.

Frozen gates: host **145 passed, 9 existing ignored**; selected server
**217 passed** (blob/upload/protected-migration/generic-table paths);
preflight **5,346 anchors**; strict **11/11** provider faults killed, complete
native blob inventory15, no invalid/stopped producer credit. Separate immutable
review is clear. The frozen runner preserves actual commands and selections.

**Limits:** arbitrary bytes in provider tests stand in for sanitized images.
The server's generic upload startup guards remain closed. There is no database
namespace enrollment, real claim/session settlement, authoritative HTTP, actual
PG backend-death fence or unknown-outcome acceptance yet. File/directory sync
requests were exercised on Linux; power-loss and other OS/filesystem durability
were not tested. No orphan collector or production conversion is introduced.
