# Generic upload declarations: local prerequisite

Frozen source a1f4b2dc848ddb3502430cb89f4a46a741bdad58, clean before/after.
This closes declaration-stage checks only. Canonical feed/store still use legacy
layers; generic storage, lifetime, HTTP and full app-port gates remain pending.

Native checks: selected compiler 340, memory 134,
actual configured PostgreSQL 96, and just preflight passed. The
strict control baseline contains 95 native assertions across six complete
inventories. All 14 controls failed at native assertion sites; no stopped or
invalid producer counted. Controls cover resource grants, visibility, emitted
identities, idempotency, source/public/nominal asset associations, required
bindings, claim storage escape, pure URL output and the two moved old anchors.
The first attempted idempotency mutation only disabled acquire while leaving
release active due to operator precedence; it survived and was corrected before
the complete retained run. No production fix was inferred from that bad fault.

Generated attached is actually imported and its semantic ABI, read grant and
absence of resource grant asserted. This does not yet prove runtime hook ABI.
Memory/PostgreSQL still refuse asset tables without an actual upload binding.
Their guard-removal control is deferred until the otherwise-supported Ref.kind
wire encoding exists, to avoid treating a second unrelated rejection as proof.

Separate immutable review of declaration code and attached test repair is clear.
Reproduce using the byte-preserving frozen/control scripts under the
[evidence directory](../evidence/E14/protected-upload-declarations-2026-11-04/).
All raw per-binary outputs, native validator JSON and source/restoration guards
are gzip archived there; SHA256SUMS covers every archived input.
