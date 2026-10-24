/* The space's workbench.yaml, read on the machine it's on
   -----------------------------------------------------
   The workbench reads this file in the browser to build its sidebar. The
   server reads it here, in node, for what only this side can answer: where
   an implementation's code is on this disk, which origins the pages'
   implementations live at, which Storybook to ask for stories. The rules
   are the same — contracts/manifest.js holds the ones about implementations,
   scenarios and lenses, and the shape checks below mirror ui/legacy/config.js,
   with the same problems at the same places: a collection, group or page by
   name. Anything malformed is reported and left out, and the rest is shown;
   `/_workbench/config` answers with the problems, which is how the editor
   lists them and `curl` can check a machine's setup.

   No vscode import: this is file reading and shape checking, and keeping it
   that way means `node config.js <folder>` prints what this machine holds.
*/

var fs = require('fs');
var path = require('path');

var yaml = require('./yaml');
var manifest = require('../../contracts/manifest');
var sizeSchema = require('../../contracts/src/sizes/schema.ts');

var FILE = 'workbench.yaml';
var LOCAL = 'workbench.local.yaml';

function text(value) {
  return typeof value === 'string' ? value.trim() : '';
}

function list(value) {
  return Array.isArray(value) ? value : [];
}

/* `space` is what every page is read against: the implementations, the
   space's sizes, the previews settings and the srcs already listed. A src
   is listed once. */
function page(raw, where, space, problems) {
  var label = text(raw && raw.label);
  var src = text(raw && raw.src);
  if (!label || !src) {
    problems.push(where + (label || src ? ' › ' + (label || src) : '') + ': every page needs a label and a src.');
    return null;
  }
  var at = where + ' › ' + label;
  if (src.charAt(0) === '/' || src.indexOf('..') > -1) {
    problems.push(at + ': “' + src + '” must be relative to the project root.');
    return null;
  }
  var reserved = manifest.srcProblem(src);
  if (reserved) {
    problems.push(at + ': ' + reserved);
    return null;
  }
  if (space.seen[src]) {
    problems.push(at + ': src “' + src + '” is already in the page tree; only its first entry is kept.');
    return null;
  }
  space.seen[src] = true;
  var item = { label: label, src: src };
  manifest.pageLensLabel(raw.lensLabel, item, at, problems);
  /* A Markdown page has no artboard size; docsPageEntry reports sizes on one. */
  if (!manifest.isDocs(src)) {
    var sized = sizeSchema.readPageSizes(raw.sizes, space.sizes, at, problems);
    if (sized) {
      item.sizes = sized.sizes;
      if (sized.ownSizes) item.ownSizes = sized.ownSizes;
    }
  }
  var icon = text(raw.icon);
  if (icon) item.icon = icon;
  var found = manifest.pageScenarios(raw.scenarios, at, problems);
  if (found.scenarios) item.scenarios = found.scenarios;
  manifest.pageMarkdown(raw, item, at, problems);
  /* A preview definition's page takes the definition's scenarios when the
     catalog is assembled, which checks its lens path maps then. */
  var ids = space.previews !== false && manifest.isPreviewSrc(src) ? null : found.ids;
  var lenses = manifest.pageLenses(raw.implementations, item, space.impls, at, problems, ids);
  if (lenses) item.implementations = lenses;
  manifest.docsPageEntry(raw, item, space.impls, at, problems);
  var code = manifest.pageCode(raw.code, space.impls, at, problems);
  if (code) item.code = code;
  return item;
}

function entries(raw, where, space, problems, inGroup) {
  var out = [];
  list(raw).forEach(function (entry) {
    var group = text(entry && entry.group);
    if (group) {
      if (inGroup) {
        problems.push(where + ' › ' + group + ': groups don’t nest.');
        return;
      }
      var items = entries(entry.items, where + ' › ' + group, space, problems, true);
      if (items.length) out.push({ group: group, items: items });
      else problems.push(where + ': group “' + group + '” has nothing in it.');
      return;
    }
    var found = page(entry, where, space, problems);
    if (found) out.push(found);
  });
  return out;
}

function collections(raw, space, problems) {
  var out = [];
  var names = [];
  list(raw).forEach(function (collection, i) {
    var name = text(collection && collection.name);
    if (!name) {
      problems.push('collection ' + (i + 1) + ' has no name.');
      return;
    }
    if (names.indexOf(name) !== -1) {
      problems.push('collection “' + name + '” is declared twice; only the first is kept.');
      return;
    }
    names.push(name);
    var items = entries(collection.items, name, space, problems, false);
    if (!items.length && !text(collection.icon)) {
      problems.push('collection “' + name + '” has nothing in it.');
      return;
    }
    out.push({ name: name, icon: text(collection.icon), items: items });
  });
  return out;
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
  var previews = manifest.previews(raw && raw.previews, problems);
  var authored = collections(raw && raw.collections, { impls: impls, sizes: sizes, previews: previews, seen: Object.create(null) }, problems);
  var imports = previews !== false || Object.keys(impls).some(function (key) { return impls[key].catalog; });
  if (!imports && !authored.some(function (collection) { return collection.items.length; })) {
    problems.push(manifest.NOTHING_TO_SHOW);
  }

  return {
    name: text(raw && raw.name) || path.basename(root),
    key: picked.key,
    root: root,
    mark: mark,
    sizes: sizes,
    collections: authored,
    implementations: impls,
    previews: previews,
    problems: problems,
    files: { main: true, local: found.local },
  };
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
