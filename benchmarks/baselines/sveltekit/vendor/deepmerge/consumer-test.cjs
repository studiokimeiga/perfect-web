const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {createRequire} = require('node:module');
const test = require('node:test');
const baseline = createRequire(path.resolve(__dirname, '../../package.json'));
const canonical = fs.readFileSync(path.join(__dirname, 'index.cjs'));

function resolvedMerge(consumer) {
  const fromConsumer = createRequire(consumer);
  const entry = fromConsumer.resolve('deepmerge');
  const metadata = fromConsumer('deepmerge/package.json');
  assert.equal(metadata.name, '@pleris/deepmerge');
  assert.equal(metadata.version, '4.3.1-pw.2');
  assert.equal(metadata.private, true);
  assert.deepEqual(fs.readFileSync(entry), canonical);
  const merge = fromConsumer('deepmerge');
  assert.equal(merge({session: null}, JSON.parse('{"session":{"__proto__":{"admin":true}}}')).session.admin, undefined);
  return merge;
}

test('the installed Svelte plugin resolves the actual fixed entry', () => {
  const merge = resolvedMerge(baseline.resolve('@sveltejs/vite-plugin-svelte'));
  const array = ['source'];
  assert.equal(merge({a: ['target']}, {a: array}, {arrayMerge: (target, source) => source ?? target}).a, array);
});

test('the installed Rollup resolver resolves the actual fixed entry', () => {
  const adapter = createRequire(baseline.resolve('@sveltejs/adapter-node/package.json'));
  const merge = resolvedMerge(adapter.resolve('@rollup/plugin-node-resolve'));
  assert.deepEqual(merge({}, {extensions: ['.js']}), {extensions: ['.js']});
});
