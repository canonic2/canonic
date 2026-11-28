import {cp, mkdir, readFile, readdir, writeFile} from 'node:fs/promises';
import {isAbsolute, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const source = fileURLToPath(new URL('../tests/fixtures/diagnostics/', import.meta.url));
export const frameworks = JSON.parse(await readFile(join(source, 'apps/web/frameworks.json'), 'utf8'));

export async function prepareWeb(framework, destination, versions) {
  const definition = Object.hasOwn(frameworks, framework) && frameworks[framework];
  if (!definition || !isAbsolute(destination ?? '')) throw Error('Select a registered web framework and a new absolute destination');
  const dependencies = {};
  for (const name of definition.packages) {
    if (!/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(versions?.[name] ?? '')) throw Error(`Select an exact installed/registry version for ${name}`);
    dependencies[name] = versions[name];
  }
  await mkdir(destination, {mode: 0o700});
  await writeFile(join(destination, 'acme-fixture-owner.json'), JSON.stringify({framework, dependencies, purpose: 'disposable diagnostic web acceptance'}, null, 2) + '\n', {flag: 'wx'});
  const project = join(destination, 'project'); await mkdir(join(project, 'src'), {recursive: true});
  await cp(join(source, 'runtime.js'), join(project, 'src/runtime.js'));
  await cp(join(source, 'apps/web/controls.js'), join(project, 'src/controls.js'));
  if (definition.builder === 'vite') {
    const html = (await readFile(join(source, 'apps/web/index.html'), 'utf8')).replace('main.js', definition.entry ?? 'main.js');
    await writeFile(join(project, 'index.html'), html);
    await cp(join(source, 'apps/web/vite.config.js'), join(project, 'vite.config.js'));
    for (const entry of await readdir(join(source, 'apps/web', framework))) {
      await cp(join(source, 'apps/web', framework, entry), join(project, entry === 'vite.config.js' ? entry : `src/${entry}`));
    }
  } else await cp(join(source, 'apps/web', framework), project, {recursive: true});
  const scripts = {
    vite: {dev: 'vite', build: 'vite build', preview: 'vite preview'},
    next: {dev: 'next dev --hostname 127.0.0.1 --port 4173', build: 'next build', preview: 'next start --hostname 127.0.0.1'},
    nuxt: {dev: 'nuxt dev --host 127.0.0.1 --port 4173', build: 'nuxt build', preview: 'nuxt preview --host 127.0.0.1'},
    astro: {dev: 'astro dev --host 127.0.0.1 --port 4173', build: 'astro build', preview: 'astro preview --host 127.0.0.1 --port 4173'},
  }[definition.builder];
  await writeFile(join(project, 'package.json'), JSON.stringify({name: `acme-diagnostics-${framework}`, private: true, type: 'module', scripts, dependencies}, null, 2) + '\n', {flag: 'wx'});
  // Keep run/preview consistent with explicit installation. pnpm 11 can otherwise
  // attempt an implicit install before a script when dependency state differs.
  await writeFile(join(project, 'pnpm-workspace.yaml'), 'ignoreScripts: true\nverifyDepsBeforeRun: error\n', {flag: 'wx'});
  return {framework, project, dependenciesInstalled: false};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [framework, destination, versionFile] = process.argv.slice(2);
  if (!isAbsolute(versionFile ?? '')) throw Error('Select an absolute JSON dependency-version file');
  console.log(JSON.stringify(await prepareWeb(framework, destination, JSON.parse(await readFile(versionFile, 'utf8')))));
}
