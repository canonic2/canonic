const test = require('node:test');
const assert = require('node:assert/strict');

// Other napi-rs modules, such as Astro's compiler, read the same variable the
// bundled Rspack binding is loaded through; it must not outlive Rspack's load.
test('loading Rspack leaves the napi-rs library path as it was', () => {
  const previous = process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  const rspack = require('./preview/rspack.cjs');
  assert.equal(typeof rspack.rspack, 'function');
  assert.equal(process.env.NAPI_RS_NATIVE_LIBRARY_PATH, previous);
});
