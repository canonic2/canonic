/* The space's workbench.yaml, read on the machine it's on
   -----------------------------------------------------
   The workbench reads this file in the browser to build its sidebar. The
   server reads it here, in node, for what only this side can answer: where
   an implementation's code is on this disk, which origins the pages'
   implementations live at, which Storybook to ask for stories. The rules
   are the same — workbench/manifest.js holds the ones about implementations,
   and the shape checks below mirror workbench/config.js.

   What this deliberately does not do is complain about the handwritten tree. The
   workbench reports every problem in it with the collection and page it came
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
var sizeSchema = require('./src/sizes/schema.ts');

var FILE = 'workbench.yaml';
var LOCAL = 'workbench.local.yaml';

function text(value) {
  return typeof value === 'string' ? value.trim() : '';
}

function list(value) {
  return Array.isArray(value) ? value : [];
}

/* Fewer than two states is no choice to make, so the page stays one row. */
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

function page(raw, where, impls, sizes, problems) {
  var label = text(raw && raw.label);
  var src = text(raw && raw.src);
  if (!label || !src || src.charAt(0) === '/' || src.indexOf('..') > -1) return null;
  if (manifest.srcProblem(src)) return null;
  var item = { label: label, src: src };
  manifest.pageLensLabel(raw.lensLabel, item, where + ' › ' + label, problems);
  /* A docs page has no artboard size; docsPageEntry reports sizes on one. */
  if (!manifest.isDocs(src)) {
    var sized = sizeSchema.readPageSizes(raw.sizes, sizes, where + ' › ' + label, problems);
    if (sized) {
      item.sizes = sized.sizes;
      if (sized.ownSizes) item.ownSizes = sized.ownSizes;
    }
  }
  var icon = text(raw.icon);
  if (icon) item.icon = icon;
  var found = states(raw.states);
  if (found) item.states = found;
  var lenses = manifest.pageLenses(raw.implementations, item, impls, where + ' › ' + label, problems);
  if (lenses) item.implementations = lenses;
  manifest.docsPageEntry(raw, item, where + ' › ' + label, problems);
  var code = manifest.pageCode(raw.code, impls, where + ' › ' + label, problems);
  if (code) item.code = code;
  return item;
}

function entries(raw, where, impls, sizes, problems, inGroup) {
  return list(raw)
    .map(function (entry) {
      var group = text(entry && entry.group);
      if (!group) return page(entry, where, impls, sizes, problems);
      if (inGroup) return null;
      var items = entries(entry.items, where + ' › ' + group, impls, sizes, problems, true);
      return items.length ? { group: group, items: items } : null;
    })
    .filter(Boolean);
}

function collections(raw, impls, sizes, problems) {
  return list(raw)
    .map(function (collection) {
      var name = text(collection && collection.name);
      if (!name) return null;
      var items = entries(collection && collection.items, name, impls, sizes, problems, false);
      return items.length || text(collection.icon) ? { name: name, icon: text(collection.icon), items: items } : null;
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

/* Which space's config to read. A folder is the workbench.yaml in it, and
   the first of its spaces when it lists several; { dir, key } is the
   space `key` of the file in `dir`. */
function locate(where) {
  if (typeof where === 'string') return { dir: path.resolve(where), key: null };
  return { dir: path.resolve(where.dir), key: where.key || null };
}

/* Both files, local over committed, or null without a workbench.yaml.
   `localRaw` is the local file alone, for what it overrides. */
function readRaw(dir) {
  var main = readFile(dir, FILE);
  if (main === null) return null;
  var local = readFile(dir, LOCAL);
  var raw = parseFile(main, FILE);
  var localRaw = local !== null ? parseFile(local, LOCAL) : null;
  if (localRaw !== null) raw = manifest.merge(raw, localRaw);
  return { raw: raw, local: local !== null, localRaw: localRaw };
}

function isMap(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/* The size keys workbench.local.yaml sets for the space `key` (null for a
   file without spaces), which the size dialogs can't change. */
function localSizeKeys(localRaw, key) {
  if (!isMap(localRaw)) return [];
  var holder = key ? (isMap(localRaw.spaces) && localRaw.spaces[key]) : localRaw;
  return isMap(holder) && isMap(holder.sizes) ? Object.keys(holder.sizes) : [];
}

/* Sizes belong to a space: in a file that lists spaces, a top-level
   `sizes` isn't shared with them, so it is reported. */
function sizesFor(found, picked, problems) {
  if (picked.key && isMap(found.raw) && found.raw.sizes !== undefined) {
    problems.push('Sizes: sizes go in each space, under spaces.<key>, when the file lists spaces.');
  }
  return sizeSchema.readSpaceSizes(picked.raw && picked.raw.sizes, problems, localSizeKeys(found.localRaw, picked.key));
}

/* The spaces a workbench.yaml holds, each { key, root }: one with a null
   key for a file without `spaces`, and for a file that doesn't parse, so
   it is still listed and can show its error. Empty without the file. */
function spacesIn(dir) {
  dir = path.resolve(dir);
  var found;
  try {
    found = readRaw(dir);
  } catch (error) {
    return [{ key: null, root: dir }];
  }
  if (!found) return [];
  var keys = manifest.spaceKeys(found.raw, []);
  if (!keys.length) return [{ key: null, root: dir }];
  return keys.map(function (key) {
    var picked = manifest.selectSpace(found.raw, key, []);
    return { key: key, root: picked.root ? path.resolve(dir, picked.root) : dir };
  });
}

/* The config, or null when the folder has no workbench.yaml at all — which
   is a project the tree simply has nothing to show for, not an error worth
   raising. A workbench.local.yaml beside it is merged over it when there is
   one; a missing one is the usual case. `root` is the folder the space
   serves, which every path in it is relative to. */
function read(where) {
  var at = locate(where);
  var found = readRaw(at.dir);
  if (!found) return null;

  var problems = [];
  var picked = manifest.selectSpace(found.raw, at.key, problems);
  var raw = picked.raw;
  var root = picked.root ? path.resolve(at.dir, picked.root) : at.dir;
  var impls = manifest.implementations(raw && raw.implementations, problems);
  var sizes = sizesFor(found, picked, problems);

  var mark = manifest.spaceMark(raw, problems);

  return {
    name: text(raw && raw.name) || path.basename(root),
    key: picked.key,
    root: root,
    mark: mark,
    sizes: sizes,
    collections: collections(raw && raw.collections, impls, sizes, problems),
    implementations: impls,
    previews: manifest.previews(raw && raw.previews, problems),
    problems: problems,
    files: { main: true, local: found.local },
  };
}

/* The committed file, for the page form and the size dialogs: its text, the
   collections the space shows — its own, or the shared ones it inherits —
   and the space's own `sizes` as written, if any. */
function source(where) {
  var at = locate(where);
  var body = readFile(at.dir, FILE);
  if (body === null) return null;
  var raw = parseFile(body, FILE);
  var picked = manifest.selectSpace(raw, at.key, []);
  return { body: body, raw: raw, key: picked.key, collections: picked.raw.collections || [], sizes: picked.raw.sizes };
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

/* `spaces.<key>.<name>` replaced in place, or added at the end of that
   space, leaving every other line of the file as it was. */
function replaceSpaceBlock(body, key, name, value) {
  var lines = String(body).split(/\r?\n/);
  var spaces = childBlock(lines, 0, lines.length, 'spaces', 0);
  if (spaces.start === -1) throw new Error('There is no spaces block in ' + FILE + '.');
  var space = childBlock(lines, spaces.start + 1, spaces.end, key, spaces.indent + 2);
  if (space.start === -1) throw new Error('There is no space “' + key + '” in ' + FILE + '.');
  var block = childBlock(lines, space.start + 1, space.end, name, space.indent + 2);
  var pad = new Array(block.indent + 1).join(' ');
  var wrapped = {};
  wrapped[name] = value;
  var replacement = yaml.stringify(wrapped).trim().split('\n').map(function (line) {
    return line ? pad + line : line;
  });
  if (block.start === -1) lines.splice.apply(lines, [block.end, 0].concat(replacement));
  else lines.splice.apply(lines, [block.start, block.end - block.start].concat(replacement));
  return lines.join('\n').replace(/\n*$/, '\n');
}

/* One top-level block of the space replaced: at the top level of a file
   without spaces, or inside the space's entry of a file that lists several. */
function replaceBlock(body, key, name, value) {
  if (key) return replaceSpaceBlock(body, key, name, value);
  var wrapped = {};
  wrapped[name] = value;
  return replaceTopLevel(body, name, yaml.stringify(wrapped).trim());
}

/* The new text, checked to parse, replaces the file in one rename. */
function writeChecked(at, next) {
  parseFile(next, FILE);
  var target = path.join(at.dir, FILE);
  var temporary = target + '.canonic-' + process.pid + '-' + Date.now();
  try {
    fs.writeFileSync(temporary, next, { mode: fs.statSync(target).mode });
    fs.renameSync(temporary, target);
  } finally {
    try { fs.unlinkSync(temporary); } catch (_) {}
  }
}

/* The page form's save. A space of a file that lists several gets its own
   collections, so the others keep theirs, shared or not. */
function updateCollections(where, updated) {
  if (!Array.isArray(updated)) throw new Error('collections must be a list.');
  var at = locate(where);
  var current = source(at);
  if (!current) throw new Error('There is no ' + FILE + ' at the project root.');
  writeChecked(at, replaceBlock(current.body, current.key, 'collections', updated));
  return source(at);
}

/* The size dialogs' save: `edit(sizes, collections)` gets the space's sizes
   and collections as written and answers with what to write (see
   src/sizes/edit.ts). Sizes are the space's own block; collections are
   replaced whole, as the page form replaces them. */
function updateSizes(where, edit) {
  var at = locate(where);
  var current = source(at);
  if (!current) throw new Error('There is no ' + FILE + ' at the project root.');
  var change = edit(current.sizes, current.collections);
  var next = current.body;
  if (change.sizes) next = replaceBlock(next, current.key, 'sizes', change.sizes);
  if (change.collections) next = replaceBlock(next, current.key, 'collections', change.collections);
  if (next !== current.body) writeChecked(at, next);
  return change;
}

function pagesIn(items, out) {
  items.forEach(function (entry) {
    if (entry.group) pagesIn(entry.items, out);
    else out.push(entry);
  });
  return out;
}

/* What the browser can't work out for itself: every path made absolute for
   this machine. Implementation roots resolve against the project; a page's
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

  var pages = {};
  config.collections.forEach(function (collection) {
    pagesIn(collection.items, []).forEach(function (item) {
      var where = collection.name + ' › ' + item.label;
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
      pages[item.src] = {
        label: item.label,
        design: path.resolve(root, item.src),
        code: code,
      };
      if (item.sizes) pages[item.src].sizes = item.sizes;
      if (item.ownSizes) pages[item.src].ownSizes = item.ownSizes;
      /* The server opens a window stream only for a window a page names. */
      Object.keys(item.implementations || {}).forEach(function (key) {
        var ref = item.implementations[key];
        if (!ref.window) return;
        pages[item.src].windows = pages[item.src].windows || {};
        pages[item.src].windows[key] = ref.window;
      });
    });
  });

  return {
    name: config.name,
    mark: mark,
    sizes: config.sizes,
    collections: config.collections,
    implementations: impls,
    pages: pages,
    files: config.files,
    problems: problems,
    previews: config.previews,
  };
}

module.exports = {
  read: read,
  list: spacesIn,
  locate: locate,
  resolve: resolve,
  source: source,
  updateCollections: updateCollections,
  updateSizes: updateSizes,
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
