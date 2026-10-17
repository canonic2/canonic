import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { start, available } from './index.ts';

const quiet = () => {};

function upstream(): Promise<http.Server> {
  const server = http.createServer((req, res) => {
    res.writeHead(200, { 'Content-Type': 'text/plain' });
    res.end(req.method + ' ' + req.url + ' ' + req.headers.host);
  });
  return new Promise(resolve => server.listen(0, '127.0.0.1', () => resolve(server)));
}

test('the front door forwards to the Node server and stops when closed', async t => {
  const node = await upstream();
  t.after(() => new Promise(resolve => node.close(resolve)));
  const door = await start({ upstream: (node.address() as AddressInfo).port, ports: [0], onChanged: quiet, onExit: quiet, diagnostic: quiet });
  assert.notEqual(door.port, (node.address() as AddressInfo).port);
  const answer = await fetch('http://127.0.0.1:' + door.port + '/_workbench/config?space=acme');
  assert.equal(answer.status, 200);
  assert.equal(await answer.text(), 'GET /_workbench/config?space=acme 127.0.0.1:' + door.port);
  await door.close();
  await assert.rejects(fetch('http://127.0.0.1:' + door.port + '/'));
  await assert.rejects(door.index({}, 1000), /not running/);
});

test('a front door that cannot start rejects instead of hanging', async () => {
  await assert.rejects(start({ upstream: 1, ports: [0], binary: '/nonexistent/workbench-server', onChanged: quiet, onExit: quiet, diagnostic: quiet }));
  assert.equal(available('/nonexistent/workbench-server'), false);
});

test('a resident index request that gets no answer times out', { skip: process.platform === 'win32' && 'needs a POSIX shell' }, async t => {
  const fs = await import('node:fs');
  const os = await import('node:os');
  const path = await import('node:path');
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-front-door-'));
  t.after(() => fs.rmSync(folder, { recursive: true, force: true }));
  const binary = path.join(folder, 'silent');
  // Starts like a front door, then reads its input without answering.
  fs.writeFileSync(binary, '#!/bin/sh\nprintf \'{"type":"listening","port":1}\\n\'\nexec cat > /dev/null\n', { mode: 0o755 });
  const door = await start({ upstream: 1, ports: [0], binary, onChanged: quiet, onExit: quiet, diagnostic: quiet });
  t.after(() => door.close());
  const started = Date.now();
  await assert.rejects(door.index({}, 200), /did not answer within 0.2 seconds/);
  assert.ok(Date.now() - started < 5000);
});
