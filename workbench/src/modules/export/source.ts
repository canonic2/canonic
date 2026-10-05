import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { packageName, parseReferences, stylesheetImports } from './references.ts';
import { splitArchives, zip } from './archive.ts';
import type { ExportView, ExportOptions, ExportResult, PageReport, PageEntry, SourceFile, SourceOwner, PackageInfo, PackageManifest, PackageExport, ExportReport, ZipEntry } from './types.ts';

const workbenchRoot = fileURLToPath(new URL('../../../', import.meta.url));
var TEXT_EXTENSIONS = ['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs', '.json', '.css', '.scss', '.sass', '.less'];
var RESOLVE_EXTENSIONS = ['', '.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs', '.json', '.css', '.scss', '.sass', '.less', '.svg', '.png', '.jpg', '.jpeg', '.gif', '.webp', '.avif', '.woff', '.woff2', '.ttf'];
var OMIT_DIRS: Record<string, boolean> = { '.git': true, '.canonic': true, 'node_modules': true, 'dist': true, 'build': true, 'coverage': true, '.next': true, '.turbo': true };
var OMIT_FILE = /(?:^|\/)(?:\.env(?:\.|$)|.*\.(?:test|spec)\.[cm]?[jt]sx?|.*\.snap$)/i;
var CONFIG_FILES = /^(?:package\.json|(?:ts|js)config(?:\.[^.]+)?\.json|vite\.config\.[cm]?[jt]s|tailwind\.config\.[cm]?[jt]s|postcss\.config\.[cm]?[jt]s)$/;

function inside(root: string, file: string) {
  var relative = path.relative(root, file);
  return relative === '' || (relative.indexOf('..' + path.sep) !== 0 && relative !== '..' && !path.isAbsolute(relative));
}

function slug(value: string | null | undefined) {
  return String(value || 'canonic').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'canonic';
}

function pageSlugs(sources: string[]) {
  var used: Record<string, boolean> = {};
  var out: Record<string, string> = {};
  sources.slice().sort().forEach(function (source) {
    var base = slug(source);
    var value = base;
    if (used[value]) value += '-' + crypto.createHash('sha256').update(source).digest('hex').slice(0, 8);
    used[value] = true;
    out[source] = value;
  });
  return out;
}

function contentHash(items: SourceFile[]) {
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

function pageReadme(page: PageReport) {
  function link(file: string) {
    return path.posix.relative(path.posix.dirname(page.readme), file) || path.posix.basename(file);
  }
  var entryLines = page.entries.length
    ? page.entries.map(function (entry) {
        var detail = entry.kind === 'design' ? 'Design' : 'Source (' + entry.implementation + ')';
        return '- ' + detail + ': [`' + entry.path + '`](' + link(entry.path) + ')';
      }).join('\n')
    : '- No portable design or source entry point was resolved for this page.';
  var fileLines = page.files.length
    ? page.files.map(function (file) { return '- [`' + file + '`](' + link(file) + ')'; }).join('\n')
    : '- No files are included in this page hash.';
  var screenshotLines = page.screenshots.length
    ? page.screenshots.map(function (shot) {
        var sizeLabel = shot.sizeLabel || (shot.size ? shot.size.charAt(0).toUpperCase() + shot.size.slice(1) : '');
        var label = shot.label + (sizeLabel ? ' · ' + sizeLabel : '');
        return '### ' + label + '\n\n![' + page.label + ' — ' + label + '](' + link(shot.path) + ')\n\n' +
          '`' + shot.width + ' × ' + shot.height + '` · [`' + shot.path + '`](' + link(shot.path) + ')';
      }).join('\n\n')
    : 'No reference screenshot was captured for this page.';
  return '# ' + page.label + '\n\n' +
    'This file describes one Workbench page for AI agents and other importers.\n\n' +
    '- Page ID: `' + page.id + '`\n' +
    '- Content hash: `' + page.hash + '`\n' +
    '- Hash algorithm: SHA-256 over each included file’s archive-relative path and raw bytes, in lexical path order\n\n' +
    'Compare the complete content hash with an earlier export to tell whether this page or one of its exported local dependencies changed. Generated READMEs, archive metadata, package configuration added by the exporter, and files outside the list below are not part of the hash.\n\n' +
    '## Entry points\n\n' + entryLines + '\n\n' +
    'Start with the entry points, preserve the existing component API and project conventions, and use the included dependencies and assets as supporting context. Do not assume omitted application code, installed packages, secrets, tests, or build output are available.\n\n' +
    '## Reference screenshots\n\n' + screenshotLines + '\n\n' +
    '## Files in this page hash\n\n' + fileLines + '\n';
}

function packageExportTarget(value: PackageExport | undefined): string | null {
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

function storybookConfigDirectories(root: string) {
  var found: string[] = [];
  var conventional = path.join(root, '.storybook');
  if (fs.existsSync(conventional)) found.push(conventional);
  var pkg: PackageManifest;
  try { pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')); } catch (_) { return found; }
  Object.keys(pkg.scripts || {}).forEach(function (name) {
    var script = String(pkg.scripts![name] || '');
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

function resolveReference(from: string, specifier: string, ownerRoot: string, allowedRoots: string[]): string | null {
  var clean = String(specifier).split(/[?#]/)[0];
  if (!clean || /^(?:[a-z]+:|\/\/|#)/i.test(clean)) return null;
  var start: string;
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

function walk(dir: string, visit: (file: string) => void) {
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

function nearestPackage(file: string, boundary: string): string | null {
  var dir = fs.existsSync(file) && fs.statSync(file).isDirectory() ? file : path.dirname(file);
  while (inside(boundary, dir)) {
    var candidate = path.join(dir, 'package.json');
    if (fs.existsSync(candidate)) return candidate;
    if (dir === boundary) break;
    dir = path.dirname(dir);
  }
  return null;
}

export function create(root: string, view: ExportView | null | undefined, options: ExportOptions = {}): ExportResult {
  options = options || {};
  root = path.resolve(root);
  var prefix = slug(view && view.name) + '-design-system';
  const implementations = view?.implementations || {};
  const pages = view?.pages || {};
  var owners: SourceOwner[] = [{ key: 'project', root: root, destination: '' }];
  Object.keys(implementations).forEach(function (key) {
    var implRoot = implementations[key].root;
    if (!implRoot) return;
    implRoot = path.resolve(implRoot);
    if (owners.some(function (owner) { return owner.root === implRoot; })) return;
    owners.push({ key: key, root: implRoot, destination: inside(root, implRoot) ? path.relative(root, implRoot) : 'implementations/' + slug(key) });
  });
  var allowedRoots = owners.map(function (owner) { return owner.root; });
  var files = new Map<string, SourceFile>();
  var queue: string[] = [];
  var dependencies = new Map<string, Set<string>>();
  var warnings: string[] = [...(options.warnings || [])];
  var packages: Record<string, boolean> = {};
  var packageInfoByManifest = new Map<string, PackageInfo | null>();
  var packageRoots: Record<string, PackageInfo> = {};
  var configuredPackages = new Set<string>();
  var spriteDirectories: string[] = [];
  var pageSources = Object.keys(pages);
  var pageDirectories = pageSlugs(pageSources);
  var pageData: Record<string, { id: string; label: string; direct: Set<string>; entries: PageEntry[] }> = {};
  pageSources.forEach(function (src) {
    var page = pages[src];
    pageData[src] = { id: src, label: page.label, direct: new Set<string>(), entries: [] };
  });

  function ownerFor(file: string, preferred?: SourceOwner | null): SourceOwner | null {
    if (preferred && inside(preferred.root, file)) return preferred;
    return owners.filter(function (owner) { return inside(owner.root, file); }).sort(function (a, b) { return b.root.length - a.root.length; })[0] || null;
  }
  function destinationFor(file: string, owner: SourceOwner) {
    var relative = path.relative(owner.root, file).replace(/\\/g, '/');
    return owner.destination ? owner.destination.replace(/\/$/, '') + '/' + relative : relative;
  }
  function rememberPackage(file: string, boundary: string): PackageInfo | null {
    var manifest = nearestPackage(file, boundary);
    if (!manifest) return null;
    if (!packageInfoByManifest.has(manifest)) {
      var info: PackageInfo | null = null;
      try {
        var pkg: PackageManifest = JSON.parse(fs.readFileSync(manifest, 'utf8'));
        info = { manifest: manifest, root: path.dirname(manifest), package: pkg };
        if (pkg.name) packageRoots[pkg.name] = info;
      } catch (_) {}
      packageInfoByManifest.set(manifest, info);
    }
    return packageInfoByManifest.get(manifest) || null;
  }
  function localPackage(from: string, specifier: string, itemOwner: SourceOwner) {
    var name = packageName(specifier);
    if (!name) return null;
    var info: PackageInfo | null | undefined = packageRoots[name];
    if (!info) {
      var importer = rememberPackage(from, itemOwner.root);
      const groups = ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies'] as const;
      var version: string | null | undefined = null;
      for (const group of groups) {
        version = importer?.package[group]?.[name];
        if (version) break;
      }
      var linked = typeof version === 'string' && version.match(/^(?:link|file):(.+)$/);
      if (linked) {
        var linkedRoot = path.resolve(importer!.root, linked[1]);
        var linkedOwner = ownerFor(linkedRoot);
        if (linkedOwner) info = rememberPackage(linkedRoot, linkedOwner.root);
      }
    }
    if (!info || info.package.name !== name) return null;
    var subpath = specifier.slice(name.length);
    var key = subpath ? '.' + subpath : '.';
    var exported = info.package.exports;
    var target: string | null | undefined = null;
    if (typeof exported === 'string' && key === '.') target = exported;
    else if (exported && typeof exported === 'object') {
      if (Object.prototype.hasOwnProperty.call(exported, key)) target = packageExportTarget((exported as Record<string, PackageExport>)[key]);
      else if (key === '.' && !Object.keys(exported).some(function (entry) { return entry.charAt(0) === '.'; })) target = packageExportTarget(exported);
    }
    if (!target && key === '.') target = info.package.module || info.package.main;
    if (!target && subpath) target = '.' + subpath;
    if (!target) return null;
    return { info: info, target: resolveReference(info.manifest, target, info.root, allowedRoots) };
  }
  function add(file: string, preferred: SourceOwner | null | undefined, reason: string, pageSource?: string) {
    file = path.resolve(file);
    var owner = ownerFor(file, preferred);
    if (!owner || OMIT_FILE.test(file.replace(/\\/g, '/'))) return;
    var stat;
    try { stat = fs.statSync(file); } catch (_) { warnings.push('Missing ' + reason + ': ' + file); return; }
    if (stat.isDirectory()) { walk(file, function (child) { add(child, owner, reason, pageSource); }); return; }
    if (!stat.isFile()) return;
    var relative = path.relative(owner.root, file).replace(/\\/g, '/');
    if (relative.split('/').some(function (part) { return OMIT_DIRS[part]; })) return;
    if (pageSource) pageData[pageSource].direct.add(file);
    if (files.has(file)) return;
    var destination = destinationFor(file, owner);
    files.set(file, { source: file, destination: destination, owner: owner });
    rememberPackage(file, owner.root);
    queue.push(file);
  }
  function linkDependency(from: string, target: string) {
    var linked = files.has(target)
      ? [target]
      : Array.from(files.keys()).filter(function (file) { return inside(target, file); });
    if (!linked.length) return;
    if (!dependencies.has(from)) dependencies.set(from, new Set());
    linked.forEach(function (file) { dependencies.get(from)!.add(file); });
  }

  /* A space of a file that lists several, or whose root is elsewhere,
     names its file; one outside the project isn't exported. */
  add(options.manifest || path.join(root, 'workbench.yaml'), owners[0], 'workbench manifest');
  Object.keys(pages).forEach(function (src) {
    var page = pages[src];
    if (page.design) {
      add(page.design, owners[0], 'design: ' + page.label, src);
      if (files.has(path.resolve(page.design))) pageData[src].entries.push({ kind: 'design', path: destinationFor(path.resolve(page.design), owners[0]) });
    }
    (page.code || []).forEach(function (entry) {
      if (!entry.path || !entry.exists) return;
      var owner = owners.find(function (candidate) { return candidate.key === entry.implementation; }) || ownerFor(entry.path);
      add(entry.path, owner, 'source: ' + page.label, src);
      if (owner) pageData[src].entries.push({ kind: 'source', implementation: entry.implementation, path: destinationFor(path.resolve(entry.path), owner) });
    });
  });
  (options.portable && options.portable.previews || []).forEach(function (preview) {
    if (!pageData[preview.file]) return;
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
  Object.keys(implementations).forEach(function (key) {
    var implementation = implementations[key];
    if (!implementation || implementation.kind !== 'storybook' || !implementation.root) return;
    var implementationRoot = path.resolve(implementation.root);
    var owner = ownerFor(implementationRoot);
    var storyPages = pageSources.filter(function (src) {
      return (pages[src].code || []).some(function (entry) { return entry.implementation === key; });
    });
    storybookConfigDirectories(implementationRoot).forEach(function (directory) {
      if (storyPages.length) storyPages.forEach(function (src) { add(directory, owner, 'Storybook configuration', src); });
      else add(directory, owner, 'Storybook configuration');
    });
  });

  for (var at = 0; at < queue.length; at += 1) {
    var file = queue[at];
    var item = files.get(file)!;
    var packageInfo = rememberPackage(file, item.owner.root);
    if (packageInfo && !configuredPackages.has(packageInfo.root)) {
      configuredPackages.add(packageInfo.root);
      try {
        fs.readdirSync(packageInfo.root).filter(function (name) { return CONFIG_FILES.test(name); }).forEach(function (name) {
          add(path.join(packageInfo!.root, name), item.owner, 'package configuration');
        });
      } catch (_) {}
    }
    if (packageInfo && !CONFIG_FILES.test(path.basename(file))) {
      Array.from(files.keys()).filter(function (candidate) {
        return path.dirname(candidate) === packageInfo!.root && CONFIG_FILES.test(path.basename(candidate));
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
    parseReferences(ext, body).forEach(function (specifier) {
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
      else if (dependency && dependency !== '@canonic2/workbench') packages[dependency] = true;
      else if (dependency === '@canonic2/workbench') return;
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

  var manifests: string[] = [];
  files.forEach(function (item) {
    var found = nearestPackage(item.source, item.owner.root);
    if (found && manifests.indexOf(found) === -1) manifests.push(found);
  });
  var dependencyVersions: Record<string, string | null | undefined> = {};
  (options.portable && options.portable.previews || []).forEach(function (preview) {
    (preview.packages || []).forEach(function (pkg) { packages[pkg.name] = true; dependencyVersions[pkg.name] = pkg.version; });
  });
  manifests.forEach(function (manifest) {
    try {
      var pkg: PackageManifest = JSON.parse(fs.readFileSync(manifest, 'utf8'));
      (['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies'] as const).forEach(function (group) {
        Object.keys(pkg[group] || {}).forEach(function (name) {
          if (packages[name] && !dependencyVersions[name]) dependencyVersions[name] = pkg[group]![name];
        });
      });
    } catch (_) { warnings.push('Could not read package manifest: ' + manifest); }
  });

  var pageReports = pageSources.slice().sort().map(function (src) {
    var data = pageData[src];
    var included = new Set<string>();
    var pending = Array.from(data.direct);
    while (pending.length) {
      var current = pending.pop()!;
      if (included.has(current) || !files.has(current)) continue;
      included.add(current);
      Array.from(dependencies.get(current) || []).forEach(function (dependency) { pending.push(dependency); });
    }
    var items = Array.from(included).map(function (file) { return files.get(file)!; });
    var page: PageReport = {
      id: data.id,
      label: data.label,
      hash: contentHash(items),
      readme: '',
      entries: data.entries,
      files: items.map(function (item) { return item.destination; }).sort(),
      screenshots: [],
    };
    return page;
  });
  var pageParents: Record<string, string> = {};
  var artifactParents: Record<string, number> = {};
  pageReports.forEach(function (page) {
    var primary = page.entries.find(function (entry) { return entry.kind === 'design'; }) || page.entries[0];
    var parent = primary ? path.posix.dirname(primary.path) : 'workbench-pages/' + pageDirectories[page.id];
    if (parent === '.') parent = '';
    pageParents[page.id] = parent;
    artifactParents[parent] = (artifactParents[parent] || 0) + 1;
  });
  var reserved = new Set(Array.from(files.values()).map(function (item) { return item.destination; }));
  reserved.add('README.md');
  reserved.add('canonic-export.json');
  var screenshotDirectories: Record<string, string> = {};
  var artifactNames: Record<string, Record<string, boolean>> = {};
  pageReports.forEach(function (page) {
    var parent = pageParents[page.id];
    var shared = artifactParents[parent] > 1;
    if (!artifactNames[parent]) artifactNames[parent] = {};
    var base = slug(page.label);
    if (artifactNames[parent][base]) base += '-' + crypto.createHash('sha256').update(page.id).digest('hex').slice(0, 8);
    artifactNames[parent][base] = true;
    var readme = path.posix.join(parent, shared ? base + '.README.md' : 'README.md');
    if (reserved.has(readme)) readme = path.posix.join(parent, base + '.README.md');
    page.readme = readme;
    reserved.add(readme);
    screenshotDirectories[page.id] = path.posix.join(parent, 'screenshots', shared ? base : '');
  });
  var reportByPage: Record<string, PageReport> = {};
  pageReports.forEach(function (page) { reportByPage[page.id] = page; });
  var screenshotEntries: ZipEntry[] = [];
  var screenshotNames: Record<string, number> = {};
  (options.screenshots || []).forEach(function (shot) {
    var page = reportByPage[shot.page];
    if (!page || !shot.body) return;
    var base = slug(shot.variant || shot.label || 'default');
    var key = page.id + '\0' + base;
    var count = (screenshotNames[key] || 0) + 1;
    screenshotNames[key] = count;
    var name = base + (count > 1 ? '-' + count : '') + '.jpg';
    var screenshot: PageReport['screenshots'][number] = {
      state: shot.state || null,
      label: shot.label || 'Default',
      size: shot.size || 'fit',
      path: path.posix.join(screenshotDirectories[page.id], name),
      width: shot.width,
      height: shot.height,
    };
    if (shot.sizeLabel) screenshot.sizeLabel = shot.sizeLabel;
    page.screenshots.push(screenshot);
    screenshotEntries.push({ name: prefix + '/' + screenshot.path, body: shot.body });
  });
  var report: ExportReport = {
    name: view?.name,
    generatedAt: new Date().toISOString(),
    selection: 'Workbench design/source/story entry points plus transitive local imports, assets, and captured visual references.',
    files: Array.from(files.values()).map(function (item) { return item.destination; }).sort(),
    pages: pageReports,
    dependencies: Object.keys(packages).sort().map(function (name) { return { name: name, version: dependencyVersions[name] || null }; }),
    warnings: Array.from(new Set(warnings)).sort(),
    captureWarnings: Array.from(new Set(options.captureWarnings || [])).sort(),
    browser: options.portable ? { entry: 'browser/index.html', manifest: 'browser/workbench.json',
      previews: options.portable.previews.map(function (preview) { return { id: preview.id, source: preview.file, entry: preview.directory + '/index.html', states: preview.states }; }),
      warnings: options.portable.warnings } : null,
  };
  var readme = '# ' + (view && view.name || 'Workbench') + ' design-system export\n\n' +
    'This archive was generated from Workbench. Workspace files keep their original project-relative paths at this archive’s root; sources outside the workspace are under `implementations/`.\n\n' +
    'The export starts with every design, component, page, and story source declared or discovered by the workbench, then includes their transitive local imports and referenced assets. Each generated page guide embeds reference screenshots for its declared states or Storybook stories at each size the page supports except Resizable; a size that fills the canvas, and Fit, are captured 1440 wide or 900 tall. Duplicate capture sizes are removed. A docs page instead embeds, for each lens, the whole page at its 960-pixel layout and each example the lens renders, cropped to its panel. It intentionally excludes dependencies installed in `node_modules`, build output, secrets, tests, and unrelated application files.\n\n' +
    'See `canonic-export.json` for the exact file list, package dependencies, unresolved references, and each page’s content hash. Each generated page guide and its `screenshots/` directory live beside that component or page’s primary exported entry point. Install the listed packages with your preferred package manager before running the copied Storybook or app setup.\n' +
    (options.portable ? '\n## Portable browser previews\n\nServe this extracted directory with any static HTTP server and open `browser/index.html`. The viewer includes searchable previews, state and size selection, editable controls, reset, actions, and documentation. Copy its address to share a selection. These compiled Workbench previews need no Electron, Workbench, package installation, or build step. Direct preview pages accept `?state=<id>`. Original editable sources remain in their project-relative locations. Check `browser.warnings` in the export report for previews that could not be built.\n' : '');
  var generatedPackage = { private: true, name: slug(view && view.name) + '-design-system', version: '0.0.0', dependencies: {} as Record<string, string> };
  report.dependencies.forEach(function (dependency) { generatedPackage.dependencies[dependency.name] = dependency.version || '*'; });
  var entries: ZipEntry[] = Array.from(files.values()).map(function (item) {
    return { name: prefix + '/' + item.destination, body: fs.readFileSync(item.source) };
  }).sort(function (a, b) { return a.name.localeCompare(b.name); });
  pageReports.forEach(function (page) {
    entries.push({ name: prefix + '/' + page.readme, body: pageReadme(page) });
  });
  screenshotEntries.forEach(function (entry) { entries.push(entry); });
  (options.portable && options.portable.files || []).forEach(function (file) {
    if (!/^browser\//.test(file.path) || file.path.split('/').some(function (part) { return part === '..'; })) throw new Error('Invalid portable export path');
    entries.push({ name: prefix + '/' + file.path, body: Buffer.from(file.data, 'base64') });
  });
  if (options.portable && !files.has(path.join(root, 'workbench-env.d.ts'))) {
    var types = fs.readFileSync(path.join(workbenchRoot, 'preview', 'api.d.ts'), 'utf8');
    entries.push({ name: prefix + '/workbench-env.d.ts', body: '// Canonic types: see browser/CANONIC-LICENSE.txt.\n' + 'declare module "@canonic2/workbench" {\n' + types + '\n}\n' });
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
    ...archives.map(function (archive) {
      return { name: archive.filename, body: archive.body };
    })]),
  };
  return {
    filename: archives.length === 1 ? archives[0].filename : null,
    body: archives.length === 1 ? archives[0].body : null,
    archives: archives,
    download: download,
    report: report,
  };
}
