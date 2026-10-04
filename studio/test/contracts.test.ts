import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { validateCompose } from '../src/modules/runtime/compose-policy.ts';
import type { ComposeModel, RuntimeTask } from '../src/modules/runtime/types.ts';
import { validatePayload, required } from '../app/desktop/request.ts';
import { StateStore } from '../src/modules/state/index.ts';

void test('resolved Compose policy rejects session-sharing and host exposure', () => {
  const task: RuntimeTask = {
    checkout: '/tmp/acme-checkout',
    ports: { REDIS_PORT: 12345 },
    port: 12345,
    url: 'http://127.0.0.1:12345',
    env: {},
    composeName: 'studio_project_task',
    composeOutput: '',
    composeStarted: false,
    process: null,
  };
  const config: ComposeModel = {
    volumes: { data: { name: 'studio_project_task_data' } },
    networks: { default: { name: 'studio_project_task_default' } },
    services: {
      redis: {
        ports: [{ host_ip: '127.0.0.1', published: '12345' }],
        volumes: [{ type: 'bind', source: '/tmp/acme-checkout/data' }],
      },
    },
  };
  assert.doesNotThrow(() => validateCompose(config, task));
  for (const change of [
    { volumes: { data: { name: 'shared' } } },
    { networks: { default: { name: 'studio_project_task_default', external: true } } },
    { services: { redis: { container_name: 'shared-redis' } } },
    { services: { redis: { network_mode: 'host' } } },
    { services: { redis: { privileged: true } } },
    { services: { redis: { ports: [{ host_ip: '0.0.0.0', published: 12345 }] } } },
    { services: { redis: { ports: [{ host_ip: '127.0.0.1', published: 54321 }] } } },
    {
      services: {
        redis: { volumes: [{ type: 'bind', source: '/tmp/acme-checkout-neighbor/data' }] },
      },
    },
  ])
    assert.throws(() => validateCompose({ ...config, ...change }, task));
});

void test('desktop payloads reject invalid field types and missing identities', () => {
  for (const value of [null, [], 'task', { sessionId: 123 }, { name: 'x'.repeat(1001) }])
    assert.throws(() => validatePayload(value));
  assert.equal(required(validatePayload({ sessionId: 'task' }), 'sessionId'), 'task');
  assert.throws(() => required(validatePayload({}), 'projectId'), /Missing projectId/);
});

void test('invalid persisted ownership is rejected without replacing the state file', async (t) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'studio-state-contract-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const project = {
    id: 'project',
    name: 'Acme',
    repository: root,
    activeSessionId: 'session',
    sessions: [{ id: 'session', name: 'Main', branch: 'main', checkout: root, managed: false }],
  };
  for (const value of [
    { version: 2, projects: [] },
    { version: 1, projects: [project], activeProjectId: 'other' },
    { version: 1, projects: [{ ...project, activeSessionId: 'other' }], activeProjectId: null },
    { version: 1, projects: [project, project], activeProjectId: null },
    { version: 1, projects: [{ ...project, repository: 'relative' }], activeProjectId: null },
  ]) {
    const source = JSON.stringify(value);
    await writeFile(path.join(root, 'state.json'), source);
    await assert.rejects(new StateStore(root).init());
    assert.equal(await readFile(path.join(root, 'state.json'), 'utf8'), source);
  }
});
