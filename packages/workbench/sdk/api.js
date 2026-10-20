// Definitions are ordinary modules. The compiler supplies this import without
// installing a package in the consuming project. The same API runs in exports.
export function definePreview(preview) { return preview; }
// A docs page: Markdown with examples its lenses render. `kind` tells the
// compiler's index it from a preview.
export function defineDocs(docs) { return Object.assign({}, docs, { kind: 'docs' }); }
export function defineConfig(config) { return config; }
export function defineAdapter(adapter) { return adapter; }
// Compiler plugins: plain descriptions of capabilities Workbench's compiler
// provides. The project's paths and names are their options.
export function virtualModule(options) { return Object.assign({ plugin: 'virtual-module' }, options); }
export function resolveImport(options) { return Object.assign({ plugin: 'resolve' }, options); }
export function importGlob(options) { return Object.assign({ plugin: 'import-glob' }, options); }
export function svgSprite(options) { return Object.assign({ plugin: 'svg-sprite' }, options); }
