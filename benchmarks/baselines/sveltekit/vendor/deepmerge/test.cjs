const assert = require('node:assert/strict');
const test = require('node:test');
const merge = require('./index.cjs');

for (const [name, target, source] of [
  ['null placeholder', {session: null}, JSON.parse('{"session":{"__proto__":{"admin":true}}}')],
  ['undefined placeholder', {session: undefined}, JSON.parse('{"session":{"__proto__":{"admin":true}}}')],
  ['primitive placeholder', {session: 1}, JSON.parse('{"session":{"__proto__":{"admin":true}}}')],
  ['null prototype target', Object.create(null), JSON.parse('{"__proto__":{"admin":true}}')],
  ['own proto in target', JSON.parse('{"__proto__":{"admin":true}}'), {}],
  ['nested own proto target', JSON.parse('{"session":{"__proto__":{"admin":true}}}'), {}],
]) {
  test(`prototype payload: ${name}`, () => {
    const result = merge(target, source);
    const object = name.includes('placeholder') || name.startsWith('nested') ? result.session : result;
    assert.equal(object.admin, undefined);
    assert.equal(Object.getPrototypeOf(object), Object.prototype);
    assert.equal(Object.hasOwn(object, '__proto__'), false);
    assert.equal({}.admin, undefined);
  });
}

for (const key of ['__proto__', 'constructor', 'prototype']) {
  for (const side of ['target', 'source']) {
    test(`reserved ${key} on ${side} is skipped before reading`, () => {
      const payload = {};
      Object.defineProperty(payload, key, {enumerable: true, get() {throw Error('reserved key was read');}});
      const result = side === 'target' ? merge(payload, {}) : merge({}, payload);
      assert.equal(Object.hasOwn(result, key), false);
    });
  }
}

test('ordinary configuration and nearby names remain own values and clone', () => {
  const source = {compilerOptions: {dev: true}, list: [{id: 2}], __proto__suffix: 3, prototypeName: 4};
  const result = merge({compilerOptions: {css: 'external'}, list: [{id: 1}]}, source);
  assert.deepEqual(result, {compilerOptions: {css: 'external', dev: true}, list: [{id: 1}, {id: 2}], __proto__suffix: 3, prototypeName: 4});
  assert.notEqual(result.compilerOptions, source.compilerOptions);
  assert.notEqual(result.list[1], source.list[0]);
});

test('Svelte array replacement option preserves source array identity', () => {
  const array = ['new'];
  const result = merge({extensions: ['old']}, {extensions: array}, {arrayMerge: (target, source) => source ?? target});
  assert.equal(result.extensions, array);
});

test('Rollup defaults are cloned without modifying the input', () => {
  const defaults = {extensions: ['.js', '.json'], mainFields: ['module', 'main'], moduleDirectories: ['node_modules']};
  const result = merge({}, defaults);
  assert.deepEqual(result, defaults);
  assert.notEqual(result.extensions, defaults.extensions);
  assert.deepEqual(defaults.extensions, ['.js', '.json']);
});

test('symbols, dates, regexes, functions and false clone keep upstream semantics', () => {
  const symbol = Symbol('config');
  const hidden = Symbol('hidden');
  const date = new Date(0), regex = /config/, fn = () => 1;
  const source = {[symbol]: {enabled: true}, date, regex, fn};
  Object.defineProperty(source, hidden, {value: true});
  const result = merge({}, source);
  assert.deepEqual(result[symbol], {enabled: true});
  assert.notEqual(result[symbol], source[symbol]);
  assert.equal(Object.hasOwn(result, hidden), false);
  assert.equal(result.date, date);
  assert.equal(result.regex, regex);
  assert.equal(result.fn, fn);
  assert.equal(merge({}, {config: source}, {clone: false}).config, source);
});

test('all, custom merge and mismatched arrays preserve upstream semantics', () => {
  assert.deepEqual(merge.all([{a: 1}, {b: 2}]), {a: 1, b: 2});
  assert.throws(() => merge.all({}), /first argument should be an array/);
  assert.deepEqual(merge({a: {x: 1}}, {a: {y: 2}}, {customMerge: key => key === 'a' ? () => ({custom: true}) : undefined}), {a: {custom: true}});
  assert.deepEqual(merge({a: 1}, [2]), [2]);
  assert.deepEqual(merge([2], {a: 1}), {a: 1});
});
