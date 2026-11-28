// Extract the pinned Playwright device descriptors without importing or executing upstream code.
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
export const coreSha256 = '829521b67ed7be90c7d6d6098ff39f2241562d47e4ad7ccab13710924a56001d';
const digest = value => createHash('sha256').update(value).digest('hex');
const browsers = new Set(['chromium', 'firefox', 'webkit']);

function size(value, name) {
  if (!value || typeof value !== 'object' || Object.keys(value).sort().join() !== 'height,width'
    || !Number.isInteger(value.width) || !Number.isInteger(value.height) || value.width < 1 || value.height < 1) {
    throw new Error(`Invalid ${name} size`);
  }
}

// The bundled JSON module is an object literal with bare identifier keys and
// double-quoted string values; quote the keys and parse it as JSON.
export function descriptors(bundle) {
  const marker = '// packages/isomorphic/deviceDescriptorsSource.json\n';
  const section = bundle.split(marker)[1];
  const literal = section?.match(/^var deviceDescriptorsSource_default;\nvar init_deviceDescriptorsSource = __esm\(\{\n {2}"packages\/isomorphic\/deviceDescriptorsSource\.json"\(\) \{\n {4}deviceDescriptorsSource_default = (\{[\s\S]*?\n {4}\});\n {2}\}\n\}\);\n/);
  if (!literal) throw new Error('Missing pinned device descriptor table');
  const json = literal[1].replace(/^(\s+)([A-Za-z][A-Za-z0-9]*): /gm, '$1"$2": ');
  const table = JSON.parse(json);
  const fields = 'defaultBrowserType,deviceScaleFactor,hasTouch,isMobile,userAgent,viewport';
  for (const [name, descriptor] of Object.entries(table)) {
    if (!/^[ -~]{1,64}$/.test(name)) throw new Error(`Invalid device name: ${name}`);
    const keys = Object.keys(descriptor).filter(key => key !== 'screen').sort().join();
    if (keys !== fields || typeof descriptor.userAgent !== 'string' || !descriptor.userAgent
      || typeof descriptor.deviceScaleFactor !== 'number' || !(descriptor.deviceScaleFactor > 0)
      || typeof descriptor.isMobile !== 'boolean' || typeof descriptor.hasTouch !== 'boolean'
      || !browsers.has(descriptor.defaultBrowserType)) {
      throw new Error(`Invalid device descriptor: ${name}`);
    }
    size(descriptor.viewport, `${name} viewport`);
    if ('screen' in descriptor) size(descriptor.screen, `${name} screen`);
  }
  return JSON.stringify(table, null, 2) + '\n';
}

async function main() {
  const [packageRoot, mode] = process.argv.slice(2);
  if (!packageRoot || mode && mode !== '--check') throw new Error('Supply PLAYWRIGHT_CORE [--check]');
  const core = await readFile(resolve(packageRoot, 'lib/coreBundle.js'));
  if (digest(core) !== coreSha256) throw new Error('Pinned device descriptor bundle changed');
  const data = descriptors(core.toString('utf8'));
  const manifest = JSON.stringify({playwrightVersion:'1.64.1',source:'lib/coreBundle.js',sourceModule:'packages/isomorphic/deviceDescriptorsSource.json',
    sourceSha256:coreSha256,data:'src/browser/devices.json',dataSha256:digest(data),license:'third-party/browser/upstream-notices/LICENSE'}, null, 2) + '\n';
  for (const [file, value] of [['src/browser/devices.json', data], ['tools/browser-provenance/device-descriptors.json', manifest]]) {
    if (mode === '--check') {
      if (await readFile(resolve(root, file), 'utf8') !== value) throw new Error(`Pinned device descriptor drift: ${file}`);
    } else {
      await writeFile(resolve(root, file), value);
    }
  }
  process.stdout.write('Verified pinned device descriptors\n');
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {process.stderr.write(error.message + '\n'); process.exitCode = 1;});
}
