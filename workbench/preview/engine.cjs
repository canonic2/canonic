const fs = require('node:fs');
const path = require('node:path');
const bundled = path.join(__dirname, 'bin', process.platform === 'win32' ? 'esbuild.exe' : 'esbuild');
// esbuild reads this at import time. Packaged workers use the binary supplied
// by their platform VSIX, rather than a project dependency or a host install.
if (fs.existsSync(bundled)) {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, 'bin', 'compiler.json'), 'utf8'));
  if (manifest.target !== process.platform + '-' + process.arch || manifest.version !== require('esbuild/package.json').version) {
    throw new Error('Bundled preview compiler does not match this host or esbuild version. Rebuild Workbench for this platform.');
  }
  process.env.ESBUILD_BINARY_PATH = bundled;
}
module.exports = require('esbuild');
