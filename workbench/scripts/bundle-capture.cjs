/* Build-time download only. The installed extension launches this runtime
   directly and never installs dependencies or changes the editor's flags. */
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var crypto = require('node:crypto');
var tar = require('tar');
var ROOT = path.resolve(__dirname, '..');
var VERSION = '44.2.0';
var TARGETS = ['darwin-x64', 'darwin-arm64', 'win32-x64', 'win32-arm64', 'linux-x64', 'linux-arm64'];

async function bundle(target) {
  if (!TARGETS.includes(target)) throw new Error('Unsupported capture target: ' + target);
  var parts = target.split('-');
  var work = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-capture-build-'));
  try {
    var packager = (await import('@electron/packager')).packager;
    var results = await packager({
      dir: path.join(ROOT, 'capture-helper'), out: work,
      name: 'Canonic Capture', executableName: 'canonic-capture',
      appBundleId: 'com.canonic.capture', electronVersion: VERSION,
      platform: parts[0], arch: parts[1], overwrite: true, prune: false,
      // VS Code's Electron filesystem treats .asar paths as directories.
      // Plain helper sources let its extension host unpack this app normally.
      asar: false,
      extendInfo: { LSUIElement: true },
      // A local archive cache is useful for offline builds and CI. Packager
      // otherwise downloads the pinned official Electron distribution.
      electronZipDir: process.env.CANONIC_ELECTRON_ZIP_DIR || undefined,
      download: { cacheRoot: path.join(os.tmpdir(), 'canonic-electron-cache') },
    });
    var destination = path.join(ROOT, 'capture-runtime');
    fs.rmSync(destination, { recursive: true, force: true });
    fs.mkdirSync(destination, { recursive: true });
    var archive = path.join(destination, 'runtime.tar.gz');
    // The archive hash is also the installed runtime's cache key. Packager
    // writes fresh mtimes on every build, so normalize tar metadata; otherwise
    // an unchanged helper is needlessly unpacked again after every VSIX install.
    await tar.c({
      file: archive, gzip: true, cwd: results[0],
      portable: true, mtime: new Date(0),
    }, ['.']);
    var hash = crypto.createHash('sha256');
    for await (var chunk of fs.createReadStream(archive)) hash.update(chunk);
    fs.writeFileSync(path.join(destination, 'runtime.json'), JSON.stringify({ target: target, electron: VERSION, sha256: hash.digest('hex') }) + '\n');
    console.log('Bundled background capture helper for ' + target);
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

if (require.main === module) bundle(process.argv[2] || process.platform + '-' + process.arch).catch(function (error) {
  console.error(error); process.exitCode = 1;
});
module.exports = { bundle: bundle, targets: TARGETS };
