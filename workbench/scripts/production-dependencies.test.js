const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { productionDependencies } = require('./production-dependencies.cjs');

function write(root, dir, manifest) {
  fs.mkdirSync(path.join(root, dir), { recursive: true });
  fs.writeFileSync(path.join(root, dir, 'package.json'), JSON.stringify(manifest));
}

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-dependencies-'));
  write(root, '.', {
    name: 'acme-extension',
    dependencies: { 'acme-render': '1.0.0' },
    optionalDependencies: { 'acme-native-darwin': '1.0.0', 'acme-native-linux': '1.0.0' },
    devDependencies: { 'acme-bundler': '1.0.0' },
  });
  write(root, 'node_modules/acme-render', { name: 'acme-render', dependencies: { 'acme-utils': '2.0.0' }, peerDependencies: { 'acme-peer': '1.0.0' } });
  write(root, 'node_modules/acme-render/node_modules/acme-utils', { name: 'acme-utils', version: '2.0.0', dependencies: { 'acme-shared': '1.0.0' } });
  write(root, 'node_modules/acme-utils', { name: 'acme-utils', version: '1.0.0' });
  write(root, 'node_modules/acme-shared', { name: 'acme-shared' });
  write(root, 'node_modules/acme-native-darwin', { name: 'acme-native-darwin' });
  write(root, 'node_modules/acme-bundler', { name: 'acme-bundler', dependencies: { 'acme-cli': '1.0.0' } });
  write(root, 'node_modules/acme-cli', { name: 'acme-cli' });
  return root;
}

test('production dependencies follow nested and hoisted placement, without dev-only packages', () => {
  const root = fixture();
  try {
    assert.deepEqual(productionDependencies(root).map(dir => path.relative(root, dir).split(path.sep).join('/')), [
      'node_modules/acme-native-darwin',
      'node_modules/acme-render',
      'node_modules/acme-render/node_modules/acme-utils',
      'node_modules/acme-shared',
    ]);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('a missing required dependency stops packaging', () => {
  const root = fixture();
  try {
    fs.rmSync(path.join(root, 'node_modules/acme-shared'), { recursive: true });
    assert.throws(() => productionDependencies(root), /Missing production dependency acme-shared of acme-utils/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('the extension ships its runtime dependencies and none of its tooling', () => {
  const names = productionDependencies(path.resolve(__dirname, '..')).map(dir => {
    const parts = dir.split(path.sep);
    const at = parts.lastIndexOf('node_modules');
    return parts[at + 1].startsWith('@') ? parts[at + 1] + '/' + parts[at + 2] : parts[at + 1];
  });
  for (const name of ['esbuild', 'lucide', 'marked', 'highlight.js', 'picomatch', 'tar', '@vue/compiler-sfc']) assert.ok(names.includes(name), name);
  for (const name of ['@vscode/vsce', '@electron/packager', 'astro', 'typescript', 'xmlbuilder', 'commander']) assert.ok(!names.includes(name), name);
});
