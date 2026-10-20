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

   The server binary lists and describes spaces (see spaces.rs): a space's
   id is a hash of its file's folder and key, the same in every window and
   run, and its default colour comes from that id. */

var childProcess = require('child_process');
var fs = require('fs');
var path = require('path');
var binaryPath = require('./binary').binaryPath;

var FILE = 'workbench.yaml';

function hasWorkbench(dir) {
  return fs.existsSync(path.join(dir, FILE));
}

/* Every space the folders' workbench.yaml files hold, as the switchers
   draw them: { id, key, name, dir, root, initial, color, icon, image }.
   The server binary reads the files with the same rules as every other
   reader of the configuration; a space listed by two folders is the first
   one's. */
function listed(dirs) {
  if (!dirs.length) return [];
  var out = childProcess.execFileSync(binaryPath(), ['spaces'].concat(dirs), {
    encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'],
  });
  return JSON.parse(out);
}

/* One space as the switchers draw it. `space` is { dir, key, root,
   removable }; `dir` holds its workbench.yaml and `root` is what it serves. */
function describe(space) {
  var dir = path.resolve(space.dir || space.root);
  var key = space.key || null;
  var found = listed([dir]).filter(function (candidate) { return candidate.key === key; })[0];
  var root = path.resolve(space.root || (found && found.root) || dir);
  var described = found || { id: null, key: key, name: path.basename(root), dir: dir, root: root, initial: '?', color: null, icon: null, image: null };
  return Object.assign({}, described, { root: root, removable: !!space.removable });
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
    var removable = {};
    folders.forEach(function (folder) { removable[folder.dir] = folder.removable; });
    return listed(folders.map(function (folder) { return folder.dir; })).map(function (space) {
      return Object.assign(space, { removable: !!removable[space.dir] });
    });
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
    return spaces;
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
  hasWorkbench: hasWorkbench,
  describe: describe,
  listed: listed,
  FILE: FILE,
};
