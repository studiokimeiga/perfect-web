#!/bin/bash
set -euo pipefail
# COPY THIS EVIDENCE DIRECTORY OUTSIDE THE REPOSITORY FIRST, then run it there.
# Checking out earlier source removes the in-repository evidence directory.
# Runners retain exact original local repo/setup/helper paths; use the recorded
# environment or adjust those local paths for an isolated checkout.
# Run in an isolated checkout with source /home/haki/pw-setup/env.sh available,
# and an actual throwaway PW_STORE_DATABASE_URL; runners require clean exact HEAD.
# The scripts retain the exact local paths used for these receipts.
# Preserve existing dirty worktrees. No production conversion is included.
# Each checkout below is a separate immutable source record.
records="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$records" in
  /home/haki/code/pw-wt/data-path/*) echo 'copy the evidence outside the checkout before reproducing' >&2; exit 1;;
esac
for name in windows_frozen.sh windows_review_frozen.sh windows_guidance_frozen.sh windows_bounded_controls.py; do
  gzip -dc "$records/$name.gz" > "$records/$name"
done
cd /home/haki/code/pw-wt/data-path
test -z "$(git status --porcelain)"
git checkout 79de0dd0970cf0477d5d67b814eb1038b7ceacfd
bash "$records/windows_frozen.sh" 79de0dd0970cf0477d5d67b814eb1038b7ceacfd
git checkout d417983f91c8eae565d139ccce37dea85feda4ab
bash "$records/windows_review_frozen.sh" d417983f91c8eae565d139ccce37dea85feda4ab
# bounded_checker_mutations.py ignores the argument here: this runs all 47.
git checkout 5b7f5be8e26f84852ebffce6a194acdd3f651943
bash "$records/windows_guidance_frozen.sh" 5b7f5be8e26f84852ebffce6a194acdd3f651943
# The guidance runner uses the archived windows_bounded_controls.py to select
# two moved controls with complete strict native assertion inventories.
