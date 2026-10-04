/* The project's workbench.yaml, read on the machine it's on
   -------------------------------------------------------
   The workbench reads this file in the browser to build its sidebar. The
   server reads it here, in node, for what only this side can answer: where
   an implementation's code is on this disk, which origins the screens'
   implementations live at, which Storybook to ask for stories. The rules
   are the same — workbench/manifest.js holds the ones about implementations,
   and the shape checks below mirror workbench/config.js.

   What this deliberately does not do is complain about the handwritten tree. The
   workbench reports every problem in it with the section and screen it came
   from — having the editor say the same thing again in a different voice
   helps nobody. Here, anything malformed is skipped and the rest is shown.
   Problems with implementations are kept, though: `/_workbench/config`
   answers with them, which is how `curl` can check a machine's setup.

   No vscode import: this is file reading and shape checking, and keeping it
   that way means `node config.js <folder>` prints what this machine holds.
*/

var fs = require('fs');
var path = require('path');

var yaml = require('./yaml');
var manifest = require('./workbench/manifest');

var FILE = 'workbench.yaml';
var LOCAL = 'workbench.local.yaml';

function text(value) {
  return typeof value === 'string' ? value.trim() : '';
}

function list(value) {
  return Array.isArray(value) ? value : [];
}

/* Fewer than two states is no choice to make, so the screen stays one row. */
function states(raw) {
  var out = list(raw)
    .map(function (state) {
      var id = text(state && state.id);
      var label = text(state && state.label);
      return id && label && /^[a-z0-9-]+$/.test(id) ? { id: id, label: label } : null;
    })
    .filter(Boolean);
  return out.length > 1 ? out : null;
}

function screen(raw, where, impls, problems) {
  var label = text(raw && raw.label);
  var src = text(raw && raw.src);
  if (!label || !src || src.charAt(0) === '/' || src.indexOf('..') > -1) return null;
  if (src.indexOf(':') > -1 || src.indexOf('~') > -1) return null;
  var item = { label: label, src: src };
  item.viewports = manifest.screenViewports(raw.viewports, where + ' › ' + label, problems);
  var icon = text(raw.icon);
  if (icon) item.icon = icon;
  var found = states(raw.states);
  if (found) item.states = found;
  var lenses = manifest.screenLenses(raw.implementations, item, impls, where + ' › ' + label, problems);
  if (lenses) item.implementations = lenses;
  var code = manifest.screenCode(raw.code, impls, where + ' › ' + label, problems);
  if (code) item.code = code;
  return item;
}

function entries(raw, where, impls, problems, inFolder) {
  return list(raw)
    .map(function (entry) {
      var folder = text(entry && entry.folder);
      if (!folder) return screen(entry, where, impls, problems);
      if (inFolder) return null;
      var items = entries(entry.items, where + ' › ' + folder, impls, problems, true);
      return items.length ? { folder: folder, items: items } : null;
    })
    .filter(Boolean);
}

function sections(raw, impls, problems) {
  return list(raw)
    .map(function (section) {
      var name = text(section && section.name);
      if (!name) return null;
      var items = entries(section && section.items, name, impls, problems, false);
      return items.length ? { name: name, icon: text(section.icon), items: items } : null;
    })
    .filter(Boolean);
}

/* The file's text, or null when it isn't there. Anything else wrong with it
   is something the author wants to know about. */
function readFile(root, name) {
  try {
    return fs.readFileSync(path.join(root, name), 'utf8');
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw new Error('Couldn’t read ' + name + ': ' + String(error.message || error));
  }
}

function parseFile(body, name) {
  try {
    return yaml.parse(body);
  } catch (error) {
    throw new Error(name + ', ' + String(error.message || error));
  }
}

/* Which project's config to read. A folder is the workbench.yaml in it, and
   the first of its projects when it lists several; { dir, key } is the
   project `key` of the file in `dir`. */
function locate(where) {
  if (typeof where === 'string') return { dir: path.resolve(where), key: null };
  return { dir: path.resolve(where.dir), key: where.key || null };
}

/* Both files, local over committed, or null without a workbench.yaml. */
function readRaw(dir) {
  var main = readFile(dir, FILE);
  if (main === null) return null;
  var local = readFile(dir, LOCAL);
  var raw = parseFile(main, FILE);
  if (local !== null) raw = manifest.merge(raw, parseFile(local, LOCAL));
  return { raw: raw, local: local !== null };
}

/* The projects a workbench.yaml holds, each { key, root }: one with a null
   key for a file without `projects`, and for a file that doesn't parse, so
   it is still listed and can show its error. Empty without the file. */
function projectsIn(dir) {
  dir = path.resolve(dir);
  var found;
  try {
    found = readRaw(dir);
  } catch (error) {
    return [{ key: null, root: dir }];
  }
  if (!found) return [];
  var keys = manifest.projectKeys(found.raw, []);
  if (!keys.length) return [{ key: null, root: dir }];
  return keys.map(function (key) {
    var picked = manifest.selectProject(found.raw, key, []);
    return { key: key, root: picked.root ? path.resolve(dir, picked.root) : dir };
  });
}

/* The config, or null when the project has no workbench.yaml at all — which
   is a project the tree simply has nothing to show for, not an error worth
   raising. A workbench.local.yaml beside it is merged over it when there is
   one; a missing one is the usual case. `root` is the folder the project
   serves, which every path in it is relative to. */
function read(where) {
  var at = locate(where);
  var found = readRaw(at.dir);
  if (!found) return null;

  var problems = [];
  var picked = manifest.selectProject(found.raw, at.key, problems);
  var raw = picked.raw;
  var root = picked.root ? path.resolve(at.dir, picked.root) : at.dir;
  var impls = manifest.implementations(raw && raw.implementations, problems);

  var mark = manifest.projectMark(raw, problems);

  return {
    name: text(raw && raw.name) || path.basename(root),
    key: picked.key,
    root: root,
    mark: mark,
    sections: sections(raw && raw.sections, impls, problems),
    implementations: impls,
    previews: manifest.previews(raw && raw.previews, problems),
    problems: problems,
    files: { main: true, local: found.local },
  };
}

/* The committed file, for the page form: its text, and the sections the
   project shows — its own, or the shared ones it inherits. */
function source(where) {
  var at = locate(where);
  var body = readFile(at.dir, FILE);
  if (body === null) return null;
  var raw = parseFile(body, FILE);
  var picked = manifest.selectProject(raw, at.key, []);
  return { body: body, raw: raw, key: picked.key, sections: picked.raw.sections || [] };
}

function replaceTopLevel(body, key, replacement) {
  var lines = String(body).split(/\r?\n/);
  var start = -1;
  var end = lines.length;
  for (var i = 0; i < lines.length; i += 1) {
    if (new RegExp('^' + key + '\\s*:').test(lines[i])) { start = i; break; }
  }
  if (start !== -1) {
    for (var j = start + 1; j < lines.length; j += 1) {
      if (/^[^\s#][^:]*\s*:/.test(lines[j])) { end = j; break; }
    }
    lines.splice.apply(lines, [start, end - start].concat(replacement.split('\n')));
  } else {
    while (lines.length && !lines[lines.length - 1]) lines.pop();
    if (lines.length) lines.push('');
    lines = lines.concat(replacement.split('\n'));
  }
  return lines.join('\n').replace(/\n*$/, '\n');
}

function indentOf(line) {
  return /^ */.exec(line)[0].length;
}

function isBlank(line) {
  return /^\s*(#.*)?$/.test(line);
}

/* The block of `key:` among the direct children of lines [from, to): its
   first line, the line after its last, and its indent. Without one, where a
   new child would go and at what indent. */
function childBlock(lines, from, to, key, fallbackIndent) {
  var depth = null;
  var pattern = null;
  for (var i = from; i < to; i += 1) {
    if (isBlank(lines[i])) continue;
    var indent = indentOf(lines[i]);
    if (depth === null) {
      depth = indent;
      pattern = new RegExp('^ {' + depth + '}' + key.replace(/[-]/g, '\\-') + '\\s*:');
    }
    if (indent < depth) break;
    if (indent !== depth || !pattern.test(lines[i])) continue;
    var end = i + 1;
    while (end < to && (isBlank(lines[end]) || indentOf(lines[end]) > depth)) end += 1;
    while (end > i + 1 && !lines[end - 1].trim()) end -= 1;
    return { start: i, end: end, indent: depth };
  }
  var after = to;
  while (after > from && !lines[after - 1].trim()) after -= 1;
  return { start: -1, end: after, indent: depth === null ? fallbackIndent : depth };
}

/* `projects.<key>.sections` replaced in place, or added at the end of that
   project, leaving every other line of the file as it was. */
function replaceProjectSections(body, key, updated) {
  var lines = String(body).split(/\r?\n/);
  var projects = childBlock(lines, 0, lines.length, 'projects', 0);
  if (projects.start === -1) throw new Error('There is no projects block in ' + FILE + '.');
  var project = childBlock(lines, projects.start + 1, projects.end, key, projects.indent + 2);
  if (project.start === -1) throw new Error('There is no project “' + key + '” in ' + FILE + '.');
  var block = childBlock(lines, project.start + 1, project.end, 'sections', project.indent + 2);
  var pad = new Array(block.indent + 1).join(' ');
  var replacement = yaml.stringify({ sections: updated }).trim().split('\n').map(function (line) {
    return line ? pad + line : line;
  });
  if (block.start === -1) lines.splice.apply(lines, [block.end, 0].concat(replacement));
  else lines.splice.apply(lines, [block.start, block.end - block.start].concat(replacement));
  return lines.join('\n').replace(/\n*$/, '\n');
}

/* The page form's save. A project of a file that lists several gets its own
   sections, so the others keep theirs, shared or not. */
function updateSections(where, updated) {
  if (!Array.isArray(updated)) throw new Error('sections must be a list.');
  var at = locate(where);
  var current = source(at);
  if (!current) throw new Error('There is no ' + FILE + ' at the project root.');
  var next = current.key
    ? replaceProjectSections(current.body, current.key, updated)
    : replaceTopLevel(current.body, 'sections', yaml.stringify({ sections: updated }).trim());
  parseFile(next, FILE);
  var target = path.join(at.dir, FILE);
  var temporary = target + '.canonic-' + process.pid + '-' + Date.now();
  try {
    fs.writeFileSync(temporary, next, { mode: fs.statSync(target).mode });
    fs.renameSync(temporary, target);
  } finally {
    try { fs.unlinkSync(temporary); } catch (_) {}
  }
  return source(at);
}

function screensIn(items, out) {
  items.forEach(function (entry) {
    if (entry.folder) screensIn(entry.items, out);
    else out.push(entry);
  });
  return out;
}

/* What the browser can't work out for itself: every path made absolute for
   this machine. Implementation roots resolve against the project; a screen's
   code resolves against its implementation's root — or nowhere, when that
   implementation hasn't said where it lives, which is named as a problem. */
function resolve(root, config) {
  if (!config) return null;
  root = path.resolve(config.root || root);
  var problems = config.problems.slice();
  var mark = config.mark || { color: null, icon: null, image: null };
  if (mark.image && !fs.existsSync(path.resolve(root, mark.image))) {
    problems.push('icon: ' + mark.image + ' isn’t in the project.');
  }

  var impls = {};
  Object.keys(config.implementations).forEach(function (key) {
    var impl = config.implementations[key];
    impls[key] = Object.assign({}, impl, { root: impl.root ? path.resolve(root, impl.root) : null });
  });

  var screens = {};
  config.sections.forEach(function (section) {
    screensIn(section.items, []).forEach(function (item) {
      var where = section.name + ' › ' + item.label;
      var missing = {};
      var code = [];
      (item.code || []).forEach(function (entry) {
        var impl = impls[entry.implementation];
        if (!impl.root) {
          if (!missing[entry.implementation]) {
            problems.push(
              where + ': code for “' + entry.implementation + '” can’t resolve — implementation “' +
                entry.implementation + '” has no root.'
            );
          }
          missing[entry.implementation] = true;
          code.push({ implementation: entry.implementation, path: null, relative: entry.path, exists: false });
          return;
        }
        var file = path.resolve(impl.root, entry.path);
        code.push({ implementation: entry.implementation, path: file, relative: entry.path, exists: fs.existsSync(file) });
      });
      screens[item.src] = {
        label: item.label,
        design: path.resolve(root, item.src),
        code: code,
      };
      /* The server opens a window stream only for a window a screen names. */
      Object.keys(item.implementations || {}).forEach(function (key) {
        var ref = item.implementations[key];
        if (!ref.window) return;
        screens[item.src].windows = screens[item.src].windows || {};
        screens[item.src].windows[key] = ref.window;
      });
    });
  });

  return {
    name: config.name,
    mark: mark,
    sections: config.sections,
    implementations: impls,
    screens: screens,
    files: config.files,
    problems: problems,
    previews: config.previews,
  };
}

module.exports = {
  read: read,
  list: projectsIn,
  locate: locate,
  resolve: resolve,
  source: source,
  updateSections: updateSections,
  FILE: FILE,
  LOCAL: LOCAL,
};

/* `node config.js <folder>` — what this machine holds, without an editor. */
if (require.main === module) {
  try {
    var root = process.argv[2] || process.cwd();
    console.log(JSON.stringify(resolve(root, read(root)), null, 2));
  } catch (error) {
    console.error(String(error.message || error));
    process.exitCode = 1;
  }
}
