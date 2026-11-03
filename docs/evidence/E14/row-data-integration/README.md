# Local row/data integration receipts

Code/test source `497c0a6c73f86134746791faed5afd9b994023f3`.
Read `manifest.json` for commands, hashes and limits. Gzip files preserve the
raw bytes. All six final steps exited zero. The earlier diagnostic campaign
completed with two INVALID controls and has no acceptance credit.

CI began with only the explicitly hashed generated evidence inputs dirty.
These are local source receipts, not a hosted published-head landing gate.
The PR72 audit separately checks archive digests, 160 successful recipe
records, output availability and test-merge provenance; it does not reexecute
those hosted recipes or transfer their result to this source.
