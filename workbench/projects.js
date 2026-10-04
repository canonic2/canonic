/* Projects
   --------
   The workbenches one window, or one `node server.js`, can switch between.
   They come from workbench.yaml files: a file without `projects` is one
   project, served from its own folder, and a file with `projects` is one per
   entry, each served from its own `root` — the file's folder unless it says
   otherwise, so several projects can share a folder. Each project gets a
   server of its own: the server, the config readers, the preview worker, and
   every route assume one root and one config, so a second project is a
   second server rather than a second project threaded through all of them.
   A single project is the same one server it always was.

   Servers start the first time a project is opened, not when it is listed:
   listing a project costs one read of its workbench.yaml, while starting one
   can open terminals and compile previews.

   A project's id is a hash of its file's folder, and of its key when the file
   lists several, so it is the same in every window, in every run, and in the
   canvas's address, without saying where the folder is. Its default mark —
   the letter and colour drawn beside its name — comes from that id too, so a
   project keeps its colour wherever it is shown. */

var crypto = require('crypto');
var fs = require('fs');
var path = require('path');
var config = require('./config');

var COLORS = ['blue', 'green', 'orange', 'purple', 'pink', 'teal'];

function projectId(dir, key) {
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

/* What workbench.yaml says about the project: its `name`, `color` and
   `icon`, with workbench.local.yaml over them. A file that doesn't parse
   still lists the project, under its folder name with the default mark, so it
   can be opened and its error read. */
function read(project) {
  try {
    return config.read({ dir: project.dir, key: project.key }) || {};
  } catch (error) {
    return {};
  }
}

function initialOf(name) {
  var match = /[\p{L}\p{N}]/u.exec(String(name || ''));
  return match ? match[0].toLocaleUpperCase() : '?';
}

/* An image icon travels as a data URI. Each project is its own server and
   its own folder, and the switcher draws every project's mark wherever it
   is — the editor's sidebar, another project's canvas — so the image can't
   be fetched from where it lives. One that is missing, too big or outside
   the project leaves the letter in its place. */
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

/* A project as the switchers draw it. `project` is { dir, key, root,
   removable }; `dir` holds its workbench.yaml and `root` is what it serves. */
function describe(project) {
  var dir = path.resolve(project.dir || project.root);
  var key = project.key || null;
  var id = projectId(dir, key);
  var found = read({ dir: dir, key: key });
  var root = path.resolve(project.root || found.root || dir);
  var mark = found.mark || {};
  var name = found.name || path.basename(root);
  var image = imageOf(root, mark.image);
  /* An image is drawn as it is, on no colour unless the project names one. */
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
    removable: !!project.removable,
  };
}

/* `start(project)` is given { dir, key, root } and answers with a running
   server ({ url, close() }). `set` takes folders as { dir, removable } in the
   order they are listed; removable marks a folder the user added, as against
   one that came with the window. */
function create(options) {
  var start = options.start;
  var folders = [];
  var running = {}; /* id -> { root, server: Promise of the running server } */
  var closed = false;

  /* Every folder's projects, read fresh: a saved workbench.yaml may have
     added, removed or moved one. A project listed by two folders' files is
     the first one's. */
  function expand() {
    var seen = {};
    var out = [];
    folders.forEach(function (folder) {
      config.list(folder.dir).forEach(function (found) {
        var id = projectId(folder.dir, found.key);
        if (seen[id]) return;
        seen[id] = true;
        out.push({ id: id, dir: folder.dir, key: found.key, root: found.root, removable: folder.removable });
      });
    });
    return out;
  }

  /* Servers whose project left the list, or now serves another root, stop. */
  function settle(projects) {
    var current = {};
    projects.forEach(function (project) { current[project.id] = project; });
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
    var projects = expand();
    settle(projects);
    return projects.map(describe);
  }

  /* The running server for a listed project, started if it isn't. A start
     that fails is forgotten, so opening the project again tries again. */
  function open(id) {
    if (closed) return Promise.reject(new Error('Workbench has stopped.'));
    var project = expand().filter(function (candidate) { return candidate.id === id; })[0];
    if (!project) return Promise.reject(new Error('That project isn’t in the Workbench list.'));
    if (running[id] && running[id].root !== project.root) stop(id);
    if (!running[id]) {
      var entry = { root: project.root, server: null };
      entry.server = Promise.resolve().then(function () {
        return start({ dir: project.dir, key: project.key, root: project.root });
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
  projectId: projectId,
  hasWorkbench: hasWorkbench,
  describe: describe,
  COLORS: COLORS,
};
