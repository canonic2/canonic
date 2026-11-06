import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const html = await readFile(new URL('./status.html', import.meta.url), 'utf8');
const source = html.match(/<script>([\s\S]*?)<\/script>/)[1];

function frame() {
  const sent = [];
  const listeners = {};
  const button = { disabled: true, addEventListener: (type, handler) => { button[type] = handler; } };
  const status = { textContent: '' };
  const parent = { postMessage: (message, origin) => sent.push({ message: JSON.parse(JSON.stringify(message)), origin }) };
  const window = { parent, addEventListener: (type, handler) => { listeners[type] = handler; } };
  vm.runInNewContext(source, { window, document: { getElementById: id => id === 'status' ? status : button } });
  const receive = (message, origin = 'https://host.example.com', sender = parent) => listeners.message({ source: sender, origin, data: { jsonrpc: '2.0', ...message } });
  return { sent, button, status, receive };
}

test('inline Apps document handshakes, receives results and refreshes through the host', async () => {
  assert.doesNotMatch(html, /(?:src|href)\s*=\s*["']https?:|\bimport\s|\bfetch\s*\(/);
  const ui = frame();
  assert.equal(ui.sent[0].message.method, 'ui/initialize');
  assert.deepEqual(ui.sent[0].message.params.appCapabilities, { availableDisplayModes: ['inline'] });
  ui.receive({ id: 1, result: { protocolVersion: '2026-01-26', hostCapabilities: { serverTools: {} } } });
  await Promise.resolve();
  assert.equal(ui.sent[1].message.method, 'ui/notifications/initialized');
  assert.equal(ui.sent[1].origin, 'https://host.example.com');
  assert.equal(ui.button.disabled, false);
  ui.receive({ method: 'ui/notifications/tool-result', params: { structuredContent: { service: 'Acme', status: 'healthy' } } });
  assert.equal(ui.status.textContent, 'Acme: healthy');
  const clicked = ui.button.click();
  assert.equal(ui.button.disabled, true);
  assert.deepEqual(ui.sent[2].message.params, { name: 'acme_refresh_status', arguments: {} });
  ui.receive({ id: 2, result: { content: [{ type: 'text', text: 'Acme is healthy.' }] } });
  await clicked;
  assert.equal(ui.status.textContent, 'Acme is healthy.');
  assert.equal(ui.button.disabled, false);
});

test('the view ignores other senders and origins and handles cancellation and host refusals', async () => {
  const ui = frame();
  const init = { id: 1, result: { protocolVersion: '2026-01-26', hostCapabilities: { serverTools: {} } } };
  ui.receive(init, 'https://host.example.com', {});
  await Promise.resolve();
  assert.equal(ui.sent.length, 1);
  ui.receive(init);
  await Promise.resolve();
  ui.receive({ method: 'ui/notifications/tool-result', params: { structuredContent: { service: 'Wrong', status: 'wrong' } } }, 'https://other.example.com');
  assert.notEqual(ui.status.textContent, 'Wrong: wrong');
  const clicked = ui.button.click();
  ui.receive({ id: 2, error: { code: -32602, message: 'Refresh refused.' } });
  await clicked;
  assert.equal(ui.status.textContent, 'Refresh refused.');
  assert.equal(ui.button.disabled, false);
  ui.receive({ method: 'ui/notifications/tool-cancelled' });
  assert.equal(ui.status.textContent, 'Status request cancelled.');
});

test('host requests with matching IDs leave pending refresh responses intact', async () => {
  const ui = frame();
  ui.receive({ id: 1, result: { protocolVersion: '2026-01-26', hostCapabilities: { serverTools: {} } } });
  await Promise.resolve();
  ui.receive({ method: 'ui/notifications/tool-result', params: { structuredContent: { service: 'Acme', status: 'healthy' } } });
  const clicked = ui.button.click();
  const id = ui.sent.at(-1).message.id;
  assert.equal(id, 2);
  ui.receive({ id, method: 'ping', params: {} });
  await Promise.resolve();
  assert.deepEqual(ui.sent.at(-1).message, { jsonrpc: '2.0', id, result: {} });
  assert.equal(ui.status.textContent, 'Acme: healthy');
  assert.equal(ui.button.disabled, true);
  // Unknown requests and malformed responses also cannot settle this refresh.
  ui.receive({ id, method: 'acme/unknown' });
  ui.receive({ id });
  ui.receive({ id, error: null });
  ui.receive({ id, result: {}, error: { code: -32603, message: 'Invalid envelope' } });
  await Promise.resolve();
  assert.equal(ui.button.disabled, true);
  ui.receive({ id, result: { structuredContent: { service: 'Acme', status: 'refreshed' } } });
  await clicked;
  assert.equal(ui.status.textContent, 'Acme: refreshed');
  assert.equal(ui.button.disabled, false);
});
