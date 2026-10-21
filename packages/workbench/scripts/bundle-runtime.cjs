/* Build-time download only. The installed extension launches this runtime
   directly and never installs dependencies or changes the editor's flags. */
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var crypto = require('node:crypto');
var tar = require('tar');
var ROOT = path.resolve(__dirname, "..");
var VERSION = '44.2.0';
var TARGETS = ['darwin-x64', 'darwin-arm64', 'win32-x64', 'win32-arm64', 'linux-x64', 'linux-arm64'];

async function digest(file) {
  var hash = crypto.createHash('sha256');
  for await (var chunk of fs.createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

function inputs(root, target) {
  var hash = crypto.createHash('sha256');
  hash.update(JSON.stringify({ target: target, electron: VERSION }));
  function record(file, relative, ancestors = new Set()) {
    var real = fs.realpathSync(file);
    if (ancestors.has(real)) throw new Error('Circular runtime input: ' + file);
    var stat = fs.statSync(file);
    hash.update(JSON.stringify([relative, stat.mode, stat.isDirectory() ? 'directory' :
      crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')]));
    if (stat.isDirectory()) {
      var next = new Set(ancestors).add(real);
      for (var name of fs.readdirSync(file).sort()) record(path.join(file, name), relative + '/' + name, next);
    }
  }
  record(__filename, 'bundler');
  record(path.join(root, 'pnpm-lock.yaml'), 'lockfile');
  record(path.join(root, 'capture/app'), 'app');
  return hash.digest('hex');
}

async function bundle(target, options = {}) {
  if (!TARGETS.includes(target)) throw new Error('Unsupported Electron runtime target: ' + target);
  var root = options.root || ROOT;
  var destination = path.join(root, 'capture/runtime');
  var sourceHash = inputs(root, target);
  if (options.reuse) {
    try {
      var cached = JSON.parse(fs.readFileSync(path.join(destination, 'runtime.json'), 'utf8'));
      if (cached && cached.target === target && cached.electron === VERSION && cached.sourceSha256 === sourceHash &&
          cached.sha256 === await digest(path.join(destination, 'runtime.tar.gz'))) {
        console.log('Reused Electron runtime for ' + target);
        return;
      }
    } catch (error) {
      if (!(error instanceof SyntaxError) && error.code !== 'ENOENT') throw error;
    }
  }
  var parts = target.split('-');
  var work = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-electron-build-'));
  try {
    var packager = options.packager || (await import('@electron/packager')).packager;
    var results = await packager({
      dir: path.join(root, 'capture/app'), out: work,
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
    fs.writeFileSync(path.join(destination, 'runtime.json'), JSON.stringify({ target: target, electron: VERSION, sha256: await digest(archive), sourceSha256: sourceHash }) + '\n');
    console.log('Bundled Electron runtime for capture and previews on ' + target);
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

if (require.main === module) bundle(process.argv.slice(2).find(arg => !arg.startsWith('--')) || process.platform + '-' + process.arch, { reuse: process.argv.includes('--reuse') }).catch(function (error) {
  console.error(error); process.exitCode = 1;
});
module.exports = { bundle: bundle, targets: TARGETS };
