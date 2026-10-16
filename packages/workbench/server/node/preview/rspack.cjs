const fs = require('node:fs');
const path = require('node:path');
const bundled = path.join(__dirname, "./bin/rspack.node");

// Packaged workers use the native binding supplied by their platform VSIX,
// rather than a project dependency or the host's optional package. Rspack's
// loader reads NAPI_RS_NATIVE_LIBRARY_PATH when it is first required. Other
// napi-rs modules read it too, such as Astro's compiler, so it is set only
// while Rspack loads its binding.
function load() {
  if (!fs.existsSync(bundled)) return require('@rspack/core');
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "./bin/compiler.json"), 'utf8'));
  if (manifest.target !== process.platform + '-' + process.arch || manifest.rspack !== require('@rspack/core/package.json').version) {
    throw new Error('Bundled Rspack binding does not match this host or Rspack version. Rebuild Workbench for this platform.');
  }
  const previous = process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  process.env.NAPI_RS_NATIVE_LIBRARY_PATH = bundled;
  try { return require('@rspack/core'); }
  finally {
    if (previous === undefined) delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
    else process.env.NAPI_RS_NATIVE_LIBRARY_PATH = previous;
  }
}

// Rspack 2 is published as ES modules only; Node 24 requires them directly.
module.exports = load();
