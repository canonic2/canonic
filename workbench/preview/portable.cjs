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
  // Docs pages: those workbench.yaml declares and those definitions declare.
  let config = null;
  try { config = require('../config').read(compiler.root); } catch (error) { warnings.push('workbench.yaml: ' + error.message); }
  // Each preview carries its sizes as the space resolves them, so the viewer needs no space.
  const { readPageSizes } = require('../src/sizes/schema.ts');
  const { supported } = require('../src/sizes/browser/choice.ts');
  const { defaultSizes } = require('../src/sizes/browser/size.ts');
  const spaceSizes = config && config.sizes || defaultSizes();
  for (const preview of previews) preview.sizes = supported(spaceSizes, readPageSizes(preview.sizes, spaceSizes, preview.file, warnings));
  const discovered = (index.docs || []).map(page => ({ src: page.src, label: page.title.split('/').filter(Boolean).pop() || page.id, lens: page.lens, lenses: page.lenses }));
  const { docsPages } = require('../src/docs/pages.ts');
  const { portableDocs } = require('../src/docs/portable.ts');
  const docs = await portableDocs(compiler, docsPages(config, discovered), add);
  warnings.push(...docs.warnings);
  for (const file of ['index.html', 'viewer.js', 'viewer.css']) add(file, fs.readFileSync(path.join(__dirname, 'viewer', file)));
  for (const file of ['preview-controls.js', 'preview-controls.css']) add(file, fs.readFileSync(path.join(__dirname, '..', 'workbench', file)));
  add('CANONIC-LICENSE.txt', fs.readFileSync(path.join(__dirname, '..', 'LICENSE')));
  add('workbench.json', JSON.stringify({ version: 1, name: options.name || path.basename(compiler.root),
    previews: previews.map(({ files, ...preview }) => preview), docs: docs.pages, warnings }, null, 2));
  return { files, previews, docs: docs.pages, warnings };
}

module.exports = { create };
