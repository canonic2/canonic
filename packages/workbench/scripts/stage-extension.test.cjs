const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { stageExtension } = require('./stage-extension.cjs');

const separatedUi = [
  'ui/src/views/toolbar/bindings.js', 'ui/src/views/toolbar/legacy-presentation.js', 'ui/src/views/toolbar/toolbar.css',
  'ui/src/views/configure-pages/config-editor.js', 'ui/src/views/configure-pages/config-editor.css',
  'ui/src/views/export/bindings.js', 'ui/src/views/export/export.css', 'ui/src/views/export/register-export.js',
  'ui/src/views/export/components/export-dialog/element.js', 'ui/src/views/export/components/export-dialog/styles.js',
  'ui/src/views/top-bar/bindings.js', 'ui/src/views/top-bar/presentation.js', 'ui/src/views/top-bar/size-bindings.js',
  'ui/src/views/page-list/browser-bindings.js', 'ui/src/views/canvas/legacy-bindings.js',
  'ui/src/modules/project/configuration.js', 'ui/src/modules/review/browser-export.js',
  'ui/src/modules/shortcuts/shell.js', 'ui/src/modules/canvas/legacy-sizes.js', 'ui/src/modules/docs/legacy-rules.js',
];

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-workbench-package-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const write = (file, body) => {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), body);
  };
  write('package.json', JSON.stringify({ version: '1.0.0', dependencies: { 'acme-runtime': '1.0.0' } }));
  write('extension/package.json', JSON.stringify({ name: 'acme-workbench', version: '1.0.0', main: './extension.js',
    contributes: { viewsContainers: { activitybar: [{ icon: 'icon.svg' }] } } }));
  write('sdk/package.json', JSON.stringify({ version: '1.0.0' }));
  for (const file of ['extension/extension.js', 'extension/icon.svg', 'extension/.vscodeignore', 'capture/app/main.cjs',
    'ui/src/app/workbench/index.html', 'ui/src/app/renderer/content.html', 'ui/src/app/renderer/content.css', 'ui/legacy/content-frames.js',
    'ui/src/modules/preview-sessions/content.js', 'ui/src/modules/preview-sessions/renderer-bridge.js',
    'ui/src/app/sidebar/sidebar.js', 'ui/src/app/renderer/renderer.js',
    'ui/src/modules/preview-sessions/index.js', 'ui/src/modules/navigation/index.js',
    'ui/src/modules/canvas/index.js', 'ui/src/modules/review/index.js', 'server/node/src/modules/review/prompt.ts',
    'contracts/manifest.js', 'server/node/server.js',
    'server/bin/acme/workbench-server', 'README.md', 'LICENSE', 'CHANGELOG.md', ...separatedUi]) write(file, 'Acme');
  write('ui/src/views/toolbar/legacy-presentation.test.js', 'Test source');
  write('server/target/debug/workbench-server', 'Build output');
  write('server/node/server.test.js', 'Test source');
  write('node_modules/acme-runtime/package.json', JSON.stringify({ name: 'acme-runtime', version: '1.0.0' }));
  write('node_modules/acme-runtime/index.js', 'module.exports = 1;');
  write('node_modules/acme-dev/package.json', JSON.stringify({ name: 'acme-dev', version: '1.0.0' }));
  return { root, write };
}

test('the VSIX assembly preserves cross-part paths and includes only production dependencies and selected binaries', t => {
  const { root } = fixture(t);
  const stage = stageExtension(root);
  t.after(() => fs.rmSync(stage, { recursive: true, force: true }));
  const manifest = JSON.parse(fs.readFileSync(path.join(stage, 'package.json'), 'utf8'));
  assert.equal(manifest.main, './extension/extension.js');
  assert.equal(manifest.icon, 'extension/icon.png');
  assert.equal(manifest.contributes.viewsContainers.activitybar[0].icon, 'extension/icon.svg');
  for (const file of [manifest.main, 'server/node/server.js', 'ui/src/app/renderer/content.html', 'ui/src/app/renderer/content.css', 'ui/legacy/content-frames.js',
    'ui/src/modules/preview-sessions/content.js', 'ui/src/modules/preview-sessions/renderer-bridge.js',
    'ui/src/app/sidebar/sidebar.js', 'ui/src/app/renderer/renderer.js',
    'ui/src/modules/preview-sessions/index.js', 'ui/src/modules/navigation/index.js',
    'ui/src/modules/canvas/index.js', 'ui/src/modules/review/index.js', 'server/node/src/modules/review/prompt.ts', 'contracts/manifest.js',
    'server/bin/acme/workbench-server', 'node_modules/acme-runtime/index.js']) assert.ok(fs.existsSync(path.join(stage, file)), file);
  for (const file of ['server/target', 'server/node/server.test.js', 'node_modules/acme-dev']) assert.ok(!fs.existsSync(path.join(stage, file)), file);
  for (const file of separatedUi) {
    const shipped = path.join(stage, file);
    assert.equal(fs.readFileSync(shipped, 'utf8'), 'Acme', file);
    assert.equal(fs.lstatSync(shipped).isSymbolicLink(), false, file);
  }
  assert.equal(fs.existsSync(path.join(stage, 'ui/src/views/toolbar/legacy-presentation.test.js')), false);
  assert.equal(JSON.parse(fs.readFileSync(path.join(root, 'extension/package.json'), 'utf8')).main, './extension.js');
});

test('a version mismatch or missing production dependency stops packaging', t => {
  const { root, write } = fixture(t);
  write('sdk/package.json', JSON.stringify({ version: '2.0.0' }));
  assert.throws(() => stageExtension(root), /versions must match/);
  write('sdk/package.json', JSON.stringify({ version: '1.0.0' }));
  fs.rmSync(path.join(root, 'node_modules/acme-runtime'), { recursive: true });
  assert.throws(() => stageExtension(root), /Missing production dependency acme-runtime/);
});
