// Prepare disposable real framework projects for explicit native build acceptance.
import { createHash } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { access, mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { basename, join, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';

const [framework, selectedRoot] = process.argv.slice(2);
if (!['flutter', 'react-native', 'expo'].includes(framework) || !selectedRoot) {
  throw new Error('Usage: node tools/bootstrap-build-project.mjs flutter|react-native|expo /private/tmp/testrun-build-NAME');
}
const root = resolve(selectedRoot);
if (!root.startsWith('/private/tmp/') || !basename(root).startsWith('testrun-build-')) {
  throw new Error('Select a disposable /private/tmp/testrun-build-* root');
}
await mkdir(root, { recursive: true });
const marker = join(root, 'fixture-owner.json');
try {
  const previous = JSON.parse(await readFile(marker, 'utf8'));
  if (previous.framework !== framework) throw new Error('Fixture owner mismatch');
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
  await writeFile(marker, JSON.stringify({ framework, purpose: 'Testrun native acceptance' }), { flag: 'wx' });
}
const project = join(root, 'project');
const env = { ...process.env, CI: '1', COCOAPODS_DISABLE_STATS: 'true', PUB_CACHE: join(root, 'pub-cache') };
async function run(executable, args, cwd = root) {
  await new Promise((accept, reject) => {
    const child = spawn(executable, args, { cwd, env, stdio: 'inherit' });
    child.on('error', reject);
    child.on('exit', code => code === 0 ? accept() : reject(new Error(`${executable} exited ${code}`)));
  });
}
async function json(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Fetch ${url}: ${response.status}`);
  return response.json();
}
async function exists(path) { try { await access(path); return true; } catch { return false; } }
async function nativeLinker() {
  const path = join(project, 'pnpm-workspace.yaml');
  const contents = await exists(path) ? await readFile(path, 'utf8') : '';
  if (!/^nodeLinker:/m.test(contents)) await writeFile(path, `${contents}\nnodeLinker: hoisted\n`);
  else if (!/^nodeLinker:\s*hoisted\s*$/m.test(contents)) throw new Error('Native fixture requires nodeLinker: hoisted');
  const metadata = join(project, 'node_modules/.modules.yaml');
  return await exists(metadata) && !/^nodeLinker:\s*hoisted\s*$/m.test(await readFile(metadata, 'utf8')) ? ['--force'] : [];
}
let versions = {};
if (framework === 'flutter') {
  const index = await json('https://storage.googleapis.com/flutter_infra_release/releases/releases_macos.json');
  const architecture = process.arch === 'arm64' ? 'arm64' : 'x64';
  const release = index.releases.find(item => item.hash === index.current_release.stable && item.dart_sdk_arch === architecture);
  if (!release) throw new Error('No stable Flutter archive for this host');
  versions = { flutter: release.version, dart: release.dart_sdk_version, sha256: release.sha256 };
  const archive = join(root, 'flutter.zip');
  if (!await exists(join(root, 'flutter/bin/flutter'))) {
    if (!await exists(archive)) {
      const response = await fetch(`${index.base_url}/${release.archive}`);
      if (!response.ok) throw new Error(`Flutter download: ${response.status}`);
      let bytes = 0; let reported = 0;
      const hash = createHash('sha256');
      const progress = new Transform({ transform(chunk, encoding, callback) {
        hash.update(chunk); bytes += chunk.length;
        if (bytes - reported >= 64 * 1024 * 1024) { reported = bytes; console.log(`Flutter download: ${Math.round(bytes / 1024 / 1024)} MiB`); }
        callback(null, chunk);
      } });
      await pipeline(Readable.fromWeb(response.body), progress, createWriteStream(`${archive}.partial`, { flags: 'w' }));
      if (hash.digest('hex') !== release.sha256) throw new Error('Flutter archive digest mismatch');
      await rename(`${archive}.partial`, archive);
    }
    await run('/usr/bin/ditto', ['-x', '-k', archive, root]);
  }
  if (!await exists(join(project, 'pubspec.yaml'))) {
    await run(join(root, 'flutter/bin/flutter'), ['create', '--platforms=android,ios', '--org', 'com.example.acme', '--project-name', 'buildfixture', project]);
  }
} else if (framework === 'react-native') {
  const native = await json('https://registry.npmjs.org/react-native/latest');
  const cli = await json('https://registry.npmjs.org/@react-native-community/cli/latest');
  versions = { 'react-native': native.version, cli: cli.version };
  if (!await exists(join(project, 'package.json'))) {
    await run('pnpm', ['dlx', `@react-native-community/cli@${cli.version}`, 'init', 'Acme', '--version', native.version, '--directory', project, '--skip-install', '--skip-git-init']);
  }
  await run('pnpm', ['install', ...await nativeLinker()], project);
  const installedNative = JSON.parse(await readFile(join(project, 'node_modules/react-native/package.json'), 'utf8'));
  await run('pnpm', ['add', '--save-dev', '--save-exact', `@react-native/gradle-plugin@${installedNative.version}`], project);
} else {
  const create = await json('https://registry.npmjs.org/create-expo-app/latest');
  const eas = await json('https://registry.npmjs.org/eas-cli/latest');
  versions = { 'create-expo-app': create.version, 'eas-cli': eas.version };
  if (!await exists(join(project, 'package.json'))) {
    await run('pnpm', ['dlx', `create-expo-app@${create.version}`, project, '--template', 'blank', '--no-install']);
  }
  const deniedOptionalBuilds = ['--allow-build=!dtrace-provider', '--allow-build=!protobufjs'];
  await run('pnpm', [...deniedOptionalBuilds, 'install', '--ignore-scripts', ...await nativeLinker()], project);
  await run('pnpm', [...deniedOptionalBuilds, 'add', '--ignore-scripts', '--save-dev', `eas-cli@${eas.version}`], project);
  const app = JSON.parse(await readFile(join(project, 'app.json'), 'utf8'));
  app.expo.name = 'Acme';
  app.expo.slug = 'acme-build-fixture';
  app.expo.android = { ...app.expo.android, package: 'com.example.acme.expofixture' };
  app.expo.ios = { ...app.expo.ios, bundleIdentifier: 'com.example.acme.expofixture' };
  await writeFile(join(project, 'app.json'), JSON.stringify(app, null, 2));
}
await writeFile(join(root, 'versions.json'), JSON.stringify(versions, null, 2) + '\n');
const sdk = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT;
const profiles = {};
if (framework === 'flutter') {
  const env = { PATH: `${join(root, 'flutter/bin')}:${process.env.PATH}`, PUB_CACHE: join(root, 'pub-cache'), ...(sdk ? { ANDROID_HOME: sdk } : {}) };
  for (const mode of ['debug', 'release']) profiles[`android-${mode}`] = { builder: 'flutter', platform: 'android', target_kind: 'emulator', env, flutter: { mode, dart_defines: { API_URL: 'https://staging.example.com' } } };
  profiles['ios-debug'] = { builder: 'flutter', platform: 'ios', target_kind: 'simulator', env, flutter: { mode: 'debug', dart_defines: { API_URL: 'https://staging.example.com' } } };
} else if (framework === 'react-native') {
  for (const variant of ['debug', 'release']) profiles[`android-${variant}`] = { builder: 'react-native', platform: 'android', target_kind: 'emulator', env: sdk ? { ANDROID_HOME: sdk } : {}, timeout_ms: 3600000, 'react-native': { variant }, artifact: { path: `android/app/build/outputs/apk/${variant}/app-${variant}.apk` } };
  profiles['ios-release'] = { builder: 'react-native', platform: 'ios', target_kind: 'simulator', timeout_ms: 3600000, 'react-native': { configuration: 'Release', scheme: 'Acme' } };
} else {
  await writeFile(join(project, 'eas.json'), JSON.stringify({ build: { acceptance: { distribution: 'internal', android: { buildType: 'apk' }, ios: { simulator: true } } } }, null, 2));
  await mkdir(join(root, 'eas-home'), { recursive: true });
  profiles.android = { builder: 'expo', platform: 'android', target_kind: 'emulator', timeout_ms: 3600000, env: { HOME: join(root, 'eas-home'), EAS_NO_VCS: '1', ...(sdk ? { ANDROID_HOME: sdk } : {}) }, expo: { profile: 'acceptance' } };
  for (const mode of ['debug', 'release']) profiles[`android-native-${mode}`] = { builder: 'gradle', platform: 'android', target_kind: 'emulator', cwd: 'android', timeout_ms: 3600000, gradle: { tasks: [`assemble${mode[0].toUpperCase()}${mode.slice(1)}`] }, artifact: { path: `app/build/outputs/apk/${mode}/app-${mode}.apk` } };
}
await writeFile(join(project, 'testrun.yml'), JSON.stringify({ version: 1, builds: profiles }, null, 2) + '\n');
console.log(JSON.stringify({ framework, project, versions }));
