import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('../src/lib/route-sort.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 }
}).outputText;
const { sortRoutesForDisplay } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const route = (id, name) => ({ id, name });

test('Japanese route names are ascending', () => {
  assert.deepEqual(sortRoutesForDisplay([route('2', '東西線'), route('1', '中央線')]).map(r => r.name), ['中央線', '東西線']);
});

test('numbers in route names use natural ordering', () => {
  const routes = [route('3', '中央線10'), route('2', '中央線2'), route('1', '中央線')];
  assert.deepEqual(sortRoutesForDisplay(routes).map(r => r.name), ['中央線', '中央線2', '中央線10']);
});

test('equal names use route ID as a deterministic second key', () => {
  const routes = [route('B', '中央線'), route('A', '中央線')];
  assert.deepEqual(sortRoutesForDisplay(routes).map(r => r.id), ['A', 'B']);
});

test('base sensitivity ignores case', () => {
  assert.deepEqual(sortRoutesForDisplay([route('B', 'airport'), route('A', 'Airport')]).map(r => r.id), ['A', 'B']);
});

test('snapshot array and route objects remain unchanged', () => {
  const routes = Object.freeze([Object.freeze(route('2', '東西線')), Object.freeze(route('1', '中央線'))]);
  const sorted = sortRoutesForDisplay(routes);
  assert.notEqual(sorted, routes);
  assert.equal(routes[0].id, '2');
  assert.equal(sorted[0], routes[1]);
});
