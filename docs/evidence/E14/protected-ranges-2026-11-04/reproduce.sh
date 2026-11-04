#!/bin/bash
set -euo pipefail
# COPY THIS DIRECTORY OUTSIDE THE REPOSITORY BEFORE CHECKING OUT OLDER SOURCE.
# Use an isolated checkout and preserve all existing dirty worktrees.
# These runners retain original local repo/setup paths. Use that environment
# or adjust only those local paths. Require an actual throwaway PostgreSQL URL.
records="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$records" in
 /home/haki/code/pw-wt/data-path/*) echo 'copy evidence outside checkout first' >&2; exit 1;;
esac
for name in ranges_frozen.sh ranges_review_frozen.sh; do
 gzip -dc "$records/$name.gz" > "$records/$name"
done
cd /home/haki/code/pw-wt/data-path
test -z "$(git status --porcelain)"
git checkout f4fc668f9d6783993203fe27e02180a3e377fc0e
bash "$records/ranges_frozen.sh"
git checkout 06a6ad18a190cf2b69df3809e20213e5b3e6baa3
bash "$records/ranges_review_frozen.sh" 06a6ad18a190cf2b69df3809e20213e5b3e6baa3
# All other range*.log files are earlier working diagnostics, including
# native reds, invalid fixtures and old-baseline failures. They are retained
# without being relabeled as frozen acceptance. Their one-shot input writers
# are archived byte-for-byte as gzip; do not run them over the final source.
