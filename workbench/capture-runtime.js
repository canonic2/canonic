/* Preserve macOS framework links inside the VSIX. Unpack the bundled runtime
   once, then reuse it; the content hash separates updates. Concurrent hosts
   publish a complete cache directory atomically. No runtime network access. */
var crypto = require('node:crypto');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var tar = require('tar');
var BUNDLE = path.join(__dirname, 'capture-runtime');

function executable(root, platform) {
  platform = platform || process.platform;
  if (platform === 'darwin') return path.join(root, 'Canonic Capture.app/Contents/MacOS/canonic-capture');
  return path.join(root, platform === 'win32' ? 'canonic-capture.exe' : 'canonic-capture');
}

function available() {
  if (process.platform === 'linux' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) return false;
  if (process.platform === 'darwin' && Number(os.release().split('.')[0]) < 22) return false;
  try {
    var manifest = JSON.parse(fs.readFileSync(path.join(BUNDLE, 'runtime.json'), 'utf8'));
    return manifest.target === process.platform + '-' + process.arch && fs.existsSync(path.join(BUNDLE, 'runtime.tar.gz'));
  } catch (_) { return false; }
}

/* Each update ships a new hash, and a runtime is hundreds of megabytes. Once
   the current one is in place, the others go, along with any staging
   directory an interrupted extraction left behind. Best effort: a directory
   that won't go is left for the next activation. */
var STALE_STAGE = 60 * 60 * 1000;

async function prune(storage, keep) {
  var entries;
  try { entries = await fs.promises.readdir(storage); } catch (_) { return; }
  for (var i = 0; i < entries.length; i++) {
    var entry = entries[i];
    var remove = /^[a-f0-9]{64}$/.test(entry) && entry !== keep;
    if (!remove && entry.indexOf('.unpack-') === 0) {
      try { remove = Date.now() - (await fs.promises.stat(path.join(storage, entry))).mtimeMs > STALE_STAGE; } catch (_) {}
    }
    if (!remove) continue;
    try { await fs.promises.rm(path.join(storage, entry), { recursive: true, force: true, maxRetries: 2 }); } catch (_) {}
  }
}

async function prepare(storage, bundle) {
  bundle = bundle || BUNDLE;
  var manifest = JSON.parse(await fs.promises.readFile(path.join(bundle, 'runtime.json'), 'utf8'));
  if (manifest.target !== process.platform + '-' + process.arch || !/^[a-f0-9]{64}$/.test(manifest.sha256)) {
    throw new Error('The bundled capture runtime does not match this extension host');
  }
  storage = storage || path.join(os.tmpdir(), 'canonic-capture-runtime');
  var destination = path.join(storage, manifest.sha256);
  var binary = executable(destination);
  if (!fs.existsSync(binary)) await extract(bundle, manifest, storage, destination, binary);
  await prune(storage, manifest.sha256);
  return binary;
}

async function extract(bundle, manifest, storage, destination, binary) {
  await fs.promises.mkdir(storage, { recursive: true });
  var stage = await fs.promises.mkdtemp(path.join(storage, '.unpack-'));
  try {
    var archive = path.join(bundle, 'runtime.tar.gz');
    var hash = crypto.createHash('sha256');
    for await (var chunk of fs.createReadStream(archive)) hash.update(chunk);
    if (hash.digest('hex') !== manifest.sha256) throw new Error('The capture runtime archive is damaged');
    await tar.x({ file: archive, cwd: stage, strict: true });
    await fs.promises.access(executable(stage));
    try { await fs.promises.rename(stage, destination); }
    catch (error) {
      if (!['EEXIST', 'ENOTEMPTY', 'EPERM'].includes(error.code) || !fs.existsSync(binary)) throw error;
    }
  } finally { await fs.promises.rm(stage, { recursive: true, force: true }); }
}

module.exports = { executable: executable, available: available, prepare: prepare, prune: prune };
