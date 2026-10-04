// Definitions are ordinary modules. The compiler supplies this import without
// installing a package in the consuming project. The same API runs in exports.
export function definePreview(preview) { return preview; }
// A docs page: Markdown with examples its lenses render. `kind` tells the
// compiler's index it from a preview.
export function defineDocs(docs) { return Object.assign({}, docs, { kind: 'docs' }); }
export function defineConfig(config) { return config; }
export function defineAdapter(adapter) { return adapter; }
