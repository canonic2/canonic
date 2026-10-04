/* Runs vsce with the extension's production dependencies taken from
   productionDependencies() instead of `npm list`, which misreads the pnpm
   tree. vsce resolves dependencies through its npm module's exported
   getDependencies at call time, so replacing that export is enough. */
const path = require('node:path');
const { productionDependencies } = require('./production-dependencies.cjs');

const npm = require('@vscode/vsce/out/npm');
if (typeof npm.getDependencies !== 'function') throw new Error('This vsce version has no getDependencies to replace; update scripts/vsce.cjs.');
npm.getDependencies = async function (cwd, dependencies) {
  if (dependencies === 'none') return [cwd];
  return [path.resolve(cwd)].concat(productionDependencies(cwd));
};

require('@vscode/vsce/out/main')(process.argv);
