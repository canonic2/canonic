import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {access, mkdir, mkdtemp, readFile, readdir, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {layout, remove, setup} from './setup-diagnostic-flutter.mjs';

const bytes = Buffer.from('acme flutter archive');
const pinned = {version: '9.9.9', revision: 'a'.repeat(40), dart: '9.1.0', baseURL: 'https://example.com/releases',
  archives: {x64: {path: 'stable/macos/flutter.zip', sha256: createHash('sha256').update(bytes).digest('hex')}, arm64: {path: 'stable/macos/flutter_arm64.zip', sha256: '0'.repeat(64)}}};
const present = path => access(path).then(() => true, () => false);

function peers(calls, {body = bytes, version = {frameworkVersion: '9.9.9', frameworkRevision: 'a'.repeat(40), dartSdkVersion: '9.1.0 (stable)'}} = {}) {
  return {
    fetcher: async url => { calls.push(['fetch', url]); return new Response(body); },
    // The archive peer extracts a fake SDK where unzip would.
    runner: async (program, args) => {
      calls.push([program, ...args]);
      if (program === '/usr/bin/unzip') { const staging = args[3]; await mkdir(join(staging, 'flutter/bin'), {recursive: true}); await writeFile(join(staging, 'flutter/bin/flutter'), ''); }
    },
    reader: async (program, args) => { calls.push([program, ...args]); return `Building flutter tool...\n${JSON.stringify(version)}`; },
  };
}

test('Flutter setup verifies the pinned archive, keeps state in the cache and reuses it', async t => {
  const root = await mkdtemp(join(tmpdir(), 'testrun-flutter-setup-')); t.after(() => rm(root, {recursive: true, force: true}));
  const cache = join(root, 'cache'), calls = [];
  const result = await setup({cache, arch: 'x64', platform: 'darwin', pinned, ...peers(calls)});
  const paths = layout(cache, pinned);
  assert.equal(result.launcher, paths.launcher); assert.equal(result.dart, '9.1.0 (stable)');
  assert.deepEqual(calls[0], ['fetch', 'https://example.com/releases/stable/macos/flutter.zip']);
  const launcher = await readFile(paths.launcher, 'utf8');
  for (const line of [`export HOME='${paths.home}'`, `export PUB_CACHE='${paths.pubCache}'`, `export GRADLE_USER_HOME='${paths.gradleHome}'`, `export ANDROID_USER_HOME='${paths.androidUserHome}'`, `exec '${join(paths.sdk, 'bin/flutter')}' "$@"`])
    assert.ok(launcher.includes(line), line);
  assert.ok(calls.some(call => call[0] === paths.launcher && call[1] === 'precache' && call.includes('--android')));
  // A warm cache revalidates the archive without downloading or extracting again.
  const warm = [];
  await setup({cache, arch: 'x64', platform: 'darwin', pinned, ...peers(warm)});
  assert.ok(!warm.some(call => call[0] === 'fetch' || call[0] === '/usr/bin/unzip'));
});

test('Flutter setup refuses a checksum or identity mismatch without publishing an SDK', async t => {
  const root = await mkdtemp(join(tmpdir(), 'testrun-flutter-refusal-')); t.after(() => rm(root, {recursive: true, force: true}));
  const cache = join(root, 'cache');
  await assert.rejects(setup({cache, arch: 'x64', platform: 'darwin', pinned, ...peers([], {body: Buffer.from('tampered')})}), /checksum mismatch/);
  assert.deepEqual(await readdir(layout(cache, pinned).archives), []);
  assert.equal(await present(layout(cache, pinned).sdk), false);
  await assert.rejects(setup({cache, arch: 'x64', platform: 'darwin', pinned, ...peers([], {version: {frameworkVersion: '9.9.8', frameworkRevision: 'a'.repeat(40), dartSdkVersion: '9.1.0'}})}), /differs from the pinned release/);
  await assert.rejects(setup({cache, platform: 'linux', pinned}), /macOS hosts/);
});

test('Flutter cache removal deletes only a marked Testrun cache', async t => {
  const root = await mkdtemp(join(tmpdir(), 'testrun-flutter-remove-')); t.after(() => rm(root, {recursive: true, force: true}));
  const unmarked = join(root, 'other'); await mkdir(unmarked); await writeFile(join(unmarked, 'keep'), 'user data');
  await assert.rejects(remove({cache: unmarked}), /nothing was removed/);
  assert.equal(await readFile(join(unmarked, 'keep'), 'utf8'), 'user data');
  const cache = join(root, 'cache');
  await setup({cache, arch: 'x64', platform: 'darwin', pinned, ...peers([])});
  assert.deepEqual(await remove({cache}), {removed: cache});
  assert.equal(await present(cache), false);
});
