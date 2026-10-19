/* Static fixture host for UI tests. No Workbench server or compiler imports. */
const fs = require('node:fs/promises');
const path = require('node:path');

exports.createUiAssets = function (root) {
  return async function (address) {
    const relative = decodeURIComponent(new URL(address, 'http://example.com').pathname).replace(/^\/_workbench\//, '');
    const file = path.resolve(root, relative);
    if (!file.startsWith(root + path.sep) || !/\.(?:js|css)$/.test(file)) throw new Error('Invalid fixture asset: ' + relative);
    return { status: 200, type: file.endsWith('.css') ? 'text/css' : 'text/javascript', body: await fs.readFile(file) };
  };
};
