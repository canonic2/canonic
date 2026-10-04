/* Design-system export
   --------------------
   The workbench is already the project's curated visual surface. Its design
   files and resolved code pointers are the export entry points; from those,
   follow only local references so real helpers/assets come along without
   unrelated application code. */

var fs = require('fs');
var path = require('path');
var crypto = require('crypto');

var TEXT_EXTENSIONS = ['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs', '.json', '.css', '.scss', '.sass', '.less'];
var RESOLVE_EXTENSIONS = ['', '.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs', '.json', '.css', '.scss', '.sass', '.less', '.svg', '.png', '.jpg', '.jpeg', '.gif', '.webp', '.avif', '.woff', '.woff2', '.ttf'];
var OMIT_DIRS = { '.git': true, '.canonic': true, 'node_modules': true, 'dist': true, 'build': true, 'coverage': true, '.next': true, '.turbo': true };
var OMIT_FILE = /(?:^|\/)(?:\.env(?:\.|$)|.*\.(?:test|spec)\.[cm]?[jt]sx?|.*\.snap$)/i;
var CONFIG_FILES = /^(?:package\.json|(?:ts|js)config(?:\.[^.]+)?\.json|vite\.config\.[cm]?[jt]s|tailwind\.config\.[cm]?[jt]s|postcss\.config\.[cm]?[jt]s)$/;
var MAX_ARCHIVE_BYTES = 10 * 1000 * 1000;

function inside(root, file) {
  var relative = path.relative(root, file);
  return relative === '' || (relative.indexOf('..' + path.sep) !== 0 && relative !== '..' && !path.isAbsolute(relative));
}

function slug(value) {
  return String(value || 'canonic').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'canonic';
}

function screenSlugs(sources) {
  var used = {};
  var out = {};
  sources.slice().sort().forEach(function (source) {
    var base = slug(source);
    var value = base;
    if (used[value]) value += '-' + crypto.createHash('sha256').update(source).digest('hex').slice(0, 8);
    used[value] = true;
    out[source] = value;
  });
  return out;
}

function contentHash(items) {
  var hash = crypto.createHash('sha256');
  items.slice().sort(function (a, b) { return a.destination < b.destination ? -1 : a.destination > b.destination ? 1 : 0; }).forEach(function (item) {
    var name = Buffer.from(item.destination);
    var body = fs.readFileSync(item.source);
    var size = Buffer.alloc(8);
    size.writeUInt32BE(name.length, 0);
    size.writeUInt32BE(body.length, 4);
    hash.update(size);
    hash.update(name);
    hash.update(body);
  });
  return 'sha256:' + hash.digest('hex');
}

function screenReadme(screen) {
  function link(file) {
    return path.posix.relative(path.posix.dirname(screen.readme), file) || path.posix.basename(file);
  }
  var entryLines = screen.entries.length
    ? screen.entries.map(function (entry) {
        var detail = entry.kind === 'design' ? 'Design' : 'Source (' + entry.implementation + ')';
        return '- ' + detail + ': [`' + entry.path + '`](' + link(entry.path) + ')';
      }).join('\n')
    : '- No portable design or source entry point was resolved for this screen.';
  var fileLines = screen.files.length
    ? screen.files.map(function (file) { return '- [`' + file + '`](' + link(file) + ')'; }).join('\n')
    : '- No files are included in this screen hash.';
  var screenshotLines = screen.screenshots.length
    ? screen.screenshots.map(function (shot) {
        var label = shot.label + (shot.viewport ? ' · ' + shot.viewport.charAt(0).toUpperCase() + shot.viewport.slice(1) : '');
        return '### ' + label + '\n\n![' + screen.label + ' — ' + label + '](' + link(shot.path) + ')\n\n' +
          '`' + shot.width + ' × ' + shot.height + '` · [`' + shot.path + '`](' + link(shot.path) + ')';
      }).join('\n\n')
    : 'No reference screenshot was captured for this screen.';
  return '# ' + screen.label + '\n\n' +
    'This file describes one Workbench screen for AI agents and other importers.\n\n' +
    '- Screen ID: `' + screen.id + '`\n' +
    '- Content hash: `' + screen.hash + '`\n' +
    '- Hash algorithm: SHA-256 over each included file’s archive-relative path and raw bytes, in lexical path order\n\n' +
    'Compare the complete content hash with an earlier export to tell whether this screen or one of its exported local dependencies changed. Generated READMEs, archive metadata, package configuration added by the exporter, and files outside the list below are not part of the hash.\n\n' +
    '## Entry points\n\n' + entryLines + '\n\n' +
    'Start with the entry points, preserve the existing component API and project conventions, and use the included dependencies and assets as supporting context. Do not assume omitted application code, installed packages, secrets, tests, or build output are available.\n\n' +
    '## Reference screenshots\n\n' + screenshotLines + '\n\n' +
    '## Files in this screen hash\n\n' + fileLines + '\n';
}

function packageName(specifier) {
  if (!specifier || specifier.charAt(0) === '.' || specifier.charAt(0) === '/' || specifier.charAt(0) === '#') return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(specifier)) return null;
  var parts = specifier.split('/');
  return specifier.charAt(0) === '@' ? parts.slice(0, 2).join('/') : parts[0];
}

function packageExportTarget(value) {
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) {
    for (var i = 0; i < value.length; i += 1) {
      var listed = packageExportTarget(value[i]);
      if (listed) return listed;
    }
    return null;
  }
  if (!value || typeof value !== 'object') return null;
  var conditions = ['default', 'import', 'browser', 'require', 'react-native'];
  for (var j = 0; j < conditions.length; j += 1) {
    var selected = packageExportTarget(value[conditions[j]]);
    if (selected) return selected;
  }
  return null;
}

function storybookConfigDirectories(root) {
  var found = [];
  var conventional = path.join(root, '.storybook');
  if (fs.existsSync(conventional)) found.push(conventional);
  var pkg;
  try { pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')); } catch (_) { return found; }
  Object.keys(pkg.scripts || {}).forEach(function (name) {
    var script = String(pkg.scripts[name] || '');
    if (!/\bstorybook\b/.test(script)) return;
    var pattern = /(?:--config-dir|-c)(?:=|\s+)(?:"([^"]+)"|'([^']+)'|([^\s]+))/g;
    var match;
    while ((match = pattern.exec(script))) {
      var directory = path.resolve(root, match[1] || match[2] || match[3]);
      if (inside(root, directory) && fs.existsSync(directory) && found.indexOf(directory) === -1) found.push(directory);
    }
  });
  return found;
}

function withoutComments(source) {
  var out = '';
  var quote = null;
  for (var i = 0; i < source.length; i += 1) {
    var char = source[i];
    var next = source[i + 1];
    if (quote) {
      out += char;
      if (char === '\\') { if (next) out += source[++i]; }
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"' || char === "'" || char === '`') { quote = char; out += char; continue; }
    if (char === '/' && next === '/') {
      while (i < source.length && source[i] !== '\n') i += 1;
      out += '\n';
      continue;
    }
    if (char === '/' && next === '*') {
      i += 2;
      while (i < source.length && !(source[i] === '*' && source[i + 1] === '/')) i += 1;
      i += 1;
      out += ' ';
      continue;
    }
    out += char;
  }
  return out;
}

function references(file, body) {
  var ext = path.extname(file).toLowerCase();
  var out = [];
  function take(pattern) {
    var match;
    while ((match = pattern.exec(body))) if (out.indexOf(match[1]) === -1) out.push(match[1]);
  }
  if (['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs'].indexOf(ext) > -1) {
    body = withoutComments(body);
    take(/(?:import|export)\s+(?:type\s+)?(?:[^"'`;]*?\s+from\s+)?["']([^"']+)["']/g);
    take(/(?:import|require)\s*\(\s*["']([^"']+)["']\s*\)/g);
    take(/new\s+URL\s*\(\s*["']([^"']+)["']\s*,\s*import\.meta\.url\s*\)/g);
  }
  if (['.css', '.scss', '.sass', '.less'].indexOf(ext) > -1) {
    take(/@(?:import|use|forward)\s+(?:url\(\s*)?["']([^"']+)["']/g);
    take(/url\(\s*["']?([^"')]+)["']?\s*\)/g);
  }
  if (ext === '.html' || ext === '.htm') take(/(?:src|href)\s*=\s*["']([^"'#]+)["']/gi);
  return out;
}

function stylesheetImports(body) {
  var imports = new Set();
  var pattern = /@(?:import|use|forward)\s+(?:url\(\s*)?["']([^"']+)["']/g;
  var match;
  while ((match = pattern.exec(body))) imports.add(match[1]);
  return imports;
}

function resolveReference(from, specifier, ownerRoot, allowedRoots) {
  var clean = String(specifier).split(/[?#]/)[0];
  if (!clean || /^(?:[a-z]+:|\/\/|#)/i.test(clean)) return null;
  var start;
  if (/^[@~]\//.test(clean)) start = path.resolve(ownerRoot, 'src', clean.slice(2));
  else start = clean.charAt(0) === '/' ? path.resolve(ownerRoot, '.' + clean) : path.resolve(path.dirname(from), clean);
  if (!allowedRoots.some(function (root) { return inside(root, start); })) return null;
  if (/\/$/.test(clean)) {
    try { if (fs.statSync(start).isDirectory()) return start; } catch (_) {}
  }
  for (var i = 0; i < RESOLVE_EXTENSIONS.length; i += 1) {
    var candidate = start + RESOLVE_EXTENSIONS[i];
    try { if (fs.statSync(candidate).isFile()) return candidate; } catch (_) {}
  }
  // TypeScript ESM source commonly spells a future JavaScript output path.
  if (/\.jsx?$/.test(start)) {
    var stem = start.replace(/\.jsx?$/, '');
    var typescript = /\.jsx$/.test(start) ? ['.tsx', '.ts'] : ['.ts', '.tsx'];
    for (var t = 0; t < typescript.length; t += 1) {
      try { if (fs.statSync(stem + typescript[t]).isFile()) return stem + typescript[t]; } catch (_) {}
    }
  }
  for (var j = 1; j < RESOLVE_EXTENSIONS.length; j += 1) {
    var index = path.join(start, 'index' + RESOLVE_EXTENSIONS[j]);
    try { if (fs.statSync(index).isFile()) return index; } catch (_) {}
  }
  return null;
}

function walk(dir, visit) {
  var entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch (_) { return; }
  entries.forEach(function (entry) {
    if (entry.name.charAt(0) === '.' && entry.name !== '.storybook') return;
    if (entry.isDirectory() && OMIT_DIRS[entry.name]) return;
    var file = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(file, visit);
    else if (entry.isFile()) visit(file);
  });
}

function nearestPackage(file, boundary) {
  var dir = fs.existsSync(file) && fs.statSync(file).isDirectory() ? file : path.dirname(file);
  while (inside(boundary, dir)) {
    var candidate = path.join(dir, 'package.json');
    if (fs.existsSync(candidate)) return candidate;
    if (dir === boundary) break;
    dir = path.dirname(dir);
  }
  return null;
}

function crc32(buffer) {
  var crc = 0xffffffff;
  for (var i = 0; i < buffer.length; i += 1) {
    crc ^= buffer[i];
    for (var bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

/* Stored ZIP entries avoid adding a runtime/native dependency to the extension. */
function zip(entries) {
  var local = [];
  var central = [];
  var offset = 0;
  entries.forEach(function (entry) {
    var name = Buffer.from(entry.name.replace(/\\/g, '/'));
    var body = Buffer.isBuffer(entry.body) ? entry.body : Buffer.from(entry.body);
    var crc = crc32(body);
    var header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0); header.writeUInt16LE(20, 4); header.writeUInt16LE(0x0800, 6);
    header.writeUInt32LE(crc, 14); header.writeUInt32LE(body.length, 18); header.writeUInt32LE(body.length, 22);
    header.writeUInt16LE(name.length, 26);
    local.push(header, name, body);
    var record = Buffer.alloc(46);
    record.writeUInt32LE(0x02014b50, 0); record.writeUInt16LE(20, 4); record.writeUInt16LE(20, 6); record.writeUInt16LE(0x0800, 8);
    record.writeUInt32LE(crc, 16); record.writeUInt32LE(body.length, 20); record.writeUInt32LE(body.length, 24);
    record.writeUInt16LE(name.length, 28); record.writeUInt32LE(offset, 42);
    central.push(record, name);
    offset += header.length + name.length + body.length;
  });
  var centralBody = Buffer.concat(central);
  var end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(entries.length, 8); end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBody.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat(local.concat([centralBody, end]));
}

function zipSize(entries) {
  return 22 + entries.reduce(function (total, entry) {
    var name = Buffer.byteLength(entry.name.replace(/\\/g, '/'));
    var body = Buffer.isBuffer(entry.body) ? entry.body.length : Buffer.byteLength(entry.body);
    return total + body + 76 + name * 2;
  }, 0);
}

function splitArchives(prefix, payload, readme, report, maximum) {
  maximum = Number(maximum) || MAX_ARCHIVE_BYTES;
  if (maximum < 1024) throw new Error('Export ZIP limit must be at least 1024 bytes');
  var groups = [];
  var current = [];
  var reportReserve = Math.max(32768, payload.length * 160);
  var baseReport = JSON.stringify(Object.assign({}, report, { parts: [] }), null, 2) + '\n';

  function provisional(group) {
    var listed = JSON.stringify({ part: 9999, of: 9999, files: group.map(function (entry) {
      return entry.name.slice(prefix.length + 1);
    }) }, null, 2) + '\n';
    return zipSize([
      { name: prefix + '/README.md', body: readme },
      { name: prefix + '/canonic-export.json', body: baseReport + ' '.repeat(reportReserve) },
      { name: prefix + '/canonic-export-part-9999.json', body: listed },
    ].concat(group));
  }

  payload.forEach(function (entry) {
    var candidate = current.concat([entry]);
    if (current.length && provisional(candidate) > maximum) {
      groups.push(current);
      current = [entry];
    } else current = candidate;
    if (provisional(current) > maximum) {
      throw new Error('“' + entry.name.slice(prefix.length + 1) + '” cannot fit inside the 10 MB export ZIP limit');
    }
  });
  if (current.length || !groups.length) groups.push(current);

  var archives;
  while (true) {
    var total = groups.length;
    var width = Math.max(2, String(total).length);
    function filename(index) {
      return total === 1 ? prefix + '.zip' : prefix + '-part-' + String(index + 1).padStart(width, '0') + '-of-' + String(total).padStart(width, '0') + '.zip';
    }
    report.parts = groups.map(function (group, index) {
      return { number: index + 1, filename: filename(index), files: group.length };
    });
    var reportBody = JSON.stringify(report, null, 2) + '\n';
    var splitReadme = readme + '\n## Archive parts\n\n' +
      (total === 1
        ? 'This export fits in one ZIP file.\n'
        : 'This export is split into ' + total + ' ZIP files so every upload remains at or below 10 MB. Extract every part into the same directory; their shared top-level folder and repeated metadata are designed to merge.\n');
    archives = groups.map(function (group, index) {
      var part = {
        part: index + 1, of: total, filename: filename(index),
        files: group.map(function (entry) { return entry.name.slice(prefix.length + 1); }),
      };
      var common = [
        { name: prefix + '/README.md', body: splitReadme },
        { name: prefix + '/canonic-export.json', body: reportBody },
        { name: prefix + '/canonic-export-part-' + String(index + 1).padStart(width, '0') + '.json', body: JSON.stringify(part, null, 2) + '\n' },
      ];
      var body = zip(common.concat(group));
      return { filename: filename(index), body: body, files: part.files };
    });
    var oversized = archives.findIndex(function (archive) { return archive.body.length > maximum; });
    if (oversized < 0) break;
    if (groups[oversized].length <= 1) {
      throw new Error('“' + groups[oversized][0].name.slice(prefix.length + 1) + '” cannot fit inside the 10 MB export ZIP limit');
    }
    var moved = groups[oversized].pop();
    if (groups[oversized + 1]) groups[oversized + 1].unshift(moved);
    else groups.push([moved]);
  }
  return archives;
}

function create(root, view, options) {
  options = options || {};
  root = path.resolve(root);
  var prefix = slug(view && view.name) + '-design-system';
  var owners = [{ key: 'project', root: root, destination: '' }];
  Object.keys((view && view.implementations) || {}).forEach(function (key) {
    var implRoot = view.implementations[key].root;
    if (!implRoot) return;
    implRoot = path.resolve(implRoot);
    if (owners.some(function (owner) { return owner.root === implRoot; })) return;
    owners.push({ key: key, root: implRoot, destination: inside(root, implRoot) ? path.relative(root, implRoot) : 'implementations/' + slug(key) });
  });
  var allowedRoots = owners.map(function (owner) { return owner.root; });
  var files = new Map();
  var queue = [];
  var dependencies = new Map();
  var warnings = [];
  var packages = {};
  var packageInfoByManifest = new Map();
  var packageRoots = {};
  var configuredPackages = new Set();
  var spriteDirectories = [];
  var screenSources = Object.keys((view && view.screens) || {});
  var screenDirectories = screenSlugs(screenSources);
  var screenData = {};
  screenSources.forEach(function (src) {
    var screen = view.screens[src];
    screenData[src] = { id: src, label: screen.label, direct: new Set(), entries: [] };
  });

  function ownerFor(file, preferred) {
    if (preferred && inside(preferred.root, file)) return preferred;
    return owners.filter(function (owner) { return inside(owner.root, file); }).sort(function (a, b) { return b.root.length - a.root.length; })[0] || null;
  }
  function destinationFor(file, owner) {
    var relative = path.relative(owner.root, file).replace(/\\/g, '/');
    return owner.destination ? owner.destination.replace(/\/$/, '') + '/' + relative : relative;
  }
  function rememberPackage(file, boundary) {
    var manifest = nearestPackage(file, boundary);
    if (!manifest) return null;
    if (!packageInfoByManifest.has(manifest)) {
      var info = null;
      try {
        var pkg = JSON.parse(fs.readFileSync(manifest, 'utf8'));
        info = { manifest: manifest, root: path.dirname(manifest), package: pkg };
        if (pkg.name) packageRoots[pkg.name] = info;
      } catch (_) {}
      packageInfoByManifest.set(manifest, info);
    }
    return packageInfoByManifest.get(manifest);
  }
  function localPackage(from, specifier, itemOwner) {
    var name = packageName(specifier);
    if (!name) return null;
    var info = packageRoots[name];
    if (!info) {
      var importer = rememberPackage(from, itemOwner.root);
      var groups = ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies'];
      var version = null;
      groups.some(function (group) {
        version = importer && importer.package[group] && importer.package[group][name];
        return !!version;
      });
      var linked = typeof version === 'string' && version.match(/^(?:link|file):(.+)$/);
      if (linked) {
        var linkedRoot = path.resolve(importer.root, linked[1]);
        var linkedOwner = ownerFor(linkedRoot);
        if (linkedOwner) info = rememberPackage(linkedRoot, linkedOwner.root);
      }
    }
    if (!info || info.package.name !== name) return null;
    var subpath = specifier.slice(name.length);
    var key = subpath ? '.' + subpath : '.';
    var exported = info.package.exports;
    var target = null;
    if (typeof exported === 'string' && key === '.') target = exported;
    else if (exported && typeof exported === 'object') {
      if (Object.prototype.hasOwnProperty.call(exported, key)) target = packageExportTarget(exported[key]);
      else if (key === '.' && !Object.keys(exported).some(function (entry) { return entry.charAt(0) === '.'; })) target = packageExportTarget(exported);
    }
    if (!target && key === '.') target = info.package.module || info.package.main;
    if (!target && subpath) target = '.' + subpath;
    if (!target) return null;
    return { info: info, target: resolveReference(info.manifest, target, info.root, allowedRoots) };
  }
  function add(file, preferred, reason, screenSource) {
    file = path.resolve(file);
    var owner = ownerFor(file, preferred);
    if (!owner || OMIT_FILE.test(file.replace(/\\/g, '/'))) return;
    var stat;
    try { stat = fs.statSync(file); } catch (_) { warnings.push('Missing ' + reason + ': ' + file); return; }
    if (stat.isDirectory()) { walk(file, function (child) { add(child, owner, reason, screenSource); }); return; }
    if (!stat.isFile()) return;
    var relative = path.relative(owner.root, file).replace(/\\/g, '/');
    if (relative.split('/').some(function (part) { return OMIT_DIRS[part]; })) return;
    if (screenSource) screenData[screenSource].direct.add(file);
    if (files.has(file)) return;
    var destination = destinationFor(file, owner);
    files.set(file, { source: file, destination: destination, owner: owner });
    rememberPackage(file, owner.root);
    queue.push(file);
  }
  function linkDependency(from, target) {
    var linked = files.has(target)
      ? [target]
      : Array.from(files.keys()).filter(function (file) { return inside(target, file); });
    if (!linked.length) return;
    if (!dependencies.has(from)) dependencies.set(from, new Set());
    linked.forEach(function (file) { dependencies.get(from).add(file); });
  }

  /* A project of a file that lists several, or whose root is elsewhere,
     names its file; one outside the project isn't exported. */
  add(options.manifest || path.join(root, 'workbench.yaml'), owners[0], 'workbench manifest');
  Object.keys((view && view.screens) || {}).forEach(function (src) {
    var screen = view.screens[src];
    if (screen.design) {
      add(screen.design, owners[0], 'design: ' + screen.label, src);
      if (files.has(path.resolve(screen.design))) screenData[src].entries.push({ kind: 'design', path: destinationFor(path.resolve(screen.design), owners[0]) });
    }
    (screen.code || []).forEach(function (entry) {
      if (!entry.path || !entry.exists) return;
      var owner = owners.find(function (candidate) { return candidate.key === entry.implementation; }) || ownerFor(entry.path);
      add(entry.path, owner, 'source: ' + screen.label, src);
      if (owner) screenData[src].entries.push({ kind: 'source', implementation: entry.implementation, path: destinationFor(path.resolve(entry.path), owner) });
    });
  });
  (options.portable && options.portable.previews || []).forEach(function (preview) {
    if (!screenData[preview.file]) return;
    preview.files.forEach(function (file) { add(file, ownerFor(file), 'Workbench preview dependency', preview.file); });
  });
  owners.forEach(function (owner) {
    var storybook = path.join(owner.root, '.storybook');
    if (fs.existsSync(storybook)) add(storybook, owner, 'Storybook configuration');
    var entries;
    try { entries = fs.readdirSync(owner.root); } catch (_) { return; }
    entries.filter(function (name) { return CONFIG_FILES.test(name); }).forEach(function (name) {
      add(path.join(owner.root, name), owner, 'project configuration');
    });
  });
  Object.keys((view && view.implementations) || {}).forEach(function (key) {
    var implementation = view.implementations[key];
    if (!implementation || implementation.kind !== 'storybook' || !implementation.root) return;
    var implementationRoot = path.resolve(implementation.root);
    var owner = ownerFor(implementationRoot);
    var screens = screenSources.filter(function (src) {
      return (view.screens[src].code || []).some(function (entry) { return entry.implementation === key; });
    });
    storybookConfigDirectories(implementationRoot).forEach(function (directory) {
      if (screens.length) screens.forEach(function (src) { add(directory, owner, 'Storybook configuration', src); });
      else add(directory, owner, 'Storybook configuration');
    });
  });

  for (var at = 0; at < queue.length; at += 1) {
    var file = queue[at];
    var item = files.get(file);
    var packageInfo = rememberPackage(file, item.owner.root);
    if (packageInfo && !configuredPackages.has(packageInfo.root)) {
      configuredPackages.add(packageInfo.root);
      try {
        fs.readdirSync(packageInfo.root).filter(function (name) { return CONFIG_FILES.test(name); }).forEach(function (name) {
          add(path.join(packageInfo.root, name), item.owner, 'package configuration');
        });
      } catch (_) {}
    }
    if (packageInfo && !CONFIG_FILES.test(path.basename(file))) {
      Array.from(files.keys()).filter(function (candidate) {
        return path.dirname(candidate) === packageInfo.root && CONFIG_FILES.test(path.basename(candidate));
      }).forEach(function (config) { linkDependency(file, config); });
    }
    var ext = path.extname(file).toLowerCase();
    if (TEXT_EXTENSIONS.indexOf(ext) === -1 && ext !== '.html' && ext !== '.htm') continue;
    var body;
    try { body = fs.readFileSync(file, 'utf8'); } catch (_) { continue; }
    if (/^vite\.config\.[cm]?[jt]s$/.test(path.basename(file)) && packageInfo) {
      var assetPattern = /iconDirs\s*:\s*\[\s*path\.resolve\(\s*process\.cwd\(\)\s*,\s*["']([^"']+)["']\s*\)/g;
      var assetMatch;
      while ((assetMatch = assetPattern.exec(body))) {
        var assetPath = path.resolve(packageInfo.root, assetMatch[1]);
        if (!allowedRoots.some(function (allowed) { return inside(allowed, assetPath); })) continue;
        add(assetPath, item.owner, 'Vite asset directory');
        linkDependency(file, assetPath);
        spriteDirectories.push(assetPath);
      }
    }
    var cssImports = ['.css', '.scss', '.sass', '.less'].indexOf(ext) > -1 ? stylesheetImports(body) : null;
    references(file, body).forEach(function (specifier) {
      var dependency = ['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs'].indexOf(ext) > -1 && !/^[@~]\//.test(specifier)
        ? packageName(specifier)
        : null;
      // CSS imports can use the same package export map as JavaScript imports.
      var cssPackage = cssImports && cssImports.has(specifier) && packageName(specifier);
      var local = (dependency || cssPackage) && localPackage(file, specifier, item.owner);
      if (local && local.target) {
        var packageOwner = ownerFor(local.info.root);
        add(local.info.manifest, packageOwner, 'package metadata for ' + (dependency || cssPackage));
        add(local.target, packageOwner, 'local package dependency of ' + path.basename(file));
        linkDependency(file, local.info.manifest);
        linkDependency(file, local.target);
      }
      else if (dependency && dependency !== '@canonic/workbench') packages[dependency] = true;
      else if (dependency === '@canonic/workbench') return;
      else {
        var resolved = resolveReference(file, specifier, item.owner.root, allowedRoots);
        if (resolved) {
          add(resolved, item.owner, 'dependency of ' + path.basename(file));
          linkDependency(file, resolved);
        }
        else if (!/^(?:[a-z]+:|\/\/|#)/i.test(specifier)) warnings.push('Could not resolve “' + specifier + '” from ' + item.destination);
      }
    });
  }

  // A virtual sprite is registered by the app, while components reference its
  // generated symbol IDs. Keep the SVG sources in those components' closures.
  if (spriteDirectories.length) files.forEach(function (item) {
    if (!/\.[cm]?[jt]sx?$/.test(item.source)) return;
    var body;
    try { body = fs.readFileSync(item.source, 'utf8'); } catch (_) { return; }
    if (!/#icon-/.test(body)) return;
    spriteDirectories.forEach(function (directory) { linkDependency(item.source, directory); });
  });

  var manifests = [];
  files.forEach(function (item) {
    var found = nearestPackage(item.source, item.owner.root);
    if (found && manifests.indexOf(found) === -1) manifests.push(found);
  });
  var dependencyVersions = {};
  (options.portable && options.portable.previews || []).forEach(function (preview) {
    (preview.packages || []).forEach(function (pkg) { packages[pkg.name] = true; dependencyVersions[pkg.name] = pkg.version; });
  });
  manifests.forEach(function (manifest) {
    try {
      var pkg = JSON.parse(fs.readFileSync(manifest, 'utf8'));
      ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies'].forEach(function (group) {
        Object.keys(pkg[group] || {}).forEach(function (name) {
          if (packages[name] && !dependencyVersions[name]) dependencyVersions[name] = pkg[group][name];
        });
      });
    } catch (_) { warnings.push('Could not read package manifest: ' + manifest); }
  });

  var screenReports = screenSources.slice().sort().map(function (src) {
    var data = screenData[src];
    var included = new Set();
    var pending = Array.from(data.direct);
    while (pending.length) {
      var current = pending.pop();
      if (included.has(current) || !files.has(current)) continue;
      included.add(current);
      Array.from(dependencies.get(current) || []).forEach(function (dependency) { pending.push(dependency); });
    }
    var items = Array.from(included).map(function (file) { return files.get(file); });
    var screen = {
      id: data.id,
      label: data.label,
      hash: contentHash(items),
      readme: null,
      entries: data.entries,
      files: items.map(function (item) { return item.destination; }).sort(),
      screenshots: [],
    };
    return screen;
  });
  var screenParents = {};
  var artifactParents = {};
  screenReports.forEach(function (screen) {
    var primary = screen.entries.find(function (entry) { return entry.kind === 'design'; }) || screen.entries[0];
    var parent = primary ? path.posix.dirname(primary.path) : 'screens/' + screenDirectories[screen.id];
    if (parent === '.') parent = '';
    screenParents[screen.id] = parent;
    artifactParents[parent] = (artifactParents[parent] || 0) + 1;
  });
  var reserved = new Set(Array.from(files.values()).map(function (item) { return item.destination; }));
  reserved.add('README.md');
  reserved.add('canonic-export.json');
  var screenshotDirectories = {};
  var artifactNames = {};
  screenReports.forEach(function (screen) {
    var parent = screenParents[screen.id];
    var shared = artifactParents[parent] > 1;
    if (!artifactNames[parent]) artifactNames[parent] = {};
    var base = slug(screen.label);
    if (artifactNames[parent][base]) base += '-' + crypto.createHash('sha256').update(screen.id).digest('hex').slice(0, 8);
    artifactNames[parent][base] = true;
    var readme = path.posix.join(parent, shared ? base + '.README.md' : 'README.md');
    if (reserved.has(readme)) readme = path.posix.join(parent, base + '.README.md');
    screen.readme = readme;
    reserved.add(readme);
    screenshotDirectories[screen.id] = path.posix.join(parent, 'screenshots', shared ? base : '');
  });
  var reportByScreen = {};
  screenReports.forEach(function (screen) { reportByScreen[screen.id] = screen; });
  var screenshotEntries = [];
  var screenshotNames = {};
  (options.screenshots || []).forEach(function (shot) {
    var screen = reportByScreen[shot.screen];
    if (!screen || !shot.body) return;
    var base = slug(shot.variant || shot.label || 'default');
    var key = screen.id + '\0' + base;
    var count = (screenshotNames[key] || 0) + 1;
    screenshotNames[key] = count;
    var name = base + (count > 1 ? '-' + count : '') + '.jpg';
    var screenshot = {
      state: shot.state || null,
      label: shot.label || 'Default',
      viewport: shot.viewport || 'fit',
      path: path.posix.join(screenshotDirectories[screen.id], name),
      width: shot.width,
      height: shot.height,
    };
    screen.screenshots.push(screenshot);
    screenshotEntries.push({ name: prefix + '/' + screenshot.path, body: shot.body });
  });
  var report = {
    name: view && view.name,
    generatedAt: new Date().toISOString(),
    selection: 'Workbench design/source/story entry points plus transitive local imports, assets, and captured visual references.',
    files: Array.from(files.values()).map(function (item) { return item.destination; }).sort(),
    screens: screenReports,
    dependencies: Object.keys(packages).sort().map(function (name) { return { name: name, version: dependencyVersions[name] || null }; }),
    warnings: Array.from(new Set(warnings)).sort(),
    captureWarnings: Array.from(new Set(options.captureWarnings || [])).sort(),
    browser: options.portable ? { entry: 'browser/index.html', manifest: 'browser/workbench.json',
      previews: options.portable.previews.map(function (preview) { return { id: preview.id, source: preview.file, entry: preview.directory + '/index.html', states: preview.states }; }),
      warnings: options.portable.warnings } : null,
  };
  var readme = '# ' + (view && view.name || 'Workbench') + ' design-system export\n\n' +
    'This archive was generated from Workbench. Workspace files keep their original project-relative paths at this archive’s root; sources outside the workspace are under `implementations/`.\n\n' +
    'The export starts with every design, component, page, and story source declared or discovered by the workbench, then includes their transitive local imports and referenced assets. Each generated screen guide embeds reference screenshots for its declared states or Storybook stories at the configured viewports: desktop, mobile, both for responsive, or the standard fit frame. Duplicate capture sizes are removed. It intentionally excludes dependencies installed in `node_modules`, build output, secrets, tests, and unrelated application files.\n\n' +
    'See `canonic-export.json` for the exact file list, package dependencies, unresolved references, and each screen’s content hash. Each generated screen guide and its `screenshots/` directory live beside that component or page’s primary exported entry point. Install the listed packages with your preferred package manager before running the copied Storybook or app setup.\n' +
    (options.portable ? '\n## Portable browser previews\n\nServe this extracted directory with any static HTTP server and open `browser/index.html`. The viewer includes searchable previews, state and viewport selection, editable controls, reset, actions, and documentation. Copy its address to share a selection. These compiled Workbench previews need no Electron, Workbench, package installation, or build step. Direct preview pages accept `?state=<id>`. Original editable sources remain in their project-relative locations. Check `browser.warnings` in the export report for previews that could not be built.\n' : '');
  var generatedPackage = { private: true, name: slug(view && view.name) + '-design-system', version: '0.0.0', dependencies: {} };
  report.dependencies.forEach(function (dependency) { generatedPackage.dependencies[dependency.name] = dependency.version || '*'; });
  var entries = Array.from(files.values()).map(function (item) {
    return { name: prefix + '/' + item.destination, body: fs.readFileSync(item.source) };
  }).sort(function (a, b) { return a.name.localeCompare(b.name); });
  screenReports.forEach(function (screen) {
    entries.push({ name: prefix + '/' + screen.readme, body: screenReadme(screen) });
  });
  screenshotEntries.forEach(function (entry) { entries.push(entry); });
  (options.portable && options.portable.files || []).forEach(function (file) {
    if (!/^browser\//.test(file.path) || file.path.split('/').some(function (part) { return part === '..'; })) throw new Error('Invalid portable export path');
    entries.push({ name: prefix + '/' + file.path, body: Buffer.from(file.data, 'base64') });
  });
  if (options.portable && !files.has(path.join(root, 'workbench-env.d.ts'))) {
    var types = fs.readFileSync(path.join(__dirname, 'preview', 'api.d.ts'), 'utf8');
    entries.push({ name: prefix + '/workbench-env.d.ts', body: 'declare module "@canonic/workbench" {\n' + types + '\n}\n' });
  }
  if (!files.has(path.join(root, 'package.json'))) {
    entries.push({ name: prefix + '/package.json', body: JSON.stringify(generatedPackage, null, 2) + '\n' });
  }
  entries.sort(function (a, b) { return a.name.localeCompare(b.name); });
  var archives = splitArchives(prefix, entries, readme, report, options.maxArchiveBytes);
  var download = archives.length === 1 ? archives[0] : {
    filename: prefix + '-parts.zip',
    body: zip([{ name: 'README.md', body:
      '# Workbench export ZIP parts\n\n' +
      'This bundle contains ' + archives.length + ' attachment-sized ZIP files. Extract this outer ZIP, then upload the numbered ZIP parts together or extract every part into one directory.\n' },
    ].concat(archives.map(function (archive) {
      return { name: archive.filename, body: archive.body };
    }))),
  };
  return {
    filename: archives.length === 1 ? archives[0].filename : null,
    body: archives.length === 1 ? archives[0].body : null,
    archives: archives,
    download: download,
    report: report,
  };
}

module.exports = { create: create, zip: zip, zipSize: zipSize, references: references, packageName: packageName, MAX_ARCHIVE_BYTES: MAX_ARCHIVE_BYTES };
