import test from 'node:test';
import assert from 'node:assert/strict';
import { validateProject, loadProject } from '../src/modules/configuration/index.ts';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';

void test('project configuration has defaults and never mutates its input', () => {
  const input = { version: 1, runtime: { command: ['$NODE', 'server.ts'] } };
  const value = validateProject(input);
  assert.ok(value.runtime);
  assert.deepEqual(value.runtime.ports, ['PORT']);
  assert.equal(value.runtime.previewPort, 'PORT');
  assert.equal(value.runtime.timeoutMs, 30000);
  assert.equal('timeoutMs' in input.runtime, false);
  assert.deepEqual(validateProject({ version: 1 }), { version: 1 });
});
void test('invalid and ambiguous runtime settings fail before executing commands', () => {
  for (const change of [
    { command: 'node server.ts' },
    { ports: ['PORT', 'PORT'] },
    { ports: ['port'] },
    { previewPort: 'BACKEND_PORT' },
    { env: { PORT: '3000' } },
    { env: { STUDIO_SESSION_ID: 'other' } },
    { env: { XDG_CONFIG_HOME: '/tmp/shared' } },
    { readyPath: '//example.com' },
    { compose: { file: '../compose.yaml' } },
    { timeoutMs: 0 },
    { mystery: true },
  ])
    assert.throws(() =>
      validateProject({ version: 1, runtime: { command: ['$NODE', 'server.ts'], ...change } }),
    );
  assert.throws(() => validateProject({ version: 2 }));
});
void test('missing configuration allows an IDE-only project; invalid JSON names the file', async (t) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'studio-config-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  assert.deepEqual(await loadProject(root), { version: 1 });
  await writeFile(path.join(root, 'studio.config.json'), '{bad');
  await assert.rejects(loadProject(root), /studio.config.json/);
});
