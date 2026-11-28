// Development-only Gradle setup and APK build. Never starts a device or installs an SDK.
import {createHash, randomUUID} from 'node:crypto';
import {createReadStream, createWriteStream} from 'node:fs';
import {access, mkdir, mkdtemp, readFile, realpath, rename, rm, writeFile} from 'node:fs/promises';
import {homedir} from 'node:os';
import {dirname, isAbsolute, join, resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {Readable, Transform} from 'node:stream';
import {pipeline} from 'node:stream/promises';
import {pathToFileURL} from 'node:url';

export const gradleVersion = '9.3.1';
const archiveName = `gradle-${gradleVersion}-bin.zip`;
const distributionURL = `https://services.gradle.org/distributions/${archiveName}`;
const packageRoot = resolve(import.meta.dirname, '..');

async function exists(path) {
  try { await access(path); return true; }
  catch (error) { if (error.code === 'ENOENT') return false; throw error; }
}

async function run(program, args, env) {
  await new Promise((accept, reject) => {
    const child = spawn(program, args, {cwd: packageRoot, env, stdio: 'inherit'});
    child.once('error', reject);
    child.once('close', (code, signal) => code === 0 ? accept() : reject(new Error(`${program} exited ${code ?? signal}`)));
  });
}

async function digest(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

export function defaultCache(platform = process.platform, home = homedir(), env = process.env) {
  return platform === 'darwin' ? join(home, 'Library/Caches/testrun/development/android')
    : join(env.XDG_CACHE_HOME || join(home, '.cache'), 'testrun/development/android');
}

// Dependencies are injectable so refusal/reuse tests need no network, SDK or JVM.
export async function setup({cache, sdk, javaHome, network = true, platform = process.platform,
  fetcher = fetch, runner = run, env = process.env}) {
  if (!['darwin', 'linux'].includes(platform)) throw new Error('Android diagnostic setup supports macOS and Linux hosts');
  for (const [name, path] of Object.entries({cache, sdk, javaHome})) {
    if (!path || !isAbsolute(path)) throw new Error(`Select an absolute ${name} path`);
  }
  for (const path of [join(javaHome, 'bin/java'), join(sdk, 'platforms/android-36/android.jar'), join(sdk, 'build-tools/36.0.0/aapt2')]) {
    if (!await exists(path)) throw new Error(`Missing installed prerequisite: ${path}; no SDK/JDK download is performed`);
  }
  await mkdir(cache, {recursive: true});
  const archive = join(cache, archiveName), checksum = `${archive}.sha256`;
  let expected;
  if (await exists(checksum)) expected = (await readFile(checksum, 'utf8')).trim();
  else {
    if (!network) throw new Error('Gradle checksum is not cached. Rerun setup-diagnostic-android with 1 as the fourth argument.');
    const response = await fetcher(`${distributionURL}.sha256`, {signal: AbortSignal.timeout(30000)});
    if (!response.ok) throw new Error(`Gradle checksum request failed: HTTP ${response.status}`);
    expected = (await response.text()).trim();
  }
  if (!/^[a-f0-9]{64}$/.test(expected)) throw new Error('Invalid Gradle SHA-256 checksum');
  if (!await exists(archive)) {
    if (!network) throw new Error('Gradle archive is not cached. Rerun setup-diagnostic-android with 1 as the fourth argument.');
    console.log(`Downloading Gradle ${gradleVersion} into ${cache}`);
    const partial = `${archive}.${randomUUID()}.partial`;
    try {
      const response = await fetcher(distributionURL, {signal: AbortSignal.timeout(300000)});
      if (!response.ok || !response.body) throw new Error(`Gradle download failed: HTTP ${response.status}`);
      let bytes = 0;
      const bound = new Transform({transform(chunk, encoding, callback) {
        bytes += chunk.length;
        callback(bytes > 256 * 1024 * 1024 ? new Error('Gradle archive exceeded 256 MiB') : null, chunk);
      }});
      await pipeline(Readable.fromWeb(response.body), bound, createWriteStream(partial, {flags: 'wx', mode: 0o600}));
      if (await digest(partial) !== expected) throw new Error('Gradle archive checksum mismatch');
      await rename(partial, archive);
    } finally { await rm(partial, {force: true}); }
  }
  // Revalidate cached bytes before extracting or using their distribution.
  if (await digest(archive) !== expected) throw new Error(`Cached Gradle archive checksum mismatch: ${archive}`);
  await writeFile(checksum, `${expected}\n`);
  const distribution = join(cache, `gradle-${gradleVersion}`), gradle = join(distribution, 'bin/gradle');
  if (!await exists(gradle)) {
    const staging = await mkdtemp(join(cache, '.gradle-extract-'));
    try {
      await runner('unzip', ['-q', archive, '-d', staging], env);
      if (!await exists(join(staging, `gradle-${gradleVersion}`, 'bin/gradle'))) throw new Error('Gradle archive lacks its expected executable');
      try { await rename(join(staging, `gradle-${gradleVersion}`), distribution); }
      catch (error) {
        // Another setup can publish the same verified distribution first.
        if (!['EEXIST', 'ENOTEMPTY'].includes(error.code) || !await exists(gradle)) throw error;
      }
    } finally { await rm(staging, {recursive: true, force: true}); }
  }
  const buildEnv = {...env, ANDROID_HOME: sdk, JAVA_HOME: javaHome, GRADLE_USER_HOME: join(cache, 'gradle-home')};
  const project = 'tests/fixtures/diagnostics/apps/android';
  const args = ['-p', project, ...(network ? [] : ['--offline']), '--no-daemon', '--console=plain', 'assembleDebug'];
  await runner(gradle, args, buildEnv);
  return {gradleVersion, cache, gradle, sdk, javaHome, network,
    apk: join(packageRoot, project, 'app/build/outputs/apk/debug/app-debug.apk')};
}

async function main() {
  const [selectedCache, selectedSDK, selectedJava, selectedNetwork] = process.argv.slice(2);
  if (selectedCache === '--help') {
    console.log('node tools/setup-diagnostic-android.mjs [/absolute/cache] [/absolute/sdk] [/absolute/jdk] [0|1]\nArguments are the cache, SDK, JDK and network setting; empty ones use their defaults. Downloads verified Gradle and build dependencies by default; network=0 requires cached inputs. Builds only; no emulator startup.');
    return;
  }
  if (selectedNetwork && !['0', '1'].includes(selectedNetwork)) throw new Error('The fourth argument controls network access and must be 0 or 1.');
  const sdk = selectedSDK || process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT
    || (process.platform === 'darwin' ? join(homedir(), 'Library/Android/sdk') : join(homedir(), 'Android/Sdk'));
  let javaHome = selectedJava || process.env.JAVA_HOME;
  if (!javaHome && process.platform === 'darwin') {
    const {execFile} = await import('node:child_process');
    const {promisify} = await import('node:util');
    javaHome = (await promisify(execFile)('/usr/libexec/java_home', ['-v', '17'], {timeout: 10000})).stdout.trim();
  }
  if (!javaHome && process.platform === 'linux') javaHome = dirname(dirname(await realpath('/usr/bin/java')));
  console.log(JSON.stringify(await setup({cache: selectedCache || defaultCache(), sdk, javaHome, network: selectedNetwork !== '0'}), null, 2));
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
