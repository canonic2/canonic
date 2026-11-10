import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm, chmod, realpath } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { pinBinary, launchEnvironment } from './binary.mjs';

const exec = promisify(execFile);
const supported = ['darwin', 'linux'].includes(process.platform);
const mutate = fileURLToPath(new URL('./binary-mutation.fixture.mjs', import.meta.url));

test('binary receipt fence launch environment omits loader options and inherited private tokens', async () => {
  const source = { ...process.env, LD_PRELOAD: 'PRIVATE_LOADER_SENTINEL', LD_LIBRARY_PATH: '/acme/loader',
    DYLD_INSERT_LIBRARIES: '/acme/injected', DYLD_LIBRARY_PATH: '/acme/libraries', NODE_OPTIONS: '--eval=unsafe',
    SHIELD_SDK_READ_CONTEXT_TOKEN: 'PRIVATE_TOKEN_SENTINEL', PATH: '/acme/bin', HOME: '/acme/home', LANG: 'C' };
  const captured = launchEnvironment(source);
  source.HOME = '/acme/changed';
  const fixture = fileURLToPath(new URL('./binary-environment.fixture.mjs', import.meta.url));
  for (const phase of ['mint', 'evaluate', 'close']) {
    const { stdout } = await exec(process.execPath, [fixture, phase], { env: captured });
    const received = JSON.parse(stdout);
    assert.equal(received.HOME, '/acme/home');
    assert.equal(received.LANG, 'C');
    for (const name of ['LD_PRELOAD', 'LD_LIBRARY_PATH', 'DYLD_INSERT_LIBRARIES', 'DYLD_LIBRARY_PATH', 'NODE_OPTIONS', 'SHIELD_SDK_READ_CONTEXT_TOKEN', 'PATH']) assert.equal(received[name], undefined);
  }
});
async function fixture(t) {
  const root = await realpath(await mkdtemp(join(tmpdir(), 'shield-binary-fence-acme-')));
  t.after(() => rm(root, { recursive: true, force: true }));
  const directory = join(root, 'bin');
  await mkdir(directory);
  const path = join(directory, 'shield');
  // This fixture exercises metadata/receipt admission, not native image loading.
  const bytes = Buffer.concat([Buffer.from(process.platform === 'darwin' ? 'cffaedfe' : '7f454c46', 'hex'), Buffer.from('Acme owned native-header fixture')]);
  await writeFile(path, bytes, { mode: 0o700 });
  const digest = createHash('sha256').update(bytes).digest('hex');
  return { path, directory, digest };
}

test('binary receipt fence rejects wrong digests and unsupported image headers', { skip: !supported }, async t => {
  const { path, digest } = await fixture(t);
  await assert.rejects(pinBinary(path, '0'.repeat(64)));
  const pin = await pinBinary(path, digest);
  await pin.check();
  await pin.close();
  await assert.rejects(pin.check());
  const script = Buffer.from('#!/bin/sh\nexit 0\n');
  await writeFile(path, script);
  await chmod(path, 0o700);
  await assert.rejects(pinBinary(path, createHash('sha256').update(script).digest('hex')));
});

test('binary receipt fence refuses child frames after bytes path or ancestor roundtrips', { skip: !supported }, async t => {
  for (const kind of ['bytes', 'path', 'ancestor']) {
    const { path, directory, digest } = await fixture(t);
    const pin = await pinBinary(path, digest);
    try {
      let mutated = false;
      await assert.rejects(pin.guard(async () => {
        await exec(process.execPath, [mutate, kind === 'ancestor' ? directory : path, kind]);
        mutated = true;
        return { disposition: 'allowed' };
      }), /identity is unavailable or changed/);
      assert.equal(mutated, true, `The ${kind} roundtrip must complete before receipt verification refuses it`);
    } finally { await pin.close(); }
  }
});

test('binary receipt fence refreshes ancestor baselines between separate owned children', { skip: !supported }, async t => {
  const { path, directory, digest } = await fixture(t);
  const pin = await pinBinary(path, digest);
  try {
    assert.equal(await pin.guard(async () => 'first verified frame'), 'first verified frame');
    await exec(process.execPath, [mutate, directory, 'ancestor']);
    assert.equal(await pin.guard(async () => 'second verified frame'), 'second verified frame');
  } finally { await pin.close(); }
});
