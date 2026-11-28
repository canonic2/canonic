import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, mkdir, readFile, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {prepare} from './prepare-diagnostic-app.mjs';

test('framework app preparation retains identity, wires faults and refuses replacement', async () => {
  const root = await mkdtemp(join(tmpdir(), 'testrun-diagnostic-apps-'));
  try {
    for (const framework of ['expo', 'react-native', 'flutter']) {
      const destination = join(root, framework); const calls = [];
      const request = {framework, destination, tool: '/selected/generator', generatorVersion: '1.2.3', runtimeVersion: '0.88.0'};
      const result = await prepare(request, async (tool, args, cwd) => {
        calls.push({tool, args, cwd});
        const project = join(cwd, 'project'); await mkdir(project);
        if (framework === 'flutter') {
          await mkdir(join(project, 'lib')); await mkdir(join(project, 'test'));
          await writeFile(join(project, 'test/widget_test.dart'), 'old counter test');
        } else {
          await writeFile(join(project, 'package.json'), '{"name":"acme","type":"module"}');
          await writeFile(join(project, 'app.json'), framework === 'expo' ? '{"expo":{"plugins":[]}}' : '{"name":"AcmeDiagnostics","displayName":"AcmeDiagnostics"}');
        }
      });
      assert.equal(calls.length, 1); assert.equal(calls[0].tool, request.tool);
      assert.equal(result.dependenciesInstalled, false); assert.equal(result.nativeBuild, false);
      assert.equal(JSON.parse(await readFile(join(destination, 'acme-fixture-owner.json'))).framework, framework);
      if (framework === 'flutter') {
        assert.ok(calls[0].args.includes('--no-pub'));
        assert.match(await readFile(join(result.project, 'lib/main.dart'), 'utf8'), /AcmeDiagnostics/);
        assert.match(await readFile(join(result.project, 'test/widget_test.dart'), 'utf8'), /retain: 1048576/);
      } else {
        assert.ok(calls[0].args.includes(framework === 'expo' ? 'create-expo-app@1.2.3' : '@react-native-community/cli@1.2.3'));
        const {AcmeFaults} = await import(pathToFileURL(join(result.project, 'diagnostics/runtime.js')));
        assert.deepEqual(JSON.parse(await readFile(join(result.project,'diagnostics/acceptance.json'),'utf8')),{enabled:false});
        assert.match(await readFile(join(result.project,'diagnostics/probe.cjs'),'utf8'),/installDiagnosticProbe/);
        const faults = new AcmeFaults(); assert.equal(faults.retain(), 1048576); faults.reset(); assert.equal(faults.retainedBytes, 0); faults.dispose();
        if (framework === 'expo') assert.equal(JSON.parse(await readFile(join(result.project, 'app.json'))).expo.android.package, 'com.example.acme.expodiagnostics');
        else {
          assert.deepEqual(calls[0].args.slice(calls[0].args.indexOf('--package-name'), calls[0].args.indexOf('--package-name') + 2), ['--package-name', 'com.example.acme.rndiagnostics']);
          assert.deepEqual(JSON.parse(await readFile(join(result.project, 'app.json'))), {name: 'AcmeDiagnostics', displayName: 'Acme Diagnostics'});
        }
      }
      await assert.rejects(prepare(request, () => assert.fail('must not regenerate')), {code: 'EEXIST'});
      assert.ok(JSON.parse(await readFile(join(destination, 'generated-manifests.json'))));
    }
    await assert.rejects(prepare({framework: 'expo', destination: join(root, 'invalid'), tool: '/tool', generatorVersion: 'latest'}), /exact generator/);
    // A registry cutoff becomes the project's pnpm minimumReleaseAge and is recorded with the owner.
    const dated = join(root, 'dated'), now = Date.parse('2026-11-11T00:00:00Z');
    const generate = async (tool, args, cwd) => { await mkdir(join(cwd, 'project')); await writeFile(join(cwd, 'project/app.json'), '{"expo":{}}'); };
    await prepare({framework: 'expo', destination: dated, tool: '/tool', generatorVersion: '5.0.0', runtimeVersion: '58.0.29', registryCutoff: '2026-10-28T00:00:00Z', now}, generate);
    assert.equal(await readFile(join(dated, 'project/pnpm-workspace.yaml'), 'utf8'), 'nodeLinker: hoisted\n# Registry cutoff 2026-10-28T00:00:00Z\nminimumReleaseAge: 20160\n');
    assert.deepEqual(JSON.parse(await readFile(join(dated, 'acme-fixture-owner.json'))).minimumReleaseAge, 20160);
    for (const registryCutoff of ['2026-10-28', '2026-11-12T00:00:00Z'])
      await assert.rejects(prepare({framework: 'expo', destination: join(root, 'undated'), tool: '/tool', generatorVersion: '5.0.0', runtimeVersion: '58.0.29', registryCutoff, now}, generate), /past UTC registry cutoff/);
    const failed = join(root, 'failed');
    await assert.rejects(prepare({framework: 'flutter', destination: failed, tool: '/tool'}, async () => { throw Error('generator failed'); }), /generator failed/);
    assert.equal(JSON.parse(await readFile(join(failed, 'acme-fixture-owner.json'))).framework, 'flutter');
  } finally { await rm(root, {recursive: true, force: true}); }
});
