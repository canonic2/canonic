/* The page the canvas is showing, for agents in the editor
   --------------------------------------------------------
   Chats in VS Code take the open text file as context and never a webview,
   so the canvas tells its own server what it shows instead, and agents ask
   the server. Every open canvas reports under its own client id: the VS Code
   tab and any browser tab. The one changed most recently is the current
   view. A canvas that closes says so; one that vanishes without saying so
   stops counting once it has missed its heartbeats.

   The server's port changes from run to run, so a Canonic project (one with
   a .canonic folder) also gets the address in .canonic/.workbench/server.json
   for hooks and MCP servers to find. Other projects get no file. */
var fs = require('node:fs');
var path = require('node:path');

var VIEW_PATH = '/_workbench/view';
var ANNOUNCE_FILE = path.join('.canonic', '.workbench', 'server.json');
var STALE_MS = 3 * 60 * 1000;
var cleanView = require('./src/agent-context/report.ts').cleanView;
var MAX_CLIENTS = 32;

function create(options) {
  var root = path.resolve(options.root);
  var now = options.now || Date.now;
  var staleMs = options.staleMs || STALE_MS;
  var clients = new Map();
  var announced = null;

  /* Only what Copy reference says, and the ids it says it with. */
  function clean(view) {
    return cleanView(view);
  }

  function prune(at) {
    clients.forEach(function (entry, id) {
      if (at - entry.seenAt > staleMs) clients.delete(id);
    });
  }

  /* `view` is null while a page is loading or nothing is picked. */
  function report(body) {
    if (!body || typeof body.client !== 'string' || !body.client || body.client.length > 100) {
      throw new Error('A canvas report needs its client id.');
    }
    var at = now();
    prune(at);
    if (body.closed) { clients.delete(body.client); return; }
    var view = clean(body.view);
    var previous = clients.get(body.client);
    if (!previous && clients.size >= MAX_CLIENTS) throw new Error('Too many open canvases.');
    var changed = !previous || JSON.stringify(previous.view) !== JSON.stringify(view);
    var activity = typeof body.activity === 'number' && Number.isFinite(body.activity) ? Math.min(at, body.activity) : null;
    clients.set(body.client, { view: view, changedAt: activity !== null ? activity : changed ? at : previous.changedAt, seenAt: at });
  }

  /* `root` lets a reader that found this address in a leftover file check
     the server still serves its project. */
  function current() {
    var at = now();
    prune(at);
    var latest = null;
    clients.forEach(function (entry) {
      if (!latest || entry.changedAt >= latest.changedAt) latest = entry;
    });
    if (!latest) return { root: root, open: false, view: null, changedAt: null };
    return { root: root, open: true, view: latest.view, changedAt: new Date(latest.changedAt).toISOString() };
  }

  function announce(baseUrl) {
    if (!fs.existsSync(path.join(root, '.canonic'))) return;
    var file = path.join(root, ANNOUNCE_FILE);
    var content = JSON.stringify({ url: baseUrl, pid: process.pid }, null, 2) + '\n';
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, content);
    announced = content;
  }

  /* Another window's server may have announced itself since; leave its file. */
  function close() {
    if (!announced) return;
    var file = path.join(root, ANNOUNCE_FILE);
    try {
      if (fs.readFileSync(file, 'utf8') === announced) fs.unlinkSync(file);
    } catch (error) { /* already gone */ }
    announced = null;
  }

  return { report: report, current: current, announce: announce, close: close };
}

module.exports = {
  create: create,
  VIEW_PATH: VIEW_PATH,
  ANNOUNCE_FILE: ANNOUNCE_FILE,
};
