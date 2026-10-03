import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('runtime management and frozen preview show dwell separately from depot travel', async () => {
  const page = await readFile(new URL('../src/routes/+page.svelte', import.meta.url), 'utf8');
  assert.ok(page.includes('<th>始発駅停車時間</th><td>{formatDwell(route.stations[0]?.dwell_millis)}'));
  assert.ok(page.includes('<th>出庫 → 始発駅所要時間</th><td>{outbound?.duration_label'));
  assert.ok(page.includes('<th>始発駅停車時間</th><td>{formatDwell(preview.stops[0]?.dwell_millis)}'));
  assert.ok(page.includes('<th>出庫 → 始発駅所要時間</th><td>{preview.outbound.duration_label}'));
});
