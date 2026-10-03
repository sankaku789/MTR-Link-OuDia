import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';

test('dwell format includes minutes and seconds without dropping milliseconds', async () => {
  const source = await readFile(new URL('../src/lib/duration-format.ts', import.meta.url), 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
  const { formatDwell } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
  assert.equal(formatDwell(90000), '90000 ms（1分30秒）');
  assert.equal(formatDwell(30500), '30500 ms（0分30.5秒）');
  assert.equal(formatDwell(0), '0 ms（0分0秒）');
  assert.equal(formatDwell(undefined), 'なし');
});
