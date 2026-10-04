var assert = require('node:assert/strict');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var test = require('node:test');
var projects = require('./projects');

function project(name, yaml) {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-projects-' + name + '-'));
  if (yaml !== null) fs.writeFileSync(path.join(root, 'workbench.yaml'), yaml === undefined ? 'name: ' + name + '\nsections: []\n' : yaml);
  return root;
}

function hub() {
  var started = [];
  var closed = [];
  var failNext = {};
  var made = projects.create({
    start: function (project) {
      var root = project.root;
      if (failNext[root]) {
        delete failNext[root];
        return Promise.reject(new Error('port in use'));
      }
      started.push(root);
      return Promise.resolve({
        url: 'http://127.0.0.1:' + (3579 + started.length - 1) + '/_workbench/',
        close: function () { closed.push(root); return Promise.resolve(); },
      });
    },
  });
  return { hub: made, started: started, closed: closed, failNext: failNext };
}

test('a project is listed by its workbench name with a stable id, initial and colour', function () {
  var root = project('Acme');
  var listed = projects.create({ start: function () {} });
  listed.set([{ root: root, removable: true }]);
  var entry = listed.list()[0];

  assert.equal(entry.name, 'Acme');
  assert.equal(entry.initial, 'A');
  assert.equal(entry.root, root);
  assert.equal(entry.removable, true);
  assert.equal(entry.id, projects.projectId(root));
  assert.match(entry.id, /^[0-9a-f]{10}$/);
  assert.ok(projects.COLORS.indexOf(entry.color) > -1);
  assert.equal(projects.describe({ root: root }).color, entry.color);
});

test('a project whose workbench.yaml has no name or does not parse is listed by its folder', function () {
  var unnamed = project('unnamed', 'sections: []\n');
  var broken = project('broken', 'name: Acme\n  sections: []\n');
  var listed = projects.create({ start: function () {} });
  listed.set([{ root: unnamed }, { root: broken }]);

  assert.deepEqual(listed.list().map(function (entry) { return entry.name; }), [path.basename(unnamed), path.basename(broken)]);
});

test('the same folder listed twice is one project, the first entry winning', function () {
  var root = project('Acme');
  var listed = projects.create({ start: function () {} });
  listed.set([{ root: root, removable: false }, { root: root + path.sep, removable: true }]);

  assert.equal(listed.list().length, 1);
  assert.equal(listed.list()[0].removable, false);
});

test('servers start when a project is first opened, once, and not when it is listed', async function () {
  var acme = project('Acme');
  var example = project('Example');
  var made = hub();
  made.hub.set([{ root: acme }, { root: example }]);
  assert.equal(made.started.length, 0);

  var id = projects.projectId(example);
  var first = await made.hub.open(id);
  var again = await made.hub.open(id);

  assert.equal(first, again);
  assert.deepEqual(made.started, [example]);
  assert.equal(made.hub.started(projects.projectId(acme)), null);
});

test('a failed start is retried the next time the project opens', async function () {
  var acme = project('Acme');
  var made = hub();
  made.hub.set([{ root: acme }]);
  made.failNext[path.resolve(acme)] = true;

  await assert.rejects(made.hub.open(projects.projectId(acme)), /port in use/);
  var running = await made.hub.open(projects.projectId(acme));
  assert.match(running.url, /^http:\/\/127\.0\.0\.1:/);
});

test('an unknown project is refused', async function () {
  var made = hub();
  made.hub.set([]);
  await assert.rejects(made.hub.open('0123456789'), /isn’t in the Workbench list/);
});

test('taking a project off the list stops its server; closing stops the rest', async function () {
  var acme = project('Acme');
  var example = project('Example');
  var made = hub();
  made.hub.set([{ root: acme }, { root: example }]);
  await made.hub.open(projects.projectId(acme));
  await made.hub.open(projects.projectId(example));

  await made.hub.set([{ root: acme }]);
  assert.deepEqual(made.closed, [path.resolve(example)]);

  await made.hub.close();
  assert.deepEqual(made.closed, [path.resolve(example), path.resolve(acme)]);
  await assert.rejects(made.hub.open(projects.projectId(acme)), /stopped/);
});

test('hasWorkbench names folders with a workbench.yaml', function () {
  assert.equal(projects.hasWorkbench(project('Acme')), true);
  assert.equal(projects.hasWorkbench(project('empty', null)), false);
});

test('a project is marked with the colour, icon and image its workbench.yaml names', function () {
  var listed = projects.create({ start: function () {} });
  var hex = project('Hex', 'name: Acme Web\ncolor: "#f5d76e"\nicon: rocket\nsections: []\n');
  var image = project('Image', 'name: Acme UI\nicon: brand/logo.svg\nsections: []\n');
  fs.mkdirSync(path.join(image, 'brand'));
  fs.writeFileSync(path.join(image, 'brand', 'logo.svg'), '<svg xmlns="http://www.w3.org/2000/svg"/>');
  var tinted = project('Tinted', 'icon: brand/missing.png\ncolor: teal\nsections: []\n');
  listed.set([{ root: hex }, { root: image }, { root: tinted }]);
  var marks = listed.list();

  assert.equal(marks[0].name, 'Acme Web');
  assert.equal(marks[0].color, '#f5d76e');
  assert.equal(marks[0].icon, 'rocket');
  assert.equal(marks[0].image, null);

  assert.match(marks[1].image, /^data:image\/svg\+xml;base64,/);
  assert.equal(Buffer.from(marks[1].image.split(',')[1], 'base64').toString(), '<svg xmlns="http://www.w3.org/2000/svg"/>');
  /* An image stands on its own unless the project names a colour. */
  assert.equal(marks[1].color, null);

  /* A missing image leaves the letter, on the colour it asked for. */
  assert.equal(marks[2].image, null);
  assert.equal(marks[2].color, 'teal');
  assert.equal(marks[2].initial, 'C');
});

test('workbench.local.yaml can rename and re-mark a project for one machine', function () {
  var root = project('Acme', 'name: Acme\ncolor: blue\nsections: []\n');
  fs.writeFileSync(path.join(root, 'workbench.local.yaml'), 'name: Acme (mine)\ncolor: red\nicon: flask-conical\n');
  var listed = projects.create({ start: function () {} });
  listed.set([{ root: root }]);
  var entry = listed.list()[0];

  assert.equal(entry.name, 'Acme (mine)');
  assert.equal(entry.color, 'red');
  assert.equal(entry.icon, 'flask-conical');
});

test('an image icon bigger than 256 KB is left out', function () {
  var root = project('Big', 'icon: big.png\nsections: []\n');
  fs.writeFileSync(path.join(root, 'big.png'), Buffer.alloc(256 * 1024 + 1));
  var listed = projects.create({ start: function () {} });
  listed.set([{ root: root }]);
  assert.equal(listed.list()[0].image, null);
});

test('one workbench.yaml can list several projects, sharing a folder or not', function () {
  var dir = project('multi', [
    'projects:',
    '  web:',
    '    name: Acme Web',
    '    color: blue',
    '  ui:',
    '    name: Acme UI',
    '    root: packages/ui',
    '    icon: logo.svg',
    '  docs:',
    '    name: Acme Docs',
  ].join('\n') + '\n');
  fs.mkdirSync(path.join(dir, 'packages', 'ui'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'packages', 'ui', 'logo.svg'), '<svg/>');
  var listed = projects.create({ start: function () {} });
  listed.set([{ dir: dir, removable: true }]);
  var found = listed.list();

  assert.deepEqual(found.map(function (p) { return p.name; }), ['Acme Web', 'Acme UI', 'Acme Docs']);
  assert.deepEqual(found.map(function (p) { return p.key; }), ['web', 'ui', 'docs']);
  assert.equal(found[0].root, path.resolve(dir));
  assert.equal(found[2].root, path.resolve(dir));
  assert.equal(found[1].root, path.join(path.resolve(dir), 'packages', 'ui'));
  assert.ok(found.every(function (p) { return p.dir === path.resolve(dir) && p.removable; }));
  assert.equal(new Set(found.map(function (p) { return p.id; })).size, 3);
  assert.equal(found[0].id, projects.projectId(dir, 'web'));
  /* A project's icon resolves against its own root. */
  assert.match(found[1].image, /^data:image\/svg\+xml;base64,/);
});

test('a single-project file keeps the id it had before files could list several', function () {
  var root = project('Acme');
  var listed = projects.create({ start: function () {} });
  listed.set([{ dir: root }]);
  assert.equal(listed.list()[0].id, projects.projectId(root));
  assert.equal(listed.list()[0].key, null);
});

test('a project dropped from the file, or moved to another root, stops its server', async function () {
  var dir = project('multi', 'projects:\n  web:\n    name: Web\n  ui:\n    name: UI\n');
  var made = hub();
  made.hub.set([{ dir: dir }]);
  await made.hub.open(projects.projectId(dir, 'web'));
  await made.hub.open(projects.projectId(dir, 'ui'));
  assert.equal(made.started.length, 2);

  fs.writeFileSync(path.join(dir, 'workbench.yaml'), 'projects:\n  web:\n    name: Web\n    root: site\n');
  fs.mkdirSync(path.join(dir, 'site'));
  made.hub.list();
  await new Promise(function (resolve) { setImmediate(resolve); });
  assert.deepEqual(made.closed.sort(), [path.resolve(dir), path.resolve(dir)]);

  /* Opening it again starts it at its new root. */
  var running = await made.hub.open(projects.projectId(dir, 'web'));
  assert.ok(running);
  assert.equal(made.started[made.started.length - 1], path.join(path.resolve(dir), 'site'));
});
