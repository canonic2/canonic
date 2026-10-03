// Definitions are ordinary modules. The compiler supplies this import without
// installing a package in the consuming project. The same API runs in exports.
export function definePreview(preview) { return preview; }
export function defineConfig(config) { return config; }
export function defineAdapter(adapter) { return adapter; }
