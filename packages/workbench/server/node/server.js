/* Workbench, without the editor
   -----------------------------
   `node server.js [folder...]` runs the same spaces the extension runs. Each
   folder's workbench.yaml is one space, or one per entry of its `spaces`;
   several are several workbenches the canvas switches between, each on its
   own Workbench server, sharing one screenshot service.

   The servers are `workbench-server serve` processes; this file and the
   extension host them through src/modules/host/, which answers what each
   server asks of the editor and the capture helper. */

var path = require('path');
var host = require('./src/modules/host/index.ts');
var spaces = require('./spaces');

module.exports = host;

if (require.main === module) {
  var dirs = process.argv.slice(2);
  var trace = dirs.includes('--trace');
  dirs = dirs.filter(function (dir) { return dir !== '--trace'; });
  var onLog = function (record) {
    console.log(record.event + ' ' + JSON.stringify(record.details || {}));
  };
  if (!dirs.length) dirs = [process.cwd()];
  var onShot = function (file) {
    console.log('saved ' + file);
  };
  var failed = function (err) {
    console.error(String(err.message || err));
    process.exit(1);
  };
  var found = spaces.listed(dirs.map(function (dir) { return path.resolve(dir); }));

  if (found.length <= 1) {
    var only = found[0] || { dir: path.resolve(dirs[0]), key: null, root: path.resolve(dirs[0]) };
    host.start({ root: only.root, config: { dir: only.dir, key: only.key }, eagerCapture: true, eagerPreviews: true, trace: trace, onLog: trace ? onLog : undefined, onShot: onShot }).then(function (running) {
      console.log('workbench on ' + running.url);
    }, failed);
  } else {
    var missing = dirs.filter(function (dir) { return !spaces.hasWorkbench(dir); });
    if (missing.length) failed(new Error('There’s no ' + spaces.FILE + ' in ' + missing.join(', ') + '.'));
    var shared = host.createCapture();
    var hub = spaces.create({
      start: function (space) {
        return host.start({
          root: space.root, config: { dir: space.dir, key: space.key },
          capture: shared, captureShared: true, eagerCapture: true, eagerPreviews: true,
          trace: trace, onLog: trace ? onLog : undefined,
          spaces: hub, onShot: onShot,
        });
      },
    });
    hub.set(dirs.map(function (dir) { return { dir: dir }; }));
    var listed = hub.list();
    Promise.all(listed.map(function (space) { return hub.open(space.id); })).then(function (servers) {
      servers.forEach(function (running, i) {
        console.log('workbench on ' + running.url + '  ' + listed[i].name);
      });
    }, failed);
  }
}
