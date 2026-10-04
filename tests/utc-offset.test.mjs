import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';

test('PC timezone offset is formatted with the correct sign and minute precision', async () => {
  const source = await readFile(new URL('../src/lib/utc-offset.ts', import.meta.url), 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
  const { formatUtcOffset } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
  assert.equal(formatUtcOffset(-540), '+09:00');
  assert.equal(formatUtcOffset(300), '-05:00');
  assert.equal(formatUtcOffset(-345), '+05:45');
  assert.equal(formatUtcOffset(0), '+00:00');
});

test('trial departure and UTC offset are separate field rows with automatic timezone initialization', async () => {
  const page = await readFile(new URL('../src/routes/+page.svelte', import.meta.url), 'utf8');
  assert.match(page, /utcOffset = formatUtcOffset\(new Date\(\)\.getTimezoneOffset\(\)\)/);
  assert.match(page, /<div class="fields">\s*<label>試験列車の車庫発/);
  assert.match(page, /<div class="fields">\s*<label>Minecraft端末のUTCオフセット/);
});
