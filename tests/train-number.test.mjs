import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('train number is sent to backend and changing it invalidates selection', async () => {
  const page = await readFile(new URL('../src/routes/+page.svelte', import.meta.url), 'utf8');
  const api = await readFile(new URL('../src/lib/api.ts', import.meta.url), 'utf8');
  assert.ok(page.includes('diagramIndex, trainType, trainNumber'));
  assert.ok(page.includes('bind:value={trainNumber} oninput={resetFromRoute}'));
  assert.ok(api.includes('diagramIndex, trainType, trainNumber }'));
});
