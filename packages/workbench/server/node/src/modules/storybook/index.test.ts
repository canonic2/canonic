import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { ownedStorybook, storybookBin } from './index.ts';

/* A project with a stand-in `storybook` package: its script listens on the
   port it is given, answers /index.json, and reports what it was asked. */
function project(t: { after(fn: () => void): void }, options: { rejectPreviewOnly?: boolean } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'wb-storybook-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const pkg = path.join(root, 'node_modules/storybook');
  fs.mkdirSync(path.join(pkg, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(root, 'package.json'), '{"name":"acme","private":true}');
  fs.writeFileSync(path.join(pkg, 'package.json'), JSON.stringify({ name: 'storybook', version: '10.0.0', bin: './bin/storybook.cjs' }));
  fs.writeFileSync(path.join(pkg, 'bin/storybook.cjs'), `
    const http = require('node:http');
    const fs = require('node:fs');
    const args = process.argv.slice(2);
    if (${!!options.rejectPreviewOnly} && args.includes('--preview-only')) { console.error("error: unknown option '--preview-only'"); process.exit(1); }
    const port = Number(args[args.indexOf('-p') + 1]);
    fs.appendFileSync(${JSON.stringify(path.join(root, 'runs.log'))}, JSON.stringify({ args, port, pid: process.pid }) + '\\n');
    http.createServer((req, res) => {
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ v: 5, entries: {}, port: http.STATUS_CODES ? port : 0, url: req.url }));
    }).listen(port, '127.0.0.1');
  `);
  const runs = () => fs.existsSync(path.join(root, 'runs.log'))
    ? fs.readFileSync(path.join(root, 'runs.log'), 'utf8').trim().split('\n').map(line => JSON.parse(line)) : [];
  return { root, runs };
}

function indexOver(socket: string): Promise<{ port: number }> {
  return new Promise((resolve, reject) => {
    http.get({ socketPath: socket, path: '/index.json', headers: { host: 'localhost' } }, res => {
      let body = '';
      res.on('data', chunk => { body += chunk; });
      res.on('end', () => resolve(JSON.parse(body)));
    }).on('error', reject);
  });
}

function portAnswers(port: number): Promise<boolean> {
  return new Promise(resolve => {
    const socket = net.connect(port, '127.0.0.1');
    socket.on('connect', () => { socket.destroy(); resolve(true); });
    socket.on('error', () => resolve(false));
  });
}

test('runs the project’s Storybook on a local socket, told the proxy’s port, and takes no port itself', async t => {
  const { root, runs } = project(t);
  const storybook = ownedStorybook({ root, timeout: 20000 });
  t.after(() => storybook.close());
  const socket = await storybook.socket(47123);
  assert.equal((await indexOver(socket)).port, 47123);
  assert.equal(await portAnswers(47123), false, 'nothing listens on the port it was given');
  assert.equal(await storybook.socket(47123), socket, 'one Storybook, started once');
  const [run] = runs();
  assert.equal(runs().length, 1);
  assert.ok(run.args.includes('--preview-only'));
  assert.ok(run.args.includes('--exact-port'));
  assert.ok(run.args.includes('--no-open'));
});

test('starts again on the next request after the Storybook exits, and stays stopped once closed', async t => {
  const { root, runs } = project(t);
  const storybook = ownedStorybook({ root, timeout: 20000 });
  t.after(() => storybook.close());
  const first = await storybook.socket(47124);
  process.kill(runs()[0].pid);
  await new Promise(resolve => setTimeout(resolve, 300));
  const second = await storybook.socket(47124);
  assert.equal(runs().length, 2, 'a second Storybook was started');
  assert.equal((await indexOver(second)).port, 47124);
  assert.equal(fs.existsSync(first), false, 'the old socket is gone');
  await storybook.close();
  await assert.rejects(storybook.socket(47124), /stopped this Storybook/);
});

test('falls back to a full dev server where --preview-only is unknown', async t => {
  const { root, runs } = project(t, { rejectPreviewOnly: true });
  const storybook = ownedStorybook({ root, timeout: 20000 });
  t.after(() => storybook.close());
  const socket = await storybook.socket(47125);
  assert.equal((await indexOver(socket)).port, 47125);
  assert.equal(runs().length, 1, 'the rejected launch never got to listen');
  assert.ok(!runs()[0].args.includes('--preview-only'));
});

test('names a project without Storybook installed', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'wb-storybook-none-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  assert.throws(() => storybookBin(root), /Storybook isn’t installed in .*set url to a Storybook you run/);
});
