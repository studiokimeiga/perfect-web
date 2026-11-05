Copy these evidence inputs outside the repository before checking out older
source, and preserve all existing dirty worktrees. The archived helpers retain
the original local WSL checkout/setup and host helper paths; restore there or
adjust only local paths. Require a real throwaway PostgreSQL URL.

At `d8c634c9e513810dc5db300e0a44478718edd355`, run archived `relations_frozen.sh` with
`relations_strict_controls.py` restored at its referenced host path. At
`9451277ab47943424fed6c35d5ab20f142903847`, run `relations_reuse_frozen.sh 9451277ab47943424fed6c35d5ab20f142903847` with
`relations_reuse_controls.py` restored. Both runners require clean source and
check the source again at completion. Other archived one-shot writers describe
earlier inputs; do not apply them to final source. Raw native per-binary receipts
and strict JSON exit/inventory validation are gzip-compressed in strict folders.
Earlier working red/setup-failure logs are retained with their original names;
only frozen/reuse receipts are acceptance. No browser/whole-CI/app-port claim.
