#!/usr/bin/env bash
# The large list in Svelte and in React, built for the head-to-head, with the
# versions the head-to-head's other apps use: Svelte and Vite from the SvelteKit
# baseline's install, React from the Next.js baseline's, esbuild from the
# workspace's. What it writes is committed, as TodoMVC's builds are.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
SVELTEKIT="$REPO/benchmarks/baselines/sveltekit/node_modules"
NEXT="$REPO/benchmarks/baselines/next-react/node_modules"
ESBUILD="$REPO/spikes/own-renderer/node_modules/.bin/esbuild"

for needed in "$SVELTEKIT/.bin/vite" "$NEXT/react/package.json" "$ESBUILD"; do
  if [ ! -e "$needed" ]; then
    echo "rows/build.sh: $needed is missing; install the baselines first" >&2
    exit 1
  fi
done

cd "$HERE/svelte"
ln -sfn "$SVELTEKIT" node_modules
rm -rf dist
"$SVELTEKIT/.bin/vite" build
rm -f node_modules

cd "$HERE/react"
rm -rf dist
mkdir -p dist
cp index.html dist/
NODE_PATH="$NEXT" "$ESBUILD" src/main.jsx --bundle --minify --format=esm --jsx=automatic \
  --define:process.env.NODE_ENV=\"production\" --outfile=dist/app.js --log-level=warning

echo "built the large list's Svelte and React"
