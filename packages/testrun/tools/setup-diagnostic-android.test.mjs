import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdir, mkdtemp, readFile, readdir, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {defaultCache, gradleVersion, setup} from './setup-diagnostic-android.mjs';

const archive = Buffer.from('Acme owned archive transport fixture');
const checksum = createHash('sha256').update(archive).digest('hex');
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'testrun-android-setup-'));
  t.after(() => rm(root, {recursive: true, force: true}));
  const sdk = join(root, 'sdk'), javaHome = join(root, 'jdk'), cache = join(root, 'cache');
  for (const path of [join(sdk, 'platforms/android-36/android.jar'), join(sdk, 'build-tools/36.0.0/aapt2'), join(javaHome, 'bin/java')]) {
    await mkdir(join(path, '..'), {recursive: true}); await writeFile(path, 'Acme prerequisite');
  }
  const requests = [], calls = [];
  return {cache, sdk, javaHome, platform: 'linux', env: {ACME_ENV: 'kept'}, requests, calls,
    fetcher: async url => { requests.push(url); return new Response(url.endsWith('.sha256') ? checksum : archive); },
    runner: async (program, args, env) => {
      calls.push({program, args, env});
      if (program === 'unzip') {
        const bin = join(args[3], `gradle-${gradleVersion}`, 'bin');
        await mkdir(bin, {recursive: true}); await writeFile(join(bin, 'gradle'), 'Acme owned extraction peer');
      }
    }};
}

test('cold setup verifies before extraction and warm offline builds reuse the same cache', async t => {
  const options = await fixture(t);
  const first = await setup(options);
  assert.equal(options.requests.length, 2);
  assert.match(options.requests[0], /^https:\/\/services\.gradle\.org\/distributions\/gradle-9\.3\.1-bin\.zip\.sha256$/);
  assert.deepEqual(options.calls.map(call => call.program), ['unzip', first.gradle]);
  assert.equal(await readFile(join(options.cache, 'gradle-9.3.1-bin.zip'), 'utf8'), archive.toString());
  const build = options.calls[1];
  const args = ['-p', 'tests/fixtures/diagnostics/apps/android', '--no-daemon', '--console=plain', 'assembleDebug'];
  assert.deepEqual(build.args, args);
  assert.equal(build.env.ANDROID_HOME, options.sdk);
  assert.equal(build.env.GRADLE_USER_HOME, join(options.cache, 'gradle-home'));
  assert.equal(build.env.JAVA_HOME, options.javaHome);
  assert.equal(build.env.ACME_ENV, 'kept');
  options.calls.length = 0;
  await setup({...options, network: false, fetcher: () => { throw Error('Offline must not fetch'); }});
  assert.deepEqual(options.calls.map(call => call.program), [first.gradle]);
  assert.deepEqual(options.calls[0].args, [...args.slice(0, 2), '--offline', ...args.slice(2)]);
});

test('cold offline setup and missing installed prerequisites refuse before external execution', async t => {
  const options = await fixture(t);
  await assert.rejects(setup({...options, network: false}), /checksum is not cached/);
  await rm(join(options.javaHome, 'bin/java'));
  await assert.rejects(setup(options), /Missing installed prerequisite/);
  assert.deepEqual(options.requests, []); assert.deepEqual(options.calls, []);
});

test('download integrity failure removes its partial file and never extracts or builds', async t => {
  const options = await fixture(t);
  await assert.rejects(setup({...options, fetcher: async url => new Response(url.endsWith('.sha256') ? checksum : 'corrupt bytes')}), /checksum mismatch/);
  assert.deepEqual(options.calls, []);
  assert.deepEqual(await readdir(options.cache), []);
});

test('cached archive corruption refuses even when an extracted executable exists', async t => {
  const options = await fixture(t);
  await setup(options); options.calls.length = 0;
  await writeFile(join(options.cache, 'gradle-9.3.1-bin.zip'), 'corrupt cached bytes');
  await assert.rejects(setup({...options, network: false}), /Cached Gradle archive checksum mismatch/);
  assert.deepEqual(options.calls, []);
});

test('extraction failure cleans staging and keeps verified inputs for retry', async t => {
  const options = await fixture(t);
  await assert.rejects(setup({...options, runner: async () => { throw Error('Acme extraction failed'); }}), /Acme extraction failed/);
  assert.equal((await readdir(options.cache)).some(name => name.startsWith('.gradle-extract-')), false);
  const result = await setup({...options, network: false, fetcher: () => { throw Error('Should reuse download'); }});
  assert.deepEqual(options.calls.map(call => call.program), ['unzip', result.gradle]);
});

test('invalid checksum and HTTP errors refuse before extraction or execution', async t => {
  const options = await fixture(t);
  await assert.rejects(setup({...options, fetcher: async () => new Response('invalid')}), /Invalid Gradle SHA-256/);
  await assert.rejects(setup({...options, fetcher: async () => new Response('unavailable', {status: 503})}), /HTTP 503/);
  assert.deepEqual(options.calls, []);
});

test('a failed APK build retains setup inputs for a later offline retry', async t => {
  const options = await fixture(t);
  const gradle = join(options.cache, `gradle-${gradleVersion}`, 'bin/gradle');
  await assert.rejects(setup({...options, runner: async (program, args, env) => {
    if (program === gradle) throw Error('Acme build failed');
    await options.runner(program, args, env);
  }}), /Acme build failed/);
  options.calls.length = 0;
  await setup({...options, network: false, fetcher: () => { throw Error('Should reuse cache after build failure'); }});
  assert.deepEqual(options.calls.map(call => call.program), [gradle]);
});

test('persistent defaults follow each host cache convention', () => {
  assert.equal(defaultCache('darwin', '/home/acme', {}), '/home/acme/Library/Caches/testrun/development/android');
  assert.equal(defaultCache('linux', '/home/acme', {}), '/home/acme/.cache/testrun/development/android');
  assert.equal(defaultCache('linux', '/home/acme', {XDG_CACHE_HOME: '/cache/acme'}), '/cache/acme/testrun/development/android');
});
