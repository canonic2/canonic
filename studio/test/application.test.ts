import { errorCode } from '../src/platform/errors.ts';
import test from 'node:test';
import type { TestContext } from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { Studio } from '../src/application/studio.ts';
import { seedDemo } from './helpers/projects.ts';
import { freePort } from '../src/platform/processes.ts';

const exec = promisify(execFile);
async function fixture(t: TestContext) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'studio-test-'));
  const manager = await new Studio({ root }).init();
  t.after(async () => {
    await manager.shutdown();
    await rm(root, { recursive: true, force: true });
  });
  await seedDemo(manager);
  return manager;
}

void test('project selection remembers each project’s session and survives reopening', async (t) => {
  const manager = await fixture(t);
  const [store, site] = manager.snapshot().projects;
  assert.equal(store.sessions.length, 2);
  const cart = store.sessions[1];
  await manager.select(store.id, cart.id);
  await manager.select(site.id);
  await manager.select(store.id);
  assert.equal(store.activeSessionId, cart.id);
  const reopened = await new Studio({ root: manager.root }).init();
  assert.equal(reopened.snapshot().activeProjectId, store.id);
  assert.equal(reopened.snapshot().projects[0].activeSessionId, cart.id);
  await assert.rejects(manager.select(store.id, site.sessions[0].id), /does not belong/);
});

void test('concurrent task creation uses separate branches and checkouts without touching main', async (t) => {
  const manager = await fixture(t);
  const project = manager.snapshot().projects[0];
  const [a, b] = await Promise.all([
    manager.createSession(project.id, 'Same name'),
    manager.createSession(project.id, 'Same name'),
  ]);
  assert.notEqual(a.checkout, b.checkout);
  assert.notEqual(a.branch, b.branch);
  await writeFile(path.join(a.checkout, 'title.txt'), 'Changed only in A');
  assert.equal(await readFile(path.join(b.checkout, 'title.txt'), 'utf8'), 'Acme Store\n');
  assert.equal(await readFile(path.join(project.repository, 'title.txt'), 'utf8'), 'Acme Store\n');
  const { stdout } = await exec('git', ['-C', a.checkout, 'branch', '--show-current']);
  assert.equal(stdout.trim(), a.branch);
  await assert.rejects(manager.createSession(project.id, 'Bad ref', '--help'), /Command failed/);
});

void test('parallel servers have distinct ports, data, ownership, and independent stop/restart', async (t) => {
  const manager = await fixture(t);
  const [a, b] = manager.snapshot().projects[0].sessions;
  const [first, second] = await Promise.all([
    manager.startService(a.id),
    manager.startService(b.id),
  ]);
  assert.notEqual(first.port, second.port);
  assert.equal((await (await fetch(`${first.url}/health`)).json()).sessionId, a.id);
  assert.equal((await (await fetch(`${second.url}/health`)).json()).sessionId, b.id);
  await fetch(first.url);
  await fetch(first.url);
  await fetch(second.url);
  assert.equal(await readFile(path.join(a.checkout, '.visits'), 'utf8'), '2');
  assert.equal(await readFile(path.join(b.checkout, '.visits'), 'utf8'), '1');
  assert.equal(await manager.startService(a.id), first, 'duplicate starts reuse their owner');
  await manager.stopService(a.id);
  await assert.rejects(fetch(`${first.url}/health`));
  assert.equal((await fetch(`${second.url}/health`)).status, 200);
  assert.equal(manager.snapshot().projects[0].sessions[0].serviceStatus, 'stopped');
  const restarted = await manager.startService(a.id);
  const html = await (await fetch(restarted.url)).text();
  assert.match(html, /Visits in this checkout: 3/);
  assert.match(manager.logs(a.id), /READY/);
});

void test('failed service starts are cleaned up and leave another session healthy', async (t) => {
  const manager = await fixture(t);
  const [a, b] = manager.snapshot().projects[0].sessions;
  const other = await manager.startService(b.id);
  await writeFile(
    path.join(a.checkout, 'studio.config.json'),
    JSON.stringify({ version: 1, runtime: { command: ['$NODE', '-e', 'process.exit(7)'] } }),
  );
  await assert.rejects(manager.startService(a.id), /Process exited \(7\)/);
  assert.equal(manager.runtimes.records.has(a.id), false);
  assert.equal((await fetch(`${other.url}/health`)).status, 200);
  assert.match(manager.snapshot().projects[0].sessions[0].error!, /Process exited/);
});

void test('stop shuts down descendant servers launched by a wrapper script', async (t) => {
  const manager = await fixture(t);
  const session = manager.snapshot().projects[0].sessions[0];
  await writeFile(
    path.join(session.checkout, 'wrapper.ts'),
    `import { spawn } from 'node:child_process';
spawn(process.execPath, ['server.ts'], { stdio: 'inherit' });
setInterval(() => {}, 1000);
`,
  );
  await writeFile(
    path.join(session.checkout, 'studio.config.json'),
    JSON.stringify({ version: 1, runtime: { command: ['$NODE', 'wrapper.ts'] } }),
  );
  const runtime = await manager.startService(session.id);
  assert.equal((await fetch(`${runtime.url}/health`)).status, 200);
  await manager.stopService(session.id);
  await assert.rejects(fetch(`${runtime.url}/health`));
});

void test('closing a project stops its sessions, preserves worktrees, and reopens the same identity', async (t) => {
  const manager = await fixture(t);
  const [project, other] = manager.snapshot().projects;
  const a = await manager.startService(project.sessions[0].id);
  const b = await manager.startService(other.sessions[0].id);
  await manager.closeProject(project.id);
  await assert.rejects(fetch(`${a.url}/health`));
  assert.equal((await fetch(`${b.url}/health`)).status, 200);
  assert.equal(manager.snapshot().projects.length, 1);
  await readFile(path.join(project.sessions[1].checkout, 'title.txt'));
  const reopened = await manager.addProject(project.repository);
  assert.equal(reopened.id, project.id);
  assert.equal(reopened.sessions.length, project.sessions.length);
});

void test('failed state persistence rolls back the new Git branch and worktree', async (t) => {
  const manager = await fixture(t);
  const project = manager.snapshot().projects[0];
  const before = (await exec('git', ['-C', project.repository, 'worktree', 'list', '--porcelain']))
    .stdout;
  await mkdir(path.join(manager.root, 'state.json.tmp'));
  await assert.rejects(manager.createSession(project.id, 'Will roll back'), /EISDIR/);
  const after = (await exec('git', ['-C', project.repository, 'worktree', 'list', '--porcelain']))
    .stdout;
  assert.equal(after, before);
  assert.equal(manager.snapshot().projects[0].sessions.length, project.sessions.length);
});

void test('different session runtimes start independently while repeated starts share ownership', async (t) => {
  const manager = await fixture(t);
  const project = manager.snapshot().projects[0];
  const [a, b] = project.sessions;
  const recipe = JSON.parse(await readFile(path.join(a.checkout, 'studio.config.json'), 'utf8'));
  recipe.runtime.provision = ['$NODE', '-e', 'setTimeout(() => {}, 2000)'];
  await writeFile(path.join(a.checkout, 'studio.config.json'), JSON.stringify(recipe));
  const slow = manager.startService(a.id);
  const duplicate = manager.startService(a.id);
  const fast = await manager.startService(b.id);
  assert.equal(
    manager.runtimes.records.has(a.id),
    false,
    'B starts before A finishes provisioning',
  );
  assert.equal((await fetch(`${fast.url}/health`)).status, 200);
  assert.equal(await duplicate, await slow);
});

void test('timed-out provisioning stops its descendant server without affecting a neighbor', async (t) => {
  const manager = await fixture(t);
  const [a, b] = manager.snapshot().projects[0].sessions;
  const neighbor = await manager.startService(b.id);
  const port = await freePort();
  const proof = path.join(a.checkout, 'provision-child.pid');
  const childScript = `import http from 'node:http'; import { writeFileSync } from 'node:fs';
http.createServer((request, response) => response.end('provision child')).listen(${port}, '127.0.0.1', () => writeFileSync(${JSON.stringify(proof)}, String(process.pid)));`;
  await writeFile(
    path.join(a.checkout, 'provision.ts'),
    `import { spawn } from 'node:child_process';
spawn(process.execPath, ['--input-type=module', '-e', ${JSON.stringify(childScript)}], { stdio: 'inherit' });
setInterval(() => {}, 1000);`,
  );
  const child = { pid: undefined as number | undefined };
  t.after(() => {
    if (child.pid) {
      try {
        process.kill(child.pid, 'SIGKILL');
      } catch (error) {
        if (errorCode(error) !== 'ESRCH') throw error;
      }
    }
  });
  await writeFile(
    path.join(a.checkout, 'studio.config.json'),
    JSON.stringify({
      version: 1,
      runtime: {
        provision: ['$NODE', 'provision.ts'],
        provisionTimeoutMs: 1000,
        command: ['$NODE', 'server.ts'],
      },
    }),
  );
  await assert.rejects(manager.startService(a.id));
  child.pid = Number(await readFile(proof, 'utf8'));
  await assert.rejects(fetch(`http://127.0.0.1:${port}`), 'provision descendants must be stopped');
  assert.equal(manager.runtimes.records.has(a.id), false);
  assert.equal((await fetch(`${neighbor.url}/health`)).status, 200);
});

void test('completed and failed provision wrappers cannot leave descendant servers behind', async (t) => {
  const manager = await fixture(t);
  const session = manager.snapshot().projects[0].sessions[0];
  for (const exitCode of [0, 7]) {
    const port = await freePort();
    const proof = path.join(session.checkout, `provision-${exitCode}.pid`);
    const script = `import http from 'node:http'; import { writeFileSync } from 'node:fs';
http.createServer((request, response) => response.end('child')).listen(${port}, '127.0.0.1', () => { writeFileSync(${JSON.stringify(proof)}, String(process.pid)); process.send('ready'); });`;
    await writeFile(
      path.join(session.checkout, 'provision.ts'),
      `import { spawn } from 'node:child_process';
const child = spawn(process.execPath, ['--input-type=module', '-e', ${JSON.stringify(script)}], { stdio: ['ignore', 'inherit', 'inherit', 'ipc'] });
child.once('message', () => process.exit(${exitCode}));`,
    );
    const child = { pid: undefined as number | undefined };
    t.after(() => {
      if (child.pid) {
        try {
          process.kill(child.pid, 'SIGKILL');
        } catch (error) {
          if (errorCode(error) !== 'ESRCH') throw error;
        }
      }
    });
    await writeFile(
      path.join(session.checkout, 'studio.config.json'),
      JSON.stringify({
        version: 1,
        runtime: {
          provision: ['$NODE', 'provision.ts'],
          command: ['$NODE', 'server.ts'],
        },
      }),
    );
    if (exitCode)
      await assert.rejects(manager.startService(session.id), /Provision command exited \(7\)/);
    else await manager.startService(session.id);
    child.pid = Number(await readFile(proof, 'utf8'));
    await assert.rejects(fetch(`http://127.0.0.1:${port}`));
    await manager.stopService(session.id);
  }
});

void test('canonical repository reopening and snapshot edits cannot duplicate or mutate project state', async (t) => {
  const manager = await fixture(t);
  const project = manager.snapshot().projects[0];
  const nested = path.join(project.repository, 'nested');
  await mkdir(nested);
  assert.equal((await manager.addProject(nested)).id, project.id);
  const snapshot = manager.snapshot();
  snapshot.projects[0].sessions[0].name = 'Changed snapshot';
  snapshot.projects.length = 0;
  assert.equal(manager.snapshot().projects.length, 2);
  assert.equal(manager.snapshot().projects[0].sessions[0].name, 'Main');
});
