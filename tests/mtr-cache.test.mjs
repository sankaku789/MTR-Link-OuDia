import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('forced refresh explains the MTR API cache and retries until the route data changes', async () => {
  const page = await readFile(new URL('../src/routes/+page.svelte', import.meta.url), 'utf8');
  assert.ok(page.includes('onclick={refreshSnapshot}'));
  assert.ok(page.includes('MTR APIは駅・路線情報を最大30秒キャッシュします'));
  assert.ok(page.includes('snapshotDataKey(value) === previous'));
  assert.ok(page.includes('Date.now() - startedAt < 31_000'));
  assert.ok(page.includes('api.snapshot(id, baseUrl, dimension)'));
});
