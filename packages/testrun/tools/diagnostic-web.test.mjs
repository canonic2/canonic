import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, readFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {frameworks, prepareWeb} from './prepare-diagnostic-web.mjs';
import {createController} from '../tests/fixtures/diagnostics/apps/web/controls.js';

test('web controls reproduce faults, correct the same observation and dispose bounded scheduled work', async () => {
  const pending = new Map(); let next = 0, state, updates = 0;
  const controller = createController(callback => { const id = ++next; pending.set(id, callback); return id; }, id => pending.delete(id));
  controller.subscribe(value => { state = value; updates++; });
  await controller.run('exception'); assert.match(state.status, /Acme JavaScript fixture failure/);
  await controller.run('retain'); assert.equal(state.status, 'retain: 1048576');
  await controller.run('expensive'); assert.equal(state.status, 'expensive: 704982704');
  await controller.run('churn'); assert.equal(state.frames, 12);
  let ticks = 0;
  while (pending.size) { const [id, callback] = pending.entries().next().value; pending.delete(id); callback(); assert.ok(++ticks <= 12); }
  assert.equal(ticks, 12); assert.equal(state.frames, 0);
  await controller.run('uncaught'); assert.equal(state.status, 'Arm this app first');
  await controller.run('churn'); await controller.run('reset'); assert.equal(pending.size, 0); assert.equal(state.status, 'Acme reset');
  const staleFailure = controller.run('bridge'); await controller.run('reset'); await staleFailure; assert.equal(state.status, 'Acme reset');
  controller.corrected(true); await controller.run('retain'); assert.equal(state.status, 'retain: 0');
  await controller.run('exception'); assert.equal(state.status, 'exception: "ready"');
  controller.arm(true); await controller.run('uncaught'); assert.equal(state.armed, false);
  await controller.run('churn'); assert.equal(state.frames, 1);
  controller.dispose(); assert.equal(pending.size, 0); const before = updates;
  await controller.run('retain'); assert.equal(updates, before);
  assert.throws(() => controller.subscribe(() => {}), /disposed/);
});

test('each web adapter materializes a private app with exact dependencies and refuses overwrites', async () => {
  const root = await mkdtemp(join(tmpdir(), 'testrun-diagnostic-web-'));
  try {
    for (const [framework, definition] of Object.entries(frameworks)) {
      const destination = join(root, framework), versions = Object.fromEntries(definition.packages.map(name => [name, '1.2.3']));
      const result = await prepareWeb(framework, destination, versions);
      const manifest = JSON.parse(await readFile(join(result.project, 'package.json')));
      assert.deepEqual(manifest.dependencies, versions); assert.equal(manifest.private, true); assert.equal(result.dependenciesInstalled, false);
      assert.equal(JSON.parse(await readFile(join(destination, 'acme-fixture-owner.json'))).framework, framework);
      assert.match(await readFile(join(result.project, 'src/runtime.js'), 'utf8'), /export class AcmeFaults/);
      assert.match(await readFile(join(result.project, 'src/controls.js'), 'utf8'), /createController/);
      if (definition.builder === 'vite') assert.match(await readFile(join(result.project, 'index.html'), 'utf8'), new RegExp(definition.entry ?? 'main.js'));
      else await readFile(join(result.project, {next:'app/page.js',nuxt:'app.vue',astro:'src/pages/index.astro'}[framework]));
      await assert.rejects(prepareWeb(framework, destination, versions), {code: 'EEXIST'});
    }
    await assert.rejects(prepareWeb('vue', join(root, 'bad'), {vite:'latest',vue:'3.0.0'}), /exact/);
    await assert.rejects(prepareWeb('unknown', join(root, 'unknown'), {}), /registered/);
  } finally { await rm(root, {recursive: true, force: true}); }
});
