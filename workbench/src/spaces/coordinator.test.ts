import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { parseSpaceRequest } from './coordinator.ts';
const require = createRequire(import.meta.url);
const server = require('../../server.js');

test('space coordination uses registered IDs, publishes complete context, and rejects cross-origin requests', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'canonic-canvas-space-'));
  writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n');
  const running = await server.start({ root, capture: { close() {} } });
  const origin = new URL(running.url).origin;
  try {
    const list = await (await fetch(origin + '/_workbench/spaces')).json();
    async function operation(body: unknown, from = origin) {
      return fetch(origin + '/_workbench/canvas/space', { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: from }, body: JSON.stringify(body) });
    }
    const catalog = await operation({ id: list.current, operation: 'config' });
    assert.equal(catalog.status, 200); assert.equal((await catalog.json()).config.name, 'Acme');
    assert.equal((await operation({ id: 'unregistered', operation: 'config' })).status, 404);
    assert.equal((await operation({ id: list.current, operation: 'config' }, 'https://example.com')).status, 403);
    const view = { version: 2, canvas: 'canvas', selected: 'a', text: 'All artboards', artboards: ['a','b'].map(id => ({ id, space: { id: list.current, name: 'Acme' }, src: id + '.html', size: { width: 393, height: 852 }, status: 'loading' })) };
    const posted = await operation({ id: list.current, operation: 'context', body: { client: 'canvas', view } });
    assert.equal(posted.status, 200);
    assert.deepEqual((await (await fetch(origin + '/_workbench/view')).json()).view, { ...view, artboards: view.artboards.map(b => ({ ...b, state: null, lens: null })) });
    await operation({ id: list.current, operation: 'context', body: { client: 'canvas', closed: true } });
    assert.equal((await (await fetch(origin + '/_workbench/view')).json()).open, false);
    assert.throws(() => parseSpaceRequest({ id: list.current, operation: 'arbitrary', url: 'https://example.com' }), /Invalid/);
    const html = await (await fetch(running.url)).text();
    assert.match(html, /id="stateButton"/);
    assert.match(html, /id="topbarOverflowMenu"/);
    assert.doesNotMatch(html, /canvasStage|duplicateArtboard|canvas-toolbar/);
  } finally { await running.close(); rmSync(root, { recursive: true, force: true }); }
});

test('authored Simulator mapping resolves a device name to its UDID and excludes undeclared peers', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'canonic-canvas-simulator-'));
  writeFileSync(path.join(root, 'page.html'), '<!doctype html><title>Acme</title>');
  writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\nimplementations:\n  ios:\n    kind: ios-simulator\ncollections:\n  - name: Pages\n    items:\n      - label: Page\n        src: page.html\n        implementations:\n          ios: Acme Phone\n');
  const calls: unknown[] = [];
  const running = await server.start({ root, capture: { close() {} }, simulators: () => [{ name: 'Acme Phone', udid: 'A' }, { name: 'Other Phone', udid: 'B' }], windowStream: { start(input: unknown) { calls.push(input); return { token: 'token' }; }, stop() {}, accept() { return false; }, close() {} } });
  try {
    async function start(udid: string) { return fetch(new URL('/_workbench/simulator/stream', running.url), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ implementation: 'ios', udid }) }); }
    assert.equal((await start('Acme Phone')).status, 200);
    assert.deepEqual(calls, [{ id: 'A', app: 'simulator', source: 'Acme Phone', codec: 'h264' }]);
    assert.equal((await start('B')).status, 403);
  } finally { await running.close(); rmSync(root, { recursive: true, force: true }); }
});
