// Extract pinned MIME data without importing or executing upstream code.
import { createHash } from 'node:crypto';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
export const sourceSha256 = '75f1431ea7ee00b2b291a0a72d4fa1c07ae04d999f21a00cd03b8f454a5840fc';
const digest = value => createHash('sha256').update(value).digest('hex');

export function mimeTypes(bundle) {
  const result = new Map();
  for (const kind of ['standard', 'other']) {
    const marker = `// node_modules/mime/dist/types/${kind}.js\n`;
    const section = bundle.split(marker)[1]?.split('\n// ')[0];
    const literal = section?.match(/^var (types\d*) = (\{[\s\S]*?\n\});\nObject\.freeze\(\1\);/);
    if (!literal) throw new Error(`Missing pinned ${kind} MIME table`);
    const table = JSON.parse(literal[2]);
    for (const [type, extensions] of Object.entries(table)) {
      if (!/^[a-z0-9!#$&^_.+-]+\/[a-z0-9!#$&^_.+-]+$/.test(type) || !Array.isArray(extensions)) throw new Error('Invalid MIME table');
      for (const extension of extensions) {
        if (typeof extension !== 'string' || !/^\*?[a-z0-9_+-]+$/.test(extension)) throw new Error('Invalid MIME extension');
        if (extension.startsWith('*')) continue;
        if (result.has(extension) && result.get(extension) !== type) throw new Error('Conflicting MIME extension');
        result.set(extension, type);
      }
    }
  }
  return JSON.stringify(Object.fromEntries([...result].sort(([a], [b]) => a.localeCompare(b, 'en'))), null, 2) + '\n';
}

export const coreSha256 = '829521b67ed7be90c7d6d6098ff39f2241562d47e4ad7ccab13710924a56001d';
// Pinned route.fulfill({path}) table used by getMimeTypeForPath: case-sensitive
// extensions after the path's last dot.
export function fulfillTypes(bundle) {
  const start = bundle.indexOf('function getMimeTypeForPath(');
  const open = bundle.indexOf('types = /* @__PURE__ */ new Map([\n', start);
  const close = bundle.indexOf('\n    ]);', open);
  if (start < 0 || open < 0 || close < 0) throw new Error('Missing pinned fulfill MIME table');
  const result = {};
  for (const line of bundle.slice(open, close).split('\n').slice(1)) {
    const entry = line.match(/^\s+\["([^"\\]+)", "([a-z0-9!#$&^_.+-]+\/[a-z0-9!#$&^_.+-]+)"\],?$/);
    if (!entry) throw new Error(`Invalid fulfill MIME entry: ${line.trim()}`);
    // `new Map(entries)` keeps the last value for a repeated extension.
    result[entry[1]] = entry[2];
  }
  return JSON.stringify(Object.fromEntries(Object.entries(result).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)), null, 2) + '\n';
}

async function main() {
  const [packageRoot, mode] = process.argv.slice(2);
  if (!packageRoot || mode && mode !== '--check') throw new Error('Supply PLAYWRIGHT_CORE [--check]');
  const bundle = await readFile(resolve(packageRoot, 'lib/utilsBundle.js'));
  if (digest(bundle) !== sourceSha256) throw new Error('Pinned MIME bundle changed');
  const data = mimeTypes(bundle.toString('utf8'));
  const notices = await readFile(resolve(packageRoot, 'lib/utilsBundle.js.LICENSE'), 'utf8');
  const license = notices.split('%% mime@4.1.0 NOTICES AND INFORMATION BEGIN HERE\n')[1]?.split('END OF mime@4.1.0 NOTICES AND INFORMATION')[0]?.trim();
  if (!license?.includes('Copyright (c) 2023 Robert Kieffer') || !license.includes('Permission is hereby granted')) throw new Error('Missing pinned MIME license');
  const core = await readFile(resolve(packageRoot, 'lib/coreBundle.js'));
  if (digest(core) !== coreSha256) throw new Error('Pinned fulfill MIME bundle changed');
  const fulfill = fulfillTypes(core.toString('utf8'));
  const manifest = JSON.stringify({playwrightVersion:'1.64.1',mimeVersion:'4.1.0',source:'lib/utilsBundle.js',sourceSha256,
    data:'src/browser/http/mime-types.json',dataSha256:digest(data),license:'third-party/browser/mime-notices/LICENSE',
    fulfillSource:'lib/coreBundle.js',fulfillSourceSha256:coreSha256,fulfillData:'src/browser/routing/fulfill-mime.json',fulfillDataSha256:digest(fulfill)}, null, 2) + '\n';
  const files = [['src/browser/http/mime-types.json',data],['src/browser/routing/fulfill-mime.json',fulfill],['third-party/browser/mime-notices/LICENSE',license+'\n'],['tools/browser-provenance/http-mime.json',manifest]];
  for (const [file, value] of files) {
    if (mode === '--check') {
      if (await readFile(resolve(root,file),'utf8') !== value) throw new Error(`Pinned MIME data drift: ${file}`);
    } else {
      await mkdir(resolve(root,file,'..'),{recursive:true});
      await writeFile(resolve(root,file),value);
    }
  }
  process.stdout.write('Verified pinned HTTP MIME inference data\n');
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {process.stderr.write(error.message+'\n');process.exitCode=1;});
}
