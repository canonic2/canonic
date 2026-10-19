const fs = require('node:fs');
const path = require('node:path');

// The portable compiler: Rspack production builds with the project's plugins.
function portableBuilder(compiler) {
  const { validate, docsRequest, mockFetches } = require('./compiler.cjs');
  const { PortableBuilder } = require('../src/modules/preview-pipeline/portable.ts');
  return new PortableBuilder(compiler, { validate, docsRequest, mockFetches, createAstro: (root, target, config) => require('./astro.cjs').create(root, target, config) },
    { previewDir: path.resolve(__dirname, '../../../ui/preview') });
}

// This package is shared by the editor ZIP exporter and the standalone CLI.
async function create(compiler, options = {}) {
  const builder = portableBuilder(compiler);
  try { return await build(compiler, builder, options); } finally { await builder.close(); }
}

async function build(compiler, builder, options) {
  const index = await compiler.index();
  const files = [];
  const previews = [];
  const warnings = index.errors.slice();
  function add(name, body) { files.push({ path: 'browser/' + name, data: Buffer.from(body).toString('base64') }); }
  for (const preview of index.previews.filter(preview => !options.selection || options.selection.pages.includes(preview.file))) {
    try {
      const result = await builder.compile(preview.file);
      for (const [name, body] of result.outputs) add(result.slug + '/' + name, body);
      previews.push({ ...preview, states: options.selection?.states.length ? preview.states.filter(state => options.selection.states.includes(state.id)) : preview.states,
        ...(result.portableNote ? { controls: {}, docs: (preview.docs || '') + '\n' + result.portableNote } : {}), directory: 'browser/' + result.slug, files: result.localFiles,
        packages: result.packages, revision: result.revision });
    } catch (error) { warnings.push(preview.file + ': ' + error.message); }
  }
  // Docs pages: those workbench.yaml declares and those definitions declare.
  let config = null;
  try { config = require('../config').read(compiler.root); } catch (error) { warnings.push('workbench.yaml: ' + error.message); }
  // Each preview carries its sizes as the space resolves them, so the viewer needs no space.
  const { readPageSizes } = require('../../../contracts/src/sizes/schema.ts');
  const { supported } = require('../../../contracts/src/sizes/browser/choice.js');
  const { defaultSizes } = require('../../../contracts/src/sizes/browser/size.js');
  const spaceSizes = config && config.sizes || defaultSizes();
  for (const preview of previews) {
    preview.sizes = supported(spaceSizes, readPageSizes(preview.sizes, spaceSizes, preview.file, warnings));
    if (options.selection?.sizes.length) preview.sizes = preview.sizes.filter(size => options.selection.sizes.includes(size.key));
  }
  const discovered = (index.docs || []).map(page => ({ src: page.src, page: page.src, label: page.title.split('/').filter(Boolean).pop() || page.id, lens: page.lens, lenses: page.lenses }));
  const { docsPages, portableDocs } = require('../src/modules/docs/index.ts');
  const docs = await portableDocs({ root: compiler.root, compileDocs: request => builder.compileDocs(request) }, docsPages(config, discovered).filter(page => !options.selection || options.selection.pages.includes(page.page || page.src)), add);
  warnings.push(...docs.warnings);
  for (const file of ['index.html', 'viewer.js', 'viewer.css']) add(file, fs.readFileSync(path.join(__dirname, "../../../ui/viewer", file)));
  for (const file of ['preview-controls.js', 'preview-controls.css']) add(file, fs.readFileSync(path.join(__dirname, "../../../ui/legacy", file)));
  add('CANONIC-LICENSE.txt', fs.readFileSync(path.join(__dirname, "../../../LICENSE")));
  add('workbench.json', JSON.stringify({ version: 1, name: options.name || path.basename(compiler.root),
    previews: previews.map(({ files, ...preview }) => preview), docs: docs.pages, warnings }, null, 2));
  return { files, previews, docs: docs.pages, warnings };
}

module.exports = { create, portableBuilder };
