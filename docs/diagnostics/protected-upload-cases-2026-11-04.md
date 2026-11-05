# Upload kind wire encoding: reviewed local prerequisite

Frozen source df206cfa002494c224611e178e5eb0764834b56c, clean before/after.
Host 119 (9 existing ignored), memory 140, actual
configured PostgreSQL 102, selected server 63 and
preflight 5,283 passed. Strict native complete inventories: row baseline48,
14/14 controls killed; PostgreSQL baseline7, 4/4 killed. No stopped process or
invalid producer counts. This closes the closed-case wire slice only: generic
upload storage/lifetime/HTTP and canonical feed/store migration remain pending.

Closed payload-free variants store canonical WIT tag strings, use actual
Val::Variant values, and compare tags consistently in host/memory/PostgreSQL
bytea keys, indexes, counts, windows and range marks. Nested fields round-trip.
AB/A_Z reverse declaration/source order relative to wire ab/a-z. The independent
conformance suite checks both adapters at its existing two sizes. Forged whole
inputs, unknown stored tags, unsupported schemas and ABI mismatches refuse.

The first source a5608eb6 passed its then-covered native/15 controls, but review
found a genuine untested upgrade path: adding Option<Kind> used migrate.rs's JSON
fallback while fresh creation used bytea. The retained actual-PG red assertion
shows json versus bytea. Shared shape::column_of now owns both mappings; the
upgrade regression preserves an old row as None, checks actual bytea, writes
Some through compiled code, and checks Some/None after reopening. A focused
mutation reinstates the old additive-only mapping and fails that assertion.
Separate immutable re-review is clear.

Review also prompted actual Wasmtime payload-bearing variant ABI versus closed
metadata with identical names/order and a valid payload-free neighbor, plus
empty/punctuation-only names. Separate faults isolate each. Unbound asset guards
now have otherwise-supported Ref.kind neighbors: removing either guard causes
the corresponding native test to fail, rather than being masked by unsupported
variant storage. The guards remain closed.

Reproduce from the frozen source with the gzip archived runner and control
helper in [the evidence directory](../evidence/E14/protected-upload-cases-2026-11-04/).
Per-binary outputs and validator JSON, source guards, raw native red witnesses
and immutable review summary are hash covered by SHA256SUMS. No upload lifecycle,
browser, whole final campaign, hosted landing or production conversion claim.
