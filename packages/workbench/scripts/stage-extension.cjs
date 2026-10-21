/* Assemble a temporary VSIX root from the product's peer source owners. */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { productionDependencies } = require('./production-dependencies.cjs');

function stageExtension(root) {
  const destination = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-workbench-vsix-'));
  try {
    const product = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
    const manifest = JSON.parse(fs.readFileSync(path.join(root, 'extension/package.json'), 'utf8'));
    const sdk = JSON.parse(fs.readFileSync(path.join(root, 'sdk/package.json'), 'utf8'));
    if (product.version !== manifest.version || sdk.version !== manifest.version) throw new Error('Workbench product, extension and SDK versions must match');
    const filter = file => !/\.test\.[cm]?[jt]s$/.test(file) && path.basename(file) !== 'node_modules';
    for (const directory of ['extension', 'capture', 'ui', 'sdk', 'contracts', 'server/node', 'server/bin']) {
      fs.cpSync(path.join(root, directory), path.join(destination, directory), { recursive: true, filter });
    }
    for (const file of ['README.md', 'LICENSE', 'CHANGELOG.md']) fs.copyFileSync(path.join(root, file), path.join(destination, file));
    for (const dependency of productionDependencies(root)) {
      fs.cpSync(dependency, path.join(destination, path.relative(root, dependency)), {
        recursive: true, dereference: true, filter: file => path.basename(file) !== 'node_modules',
      });
    }
    fs.copyFileSync(path.join(root, 'extension/.vscodeignore'), path.join(destination, '.vscodeignore'));
    for (const container of manifest.contributes?.viewsContainers?.activitybar || []) {
      container.icon = 'extension/' + container.icon;
    }
    fs.writeFileSync(path.join(destination, 'package.json'), JSON.stringify({
      ...manifest, main: './extension/extension.js', icon: 'extension/icon.png', dependencies: product.dependencies,
    }, null, 2) + '\n');
    return destination;
  } catch (error) {
    fs.rmSync(destination, { recursive: true, force: true });
    throw error;
  }
}

module.exports = { stageExtension };
