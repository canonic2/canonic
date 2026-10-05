/* Preserve macOS framework links inside the VSIX. Unpack the bundled runtime
   once, then reuse it; the content hash separates updates. Concurrent hosts
   publish a complete cache directory atomically. No runtime network access. */
var crypto = require('node:crypto');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var tar = require('tar');
var BUNDLE = path.join(__dirname, 'electron-runtime');

function executable(root, platform) {
  platform = platform || process.platform;
  if (platform === 'darwin') return path.join(root, 'Canonic Capture.app/Contents/MacOS/canonic-capture');
  return path.join(root, platform === 'win32' ? 'canonic-capture.exe' : 'canonic-capture');
}

/* Why this host can't run the bundled runtime, or null when it can. */
function unavailable() {
  if (process.platform === 'linux' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) return 'it needs a display on Linux';
  if (process.platform === 'darwin' && Number(os.release().split('.')[0]) < 22) return 'it needs macOS 13 or later';
  try {
    var manifest = JSON.parse(fs.readFileSync(path.join(BUNDLE, 'runtime.json'), 'utf8'));
    if (manifest.target !== process.platform + '-' + process.arch) return 'this build is for ' + manifest.target;
    if (!fs.existsSync(path.join(BUNDLE, 'runtime.tar.gz'))) throw new Error('missing archive');
    return null;
  } catch (_) { return 'this copy has no bundled runtime; run pnpm run bundle-runtime'; }
}

function available() {
  return !unavailable();
}

/* Each update ships a new hash, and a runtime is hundreds of megabytes. Once
   the current one is in place, the others go, along with any staging
   directory an interrupted extraction left behind. Best effort: a directory
   that won't go is left for the next activation. */
var STALE_STAGE = 60 * 60 * 1000;

async function prune(storage, keep) {
  var entries;
  var removed = 0;
  try { entries = await fs.promises.readdir(storage); } catch (_) { return removed; }
  for (var i = 0; i < entries.length; i++) {
    var entry = entries[i];
    var remove = /^[a-f0-9]{64}$/.test(entry) && entry !== keep;
    if (!remove && entry.indexOf('.unpack-') === 0) {
      try { remove = Date.now() - (await fs.promises.stat(path.join(storage, entry))).mtimeMs > STALE_STAGE; } catch (_) {}
    }
    if (!remove) continue;
    try { await fs.promises.rm(path.join(storage, entry), { recursive: true, force: true, maxRetries: 2 }); removed++; } catch (_) {}
  }
  return removed;
}

/* `report(event, details)`, when given, hears how long each step took:
   the first start after an update hashes and unpacks the whole archive. */
async function prepare(storage, bundle, report) {
  bundle = bundle || BUNDLE;
  report = report || function () {};
  var began = Date.now();
  var manifest = JSON.parse(await fs.promises.readFile(path.join(bundle, 'runtime.json'), 'utf8'));
  if (manifest.target !== process.platform + '-' + process.arch || !/^[a-f0-9]{64}$/.test(manifest.sha256)) {
    throw new Error('The bundled Electron runtime does not match this extension host');
  }
  storage = storage || path.join(os.tmpdir(), 'canonic-electron-runtime');
  var destination = path.join(storage, manifest.sha256);
  var binary = executable(destination);
  var timing = { runtime: manifest.sha256.slice(0, 12), extracted: false };
  if (!fs.existsSync(binary)) {
    report('runtime.extract.started', { runtime: timing.runtime, storage: storage });
    Object.assign(timing, await extract(bundle, manifest, storage, destination, binary), { extracted: true });
  }
  var pruning = Date.now();
  timing.pruned = await prune(storage, manifest.sha256);
  timing.pruneMs = Date.now() - pruning;
  timing.elapsedMs = Date.now() - began;
  report('runtime.prepared', timing);
  return binary;
}

async function extract(bundle, manifest, storage, destination, binary) {
  await fs.promises.mkdir(storage, { recursive: true });
  var stage = await fs.promises.mkdtemp(path.join(storage, '.unpack-'));
  var timing = {};
  try {
    var archive = path.join(bundle, 'runtime.tar.gz');
    var hash = crypto.createHash('sha256');
    var hashing = Date.now();
    for await (var chunk of fs.createReadStream(archive)) hash.update(chunk);
    timing.hashMs = Date.now() - hashing;
    if (hash.digest('hex') !== manifest.sha256) throw new Error('The Electron runtime archive is damaged');
    var unpacking = Date.now();
    await tar.x({ file: archive, cwd: stage, strict: true });
    timing.unpackMs = Date.now() - unpacking;
    await fs.promises.access(executable(stage));
    try { await fs.promises.rename(stage, destination); }
    catch (error) {
      if (!['EEXIST', 'ENOTEMPTY', 'EPERM'].includes(error.code) || !fs.existsSync(binary)) throw error;
      // Another start unpacked the same runtime first; this one's work was spent.
      timing.raced = true;
    }
  } finally { await fs.promises.rm(stage, { recursive: true, force: true }); }
  return timing;
}

module.exports = { executable: executable, available: available, unavailable: unavailable, prepare: prepare, prune: prune };
