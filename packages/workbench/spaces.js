/* Spaces
   ------
   The workbenches one window, or one `node server.js`, can switch between.
   They come from workbench.yaml files: a file without `spaces` is one
   space, served from its own folder, and a file with `spaces` is one per
   entry, each served from its own `root` — the file's folder unless it says
   otherwise, so several spaces can share a folder. Each space gets a
   server of its own: the server, the config readers, the preview worker, and
   every route assume one root and one config, so a second space is a
   second server rather than a second space threaded through all of them.
   A single space is one server.

   Servers start the first time a space is opened, not when it is listed:
   listing a space costs one read of its workbench.yaml, while starting one
   can open terminals and compile previews.

   A space's id is a hash of its file's folder, and of its key when the file
   lists several, so it is the same in every window, in every run, and in the
   canvas's address, without saying where the folder is. Its default mark —
   the letter and colour drawn beside its name — comes from that id too, so a
   space keeps its colour wherever it is shown. */

var crypto = require('crypto');
var fs = require('fs');
var path = require('path');
var config = require('./config');

var COLORS = ['blue', 'green', 'orange', 'purple', 'pink', 'teal'];

function spaceId(dir, key) {
  var at = path.resolve(dir);
  return crypto.createHash('sha1').update(key ? at + '#' + key : at).digest('hex').slice(0, 10);
}

function hasWorkbench(dir) {
  return fs.existsSync(path.join(dir, config.FILE));
}

var MAX_IMAGE = 256 * 1024;
var IMAGE_TYPES = {
  '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg',
  '.webp': 'image/webp', '.gif': 'image/gif',
};

/* What workbench.yaml says about the space: its `name`, `color` and
   `icon`, with workbench.local.yaml over them. A file that doesn't parse
   still lists the space, under its folder name with the default mark, so it
   can be opened and its error read. */
function read(space) {
  try {
    return config.read({ dir: space.dir, key: space.key }) || {};
  } catch (error) {
    return {};
  }
}

function initialOf(name) {
  var match = /[\p{L}\p{N}]/u.exec(String(name || ''));
  return match ? match[0].toLocaleUpperCase() : '?';
}

/* An image icon travels as a data URI. Each space is its own server and
   its own folder, and the switcher draws every space's mark wherever it
   is — the editor's sidebar, another space's canvas — so the image can't
   be fetched from where it lives. One that is missing, too big or outside
   the space's root leaves the letter in its place. */
function imageOf(root, file) {
  if (!file) return null;
  var full = path.resolve(root, file);
  if (full.indexOf(path.resolve(root) + path.sep) !== 0) return null;
  var type = IMAGE_TYPES[path.extname(full).toLowerCase()];
  try {
    var stat = fs.statSync(full);
    if (!type || !stat.isFile() || stat.size > MAX_IMAGE) return null;
    return 'data:' + type + ';base64,' + fs.readFileSync(full).toString('base64');
  } catch (error) {
    return null;
  }
}

/* A space as the switchers draw it. `space` is { dir, key, root,
   removable }; `dir` holds its workbench.yaml and `root` is what it serves. */
function describe(space) {
  var dir = path.resolve(space.dir || space.root);
  var key = space.key || null;
  var id = spaceId(dir, key);
  var found = read({ dir: dir, key: key });
  var root = path.resolve(space.root || found.root || dir);
  var mark = found.mark || {};
  var name = found.name || path.basename(root);
  var image = imageOf(root, mark.image);
  /* An image is drawn as it is, on no colour unless the space names one. */
  var color = mark.color || (image ? null : COLORS[parseInt(id.slice(0, 8), 16) % COLORS.length]);
  return {
    id: id,
    key: key,
    name: name,
    dir: dir,
    root: root,
    initial: initialOf(name),
    color: color,
    icon: mark.icon || null,
    image: image,
    removable: !!space.removable,
  };
}

/* `start(space)` is given { dir, key, root } and answers with a running
   server ({ url, close() }). `set` takes folders as { dir, removable } in the
   order they are listed; removable marks a folder the user added, as against
   one that came with the window. */
function create(options) {
  var start = options.start;
  var folders = [];
  var running = {}; /* id -> { root, server: Promise of the running server } */
  var closed = false;

  /* Every folder's spaces, read fresh: a saved workbench.yaml may have
     added, removed or moved one. A space listed by two folders' files is
     the first one's. */
  function expand() {
    var seen = {};
    var out = [];
    folders.forEach(function (folder) {
      config.list(folder.dir).forEach(function (found) {
        var id = spaceId(folder.dir, found.key);
        if (seen[id]) return;
        seen[id] = true;
        out.push({ id: id, dir: folder.dir, key: found.key, root: found.root, removable: folder.removable });
      });
    });
    return out;
  }

  /* Servers whose space left the list, or now serves another root, stop. */
  function settle(spaces) {
    var current = {};
    spaces.forEach(function (space) { current[space.id] = space; });
    return Promise.all(Object.keys(running).filter(function (id) {
      return !current[id] || current[id].root !== running[id].root;
    }).map(stop));
  }

  function set(next) {
    var seen = {};
    folders = (next || []).map(function (entry) {
      return { dir: path.resolve(entry.dir || entry.root), removable: !!entry.removable };
    }).filter(function (folder) {
      if (seen[folder.dir]) return false;
      seen[folder.dir] = true;
      return true;
    });
    return settle(expand());
  }

  function stop(id) {
    var pending = running[id];
    delete running[id];
    return Promise.resolve(pending && pending.server).then(function (server) {
      if (server) return server.close();
    }).catch(function () {});
  }

  function list() {
    var spaces = expand();
    settle(spaces);
    return spaces.map(describe);
  }

  /* The running server for a listed space, started if it isn't. A start
     that fails is forgotten, so opening the space again tries again. */
  function open(id) {
    if (closed) return Promise.reject(new Error('Workbench has stopped.'));
    var space = expand().filter(function (candidate) { return candidate.id === id; })[0];
    if (!space) return Promise.reject(new Error('That space isn’t in the Workbench list.'));
    if (running[id] && running[id].root !== space.root) stop(id);
    if (!running[id]) {
      var entry = { root: space.root, server: null };
      entry.server = Promise.resolve().then(function () {
        return start({ dir: space.dir, key: space.key, root: space.root });
      });
      entry.server.catch(function () {
        if (running[id] === entry) delete running[id];
      });
      running[id] = entry;
    }
    return running[id].server;
  }

  /* The servers already up, without starting any. */
  function started(id) {
    return running[id] ? running[id].server : null;
  }

  function close() {
    closed = true;
    return Promise.all(Object.keys(running).map(stop));
  }

  return { set: set, list: list, open: open, started: started, close: close };
}

module.exports = {
  create: create,
  spaceId: spaceId,
  hasWorkbench: hasWorkbench,
  describe: describe,
  COLORS: COLORS,
};
