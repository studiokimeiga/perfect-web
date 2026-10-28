# Temporary deepmerge compatibility fork

`@pleris/deepmerge@4.3.1-pw.1` is a private local fork, not an upstream release.
See ADR-0390 for scope and deletion condition. MIT copyright/license are in LICENSE.

Upstream: https://github.com/TehShrike/deepmerge at published npm release 4.3.1.
Registry metadata: https://registry.npmjs.org/deepmerge/4.3.1
Tarball: https://registry.npmjs.org/deepmerge/-/deepmerge-4.3.1.tgz

- Tarball SHA256: `4003a0a348276a95543fa969f561f6b5968629acbbb59d66018dd23e7b8f1d5c`
- Verified registry SHA512 integrity: `sha512-3sUqbMEc77XqpdNO7FRyRog+eW3ph+GYCbj+rK+uYyRMuwsVy0rMiVtPn+QJlKFvWP/1PYpapqYn0Me2knFn+A==`
- Original `package/dist/cjs.js` SHA256: `2e5160f84a1141bd927264afe4d15766f1e6cf07daef182b37ff53fea121de7d`
- Fixed `index.cjs` SHA256: `28e2b1701ac535288c7e7bfc4c3fab89bb9007cd6bd4daa87d2af7004c02d0f3`

`index.cjs` starts from the exact published CommonJS entry. Only mergeObject
changes: skip three reserved names in both loops before reads/callbacks, and
write own data properties with Object.defineProperty. The two helper functions
implement those checks. `index.d.ts` and LICENSE are byte-exact upstream files.
Package metadata deliberately identifies this local fork. No upstream UMD entry
or browser bundle is shipped; consumers use the callable CommonJS/default entry.

The same built-in tests ran against the unchanged published entry first:
17 tests, 8 passed, 9 failed. That RED and registry/tarball bytes are retained
in the PR validation packet. Reproduce current tests with:
`node --test benchmarks/baselines/sveltekit/vendor/deepmerge/test.cjs`.
Tests include source and target payloads, reserved getters, nearby names,
the consumers' array strategies, symbols, special values, cloning and custom API.
This does not protect arbitrary hostile callbacks, getters, proxies or cycles.
