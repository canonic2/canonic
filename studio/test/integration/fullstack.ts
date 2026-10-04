import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { Studio } from '../../src/application/studio.ts';
import { packageRoot } from '../../src/platform/paths.ts';
import { compose } from '../../src/modules/runtime/compose.ts';
import type { Session, Project } from '../../src/modules/state/types.ts';
import type { RuntimeTask } from '../../src/modules/runtime/types.ts';
import { asError, errorMessage } from '../../src/platform/errors.ts';

export interface FullstackFixture {
  manager: Studio;
  project: Project;
  a: Session;
  b: Session;
  ta: RuntimeTask;
  tb: RuntimeTask;
  artifacts: string;
}
interface StackReport {
  passed: boolean;
  root: string;
  platform: string;
  checks: string[];
  sessions: {
    id: string;
    branch: string;
    checkout: string;
    commit: string;
    ports: Record<string, number>;
    turboPid?: number;
    backendPid: number;
    composeName: string;
    container: string;
    volume: string;
    network: string;
  }[];
  docker?: string;
  compose?: string;
  error?: string;
  cleanup?: string | string[];
}

const exec = promisify(execFile);
const git = async (directory: string, args: string[]) =>
  (await exec('git', ['-C', directory, ...args])).stdout.trim();
const commit = async (directory: string, message: string) => {
  await git(directory, ['add', '.']);
  await git(directory, [
    '-c',
    'user.name=Acme',
    '-c',
    'user.email=dev@example.com',
    '-c',
    'commit.gpgsign=false',
    'commit',
    '-m',
    message,
  ]);
};
export async function eventually(
  check: () => boolean | Promise<boolean>,
  message: string,
  timeout = 30000,
) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  throw new Error(`Timed out: ${message}`);
}
const json = async (url: string, method = 'GET') => {
  const response = await fetch(url, { method, signal: AbortSignal.timeout(2000) });
  assert.equal(response.status, 200);
  return response.json();
};

export async function runFullstack({
  manager,
  browser,
}: { manager?: Studio; browser?: (fixture: FullstackFixture) => Promise<void> } = {}) {
  manager ||= await new Studio({
    root: path.join(packageRoot, '.studio', 'fullstack', String(Date.now())),
  }).init();
  const checks: string[] = [];
  const owned: RuntimeTask[] = [];
  const report: StackReport = {
    passed: false,
    root: manager.root,
    platform: `${process.platform}/${process.arch}`,
    checks,
    sessions: [],
  };
  const check = (label: string) => {
    checks.push(label);
    console.log(`Full stack: ${label}`);
  };
  const artifacts = path.join(packageRoot, 'artifacts');
  let failure: Error | undefined;
  let cleanupFailure: Error | undefined;
  await mkdir(artifacts, { recursive: true });
  try {
    report.docker = (
      await exec('docker', ['version', '--format', '{{.Server.Version}}'])
    ).stdout.trim();
    report.compose = (await exec('docker', ['compose', 'version', '--short'])).stdout.trim();
    const repository = path.join(manager.root, 'demo', 'fullstack');
    await cp(path.join(packageRoot, 'test/fixtures/fullstack'), repository, {
      recursive: true,
      filter: (source) => !['node_modules', '.pnpm-store'].includes(path.basename(source)),
    });
    await git(repository, ['init', '-b', 'main']);
    await commit(repository, 'Initialize Express Vite Turbo Redis fixture');
    const project = await manager.addProject(repository, 'Acme full stack');
    const [a, b] = await Promise.all([
      manager.createSession(project.id, 'Checkout A'),
      manager.createSession(project.id, 'Checkout B'),
    ]);
    for (const [session, title] of [
      [a, 'Branch A'],
      [b, 'Branch B'],
    ] as const) {
      await writeFile(path.join(session.checkout, 'title.txt'), title + '\n');
      await commit(session.checkout, title);
      assert.equal(await git(session.checkout, ['branch', '--show-current']), session.branch);
    }
    assert.notEqual(
      await git(a.checkout, ['rev-parse', 'HEAD']),
      await git(b.checkout, ['rev-parse', 'HEAD']),
    );
    assert.equal(await git(repository, ['status', '--porcelain']), '');
    check('two real branches and worktrees with independent commits; original checkout clean');
    console.log('Full stack: installing and starting both session stacks');
    const start = async (session: Session) => {
      const task = await manager.startService(session.id);
      owned.push(task);
      return task;
    };
    const [ta, tb] = await Promise.all([start(a), start(b)]);
    const allPorts = [...Object.values(ta.ports), ...Object.values(tb.ports)];
    assert.equal(new Set(allPorts).size, 6);
    assert.notEqual(ta.process!.child.pid, tb.process!.child.pid);
    for (const [session, task, title] of [
      [a, ta, 'Branch A'],
      [b, tb, 'Branch B'],
    ] as const) {
      const info = await json(`${task.url}/api/health`);
      assert.equal(info.sessionId, session.id);
      assert.equal(info.checkout, session.checkout);
      assert.equal(info.title, title);
      assert.equal(info.backendPort, task.ports.BACKEND_PORT);
      assert.equal(info.redisPort, task.ports.REDIS_PORT);
      assert.match(await (await fetch(task.url)).text(), /src\/main.ts/);
      assert.match(manager.logs(session.id), /@fixture\/backend/);
      assert.match(manager.logs(session.id), /@fixture\/frontend/);
      const containers = (await compose(task, ['ps', '--format', 'json']))
        .trim()
        .split('\n')
        .map((line) => JSON.parse(line));
      assert.equal(containers.length, 1);
      assert.equal(containers[0].Project, task.composeName);
      assert.equal(containers[0].Health, 'healthy');
      const container = JSON.parse((await exec('docker', ['inspect', containers[0].ID])).stdout)[0];
      const volume = container.Mounts.find(
        (mount: { Destination: string; Name: string }) => mount.Destination === '/data',
      );
      assert.equal(volume.Name, `${task.composeName}_data`);
      assert.deepEqual(Object.keys(container.NetworkSettings.Networks), [
        `${task.composeName}_default`,
      ]);
      report.sessions.push({
        id: session.id,
        branch: session.branch,
        checkout: session.checkout,
        commit: await git(session.checkout, ['rev-parse', 'HEAD']),
        ports: task.ports,
        turboPid: task.process!.child.pid,
        backendPid: info.pid,
        composeName: task.composeName,
        container: containers[0].ID,
        volume: volume.Name,
        network: `${task.composeName}_default`,
      });
    }
    check(
      'two Turbo process trees, Express backends, Vite frontends, healthy Compose projects and six distinct ports',
    );
    assert.equal(await manager.startService(a.id), ta);
    await json(`${ta.url}/api/increment`, 'POST');
    await json(`${ta.url}/api/increment`, 'POST');
    await json(`${tb.url}/api/increment`, 'POST');
    assert.equal((await json(`${ta.url}/api/health`)).counter, 2);
    assert.equal((await json(`${tb.url}/api/health`)).counter, 1);
    assert.equal(
      (await compose(ta, ['exec', '-T', 'redis', 'redis-cli', 'get', 'counter'])).trim(),
      '2',
    );
    assert.equal(
      (await compose(tb, ['exec', '-T', 'redis', 'redis-cli', 'get', 'counter'])).trim(),
      '1',
    );
    check(
      'Vite proxy reaches its own Express and Redis; separate persistent counters; duplicate start reuses owner',
    );
    await writeFile(path.join(a.checkout, 'title.txt'), 'Only A changed\n');
    assert.equal((await json(`${ta.url}/api/health`)).title, 'Only A changed');
    assert.equal((await json(`${tb.url}/api/health`)).title, 'Branch B');
    assert.equal(
      (await readFile(path.join(repository, 'title.txt'), 'utf8')).trim(),
      'Acme full stack',
    );
    check('live backend edits are isolated from the other branch and original checkout');
    if (browser) {
      await browser({ manager, project, a, b, ta, tb, artifacts });
      check(
        'real browser renders both frontends, counter requests and Vite HMR remain session-scoped',
      );
    }
    // Cleanup must use the launch record even if the recipe is edited meanwhile.
    const originalRecipe = await readFile(path.join(a.checkout, 'studio.config.json'), 'utf8');
    const originalCompose = await readFile(path.join(a.checkout, 'compose.yaml'), 'utf8');
    await writeFile(path.join(a.checkout, 'studio.config.json'), '{}');
    await writeFile(path.join(a.checkout, 'compose.yaml'), 'invalid: [');
    await manager.stopService(a.id);
    for (const name of ['FRONTEND_PORT', 'BACKEND_PORT', 'REDIS_PORT']) {
      const net = await import('node:net');
      await assert.rejects(
        new Promise<void>((resolve, reject) => {
          const socket = net.createConnection(ta.ports[name], '127.0.0.1');
          socket.once('connect', () => {
            socket.destroy();
            resolve();
          });
          socket.once('error', reject);
        }),
      );
    }
    assert.equal((await compose(ta, ['ps', '--all', '--quiet'])).trim(), '');
    assert.equal((await json(`${tb.url}/api/health`)).sessionId, b.id);
    check(
      'stopping A closes its Turbo descendants and container ports while B stays healthy despite recipe edits',
    );
    await writeFile(path.join(a.checkout, 'studio.config.json'), originalRecipe);
    await writeFile(path.join(a.checkout, 'compose.yaml'), originalCompose);
    const restarted = await manager.startService(a.id);
    owned.push(restarted);
    assert.equal((await json(`${restarted.url}/api/health`)).counter, browser ? 3 : 2);
    assert.equal((await json(`${tb.url}/api/health`)).counter, 1);
    check('restart restores only A’s Redis volume and leaves B’s counter intact');
    await manager.stopService(a.id);
    const failedRecipe = JSON.parse(originalRecipe);
    failedRecipe.runtime.command = ['$NODE', '-e', 'process.exit(7)'];
    await writeFile(path.join(a.checkout, 'studio.config.json'), JSON.stringify(failedRecipe));
    await assert.rejects(manager.startService(a.id), /Process exited \(7\)/);
    assert.equal(manager.runtimes.records.has(a.id), false);
    assert.equal((await compose(restarted, ['ps', '--all', '--quiet'])).trim(), '');
    assert.equal((await json(`${tb.url}/api/health`)).counter, 1);
    check('failed host startup cleans its already-running Compose stack; neighbor remains healthy');
    failedRecipe.runtime.compose.file = '../outside.yaml';
    failedRecipe.runtime.provision = undefined;
    await writeFile(path.join(a.checkout, 'studio.config.json'), JSON.stringify(failedRecipe));
    await assert.rejects(manager.startService(a.id), /inside the checkout/);
    check('Compose path escape is rejected');
    failedRecipe.runtime.compose.file = 'compose.yaml';
    await writeFile(path.join(a.checkout, 'studio.config.json'), JSON.stringify(failedRecipe));
    await writeFile(
      path.join(a.checkout, 'compose.yaml'),
      originalCompose.replace('  data:\n', '  data:\n    name: shared_counter\n'),
    );
    await assert.rejects(manager.startService(a.id), /scoped to the session/);
    await writeFile(
      path.join(a.checkout, 'compose.yaml'),
      originalCompose.replace('127.0.0.1:', '0.0.0.0:'),
    );
    await assert.rejects(manager.startService(a.id), /allocated loopback ports/);
    await writeFile(path.join(a.checkout, 'compose.yaml'), originalCompose);
    assert.equal((await json(`${tb.url}/api/health`)).counter, 1);
    check('shared volume names and public port bindings are rejected without affecting B');
    await writeFile(path.join(a.checkout, 'studio.config.json'), originalRecipe);
    await manager.shutdown();
    assert.equal((await compose(tb, ['ps', '--all', '--quiet'])).trim(), '');
    await assert.rejects(fetch(`${tb.url}/api/health`));
    check('full shutdown leaves no running fixture containers or frontend listeners');
    report.passed = true;
  } catch (error) {
    report.error = asError(error).stack;
    failure = asError(error);
  } finally {
    try {
      await manager.shutdown();
    } finally {
      const cleanupErrors = [];
      for (const task of new Map(owned.map((task) => [task.composeName, task])).values()) {
        try {
          await writeFile(
            path.join(artifacts, `${task.composeName}.log`),
            task.composeOutput + (task.process?.output || ''),
          );
          await compose(task, ['down', '--volumes', '--timeout', '3']);
          for (const [type, name] of [
            ['volume', `${task.composeName}_data`],
            ['network', `${task.composeName}_default`],
          ]) {
            const exists = await exec('docker', ['inspect', '--type', type, name]).then(
              () => true,
              (error) => {
                if (!(
                  error instanceof Error &&
                  'stderr' in error &&
                  typeof error.stderr === 'string' &&
                  /No such (object|volume|network)|network .+ not found/i.test(error.stderr)
                ))
                  throw error;
                return false;
              },
            );
            assert.equal(exists, false, `test ${type} was removed`);
          }
        } catch (error) {
          cleanupErrors.push(error);
        }
      }
      report.cleanup = cleanupErrors.length
        ? cleanupErrors.map((error) => errorMessage(error))
        : 'owned containers, networks, volumes and host process groups removed';
      if (cleanupErrors.length) report.passed = false;
      await writeFile(
        path.join(artifacts, browser ? 'fullstack-desktop.json' : 'fullstack.json'),
        JSON.stringify(report, null, 2),
      );
      if (cleanupErrors.length)
        cleanupFailure = new AggregateError(cleanupErrors, 'Fixture cleanup failed');
    }
  }
  if (failure && cleanupFailure)
    throw new AggregateError([failure, cleanupFailure], 'Fixture execution and cleanup failed', {
      cause: failure,
    });
  if (failure) throw failure;
  if (cleanupFailure) throw cleanupFailure;
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await runFullstack();
  console.log('STUDIO_FULLSTACK_PASSED');
}
