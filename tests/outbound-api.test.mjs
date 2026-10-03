import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';

const calls = [];
globalThis.__outboundTestInvoke = async (command, arguments_) => {
  calls.push({ command, arguments_ });
  return { valid: true, setting: { outbound_millis: 107000 }, duration_label: '107秒（1分47秒）' };
};
const source = (await readFile(new URL('../src/lib/api.ts', import.meta.url), 'utf8'))
  .replace("import { invoke } from '@tauri-apps/api/core';", 'const invoke = globalThis.__outboundTestInvoke;');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { api } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

test('measurement, manual input, and reload use independent settings commands, never OuDia save', async () => {
  await api.outboundStatus('s', 'r');
  await api.measureOutbound('s', 'r', '12:00:00', '+09:00');
  await api.manualOutbound('s', 'r', 107);
  assert.deepEqual(calls.map(c => c.command), ['get_outbound_status', 'measure_outbound_runtime', 'save_manual_outbound']);
  assert.deepEqual(calls[1].arguments_.input, { sessionId: 's', routeId: 'r', depotClock: '12:00:00', utcOffset: '+09:00' });
  assert.equal(calls[2].arguments_.input.seconds, 107);
});
