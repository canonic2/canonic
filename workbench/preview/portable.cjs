const fs = require('node:fs');
const path = require('node:path');

// This package is shared by the editor ZIP exporter and the standalone CLI.
async function create(compiler, options = {}) {
  const index = await compiler.index();
  const files = [];
  const previews = [];
  const warnings = index.errors.slice();
  function add(name, body) { files.push({ path: 'browser/' + name, data: Buffer.from(body).toString('base64') }); }
  for (const preview of index.previews) {
    try {
      const result = await compiler.compile(preview.file, false);
      for (const [name, body] of result.outputs) add(result.slug + '/' + name, body);
      previews.push({ ...preview, ...(result.portableNote ? { controls: {}, docs: (preview.docs || '') + '\n' + result.portableNote } : {}), directory: 'browser/' + result.slug, files: result.localFiles,
        packages: result.packages, revision: result.revision });
    } catch (error) { warnings.push(preview.file + ': ' + error.message); }
  }
  for (const file of ['index.html', 'viewer.js', 'viewer.css']) add(file, fs.readFileSync(path.join(__dirname, 'viewer', file)));
  for (const file of ['preview-controls.js', 'preview-controls.css']) add(file, fs.readFileSync(path.join(__dirname, '..', 'workbench', file)));
  add('workbench.json', JSON.stringify({ version: 1, name: options.name || path.basename(compiler.root),
    previews: previews.map(({ files, ...preview }) => preview), warnings }, null, 2));
  return { files, previews, warnings };
}

module.exports = { create };
