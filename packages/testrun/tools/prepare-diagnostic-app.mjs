// Materialize an owned framework app; no device discovery, build or installation.
import {spawn} from 'node:child_process';
import {cp, mkdir, readFile, writeFile} from 'node:fs/promises';
import {isAbsolute, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const fixtures = fileURLToPath(new URL('../tests/fixtures/diagnostics/', import.meta.url));
export async function prepare({framework, destination, tool, generatorVersion, runtimeVersion, registryCutoff = '', now = Date.now()}, run = execute) {
  if (!['expo', 'react-native', 'flutter'].includes(framework) || !isAbsolute(destination ?? '') || !isAbsolute(tool ?? '')) throw new Error('Select framework, new absolute destination and installed absolute tool');
  if (framework !== 'flutter' && !/^\d+\.\d+\.\d+$/.test(generatorVersion ?? '')) throw new Error('Select an exact generator version');
  if (framework !== 'flutter' && !/^\d+\.\d+\.\d+$/.test(runtimeVersion ?? '')) throw new Error('Select an exact Expo blank template or React Native version');
  // Optional: resolve every package from versions published before a past UTC instant,
  // through pnpm's minimumReleaseAge. The lockfile then records the exact selection.
  let minimumReleaseAge;
  if (registryCutoff) {
    const cutoff = /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/.test(registryCutoff) ? Date.parse(registryCutoff) : NaN;
    if (framework === 'flutter' || !(cutoff < now)) throw new Error('Select a past UTC registry cutoff (YYYY-MM-DDTHH:MM:SSZ) for an Expo/React Native project');
    minimumReleaseAge = Math.ceil((now - cutoff) / 60000);
  }
  await mkdir(destination, {mode: 0o700}); // Refuse existing projects, including partial prior runs.
  await writeFile(join(destination, 'acme-fixture-owner.json'), JSON.stringify({framework, generatorVersion, runtimeVersion, ...(registryCutoff ? {registryCutoff, minimumReleaseAge} : {}), purpose: 'disposable diagnostic acceptance'}, null, 2) + '\n', {flag: 'wx'});
  // Generators require a fresh project child; the enclosing root owns failures too.
  const project = join(destination, 'project');
  if (framework === 'flutter') {
    await run(tool, ['create', '--no-pub', '--platforms=android,ios', '--org', 'com.example.acme', '--project-name', 'acme_diagnostics', project], destination);
    await cp(join(fixtures, 'apps/flutter/pubspec.yaml'), join(project, 'pubspec.yaml'));
    await cp(join(fixtures, 'apps/flutter/analysis_options.yaml'), join(project, 'analysis_options.yaml'));
    for (const name of ['main.dart', 'acme_faults.dart']) await cp(join(fixtures, 'flutter', name), join(project, 'lib', name));
    await cp(join(fixtures, 'apps/flutter/diagnostics_test.dart'), join(project, 'test/widget_test.dart'));
  } else {
    const args = framework === 'expo'
      ? ['dlx', `create-expo-app@${generatorVersion}`, project, '--template', `expo-template-blank@${runtimeVersion}`, '--no-install']
      : ['dlx', `@react-native-community/cli@${generatorVersion}`, 'init', 'AcmeDiagnostics', '--version', runtimeVersion, '--directory', project, '--package-name', 'com.example.acme.rndiagnostics', '--skip-install', '--install-pods', 'false', '--skip-git-init'];
    await run(tool, args, destination);
    const appFile = framework === 'expo' ? 'App.js' : 'App.tsx';
    await cp(join(fixtures, 'apps', framework, appFile), join(project, appFile));
    await mkdir(join(project, 'diagnostics'));
    for (const name of ['AcmeDiagnostics.jsx', 'AcmeDiagnostics.d.ts', 'DiagnosticApp.jsx', 'DiagnosticApp.d.ts', 'runtime.js']) await cp(join(fixtures, name), join(project, 'diagnostics', name));
    await cp(join(fixtures, '../../../probes/react-native.js'), join(project, 'diagnostics/probe.cjs'));
    await writeFile(join(project, 'diagnostics/acceptance.json'), '{"enabled":false}\n', {flag:'wx'});
    if (framework === 'expo') {
      const path = join(project, 'app.json'); const app = JSON.parse(await readFile(path, 'utf8'));
      app.expo.name = 'Acme Diagnostics'; app.expo.slug = 'acme-diagnostics';
      app.expo.ios = {...app.expo.ios, bundleIdentifier: 'com.example.acme.expodiagnostics'};
      app.expo.android = {...app.expo.android, package: 'com.example.acme.expodiagnostics'};
      await writeFile(path, JSON.stringify(app, null, 2) + '\n');
    } else {
      // The registered component name stays the generator's; only the label changes.
      const path = join(project, 'app.json'); const app = JSON.parse(await readFile(path, 'utf8'));
      if (app.name !== 'AcmeDiagnostics') throw new Error(`Generated React Native app name ${app.name} does not match the selected fixture`);
      await writeFile(path, JSON.stringify({...app, displayName: 'Acme Diagnostics'}, null, 2) + '\n');
    }
    await writeFile(join(project, 'pnpm-workspace.yaml'), 'nodeLinker: hoisted\n' + (minimumReleaseAge ? `# Registry cutoff ${registryCutoff}\nminimumReleaseAge: ${minimumReleaseAge}\n` : ''));
  }
  const identity = Object.fromEntries(await Promise.all(['package.json', 'pubspec.yaml'].map(async name => {
    try { return [name, await readFile(join(project, name), 'utf8')]; } catch (error) { if (error.code !== 'ENOENT') throw error; return [name, null]; }
  })));
  await writeFile(join(destination, 'generated-manifests.json'), JSON.stringify(identity, null, 2) + '\n', {flag: 'wx'});
  return {framework, project, dependenciesInstalled: false, nativeBuild: false};
}
async function execute(tool, args, cwd) {
  await new Promise((accept, reject) => {
    const child = spawn(tool, args, {cwd, env: {...process.env, CI: '1', COCOAPODS_DISABLE_STATS: 'true'}, stdio: 'inherit', timeout: 300000});
    child.once('error', reject);
    child.once('exit', (code, signal) => code === 0 ? accept() : reject(new Error(`Generator failed (${code ?? signal}); preserve the owned destination for inspection`)));
  });
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [framework, destination, tool, generatorVersion, runtimeVersion, registryCutoff] = process.argv.slice(2);
  console.log(JSON.stringify(await prepare({framework, destination, tool, generatorVersion, runtimeVersion, registryCutoff})));
}
