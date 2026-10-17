import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { create } from './index.ts';

test('mobile bridge delegates literal target arguments and reports invalid input and process failures', async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sandbox-bridge-acme's-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const binary = path.join(root, '.canonic/sandbox/bin/sandbox');
  fs.mkdirSync(path.dirname(binary), { recursive: true });
  fs.writeFileSync(binary, `#!${process.execPath}
const fs = require('node:fs');
fs.writeFileSync('invocation.json', JSON.stringify(process.argv.slice(2)));
if (process.argv.includes('swipe')) { console.error('SESSION_UNCERTAIN'); process.exit(1); }
console.log(JSON.stringify({sourceWidth:1206,sourceHeight:2622}));
`, { mode: 0o755 });
  const bridge = create(root);
  const identity = 'Acme $(touch injected)';
  assert.deepEqual(await bridge.screen(identity), { width: 1206, height: 2622 });
  const read = () => JSON.parse(fs.readFileSync(path.join(root, 'invocation.json'), 'utf8'));
  assert.deepEqual(read(), ['ios', '--id', `simulator:${identity}`, 'screenshot']);
  assert.equal(fs.existsSync(path.join(root, 'injected')), false);
  await bridge.input(identity, { action: 'tap', x: 0.5, y: 0.2 });
  const tap = read();
  await assert.rejects(bridge.input(identity, { action: 'tap', x: 2, y: 0.2 }), /Invalid simulator/);
  assert.deepEqual(read(), tap, 'Invalid input dispatches no process');
  await assert.rejects(bridge.input(identity, { action: 'swipe', fromX: 0.1, fromY: 0.1, toX: 0.5, toY: 0.5, duration: 0.3 }), /SESSION_UNCERTAIN/);
  assert.deepEqual(read().slice(-2), ['--duration-ms', '300']);
});
