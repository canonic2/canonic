/* Workbench server
   ----------------
   Serves the project and the workbench over one http origin, and takes the
   workbench's screenshots.

   Two reasons this exists rather than a static server:
   - The capture helper's surface and its preview have to share an
     origin; the live-DOM fallback also needs that access to read the frame.
   - A page can't write a file. Every browser-side route to the workspace —
     a download, the File System Access API — either lands in the wrong
     folder or asks permission every time. A POST to a server that already
     has the workspace open asks nothing.

   Plain node, no dependencies, and no vscode import: extension.js is the only
   part that knows it's in an editor, so this file can be run and tested on its
   own with `node server.js <folder>`.
*/

var http = require('http');
var fs = require('fs');
var os = require('os');
var path = require('path');
var childProcess = require('child_process');
var crypto = require('crypto');

var electronCapture = require('./electron-capture');
var handoff = require('./handoff');
var config = require('./config');
var spaces = require('./spaces');
var remote = require('./remote');
var designExport = require('./src/modules/export/index.ts');
var previewService = require('./preview-service');
var previewScripts = require('./preview-scripts');
var implementationProxy = require('./proxy');
var previewCompiler = require('./preview/compiler.cjs');
var windowStream = require('./window-stream');
var agentView = require('./agent-view');
var manifest = require('./workbench/manifest');
var docsService = require('./src/docs/docs-service.ts');
var browserModules = require('./src/server/browser-modules.ts');
var canvasSpaces = require('./src/spaces/coordinator.ts');
var nativePool = require('./src/native-streams/pool.ts');
var docsList = require('./src/docs/pages.ts');
var sizeSchema = require('./src/sizes/schema.ts');
var sizeExport = require('./src/sizes/export.ts');
var sizeChoice = require('./src/sizes/browser/choice.ts');
var sizeModel = require('./src/sizes/browser/size.ts');
var sizeEdit = require('./src/sizes/edit.ts');

var SHOT_PATH = '/_workbench/shot';
var CAPTURE_WARM_PATH = '/_workbench/capture/warm';
var CAPTURE_PREPARE_PATH = '/_workbench/capture/prepare';
var CAPTURE_PAGE_PATH = '/_workbench/capture/page';
var CAPTURE_PAGE_IMAGE_PATH = '/_workbench/capture/page/image';
var CAPTURE_PAGE_PREPARE_PATH = '/_workbench/capture/page/prepare';
var CAPTURE_PATH = '/_workbench/capture';
var CAPTURE_IMAGE_PATH = '/_workbench/capture/image';
var HANDOFF_PATH = '/_workbench/handoff';
var LOG_PATH = '/_workbench/log';
var CONFIG_PATH = '/_workbench/config';
var CONFIG_FILE_PATH = '/_workbench/config-file';
var SIZES_PATH = '/_workbench/sizes';
var OPEN_PATH = '/_workbench/open';
var STORIES_PATH = '/_workbench/stories';
var SPACES_PATH = '/_workbench/spaces';
var SPACES_OPEN_PATH = '/_workbench/spaces/open';
var MANIFEST_PATH = '/_workbench/manifest/';
var EXPORT_PATH = '/_workbench/export';
var SIMULATOR_PATH = '/_workbench/simulator';
var SIMULATOR_INPUT_PATH = '/_workbench/simulator/input';
var SIMULATOR_STREAM_PATH = '/_workbench/simulator/stream';
var WINDOW_STREAM_PATH = '/_workbench/window/stream';
var HANDOFFS_DIR = path.join('.canonic', '.handoffs');

/* Two folders, one origin
   -----------------------
   The workbench ships inside this extension, so it isn't in the workspace and
   can't be found by walking up from a file in it. It gets a reserved prefix of
   its own instead, served out of the extension's own folder, while everything
   under / is the project. Same origin either way, which is what the screenshot
   needs, and what lets the workbench read the project's workbench.yaml.

   The prefix is the one the POST routes above already live under, so a project
   only ever loses one name at its root, and "_workbench" is not a folder
   anybody keeps design files in. */
var WORKBENCH_PREFIX = '/_workbench/';
var WORKBENCH_DIR = path.join(__dirname, 'workbench');
var WORKBENCH_PATH = WORKBENCH_PREFIX;
var LUCIDE_PATH = require.resolve('lucide/dist/umd/lucide.min.js');
var LUCIDE_URL = '/node_modules/lucide/dist/umd/lucide.min.js';
var MAX_SHOT = 64 * 1024 * 1024; /* comfortably covers a desktop frame PNG */
var MAX_CAPTURE = 16 * 1024 * 1024; /* live DOM, styles, media state and annotations */
var MAX_HANDOFF = 1024 * 1024; /* a canvas full of annotations is a few kB */
var MAX_LOG = 16 * 1024; /* diagnostics are metadata, never screenshots or prompts */
var MAX_VIEW = 256 * 1024; /* complete canvas context; no silent truncation */
var MAX_LOG_SESSION = 2 * 1024 * 1024; /* one noisy workbench cannot grow forever */
var REMOTE_TIMEOUT = 5000; /* a dev server that isn't running says so quickly */
var EXPORT_CAPTURE_WORKERS = 4;

/* What the annotations are dressed in when they're laid over a page the workbench
   doesn't serve — see capture-scripts.js. The tokens annotations.css leans on come from
   the workbench's own stylesheet, scoped to the layer so the page underneath
   never sees them; the hit areas and handles are the canvas's, not the
   picture's. */
var OVERLAY_CSS = [
  '#__wb_annotations{--wb-font-sans:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif;',
  '--wb-space-2:2px;--wb-space-4:4px;--wb-space-8:8px;--wb-space-12:12px;--wb-space-16:16px;--wb-space-24:24px;',
  '--wb-radius-sm:4px;--wb-radius-md:8px;--wb-radius-pill:999px;--wb-fg:#fff;--wb-fg-3:#8c8c8c;--wb-accent:#00a1ff}',
  fs.readFileSync(path.join(__dirname, 'workbench', 'annotations.css'), 'utf8'),
  '#__wb_annotations .wb-hit,#__wb_annotations .wb-sel{display:none}',
].join('\n');

/* Ports to try in order. A stable one keeps links and bookmarks working
   between sessions; if they're all taken the OS picks. */
var PORTS = [3579, 3580, 3581, 3582, 3583, 0];

var TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.avif': 'image/avif',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.txt': 'text/plain; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8',
};

function send(res, code, body, type) {
  res.writeHead(code, {
    'Content-Type': type || 'text/plain; charset=utf-8',
    'Content-Length': Buffer.byteLength(body),
    /* The whole point is seeing edits, so nothing is cached. */
    'Cache-Control': 'no-store',
  });
  res.end(body);
}

function sendImage(res, body, format) {
  res.writeHead(200, {
    'Content-Type': format === 'jpeg' ? TYPES['.jpg'] : TYPES['.png'],
    'Content-Length': body.length,
    'Cache-Control': 'no-store',
  });
  res.end(body);
}

/* "#pages/sign-in.html" arrives as a name, not a path — anything that looks
   like one is flattened, and the extension follows the requested format. */
function safeName(name, format) {
  var base = path.basename(String(name || '')).replace(/\.[^.]*$/, '');
  base = base.replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^[-.]+/, '');
  return (base || 'canonic') + (format === 'jpeg' ? '.jpg' : '.png');
}

/* Two shots of the same page are two different notes, so the second one
   gets a number rather than the first one's place. */
function freeFile(dir, name) {
  var ext = path.extname(name);
  var base = path.basename(name, ext);
  var file = path.join(dir, base + ext);
  var n = 1;
  while (fs.existsSync(file)) {
    n += 1;
    file = path.join(dir, base + '-' + n + ext);
  }
  return file;
}

function readBody(req, limit) {
  return new Promise(function (resolve, reject) {
    var chunks = [];
    var size = 0;
    req.on('data', function (chunk) {
      size += chunk.length;
      if (size > limit) {
        reject(new Error('that shot is too big'));
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on('end', function () {
      resolve(Buffer.concat(chunks));
    });
    req.on('error', reject);
  });
}

function requestJson(address, method, body, timeout) {
  return new Promise(function (resolve, reject) {
    var target = new URL(address);
    var payload = body === undefined ? null : Buffer.from(JSON.stringify(body));
    var request = http.request({
      hostname: target.hostname,
      port: target.port,
      path: target.pathname + target.search,
      method: method,
      timeout: timeout || 15000,
      headers: payload ? { 'Content-Type': 'application/json', 'Content-Length': payload.length } : {},
    }, function (response) {
      var chunks = [];
      response.on('data', function (chunk) { chunks.push(chunk); });
      response.on('end', function () {
        var text = Buffer.concat(chunks).toString('utf8');
        try {
          var parsed = JSON.parse(text || '{}');
          if (response.statusCode < 200 || response.statusCode >= 300) {
            return reject(new Error(parsed.value && parsed.value.message || parsed.error || 'WDA answered ' + response.statusCode));
          }
          resolve(parsed);
        } catch (error) { reject(new Error('WDA returned invalid JSON: ' + error.message)); }
      });
    });
    request.on('timeout', function () { request.destroy(new Error('WDA request timed out.')); });
    request.on('error', reject);
    if (payload) request.write(payload);
    request.end();
  });
}

function capturePayload(body, baseUrl) {
  var payload = JSON.parse(body.toString('utf8'));
  var width = Math.round(Number(payload.width));
  var height = Math.round(Number(payload.height));
  if (!Number.isFinite(width) || width < 1 || width > 8192) {
    throw new Error('capture width is outside the supported range');
  }
  if (!Number.isFinite(height) || height < 1 || height > 8192) {
    throw new Error('capture height is outside the supported range');
  }

  var target = new URL(String(payload.url || ''), baseUrl);
  if (target.origin !== new URL(baseUrl).origin) {
    throw new Error('capture URL must belong to this workbench');
  }

  return {
    url: target.toString(),
    width: width,
    height: height,
    scroll: {
      x: Number(payload.scroll && payload.scroll.x) || 0,
      y: Number(payload.scroll && payload.scroll.y) || 0,
    },
    annotations: typeof payload.annotations === 'string' ? payload.annotations : '',
    name: payload.name,
    format: captureFormat(payload.format),
    mirror: mirrorPayload(payload.mirror),
    revision: typeof payload.revision === 'string' ? payload.revision.slice(0, 128) : '',
  };
}

/* The same request, for a page an implementation serves. Its address has to
   be one of the origins the config names — this server fetches nothing else
   on anybody's say-so, least of all a page's. */
function pagePayload(body, origins) {
  var payload = JSON.parse(body.toString('utf8'));
  var width = Math.round(Number(payload.width));
  var height = Math.round(Number(payload.height));
  if (!Number.isFinite(width) || width < 1 || width > 8192) {
    throw new Error('capture width is outside the supported range');
  }
  if (!Number.isFinite(height) || height < 1 || height > 8192) {
    throw new Error('capture height is outside the supported range');
  }

  var target;
  try {
    target = new URL(String(payload.url || ''));
  } catch (error) {
    throw refused(403, 'that address isn’t one of this workbench’s implementations');
  }
  if (origins.indexOf(target.origin) === -1) {
    throw refused(403, 'that address isn’t one of this workbench’s implementations');
  }

  return {
    url: target.toString(),
    width: width,
    height: height,
    scroll: {
      x: Number(payload.scroll && payload.scroll.x) || 0,
      y: Number(payload.scroll && payload.scroll.y) || 0,
    },
    annotations: typeof payload.annotations === 'string' ? payload.annotations : '',
    anchors: anchorsOf(payload.anchors),
    css: OVERLAY_CSS,
    name: payload.name,
    format: captureFormat(payload.format),
    mirror: mirrorPayload(payload.mirror),
    revision: typeof payload.revision === 'string' ? payload.revision.slice(0, 128) : '',
  };
}

function pageItems(collections, out) {
  out = out || [];
  (collections || []).forEach(function (collection) {
    (collection.items || []).forEach(function visit(entry) {
      if (entry.group) (entry.items || []).forEach(visit);
      else if (entry && entry.src) out.push(entry);
    });
  });
  return out;
}

/* Stable reference sizes for every declared state or Storybook story: each
   size the page supports but Resizable (see src/sizes/export.ts). The
   background renderer loads these directly, so exporting never drives or
   annotates the visible workbench canvas. */
function exportCapturePlan(view, baseUrl) {
  var items = pageItems((view && view.collections) || []).concat(pageItems((view && view.catalogCollections) || []));
  var captures = [];
  var warnings = [];
  var seen = new Set();
  items.forEach(function (item) {
    /* Docs pages are planned separately: see docsExportPlan. */
    if (seen.has(item.src) || item.docs) return;
    seen.add(item.src);
    var implementationKey = item.implementationOnly;
    var implementation = implementationKey && view.implementations[implementationKey];
    var variants = item.states && item.states.length ? item.states : [{ id: 'default', label: 'Default' }];
    var known = view.pages && view.pages[item.src];
    var sizes = sizeExport.exportSizes(sizeChoice.supported(view.sizes || sizeModel.defaultSizes(), {
      sizes: (known && known.sizes) || item.sizes, ownSizes: (known && known.ownSizes) || item.ownSizes }));
    var preview = view.pages[item.src] && view.pages[item.src].preview;
    if (preview) {
      variants = preview.states;
      variants.forEach(function (state) {
        sizes.forEach(function (size) {
          var target = new URL(preview.file, baseUrl);
          target.searchParams.set('state', state.id);
          captures.push({ page: item.src, state: state.id, variant: state.id + '-' + size.key, label: state.label,
            size: size.key, sizeLabel: size.label, url: target.href, external: false, width: size.width, height: size.height });
        });
      });
      return;
    }
    if (implementationKey) {
      if (!implementation || implementation.kind !== 'storybook') {
        warnings.push(item.label + ': reference screenshots are not supported for ' + (implementation && implementation.kind || 'this implementation') + '.');
        return;
      }
      variants.forEach(function (state) {
        var story = view.pages[item.src] && (view.pages[item.src].stories || []).find(function (candidate) {
          return candidate.state === state.id;
        });
        if (!story) {
          warnings.push(item.label + ' — ' + state.label + ': Storybook did not provide a story id.');
          return;
        }
        sizes.forEach(function (size) {
          captures.push({
            page: item.src, state: state.id, variant: state.id + '-' + size.key, label: state.label,
            size: size.key, sizeLabel: size.label,
            url: implementation.url + '/iframe.html?id=' + encodeURIComponent(story.id) + '&viewMode=story',
            external: true, width: size.width, height: size.height,
          });
        });
      });
      return;
    }
    var resolvedPage = view.pages && view.pages[item.src];
    if (resolvedPage && resolvedPage.design && !fs.existsSync(resolvedPage.design)) {
      warnings.push(item.label + ': the design file is missing, so no reference screenshot was captured.');
      return;
    }
    variants.forEach(function (state, index) {
      var target = new URL(item.src, baseUrl);
      target.searchParams.set('actions', 'off');
      if (index > 0) target.searchParams.set('state', state.id);
      sizes.forEach(function (size) {
        captures.push({
          page: item.src, state: state.id, variant: state.id + '-' + size.key, label: state.label,
          size: size.key, sizeLabel: size.label,
          url: target.toString(), external: false, width: size.width, height: size.height,
        });
      });
    });
  });
  return { captures: captures, warnings: warnings };
}

function captureExportReferences(capture, plan, baseUrl, progress) {
  // One load per local URL during this export; a new job sees fresh sources.
  var revision = 'export-' + crypto.randomBytes(12).toString('hex');
  var screenshots = new Array(plan.captures.length);
  var warnings = plan.warnings.slice();
  // Keep every size of a page/story on the same renderer. The first shot
  // pays the navigation/render cost; the remaining sizes reuse that document.
  var groups = [];
  var groupsByUrl = new Map();
  plan.captures.forEach(function (reference, index) {
    var key = (reference.external ? 'external:' : 'local:') + reference.url;
    var group = groupsByUrl.get(key);
    if (!group) {
      group = [];
      groupsByUrl.set(key, group);
      groups.push(group);
    }
    group.push({ index: index, reference: reference });
  });
  var pool = capture.createPool && groups.length > 1
    ? capture.createPool(Math.min(EXPORT_CAPTURE_WORKERS, groups.length))
    : { workers: [capture], close: function () { return Promise.resolve(); } };
  var nextGroup = 0;
  var completed = 0;
  if (plan.captures.length) progress(0);

  function work(worker) {
    return Promise.resolve().then(async function () {
      while (nextGroup < groups.length) {
        var group = groups[nextGroup++];
        for (var i = 0; i < group.length; i += 1) {
          var index = group[i].index;
          var reference = group[i].reference;
          var payload = {
            url: reference.url, width: reference.width, height: reference.height,
            scroll: { x: 0, y: 0 }, annotations: '', anchors: [], format: 'jpeg',
            revision: reference.external || reference.docsPage ? '' : revision,
            reuse: reference.external ? 'storybook' : '',
          };
          /* A docs page loads as a page, whole, cropped to an example when the
             reference is one. */
          if (reference.docsPage) {
            payload.fullPage = true;
            if (reference.selector) payload.selector = reference.selector;
          }
          try {
            var shot = reference.external || reference.docsPage ? await worker.captureExportPage(payload) : await worker.capture(baseUrl, payload);
            var body = shot && (shot.image || shot.png) ? (shot.image || shot.png) : shot;
            if (!body || !body.length) throw new Error('the renderer returned an empty image');
            screenshots[index] = Object.assign({}, reference, { body: body });
          } catch (error) {
            warnings.push(reference.page + ' — ' + reference.label + ' · ' + reference.sizeLabel + ': ' + String(error.message || error));
          }
          completed += 1;
          progress(completed);
        }
      }
    });
  }

  return Promise.all(pool.workers.map(work)).then(function () {
    return { screenshots: screenshots.filter(Boolean), warnings: warnings };
  }).finally(function () {
    // Cleanup must not discard hundreds of successful captures. A worker
    // that resists shutdown is diagnostic noise for the next activation,
    // not a reason to fail this export.
    return Promise.resolve().then(function () { return pool.close(); }).catch(function (error) {
      warnings.push('Capture worker cleanup: ' + String(error.message || error));
    });
  });
}

/* The external address is validated above, but a bridged document is rendered
   on the workbench's inert capture page. Give the helper that local identity;
   the mirrored <base> still points styles and media at the preview origin. */
function bridgedPayload(payload, baseUrl) {
  return Object.assign({}, payload, {
    url: new URL(WORKBENCH_PREFIX + 'bridged-preview', baseUrl).toString(),
  });
}

/* The points under the annotations, one per annotation, null where an
   annotation has no element to name (a freeform note). */
function anchorsOf(raw) {
  if (!Array.isArray(raw)) return [];
  return raw.map(function (point) {
    if (!point || !Number.isFinite(Number(point.x)) || !Number.isFinite(Number(point.y))) return null;
    return { x: Number(point.x), y: Number(point.y) };
  });
}

/* What runs in every page the browsers open: the element-describing helper
   the annotation layer uses, plus one entry point DevTools can call by name. */
var DESCRIBE_SOURCE =
  fs.readFileSync(path.join(__dirname, 'workbench', 'describe.js'), 'utf8') +
  '\nwindow.__wbDescribeAt = function (x, y) { return window.wbDescribe.at(document, x, y); };\n';

/* An error that knows which status it deserves. */
function refused(status, message) {
  var error = new Error(message);
  error.status = status;
  return error;
}

function originOf(impl) {
  try {
    return new URL(manifest.address(impl)).origin;
  } catch (error) {
    return null;
  }
}

/* Where an implementation says a story lives is a path relative to the
   folder Storybook runs in: keep the ones that are actually there. */
function storyCode(impl, entry) {
  if (!impl.root) return [];
  var out = [];
  [entry.componentPath, entry.importPath].forEach(function (rel) {
    if (typeof rel !== 'string' || !rel) return;
    var file = path.resolve(impl.root, rel);
    if (out.indexOf(file) === -1 && fs.existsSync(file)) out.push(file);
  });
  return out;
}

function storyEntries(index, impl) {
  var entries = index && index.entries;
  if (!entries || typeof entries !== 'object') {
    throw refused(502, 'Storybook at ' + impl.url + ' answered with something that isn’t a Storybook index.');
  }
  return Object.keys(entries).map(function (id) {
    return entries[id];
  }).filter(function (entry) {
    return entry && entry.type === 'story' && typeof entry.id === 'string' && typeof entry.title === 'string';
  });
}

function storybookIndex(impl) {
  return remote.fetchJson(impl.url + '/index.json', { timeout: REMOTE_TIMEOUT }).then(
    function (answer) {
      if (answer.status < 200 || answer.status >= 300) {
        throw refused(502, 'Storybook at ' + impl.url + ' answered ' + answer.status + ' for /index.json.');
      }
      storyEntries(answer.body, impl);
      return answer.body;
    },
    function () {
      throw refused(503, 'Storybook at ' + impl.url + ' isn’t answering — is it running?');
    }
  );
}

/* Turn Storybook's own hierarchy into the two levels the workbench has: the
   first title segment is a collection, any middle segments are one joined group,
   and the leaf is the page. Each story under that title is one state. The
   src is deliberately synthetic and stable; imported pages have no design
   file and always open through their Storybook implementation. */
function catalogIcon(impl, title) {
  return manifest.titleIcon({ icon: impl.catalogIcon, icons: impl.catalogIcons }, title, 'book-open').icon;
}

function storybookCatalog(index, key, impl) {
  var byTitle = {};
  storyEntries(index, impl).forEach(function (entry) {
    if (!byTitle[entry.title]) byTitle[entry.title] = [];
    byTitle[entry.title].push(entry);
  });

  var collections = [];
  var byCollection = {};
  var pages = {};

  Object.keys(byTitle).forEach(function (title) {
    var entries = byTitle[title];
    var parts = title.split('/').filter(Boolean);
    var collectionName = parts.length > 1 ? parts.shift() : impl.label;
    var label = parts.length ? parts.pop() : title;
    var groupName = parts.join(' / ');
    var componentId = entries[0].id.split('--')[0];
    if (!/^[a-z0-9-]+$/.test(componentId)) {
      componentId = title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'stories';
    }
    var src = '__storybook/' + key + '/' + componentId + '.html';
    var item = {
      label: label,
      src: src,
      icon: catalogIcon(impl, title),
      states: entries.map(function (entry) {
        return { id: manifest.storyState(entry.id), label: entry.name || manifest.storyState(entry.id) };
      }),
      implementations: {},
      implementationOnly: key,
    };
    item.implementations[key] = { title: title };

    var collection = byCollection[collectionName];
    if (!collection) {
      var selected = manifest.titleIcon({ icon: impl.catalogIcon, icons: impl.catalogIcons }, collectionName, 'book-open');
      collection = { name: collectionName, icon: selected.icon, iconPriority: selected.mapped ? 5 : impl.catalogIconExplicit ? 3 : 0, items: [] };
      byCollection[collectionName] = collection;
      collections.push(collection);
    }
    if (groupName) {
      var group = collection.items.find(function (candidate) { return candidate.group === groupName; });
      if (!group) {
        group = { group: groupName, items: [] };
        collection.items.push(group);
      }
      group.items.push(item);
    } else {
      collection.items.push(item);
    }

    var code = [];
    entries.forEach(function (entry) {
      [entry.componentPath, entry.importPath].forEach(function (relative) {
        if (!impl.root || typeof relative !== 'string' || !relative) return;
        var absolute = path.resolve(impl.root, relative);
        if (code.some(function (candidate) { return candidate.path === absolute; })) return;
        code.push({ implementation: key, path: absolute, relative: relative, exists: fs.existsSync(absolute) });
      });
    });
    pages[src] = {
      label: label, design: null, code: code,
      stories: entries.map(function (entry) { return { id: entry.id, state: manifest.storyState(entry.id), label: entry.name || manifest.storyState(entry.id) }; }),
    };
  });

  return { collections: collections, pages: pages };
}

function storybookPorts(root) {
  var ports = [6006, 6007, 6008, 6009, 6010];
  function visit(dir, depth) {
    if (depth > 4) return;
    var entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch (error) { return; }
    entries.forEach(function (entry) {
      if (entry.name === 'node_modules' || entry.name === '.git' || entry.name === '.canonic') return;
      var file = path.join(dir, entry.name);
      if (entry.isDirectory()) return visit(file, depth + 1);
      if (entry.name !== 'package.json') return;
      try {
        var pkg = JSON.parse(fs.readFileSync(file, 'utf8'));
        Object.keys(pkg.scripts || {}).forEach(function (name) {
          var script = String(pkg.scripts[name] || '');
          if (!/storybook/i.test(name + ' ' + script)) return;
          var match;
          var pattern = /(?:--port(?:=|\s+)|-p\s+)(\d{2,5})/g;
          while ((match = pattern.exec(script))) {
            var port = Number(match[1]);
            if (ports.indexOf(port) === -1) ports.unshift(port);
          }
        });
      } catch (error) { /* A malformed package is not Storybook detection's error. */ }
    });
  }
  visit(root, 0);
  return ports;
}

function detectStorybook(root) {
  var candidates = storybookPorts(root).reduce(function (all, port) {
    all.push('http://127.0.0.1:' + port);
    all.push('http://localhost:' + port);
    return all;
  }, []);
  return candidates.reduce(function (pending, address) {
    return pending.catch(function () {
      var impl = { url: address };
      return storybookIndex(impl).then(function () { return address; });
    });
  }, Promise.reject()).catch(function () {
    throw refused(503, 'couldn’t automatically find a running Storybook; start it or set its URL explicitly');
  });
}

function bootedSimulators() {
  return new Promise(function (resolve, reject) {
    childProcess.execFile('xcrun', ['simctl', 'list', 'devices', '--json'], { encoding: 'utf8', timeout: 10000 }, function (error, stdout) {
      if (error) return reject(new Error('couldn’t list iOS Simulators: ' + String(error.message || error)));
      try {
        var body = JSON.parse(stdout);
        var out = [];
        Object.keys(body.devices || {}).forEach(function (runtime) {
          (body.devices[runtime] || []).forEach(function (device) {
            if (device.state !== 'Booted' || device.isAvailable === false) return;
            out.push({ name: device.name, udid: device.udid, runtime: runtime.split('.').pop().replace(/^iOS-/, 'iOS ').replaceAll('-', '.') });
          });
        });
        resolve(out);
      } catch (parseError) {
        reject(new Error('simctl returned invalid device data: ' + parseError.message));
      }
    });
  });
}

function simulatorCatalog(devices, key, impl) {
  var wanted = impl.device || 'booted';
  var selected = devices.filter(function (device) {
    return wanted === 'booted' || device.udid === wanted || device.name === wanted;
  });
  var collection = { name: impl.label, icon: impl.catalogIcon || 'smartphone', items: [] };
  var pages = {};
  selected.forEach(function (device) {
    var src = '__ios-simulator/' + key + '/' + device.udid + '.html';
    var item = {
      label: device.name,
      src: src,
      icon: impl.catalogIcon || 'smartphone',
      implementations: {},
      implementationOnly: key,
    };
    item.implementations[key] = { device: device.udid };
    collection.items.push(item);
    pages[src] = { label: device.name, design: null, code: [], simulator: device };
  });
  return { collections: selected.length ? [collection] : [], pages: pages };
}

/* Which of a Storybook's stories carry the configured title. Exact: a title
   is what Storybook shows, and "Button" is not "Components/Button" — but the
   answer says so when the difference is only the prefix. */
function storiesTitled(index, title, impl) {
  var all = storyEntries(index, impl);
  var found = all.filter(function (entry) {
    return entry.title === title;
  });
  if (!found.length) {
    var wanted = title.split('/').pop();
    var near = [];
    all.forEach(function (entry) {
      if (String(entry.title).split('/').pop() === wanted && near.indexOf(entry.title) === -1) near.push(entry.title);
    });
    throw refused(
      404,
      'No stories titled “' + title + '”' +
        (near.length ? ' — did you mean “' + near.join('”, “') + '”?' : ' at ' + impl.url + '.')
    );
  }
  return found.map(function (entry) {
    return {
      id: entry.id,
      name: entry.name,
      state: manifest.storyState(entry.id),
      importPath: entry.importPath || null,
      componentPath: entry.componentPath || null,
      code: storyCode(impl, entry),
    };
  });
}

function mirrorPayload(value) {
  if (value == null) return undefined;
  if (typeof value !== 'object' || typeof value.revision !== 'string' ||
      !value.revision || value.revision.length > 160 || !Array.isArray(value.states) ||
      !Array.isArray(value.defined) || !Array.isArray(value.animations)) throw new Error('Invalid live DOM snapshot');
  if (value.version === 1) {
    var pointer = value.pointer;
    if (pointer != null && (!Number.isFinite(pointer.x) || !Number.isFinite(pointer.y) ||
        pointer.x < 0 || pointer.y < 0 || pointer.x > 8192 || pointer.y > 8192)) throw new Error('Invalid live DOM pointer');
    function key(v) { return typeof v === 'string' && v.length > 0 && v.length <= 160; }
    if (!key(value.root) || (value.base !== null && !key(value.base)) || !Array.isArray(value.nodes) ||
        !Array.isArray(value.removed) || !value.removed.every(key) || !Array.isArray(value.cleared) || !value.cleared.every(key) ||
        !value.states.every(function (state) { return state && key(state.id); }) ||
        !value.nodes.every(function (node) {
          if (!node || !key(node.id)) return false;
          if (node.type === 3 || node.type === 8) return typeof node.text === 'string';
          return node.type === 1 && typeof node.tag === 'string' && typeof node.ns === 'string' &&
            !/^(script|iframe|object|embed)$/i.test(node.tag) && Array.isArray(node.attrs) &&
            node.attrs.every(function (attr) { return Array.isArray(attr) && typeof attr[0] === 'string' && typeof attr[1] === 'string' &&
              (attr[2] === null || typeof attr[2] === 'string') && !/^on|^http-equiv$/i.test(attr[0]); }) &&
            Array.isArray(node.children) && node.children.every(key) && (node.shadow === undefined || (Array.isArray(node.shadow) && node.shadow.every(key)));
        })) throw new Error('Invalid live DOM patch');
    var result = { version: 1, revision: value.revision, base: value.base, root: value.root, nodes: value.nodes,
      removed: value.removed, states: value.states, cleared: value.cleared, defined: value.defined, animations: value.animations,
    };
    if ('pointer' in value) result.pointer = pointer ? { x: pointer.x, y: pointer.y } : null;
    return result;
  }
  if (typeof value.html !== 'string') throw new Error('Invalid live DOM snapshot');
  return { html: value.html, revision: value.revision, states: value.states, defined: value.defined, animations: value.animations };
}

function captureFormat(format) {
  if (format == null || format === 'png') return 'png';
  if (format === 'jpeg') return 'jpeg';
  throw new Error('capture format must be png or jpeg');
}

function saveShot(root, name, body, format) {
  var dir = path.join(root, HANDOFFS_DIR);
  fs.mkdirSync(dir, { recursive: true });
  var file = freeFile(dir, safeName(name, format));
  fs.writeFileSync(file, body);
  return path.relative(root, file);
}

/* Project pages and compiled previews share one compatibility injection,
   placed before the page's own scripts. */
var withPreviewScripts = previewScripts.withPreviewScripts;

/* Serves one folder. `options.inject` marks the folder as the project's, whose
   pages get the compatibility bundle; the workbench's own folder never does. */
function serveFile(root, pathname, res, options) {
  var rel = decodeURIComponent(pathname);
  if (rel.slice(-1) === '/') rel += 'index.html';

  var file = path.join(root, rel);
  /* path.join resolves the ".."s away, so this catches anything reaching
     outside the folder however it was spelled. */
  if (file !== root && file.indexOf(root + path.sep) !== 0) {
    send(res, 403, 'Outside the folder.');
    return;
  }

  fs.readFile(file, function (err, body) {
    if (err) {
      send(res, err.code === 'ENOENT' ? 404 : 500, 'Not found: ' + rel);
      return;
    }
    var ext = path.extname(file).toLowerCase();
    if (options && options.inject && ext === '.html') {
      body = Buffer.from(withPreviewScripts(body.toString('utf8')), 'utf8');
    }
    if (options && options.head && ext === '.html') {
      body = Buffer.from(body.toString('utf8').replace('<head>', '<head>\n' + options.head), 'utf8');
    }
    send(res, 200, body, TYPES[ext] || 'application/octet-stream');
  });
}

/* Starts the server. `onShot` is called with the workspace-relative path of
   every screenshot written, so the editor can say so. `onHandoff` is given the
   composed prompt and the canvas it came from so the editor can copy it.
   `onOpen` is handed an absolute path the config resolves to — a page's
   design file, or where its implementation's code is — and puts it in front
   of the user. */
/* The screenshot service a server makes when it isn't given one. Servers for
   several spaces can share one, started with `captureShared` so closing one
   space leaves the service to whoever made it. */
function createCapture(storage, diagnostic) {
  return electronCapture.createService({ inject: DESCRIBE_SOURCE, storage: storage, diagnostic: diagnostic });
}

function start(options) {
  var root = path.resolve(options.root);
  var capture = options.capture || createCapture(options.captureStorage, function (level, event, details) {
    diagnostic(level, event, details);
  });
  var captureReady = Promise.resolve();
  var previews = null;
  var previewSettings = null;
  /* Docs pages from defineDocs definitions, as the last config resolution found them. */
  var discoveredDocs = [];
  /* Lenses frame implementations through these, so every page carries the
     preview bridge and its live DOM reaches capture. */
  var proxies = implementationProxy.create({
    bridge: function () {
      return 'http://127.0.0.1:' + server.address().port + WORKBENCH_PREFIX + 'preview-bridge.js';
    },
  });
  var windowVideo = options.windowStream || nativePool.createPool(function () { return windowStream.create({
    storage: path.join(options.captureStorage || path.join(os.tmpdir(), 'canonic-workbench-capture'), 'window-capture'),
    path: WINDOW_STREAM_PATH,
    permissionOwner: options.screenCapturePermissionOwner,
  }); });
  /* The other workbenches this one can switch to, from whoever started it:
     { list(), open(id) } — see spaces.js. Without one, this space is the
     only one there is. */
  var spaceList = options.spaces || null;
  /* Which config is this space's: the workbench.yaml in `config.dir`, and
     space `config.key` of the ones it lists. By default the file at the
     root this serves, and its first space. */
  var where = {
    dir: path.resolve((options.config && options.config.dir) || root),
    key: (options.config && options.config.key) || null,
  };
  var spaceSelf = spaces.spaceId(where.dir, where.key);
  /* A space whose config isn't at its root, or isn't the whole file, tells
     the canvas so; the canvas then reads the file from MANIFEST_PATH. */
  var canvasHead = where.key || where.dir !== root
    ? '<meta name="canonic-config" content="' + MANIFEST_PATH + '" />\n' +
      (where.key ? '<meta name="canonic-space" content="' + where.key + '" />\n' : '')
    : '';
  var onShot = options.onShot || function () {};
  var onLog = options.onLog || function () {};
  var logLimit = options.logLimit || MAX_LOG_SESSION;
  var loggedBytes = 0;
  var logLimitReported = false;
  var hasHandoff = typeof options.onHandoff === 'function';
  var shown = agentView.create({ root: root, now: options.now, staleMs: options.viewStaleMs });
  var onHandoff =
    options.onHandoff ||
    function () {
      throw new Error('nothing is set up to receive a handoff');
    };
  var onOpen = options.onOpen || function () {
    throw new Error('nothing is set up to open files');
  };
  var simulatorSessions = {};
  var autoStorybookAddresses = {};
  var simulatorQueue = Promise.resolve();
  var listSimulators = options.simulators || bootedSimulators;
  var iosCli = path.join(root, '.canonic', 'src', 'automation', 'ios-cli.mjs');
  if (!fs.existsSync(iosCli)) iosCli = path.join(__dirname, '..', 'src', 'automation', 'ios-cli.mjs');
  var simulatorStateDir = path.join('/tmp', 'canonic-workbench-ios-' + crypto.createHash('sha1').update(root).digest('hex').slice(0, 10));

  function runIosCli(args, timeout) {
    return new Promise(function (resolve, reject) {
      childProcess.execFile(process.execPath, [iosCli].concat(args), {
        cwd: root,
        encoding: 'utf8',
        maxBuffer: 16 * 1024 * 1024,
        timeout: timeout || 150000,
      }, function (error, stdout, stderr) {
        if (error) return reject(new Error(String(stderr || error.message || error).trim()));
        try { resolve(JSON.parse(stdout)); }
        catch (parseError) { reject(new Error('iOS automation returned invalid JSON: ' + parseError.message)); }
      });
    });
  }

  function startSimulatorSession(udid) {
    if (simulatorSessions[udid]) return simulatorSessions[udid];
    var sessionName = 'workbench-' + udid.toLowerCase().replace(/[^a-z0-9]+/g, '-');
    simulatorSessions[udid] = runIosCli([
      'start', '--session', sessionName, '--state-dir', simulatorStateDir,
      '--platform', 'simulator', '--udid', udid,
    ], 150000).then(function (answer) {
      var state = answer.state || {};
      if (!state.sessionId || !state.wdaBaseUrl) throw new Error('WDA started without a usable session.');
      return requestJson(state.wdaBaseUrl + '/session/' + state.sessionId + '/window/size', 'GET', undefined, 15000)
        .then(function (sizeAnswer) {
          var size = sizeAnswer.value || sizeAnswer;
          return {
            udid: udid,
            sessionName: sessionName,
            sessionId: state.sessionId,
            baseUrl: state.wdaBaseUrl,
            screen: { width: Number(size.width), height: Number(size.height) },
          };
        });
    }).catch(function (error) {
      delete simulatorSessions[udid];
      throw error;
    });
    return simulatorSessions[udid];
  }

  function simulatorInput(ask) {
    var udid = String(ask.udid || '');
    if (!udid) return Promise.reject(refused(400, 'which Simulator?'));
    return startSimulatorSession(udid).then(function (session) {
      function ratio(value) { return Number.isFinite(value) && value >= 0 && value <= 1; }
      var action;
      if (ask.action === 'tap' && ratio(ask.x) && ratio(ask.y)) {
        var tapX = Math.round(session.screen.width * ask.x);
        var tapY = Math.round(session.screen.height * ask.y);
        action = function () {
          var base = session.baseUrl + '/session/' + session.sessionId;
          return requestJson(base + '/actions', 'POST', { actions: [{
            type: 'pointer', id: 'finger1', parameters: { pointerType: 'touch' }, actions: [
              { type: 'pointerMove', duration: 0, x: tapX, y: tapY },
              { type: 'pointerDown', button: 0 },
              { type: 'pause', duration: 80 },
              { type: 'pointerUp', button: 0 },
            ],
          }] }, 15000)
            .catch(function () { return requestJson(base + '/wda/tap/0', 'POST', { x: tapX, y: tapY }, 15000); })
            .catch(function () { return requestJson(base + '/wda/tap', 'POST', { x: tapX, y: tapY }, 15000); });
        };
      } else if (ask.action === 'swipe' && [ask.fromX, ask.fromY, ask.toX, ask.toY].every(ratio)) {
        var fromX = Math.round(session.screen.width * ask.fromX);
        var fromY = Math.round(session.screen.height * ask.fromY);
        var toX = Math.round(session.screen.width * ask.toX);
        var toY = Math.round(session.screen.height * ask.toY);
        var duration = Math.max(0.05, Math.min(Number(ask.duration) || 0.2, 2));
        action = function () {
          var base = session.baseUrl + '/session/' + session.sessionId;
          return requestJson(base + '/actions', 'POST', { actions: [{
            type: 'pointer', id: 'finger1', parameters: { pointerType: 'touch' }, actions: [
              { type: 'pointerMove', duration: 0, x: fromX, y: fromY },
              { type: 'pointerDown', button: 0 },
              { type: 'pointerMove', duration: Math.round(duration * 1000), x: toX, y: toY },
              { type: 'pointerUp', button: 0 },
            ],
          }] }, 25000).catch(function () {
            return requestJson(base + '/wda/dragfromtoforduration', 'POST', {
              fromX: fromX, fromY: fromY, toX: toX, toY: toY, duration: duration,
            }, 25000);
          });
        };
      } else {
        return Promise.reject(refused(400, 'invalid Simulator input action or coordinates'));
      }
      var queued = simulatorQueue.then(action, action);
      simulatorQueue = queued.catch(function () {});
      return queued.then(function () { return { action: ask.action, screen: session.screen }; });
    });
  }

  /* The config as this machine reads it, fresh every time it's asked for:
     the two files are small, nothing is cached anywhere else in here, and
     an answer that lags an edit is exactly the kind of thing that costs an
     afternoon. Null when the project has no workbench.yaml; throws when the
     file is there and wrong. */
  function resolved() {
    return config.resolve(root, config.read(where));
  }

  /* Catalog imports are the one asynchronous part of configuration: the
     manifest names the Storybook, then its live index supplies the pages.
     A stopped catalog leaves manual pages usable and reports one problem. */
  function resolvedWithCatalogs() {
    var view = resolved();
    if (!view) return Promise.resolve(null);
    var collections = [];
    var pages = Object.assign({}, view.pages);
    var problems = view.problems.slice();
    var implementations = Object.assign({}, view.implementations);
    var autoKeys = Object.keys(implementations).filter(function (key) {
      return implementations[key].kind === 'storybook' && implementations[key].auto;
    });
    return importPreviews(view, collections, pages, problems, implementations).then(function () {
    return autoKeys.reduce(function (pending, key) {
      return pending.then(function () {
        if (autoStorybookAddresses[key]) {
          implementations[key] = Object.assign({}, implementations[key], {
            url: autoStorybookAddresses[key], auto: true,
          });
          return;
        }
        return detectStorybook(root).then(function (address) {
          autoStorybookAddresses[key] = address;
          implementations[key] = Object.assign({}, implementations[key], { url: address, auto: true });
        }).catch(function (error) {
          problems.push('implementations › ' + key + ': ' + String(error.message || error));
        });
      });
    }, Promise.resolve()).then(function () {
      var keys = Object.keys(implementations).filter(function (key) {
        var impl = implementations[key];
        return impl.kind === 'storybook' && impl.catalog && impl.url !== 'auto';
      });
      return keys.reduce(function (pending, key) {
      return pending.then(function () {
        var impl = implementations[key];
        return storybookIndex(impl).then(function (index) {
          var imported = storybookCatalog(index, key, impl);
          imported.collections.forEach(function (collection) {
            var existing = collections.find(function (candidate) { return candidate.name === collection.name; });
            if (existing) { manifest.mergeCollectionIcon(existing, collection); existing.items = existing.items.concat(collection.items); }
            else collections.push(collection);
          });
          Object.assign(pages, imported.pages);
        }).catch(function (error) {
          problems.push('implementations › ' + key + ': couldn’t import its catalog — ' + String(error.message || error));
        });
      });
      }, Promise.resolve());
    }).then(function () {
      var simulatorKeys = Object.keys(implementations).filter(function (key) {
        var impl = implementations[key];
        return impl.kind === 'ios-simulator' && impl.catalog;
      });
      if (!simulatorKeys.length) return;
      return Promise.resolve(listSimulators()).then(function (devices) {
        simulatorKeys.forEach(function (key) {
          var imported = simulatorCatalog(devices, key, implementations[key]);
          collections = collections.concat(imported.collections);
          Object.assign(pages, imported.pages);
          if (!imported.collections.length) {
            problems.push('implementations › ' + key + ': no matching booted iOS Simulator was found.');
          }
        });
      }).catch(function (error) {
        problems.push('iOS Simulator: ' + String(error.message || error));
      });
    }).then(function () {
      return importDocs(view, problems, pages);
    }).then(function () {
      return Object.assign({}, view, {
        implementations: implementations,
        catalogCollections: collections,
        pages: pages,
        problems: problems,
      });
    });
    });
  }

  /* Docs pages: pages written in Markdown, with examples their lenses
     compile in the preview worker. See src/docs/ and specs/docs-pages.md. */
  function docsPages(view) {
    return docsList.docsPages(view, discoveredDocs);
  }

  /* Why examples can't run here, or null. They run project code, like previews. */
  function docsBlocked() {
    var view = resolved();
    if (options.isTrusted === false) return 'Examples run project code, so they appear in a trusted workspace only.';
    if (view && view.previews === false) return 'Examples run project code, which previews: false turns off.';
    return null;
  }

  var docs = docsService.createDocsService({
    blocked: docsBlocked,
    readFile: function (file) {
      var target = path.resolve(root, file);
      if (target.indexOf(root + path.sep) !== 0) return Promise.resolve(null);
      return fs.promises.readFile(target, 'utf8').catch(function (error) {
        if (error.code === 'ENOENT' || error.code === 'EISDIR') return null;
        throw error;
      });
    },
    listExamples: function (lens) {
      if (!previews) return Promise.reject(new Error('The preview worker isn’t running.'));
      return previews.docs('index', docsService.lensRequest(lens));
    },
    bundle: function (lens) {
      if (!previews) return Promise.reject(new Error('The preview worker isn’t running.'));
      return previews.docs('bundle', docsService.lensRequest(lens));
    },
  });

  /* The config, with the preview worker started when a docs lens needs it:
     a docs page can be the first thing anyone asks this server for. */
  var docsResolution = null;
  function docsReady() {
    if (previews || docsResolution) return (docsResolution || Promise.resolve()).then(function () { return resolved(); });
    docsResolution = resolvedWithCatalogs().catch(function (error) { docsResolution = null; throw error; });
    return docsResolution.then(function () { return resolved(); });
  }

  var serveBrowserModule = browserModules.createBrowserModules(path.join(__dirname, 'src'), function (code, file) {
    return require('./preview/engine.cjs').transform(code, {
      loader: 'ts', format: 'esm', target: 'es2022', sourcefile: file, sourcemap: 'inline',
    }).then(function (result) { return result.code; });
  });

  /* A docs page's references, per lens: the whole page at its 960-pixel
     layout, and each example that lens renders, cropped to its panel. */
  var DOCS_EXPORT_WIDTH = 960 + 2 * 48;
  function docsExportPlan(view, baseUrl) {
    var pages = docsPages(view);
    var captures = [];
    var warnings = [];
    return pages.reduce(function (pending, page) {
      return pending.then(function () { return docs.outline(page); }).then(function (examples) {
        if (!examples) { warnings.push(page.label + ': the Markdown file doesn’t exist.'); return; }
        var lenses = page.lenses.length ? page.lenses : [null];
        return lenses.reduce(function (next, lens) {
          return next.then(function () {
            if (!lens || docsBlocked() || !previews) return [];
            return previews.docs('index', docsService.lensRequest(lens)).then(function (listed) {
              return listed.examples.map(function (example) { return example.id; });
            }, function (error) {
              warnings.push(page.label + ' — ' + lens.label + ': ' + String(error.message || error));
              return [];
            });
          }).then(function (available) {
            var url = new URL(page.src, baseUrl);
            if (lens) url.searchParams.set('lens', lens.key);
            var base = { page: page.src, state: 'default', size: 'docs', sizeLabel: 'Docs page',
              url: url.href, external: false, docsPage: true, width: DOCS_EXPORT_WIDTH, height: sizeModel.EXPORT_FILL.height };
            var prefix = lens ? lens.key + '-' : '';
            captures.push(Object.assign({}, base, { variant: prefix + 'page', label: (lens ? lens.label + ' · ' : '') + 'Whole page' }));
            examples.forEach(function (example) {
              if (available.indexOf(example.id) === -1) return;
              captures.push(Object.assign({}, base, { variant: prefix + example.id, label: (lens ? lens.label + ' · ' : '') + example.label,
                selector: '#example-' + example.id + ' [data-wb-example-stage]' }));
            });
          });
        }, Promise.resolve());
      });
    }, Promise.resolve()).then(function () { return { captures: captures, warnings: warnings }; });
  }

  /* A handoff from a docs page names the Markdown and, for each example in
     view, where its code is. Examples that can't be listed keep their IDs. */
  function docsHandoff(canvas) {
    if (!canvas.docs) return Promise.resolve();
    var page = docsPages(resolved()).find(function (candidate) { return candidate.src === canvas.src; });
    if (!page) return Promise.resolve();
    var lens = docsService.chooseLens(page, canvas.docs.lens);
    canvas.docs.markdown = page.src;
    canvas.docs.lensLabel = lens ? lens.label : null;
    if (!lens || docsBlocked() || !previews) return Promise.resolve();
    return previews.docs('index', docsService.lensRequest(lens)).then(function (listed) {
      (canvas.docs.examples || []).forEach(function (example) {
        var found = listed.examples.find(function (candidate) { return candidate.id === example.id; });
        if (found) example.file = found.file + (found.export !== 'default' ? ' (' + found.export + ')' : '');
      });
    }).catch(function () { /* The prompt still names the examples. */ });
  }

  /* Docs problems join the config's. */
  function importDocs(view, problems, resolvedPages) {
    var pages = docsPages(view);
    if (!pages.length) return Promise.resolve();
    /* The source menu: the Markdown is the page's design file; each lens adds
       its example source. */
    pages.forEach(function (page) {
      var entry = resolvedPages[page.src] = Object.assign({}, resolvedPages[page.src]);
      entry.design = path.join(root, page.src);
      entry.code = (entry.code || []).concat(page.lenses.map(function (lens) {
        var target = path.join(root, lens.examples);
        return { implementation: lens.key, path: target, relative: lens.examples, exists: fs.existsSync(target) };
      }));
    });
    return docs.problems(pages, knownListing).then(function (found) {
      problems.push.apply(problems, found);
    });
  }

  /* Each docs lens's examples as last listed. The config shows what is known
     and never waits for a listing: one is listed again in the background each
     time it's asked for (the worker answers from its cache until a file
     changes), and a listing that turns out different tells the host the
     catalog changed, so the sidebar shows its problems. */
  var docsListings = new Map();
  function knownListing(lens) {
    if (!previews) return null;
    var key = JSON.stringify(docsService.lensRequest(lens));
    var entry = docsListings.get(key);
    if (!entry) { entry = { result: null, pending: null }; docsListings.set(key, entry); }
    refreshListing(lens, entry);
    if (!entry.result) return null;
    return entry.result.error ? Promise.reject(new Error(entry.result.error)) : Promise.resolve(entry.result.value);
  }
  function refreshListing(lens, entry) {
    if (entry.pending || !previews) return entry.pending;
    var before = JSON.stringify(entry.result);
    entry.pending = previews.docs('index', docsService.lensRequest(lens)).then(function (value) {
      entry.result = { value: { examples: value.examples, problems: value.problems } };
    }, function (error) {
      entry.result = { error: String(error.message || error) };
    }).then(function () {
      entry.pending = null;
      if (JSON.stringify(entry.result) !== before) catalogChanged();
    });
    return entry.pending;
  }
  var catalogNotice = null;
  function catalogChanged() {
    if (catalogNotice || typeof options.onCatalogChanged !== 'function') return;
    catalogNotice = setTimeout(function () {
      catalogNotice = null;
      try { options.onCatalogChanged(); } catch (error) {
        diagnostic('warn', 'catalog.notify.failed', { message: String(error.message || error) });
      }
    }, 50);
  }

  /* Started with the session when the host asks: the worker, its index, and
     then every docs page's bundle, one at a time, so the first visit to a
     page is already built. A page asked for meanwhile waits for one build at
     most. */
  function warmDocs() {
    var began = Date.now();
    var built = 0;
    return docsReady().then(function (view) {
      if (!view || !previews || docsBlocked()) return;
      return docsPages(view).reduce(function (pending, page) {
        return page.lenses.reduce(function (next, lens) {
          return next.then(function () {
            if (!previews) return;
            var key = JSON.stringify(docsService.lensRequest(lens));
            var entry = docsListings.get(key);
            if (!entry) { entry = { result: null, pending: null }; docsListings.set(key, entry); }
            return Promise.all([refreshListing(lens, entry), previews.docs('bundle', docsService.lensRequest(lens))])
              .then(function () { built++; }, function () { /* The page says why when it's opened. */ });
          });
        }, pending);
      }, Promise.resolve());
    }).then(function () {
      diagnostic('info', 'docs.warmed', { bundles: built, elapsedMs: Date.now() - began });
    }, function (error) {
      diagnostic('warn', 'docs.warm.failed', { elapsedMs: Date.now() - began, message: String(error.message || error) });
    });
  }

  async function importPreviews(view, collections, pages, problems, implementations) {
    discoveredDocs = [];
    if (view.previews === false) {
      if (previews) { await previews.close(); previews = null; previewSettings = null; }
      return;
    }
    var discovered = previewCompiler.discover(root, view.previews && view.previews.include).length > 0;
    if (options.isTrusted === false) {
      if (discovered) problems.push('Workbench previews require a trusted workspace.');
      return;
    }
    /* Docs page examples compile in the same worker, so a space with docs
       lenses and no discovered previews starts it too. */
    if (!discovered && !docsPages(view).some(function (page) { return page.lenses.length; })) return;
    try {
      var settings = JSON.stringify(view.previews || {});
      if (previews && settings !== previewSettings) { await previews.close(); previews = null; }
      if (!previews) {
        previews = previewService.create(root, { previews: view.previews, storage: options.captureStorage,
          diagnostic: diagnostic,
          log: function (message) { diagnostic('warn', 'preview.worker', { message: message }); } });
        previewSettings = settings;
      }
      if (!discovered) return;
      var indexing = Date.now();
      var index = await previews.index();
      diagnostic(Date.now() - indexing >= 1000 ? 'warn' : 'info', 'previews.indexed', {
        elapsedMs: Date.now() - indexing, previews: (index.previews || []).length,
        docs: (index.docs || []).length, errors: (index.errors || []).length,
      });
      problems.push.apply(problems, index.errors);
      /* Discovered docs pages go where their titles say, like previews. A
         docs page workbench.yaml declares for the same Markdown file wins. */
      var authored = new Set(pageItems(view.collections).map(function (item) { return item.src; }));
      (index.docs || []).forEach(function (page) {
        if (authored.has(page.src)) return;
        var parts = page.title.split('/').filter(Boolean);
        var collectionName = parts.length > 1 ? parts.shift() : 'Docs';
        var label = parts.pop() || page.id;
        discoveredDocs.push({ src: page.src, label: label, lens: page.lens, lenses: page.lenses });
        var item = { src: page.src, label: label, docs: true, icon: page.icon || 'book-open',
          docsLenses: page.lenses.map(function (lens) { return { key: lens.key, label: lens.label }; }) };
        if (page.lens) item.lens = page.lens;
        if (page.states.length > 1) item.states = page.states;
        if (page.lenses.length) {
          item.implementations = {};
          page.lenses.forEach(function (lens) { item.implementations[lens.key] = { examples: lens.examples }; });
        }
        var collection = collections.find(function (candidate) { return candidate.name === collectionName; });
        if (!collection) { collection = { name: collectionName, icon: 'book-open', items: [] }; collections.push(collection); }
        var groupName = parts.join(' / ');
        if (groupName) {
          var group = collection.items.find(function (candidate) { return candidate.group === groupName; });
          if (!group) { group = { group: groupName, items: [] }; collection.items.push(group); }
          group.items.push(item);
        } else collection.items.push(item);
        pages[page.src] = Object.assign({}, pages[page.src], { label: label, design: path.join(root, page.src), docs: page });
      });
      var byId = {};
      index.previews.forEach(function (preview) {
        byId[preview.id] = preview;
        var parts = preview.title.split('/').filter(Boolean);
        var collectionName = parts.length > 1 ? parts.shift() : 'Previews';
        var label = parts.pop() || preview.id;
        var iconSettings = view.previews || {};
        var item = { src: preview.file, label: label, states: preview.states, workbench: true, icon: preview.icon || manifest.titleIcon(iconSettings, preview.title, 'component').icon };
        if (iconSettings.lensLabel) item.lensLabel = iconSettings.lensLabel;
        var previewSizes = sizeSchema.readPageSizes(preview.sizes, view.sizes, preview.file, problems);
        if (previewSizes) item.sizes = previewSizes.sizes;
        var collection = collections.find(function (candidate) { return candidate.name === collectionName; });
        if (!collection) {
          var selected = manifest.titleIcon(iconSettings, collectionName, 'component');
          collection = { name: collectionName, icon: selected.icon, iconPriority: selected.mapped ? 6 : iconSettings.icon ? 4 : 1, items: [] };
          collections.push(collection);
        }
        var groupName = parts.join(' / ');
        if (groupName) {
          var group = collection.items.find(function (item) { return item.group === groupName; });
          if (!group) { group = { group: groupName, items: [] }; collection.items.push(group); }
          group.items.push(item);
        } else collection.items.push(item);
        var previous = pages[preview.file];
        /* A page that lists the preview keeps its own sizes; without any, the definition's apply. */
        if (previewSizes && !(previous && previous.sizes)) previous = Object.assign({}, previous, { sizes: previewSizes.sizes });
        pages[preview.file] = Object.assign({}, previous, { label: previous && previous.label || label,
          design: path.join(root, preview.file), preview: preview, code: (previous && previous.code || []).concat([
            { implementation: 'workbench', path: path.join(root, preview.source), relative: preview.source, exists: true },
          ]) });
      });
      index.previews.forEach(function (preview) {
        Object.keys(preview.links || {}).forEach(function (href) {
          var to = preview.links[href];
          var target = byId[to.preview];
          if (!target) problems.push(preview.file + ': link ' + href + ' names unknown Workbench preview “' + to.preview + '”.');
          else if (to.state && !target.states.some(function (state) { return state.id === to.state; })) {
            problems.push(preview.file + ': link ' + href + ' names unknown state “' + to.state + '” of ' + to.preview + '.');
          }
        });
      });
      Object.keys(implementations).forEach(function (key) {
        if (implementations[key].kind !== 'workbench') return;
        implementations[key].base = '';
        if (implementations[key].root !== root) problems.push('Workbench implementations use this project root; use previews.config to configure adapters.');
        pageItems(view.collections).forEach(function (item) {
          var reference = item.implementations && item.implementations[key];
          if (!reference) return;
          var preview = byId[reference.preview];
          if (!preview) { problems.push(item.label + ': unknown Workbench preview “' + reference.preview + '”.'); return; }
          reference.path = '/' + preview.file;
          reference.states = {};
          preview.states.forEach(function (state) { reference.states[state.id] = '/' + preview.file + '?state=' + encodeURIComponent(state.id); });
          pages[item.src].code.push({ implementation: key, path: path.join(root, preview.source), relative: preview.source, exists: true });
        });
      });
    } catch (error) { problems.push('Workbench previews: ' + String(error.message || error)); }
  }

  function implementation(key) {
    var view = resolved();
    return (view && view.implementations[key]) || null;
  }

  function origins() {
    var view = resolved();
    if (!view) return [];
    return Object.keys(view.implementations).map(function (key) {
      var impl = view.implementations[key];
      if (impl.kind === 'storybook' && impl.auto && autoStorybookAddresses[key]) {
        impl = Object.assign({}, impl, { url: autoStorybookAddresses[key] });
      }
      return originOf(impl);
    }).filter(Boolean).concat(proxies.origins());
  }

  /* The config as the browser gets it: URL and Storybook implementations at
     their proxies. The server itself keeps talking to them directly. */
  function proxied(view) {
    if (!view) return Promise.resolve(view);
    var implementations = Object.assign({}, view.implementations);
    return Promise.all(Object.keys(implementations).map(function (key) {
      var impl = implementations[key];
      var field = impl.kind === 'url' ? 'base' : impl.kind === 'storybook' ? 'url' : null;
      if (!field || !/^https?:\/\//i.test(impl[field] || '')) return null;
      return proxies.address(impl[field]).then(function (address) {
        var copy = Object.assign({}, impl, { upstream: impl[field] });
        copy[field] = address;
        implementations[key] = copy;
      }, function (error) {
        diagnostic('warn', 'proxy.failed', { implementation: key, message: String(error.message || error) });
      });
    })).then(function () {
      return Object.assign({}, view, { implementations: implementations });
    });
  }

  /* A browser names the page a request came from. One from another site, or
     from another of these servers, is refused; a request without an Origin
     is not a page's at all — curl, or a test. */
  function sameOrigin(req) {
    var site = req.headers['sec-fetch-site'];
    if (site && site !== 'same-origin' && site !== 'none') return false;
    var origin = req.headers.origin;
    if (!origin) return true;
    var port = server.address().port;
    return origin === 'http://127.0.0.1:' + port || origin === 'http://localhost:' + port;
  }

  function json(res, status, body) {
    send(res, status, JSON.stringify(body), TYPES['.json']);
  }

  function trouble(res, err, fallback) {
    json(res, err.status || fallback || 500, { ok: false, error: String(err.message || err) });
  }

  /* A single bounded path for browser, server and editor diagnostics. The
     extension writes these into VS Code's session-managed log storage; the
     standalone server leaves them silent unless its caller opts in. Keep the
     record deliberately small and textual so prompts, DOM snapshots and image
     bytes can never accidentally become logs. */
  function diagnostic(level, event, details) {
    var record = {
      level: ['debug', 'info', 'warn', 'error'].indexOf(level) > -1 ? level : 'info',
      event: String(event || 'workbench').slice(0, 120),
      details: {},
    };
    Object.keys(details || {}).slice(0, 20).forEach(function (key) {
      var value = details[key];
      if (value == null || typeof value === 'number' || typeof value === 'boolean') record.details[key] = value;
      else record.details[key] = String(value).slice(0, 2000);
    });
    var size = Buffer.byteLength(JSON.stringify(record));
    if (loggedBytes + size > logLimit) {
      if (!logLimitReported) {
        logLimitReported = true;
        try {
          onLog({ level: 'warn', event: 'log.limit.reached', details: { bytes: loggedBytes, limit: logLimit } });
        } catch (_) {}
      }
      return;
    }
    loggedBytes += size;
    try { onLog(record); } catch (_) { /* Diagnostics must never break the workbench. */ }
  }

  function captureDiagnostic(event, surface, payload, shot) {
    var reported = shot && (shot.details || shot.captureDetails);
    var scroll = reported && reported.scroll;
    diagnostic('info', event, {
      surface: surface,
      mirrored: !!payload.mirror,
      width: payload.width,
      height: payload.height,
      requestedX: payload.scroll.x,
      requestedY: payload.scroll.y,
      rendererReported: !!scroll,
      appliedX: scroll && scroll.appliedX,
      appliedY: scroll && scroll.appliedY,
    });
  }

  var exportJobs = new Map();
  var activeExport = null;

  function pruneSettledExports() {
    var settled = Array.from(exportJobs.values()).filter(function (job) {
      return job.status !== 'running';
    }).sort(function (a, b) { return b.finishedAt - a.finishedAt; });
    // Keep the immediately previous result available for a retrying browser,
    // but do not retain a second full copy of every repeated 40+ MB export.
    settled.slice(1).forEach(function (job) { exportJobs.delete(job.id); });
  }

  function exportStatus(job) {
    return {
      ok: job.status !== 'failed', id: job.id, status: job.status,
      completed: job.completed, total: job.total, current: job.current,
      warnings: job.warnings, error: job.error || null,
      download: job.archive ? { filename: job.archive.download.filename, bytes: job.archive.download.body.length } : null,
      parts: job.archive ? job.archive.archives.map(function (archive, index) {
        return { number: index + 1, filename: archive.filename, bytes: archive.body.length };
      }) : [],
    };
  }

  function beginExport(view, baseUrl) {
    if (activeExport && activeExport.status === 'running') return activeExport;
    pruneSettledExports();
    var plan = exportCapturePlan(view, baseUrl);
    var job = {
      id: crypto.randomBytes(12).toString('hex'), status: 'running',
      completed: 0, total: plan.captures.length, current: null,
      warnings: plan.warnings.length, archive: null, error: null,
    };
    exportJobs.set(job.id, job);
    activeExport = job;
    diagnostic('info', 'export.started', { screenshots: job.total });
    job.promise = captureReady.then(function () {
      return docsExportPlan(view, baseUrl);
    }).then(function (docsPlan) {
      plan.captures = plan.captures.concat(docsPlan.captures);
      plan.warnings = plan.warnings.concat(docsPlan.warnings);
      job.total = plan.captures.length;
      return captureExportReferences(capture, plan, baseUrl, function (completed) {
        job.completed = completed;
        if (completed === 1 || (completed > 0 && completed % 100 === 0)) {
          diagnostic('info', 'export.progress', { completed: completed, total: job.total });
        }
      });
    }).then(async function (references) {
      job.archive = await designExport.build(root, view, {
        manifest: path.join(where.dir, config.FILE),
        screenshots: references.screenshots,
        captureWarnings: references.warnings,
        maxArchiveBytes: options.exportMaxArchiveBytes,
      }, previews);
      var report = job.archive.report;
      job.warnings = report.warnings.length + report.captureWarnings.length + (report.browser ? report.browser.warnings.length : 0);
      job.completed = job.total;
      job.current = null;
      job.status = 'complete';
      diagnostic('info', 'export.completed', {
        files: job.archive.report.files.length,
        screenshots: references.screenshots.length,
        archives: job.archive.archives.length,
        warnings: job.warnings,
      });
      return job;
    }).catch(function (error) {
      job.status = 'failed';
      job.current = null;
      job.error = String(error.message || error);
      diagnostic('error', 'export.failed', { message: job.error });
      return job;
    });
    job.promise.then(function () {
      if (activeExport === job) activeExport = null;
      job.finishedAt = Date.now();
      var retention = Number(options.exportRetentionMs) || 15 * 60 * 1000;
      var expiry = setTimeout(function () { exportJobs.delete(job.id); }, retention);
      if (expiry.unref) expiry.unref();
    });
    return job;
  }

  function sendArchive(res, archive, head) {
    res.writeHead(200, {
      'Content-Type': 'application/zip',
      'Content-Length': archive.body.length,
      'Content-Disposition': 'attachment; filename="' + archive.filename + '"',
      'Cache-Control': 'no-store',
    });
    res.end(head ? undefined : archive.body);
  }

  function archivePart(bundle, url) {
    var raw = url.searchParams.get('part');
    if (!raw) return bundle.download;
    var number = raw ? Number(raw) : 1;
    if (!Number.isInteger(number) || number < 1 || number > bundle.archives.length) throw refused(404, 'That export ZIP part does not exist.');
    return bundle.archives[number - 1];
  }

  var server = http.createServer(function (req, res) {
    var url = new URL(req.url, 'http://127.0.0.1');
    var address = server.address();
    if (!address) { send(res, 503, 'Workbench is stopping.'); return; }
    var baseUrl = 'http://127.0.0.1:' + address.port + '/';

    if (url.pathname === WORKBENCH_PREFIX + 'preview-compat.js') {
      send(res, 200, previewScripts.source, 'text/javascript; charset=utf-8');
      return;
    }
    if (url.pathname.indexOf(browserModules.PREFIX) === 0) {
      if (req.method !== 'GET' && req.method !== 'HEAD') { send(res, 405, 'GET a Workbench module here.'); return; }
      serveBrowserModule(url.pathname).then(function (asset) { send(res, asset.status, asset.body, asset.type); })
        .catch(function (error) { trouble(res, error, 500); });
      return;
    }

    if (url.pathname === docsService.SOURCE_PATH || url.pathname === docsService.REVISION_PATH || url.pathname === docsService.BUNDLE_PATH) {
      if (req.method !== 'GET') { send(res, 405, 'GET a docs page’s source, revision, or bundle here.'); return; }
      docsReady().then(function (view) {
        var page = docsPages(view).find(function (candidate) { return candidate.src === url.searchParams.get('page'); });
        if (!page) throw refused(404, 'Not a docs page: ' + url.searchParams.get('page'));
        var lens = url.searchParams.get('lens');
        if (url.pathname === docsService.REVISION_PATH) {
          return docs.revision(page, lens).then(function (revision) { json(res, 200, { revision: revision }); });
        }
        if (url.pathname === docsService.BUNDLE_PATH) {
          return docs.bundleInfo(page, lens).then(function (info) { json(res, 200, info); });
        }
        return docs.exampleSource(page, lens, url.searchParams.get('example') || '').then(function (code) { json(res, 200, code); });
      }).catch(function (error) { trouble(res, error, 500); });
      return;
    }

    /* A declared docs page is served at its Markdown path as the page; any
       other Markdown file is served as it is. */
    if (/\.md$/i.test(url.pathname) && (req.method === 'GET' || req.method === 'HEAD')) {
      var docsSrc;
      try { docsSrc = decodeURIComponent(url.pathname.slice(1)); } catch (_) { docsSrc = null; }
      /* Discovered pages are known once the config has been resolved with
         the worker; a broken config serves the file as it is. */
      docsReady().catch(function () { return null; }).then(function (view) {
        var pages = docsPages(view);
        var docsPage = pages.find(function (candidate) { return candidate.src === docsSrc; });
        if (!docsPage) { serveFile(root, url.pathname, res, { inject: true }); return; }
        return docs.page(docsPage, { lens: url.searchParams.get('lens'), state: url.searchParams.get('state') },
          new Set(pages.map(function (page) { return page.src; })), {}, { defer: true }).then(function (html) {
          send(res, 200, withPreviewScripts(html), 'text/html; charset=utf-8');
        });
      }).catch(function (error) { trouble(res, error, 500); });
      return;
    }

    if (url.pathname === WORKBENCH_PREFIX + 'preview-runtime.js') {
      serveFile(path.join(__dirname, 'preview'), '/browser.js', res);
      return;
    }
    if (url.pathname === WORKBENCH_PREFIX + 'preview-requests.js') {
      serveFile(path.join(__dirname, 'preview'), '/requests.js', res);
      return;
    }

    if (url.pathname.indexOf('/_workbench/previews/') === 0 || /\.workbench\.tsx?$/.test(url.pathname)) {
      if (req.method !== 'GET') { send(res, 405, 'GET a Workbench preview here.'); return; }
      // Assets belong to an already compiled preview. Recheck configuration
      // and trust, but don't rebuild every catalog for each CSS/image/script.
      if (url.pathname.indexOf('/_workbench/previews/') === 0 && previews) {
        try {
          var previewView = config.read(where);
          if (previewView && previewView.previews !== false && options.isTrusted !== false &&
              JSON.stringify(previewView.previews || {}) === previewSettings) {
            previews.proxy(req, res, url.pathname.slice('/_workbench/previews'.length) + url.search)
              .catch(function (error) { if (!res.headersSent) trouble(res, error, 500); else res.destroy(); });
            return;
          }
        } catch (error) { trouble(res, error, 500); return; }
      }
      resolvedWithCatalogs().then(function (view) {
        if (!previews || !view || view.previews === false || options.isTrusted === false) throw refused(404, 'Workbench previews are unavailable.');
        if (url.pathname.indexOf('/_workbench/previews/') === 0) return previews.proxy(req, res, url.pathname.slice('/_workbench/previews'.length) + url.search);
        var relative = decodeURIComponent(url.pathname.slice(1));
        if (!view.pages[relative] || !view.pages[relative].preview) throw refused(404, 'This preview is not in the catalog.');
        return previews.proxy(req, res, '/page?file=' + encodeURIComponent(relative));
      }).catch(function (error) { if (!res.headersSent) trouble(res, error, 500); else res.destroy(); });
      return;
    }

    if (url.pathname === LOG_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST a diagnostic here.');
        return;
      }
      readBody(req, MAX_LOG)
        .then(function (body) {
          var entry = JSON.parse(body.toString('utf8'));
          diagnostic(entry.level, entry.event, entry.details);
          json(res, 200, { ok: true });
        })
        .catch(function (err) { trouble(res, err, 400); });
      return;
    }

    /* Each canvas posts the page it shows; agents GET the latest one. */
    if (url.pathname === agentView.VIEW_PATH) {
      if (req.method === 'GET' || req.method === 'HEAD') {
        json(res, 200, Object.assign({ ok: true }, shown.current()));
        return;
      }
      if (req.method !== 'POST') {
        send(res, 405, 'POST the canvas view here, or GET the current one.');
        return;
      }
      readBody(req, MAX_VIEW)
        .then(function (body) {
          shown.report(JSON.parse(body.toString('utf8')));
          json(res, 200, { ok: true });
        })
        .catch(function (err) { trouble(res, err, 400); });
      return;
    }

    /* Custom size… and Edit sizes…: `add` one size, to the space or one page,
       or `update` the space's sizes as the dialog left them. Answers with the
       space's sizes as they now resolve. See src/sizes/edit.ts. */
    if (url.pathname === SIZES_PATH) {
      if (req.method !== 'POST') { send(res, 405, 'POST a size change here.'); return; }
      readBody(req, MAX_HANDOFF)
        .then(function (body) {
          var ask = JSON.parse(body.toString('utf8'));
          var before = config.read(where);
          if (!before) throw new Error('There is no ' + config.FILE + ' at the project root.');
          var local = before.sizes.filter(function (size) { return size.local; }).map(function (size) { return size.key; });
          var key = null;
          config.updateSizes(where, function (sizes, collections) {
            if (ask && ask.action === 'add') {
              var added = sizeEdit.addSize(sizes, collections, ask.size || {});
              key = added.key;
              return added;
            }
            if (ask && ask.action === 'update') return sizeEdit.updateSizes(sizes, collections, ask.sizes || [], local);
            throw new Error('Ask to add a size or update the sizes.');
          });
          var after = config.read(where);
          json(res, 200, { ok: true, key: key, sizes: after ? after.sizes : [] });
        })
        .catch(function (error) { trouble(res, error, 400); });
      return;
    }

    /* The form editor changes only the committed collections block. The rest of
       the file, including implementations and its comments, stays untouched. */
    if (url.pathname === CONFIG_FILE_PATH) {
      if (req.method === 'GET') {
        try {
          var configSource = config.source(where);
          if (!configSource) json(res, 404, { ok: false, error: 'There is no ' + config.FILE + ' at the project root.' });
          else {
            var spaceView = config.read(where);
            json(res, 200, { ok: true, collections: configSource.collections,
              sizes: spaceView ? spaceView.sizes.map(function (size) { return size.key; }) : [] });
          }
        } catch (error) { trouble(res, error, 400); }
        return;
      }
      if (req.method !== 'POST') { send(res, 405, 'GET the page form or POST its collections here.'); return; }
      readBody(req, MAX_HANDOFF)
        .then(function (body) {
          var ask = JSON.parse(body.toString('utf8'));
          var saved = config.updateCollections(where, ask.collections);
          json(res, 200, { ok: true, collections: saved.collections });
        })
        .catch(function (error) { trouble(res, error, 400); });
      return;
    }

    /* The spaces the canvas can switch between, this one marked current.
       Opening one answers with its workbench URL, starting its server first
       if it isn't running. Only a page this server served may ask: starting a
       space can run its start commands. */
    if (url.pathname === WORKBENCH_PREFIX + 'canvas/space') {
      if (req.method !== 'POST') { send(res, 405, 'POST a canvas space operation.'); return; }
      if (!sameOrigin(req)) { json(res, 403, { ok: false, error: 'Only this canvas can access space services.' }); return; }
      readBody(req, MAX_VIEW).then(function (body) {
        var ask = canvasSpaces.parseSpaceRequest(JSON.parse(body.toString('utf8')));
        return canvasSpaces.coordinate(ask, async function (id) {
          if (id === spaceSelf) return { url: baseUrl + WORKBENCH_PATH.slice(1) };
          if (!spaceList) throw refused(404, 'That space is not in the Workbench list.');
          var listed = await spaceList.list();
          if (!listed.some(function (p) { return p.id === id; })) throw refused(404, 'That space is no longer available.');
          return spaceList.open(id);
        });
      }).then(function (answer) { json(res, 200, Object.assign({ ok: true }, answer)); })
        .catch(function (error) { trouble(res, error, 400); });
      return;
    }
    if (url.pathname === SPACES_PATH) {
      if (req.method !== 'GET' && req.method !== 'HEAD') { send(res, 405, 'GET the spaces here.'); return; }
      Promise.resolve()
        .then(function () {
          return spaceList ? spaceList.list() : [spaces.describe({ dir: where.dir, key: where.key, root: root })];
        })
        .then(function (listed) {
          json(res, 200, { ok: true, current: spaceSelf, spaces: listed });
        })
        .catch(function (err) { trouble(res, err, 500); });
      return;
    }

    if (url.pathname === SPACES_OPEN_PATH) {
      if (req.method !== 'POST') { send(res, 405, 'POST a space id here.'); return; }
      if (!sameOrigin(req)) { json(res, 403, { ok: false, error: 'Only the workbench can switch spaces.' }); return; }
      readBody(req, MAX_VIEW)
        .then(function (body) {
          var ask = JSON.parse(body.toString('utf8'));
          var id = String((ask && ask.id) || '');
          if (id === spaceSelf) return { url: 'http://127.0.0.1:' + server.address().port + WORKBENCH_PATH };
          if (!spaceList) throw refused(404, 'That space isn’t in the Workbench list.');
          return Promise.resolve(spaceList.open(id)).then(function (opened) {
            diagnostic('info', 'space.opened', { space: id });
            return { url: opened.url };
          });
        })
        .then(function (answer) { json(res, 200, Object.assign({ ok: true }, answer)); })
        .catch(function (err) { trouble(res, err, 400); });
      return;
    }

    /* What this machine makes of the config: every path absolute, every
       problem named. The workbench reads it for the source menu and the
       handoff; `curl` reads it to check a setup. */
    if (url.pathname === CONFIG_PATH) {
      /* The canvas shows nothing until this answers, so every answer says
         how long it took and where the time went. */
      var configBegan = Date.now();
      var configTiming = {};
      diagnostic('info', 'config.requested', {});
      Promise.resolve()
        .then(resolvedWithCatalogs)
        .then(function (view) {
          configTiming.catalogsMs = Date.now() - configBegan;
          return proxied(view);
        })
        .then(function (view) {
          configTiming.proxyMs = Date.now() - configBegan - configTiming.catalogsMs;
          configTiming.elapsedMs = Date.now() - configBegan;
          configTiming.found = !!view;
          diagnostic(configTiming.elapsedMs >= 1000 ? 'warn' : 'info', 'config.resolved', configTiming);
          if (!view) json(res, 404, { ok: false, error: 'There is no ' + config.FILE + ' at the project root.' });
          else json(res, 200, Object.assign({ ok: true }, view));
        })
        .catch(function (err) {
          diagnostic('warn', 'config.failed', { elapsedMs: Date.now() - configBegan, message: String(err.message || err) });
          trouble(res, err, 500);
        });
      return;
    }

    if (url.pathname === EXPORT_PATH) {
      if (req.method !== 'GET' && req.method !== 'HEAD' && req.method !== 'POST') {
        send(res, 405, 'POST to start or GET this design-system export.');
        return;
      }
      var jobId = String(url.searchParams.get('job') || '');
      if (jobId) {
        var existing = exportJobs.get(jobId);
        if (!existing) { json(res, 404, { ok: false, error: 'That export job is no longer available.' }); return; }
        if (url.searchParams.get('download') === '1') {
          if (existing.status !== 'complete') { json(res, 409, exportStatus(existing)); return; }
          try { sendArchive(res, archivePart(existing.archive, url), req.method === 'HEAD'); }
          catch (error) { trouble(res, error, error.status || 400); }
          return;
        }
        json(res, 200, exportStatus(existing));
        return;
      }
      Promise.resolve()
        .then(resolvedWithCatalogs)
        .then(function (view) {
          if (!view) throw refused(404, 'There is no ' + config.FILE + ' at the project root.');
          var job = beginExport(view, baseUrl);
          if (req.method === 'POST') { json(res, 202, exportStatus(job)); return; }
          return job.promise.then(function () {
            if (job.status === 'failed') throw new Error(job.error);
            sendArchive(res, archivePart(job.archive, url), req.method === 'HEAD');
          });
        })
        .catch(function (err) { trouble(res, err, 500); });
      return;
    }

    /* VS Code's webview forwarding does not reliably preserve a WebSocket
       upgrade. Embedded JPEG clients use this ordinary streamed response;
       the private token and loopback binding are the same as the socket. */
    if (url.pathname === WINDOW_STREAM_PATH && req.method === 'GET') {
      if (!windowVideo.acceptHttp || !windowVideo.acceptHttp(req, res)) {
        send(res, 403, 'That window stream is not active.');
      }
      return;
    }

    /* A window lens: stream the window a page names from the app its
       implementation declares. The request only says which page; the app and
       title come from the config, so a page can't ask for any other window. */
    if (url.pathname === WINDOW_STREAM_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST a window stream request here.');
        return;
      }
      readBody(req, MAX_HANDOFF)
        .then(function (body) {
          var ask = JSON.parse(body.toString('utf8'));
          return resolvedWithCatalogs().then(function (view) {
            var key = String(ask.implementation || '');
            var src = String(ask.src || '');
            var impl = view && view.implementations[key];
            var page = view && Object.prototype.hasOwnProperty.call(view.pages, src) ? view.pages[src] : null;
            var title = page && page.windows && page.windows[key];
            if (!impl || impl.kind !== 'window' || !title) {
              throw refused(403, 'that window isn’t declared by this workbench');
            }
            if (ask.stop) {
              return Promise.resolve(windowVideo.stop(key + '\n' + src)).then(function () {
                json(res, 200, { ok: true, stopped: true });
              });
            }
            var codec = ask.codec === 'jpeg' ? 'jpeg' : 'h264';
            return Promise.resolve(windowVideo.start({
              app: impl.app,
              source: title,
              id: key + '\n' + src,
              codec: codec,
            })).then(function (result) {
              json(res, 200, {
                ok: true,
                stream: WINDOW_STREAM_PATH + '?token=' + encodeURIComponent(result.token),
                source: result.source,
                codec: result.codec || codec,
              });
            });
          });
        })
        .catch(function (err) { trouble(res, err, 500); });
      return;
    }

    if (url.pathname === SIMULATOR_PATH || url.pathname === SIMULATOR_INPUT_PATH || url.pathname === SIMULATOR_STREAM_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST a Simulator request here.');
        return;
      }
      readBody(req, MAX_HANDOFF)
        .then(function (body) {
          var ask = JSON.parse(body.toString('utf8'));
          return resolvedWithCatalogs().then(async function (view) {
            var key = String(ask.implementation || '');
            var udid = String(ask.udid || '');
            var impl = view && view.implementations[key];
            var devices = impl && impl.kind === 'ios-simulator' ? await Promise.resolve(listSimulators()) : [];
            var device = devices.find(function (entry) { return entry.udid === udid || entry.name === udid; });
            var allowed = view && Object.keys(view.pages).some(function (src) {
              return (src.indexOf('__ios-simulator/' + key + '/') === 0
                && view.pages[src].simulator
                && view.pages[src].simulator.udid === udid);
            });
            if (!allowed && device) allowed = pageItems(view.collections).some(function (item) {
              var mapping = item.implementations && item.implementations[key];
              return mapping && (mapping.device === device.udid || mapping.device === device.name);
            });
            if (!impl || impl.kind !== 'ios-simulator' || !allowed) {
              throw refused(403, 'that Simulator isn’t declared by this workbench');
            }
            if (device) udid = device.udid;
            ask.udid = udid;
            if (url.pathname === SIMULATOR_STREAM_PATH) {
              if (ask.stop) {
                return Promise.resolve(windowVideo.stop(udid)).then(function () {
                  json(res, 200, { ok: true, stopped: true });
                });
              }
              return Promise.resolve(windowVideo.start({
                source: device ? device.name : view.pages[Object.keys(view.pages).find(function (src) {
                  return src.indexOf('__ios-simulator/' + key + '/') === 0
                    && view.pages[src].simulator && view.pages[src].simulator.udid === udid;
                })].label,
                app: 'simulator',
                id: udid,
                codec: ask.codec === 'jpeg' ? 'jpeg' : 'h264',
              })).then(function (result) {
                json(res, 200, {
                  ok: true,
                  stream: WINDOW_STREAM_PATH + '?token=' + encodeURIComponent(result.token),
                  source: result.source,
                  codec: result.codec || (ask.codec === 'jpeg' ? 'jpeg' : 'h264'),
                });
              });
            }
            if (url.pathname === SIMULATOR_PATH) {
              return startSimulatorSession(udid).then(function (session) {
                json(res, 200, { ok: true, udid: udid, screen: session.screen });
              });
            }
            return simulatorInput(ask).then(function (result) {
              json(res, 200, Object.assign({ ok: true }, result));
            });
          });
        })
        .catch(function (err) { trouble(res, err, 500); });
      return;
    }

    /* Open a page's source in the editor: its design file, or one of the
       places its implementation lives. Only paths the config resolves to —
       a page in the frame could POST here, and it is not getting to name a
       file on this machine. */
    if (url.pathname === OPEN_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST the page here.');
        return;
      }
      readBody(req, MAX_HANDOFF)
        .then(function (body) {
          var ask = JSON.parse(body.toString('utf8'));
          return resolvedWithCatalogs().then(function (view) {
            var page = view && view.pages[String(ask.src || '')];
            if (!page) throw refused(404, 'that page isn’t in the workbench');
            var file = null;
            if (!ask.path) file = page.design;
            else {
              page.code.forEach(function (entry) {
                if (entry.path && entry.path === ask.path) file = entry.path;
              });
            }
            if (!file) throw refused(404, 'not a path this workbench knows');
            if (!fs.existsSync(file)) throw refused(404, 'not on this machine: ' + file);
            return Promise.resolve(onOpen(file)).then(function () {
              json(res, 200, { ok: true, opened: file });
            });
          });
        })
        .catch(function (err) {
          trouble(res, err, 500);
        });
      return;
    }

    /* A screenshot of a page an implementation serves, taken by the browser
       going there itself — the frame can't be read across the origin. */
    if (url.pathname === CAPTURE_PAGE_PATH || url.pathname === CAPTURE_PAGE_IMAGE_PATH ||
        url.pathname === CAPTURE_PAGE_PREPARE_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST a capture request here.');
        return;
      }
      readBody(req, MAX_CAPTURE)
        .then(function (body) {
          var payload = pagePayload(body, origins());
          if (url.pathname === CAPTURE_PAGE_PREPARE_PATH) {
            var prepare = payload.mirror ? capture.prepare(baseUrl, bridgedPayload(payload, baseUrl)) : capture.preparePage(payload);
            return Promise.resolve(prepare).then(function () {
              json(res, 200, { ok: true, prepared: true });
            });
          }
          captureDiagnostic('capture.requested', 'implementation', payload);
          var taking = payload.mirror ? capture.capture(baseUrl, bridgedPayload(payload, baseUrl)) : capture.capturePage(payload);
          return Promise.resolve(taking).then(function (shot) {
            captureDiagnostic('capture.completed', 'implementation', payload, shot);
            var png = shot && (shot.image || shot.png) ? (shot.image || shot.png) : shot;
            if (!png || !png.length) throw new Error('native capture returned an empty image');
            if (url.pathname === CAPTURE_PAGE_IMAGE_PATH) {
              if (url.searchParams.get('review') === '1') {
                var inspected = shot && (shot.targets || (shot.captureDetails && shot.captureDetails.targets));
                json(res, 200, { ok: true, image: png.toString('base64'), type: payload.format === 'jpeg' ? 'image/jpeg' : 'image/png', targets: inspected || null });
                return;
              }
              sendImage(res, png, payload.format);
              return;
            }
            var file = saveShot(root, payload.name, png, payload.format);
            onShot(file);
            var targets = shot && (shot.targets || (shot.captureDetails && shot.captureDetails.targets));
            json(res, 200, { ok: true, file: file, targets: targets || null });
          });
        })
        .catch(function (err) {
          trouble(res, err, 503);
        });
      return;
    }

    /* The stories a Storybook holds under one title, with where their code
       is on this machine. Fetched from the Storybook's own index each time —
       stories are added while the workbench is open. */
    if (url.pathname === STORIES_PATH) {
      var storyKey = String(url.searchParams.get('implementation') || '');
      var title = String(url.searchParams.get('title') || '');
      Promise.resolve()
        .then(function () {
          return resolvedWithCatalogs().then(function (view) {
            var impl = view && view.implementations[storyKey];
            if (!impl || impl.kind !== 'storybook' || impl.url === 'auto') {
              throw refused(400, 'there is no available Storybook implementation called “' + storyKey + '”');
            }
            if (!title) throw refused(400, 'which title?');
            return storybookIndex(impl).then(function (index) {
              return storiesTitled(index, title, impl);
            });
          }).then(function (stories) {
            json(res, 200, { ok: true, implementation: storyKey, title: title, stories: stories });
          });
        })
        .catch(function (err) {
          trouble(res, err, 500);
        });
      return;
    }

    if (url.pathname === CAPTURE_WARM_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST here to warm native capture.');
        return;
      }
      Promise.resolve(capture.warm(baseUrl))
        .then(function () {
          send(res, 200, JSON.stringify({ ok: true, warmed: true }), TYPES['.json']);
        })
        .catch(function (err) {
          send(res, 503, JSON.stringify({ ok: false, error: String(err.message || err) }), TYPES['.json']);
        });
      return;
    }

    if (url.pathname === CAPTURE_PREPARE_PATH || url.pathname === CAPTURE_PATH ||
        url.pathname === CAPTURE_IMAGE_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST a capture request here.');
        return;
      }
      readBody(req, MAX_CAPTURE)
        .then(function (body) {
          var payload = capturePayload(body, baseUrl);
          if (url.pathname === CAPTURE_PREPARE_PATH) {
            return Promise.resolve(capture.prepare(baseUrl, payload)).then(function () {
              return { prepared: true };
            });
          }
          captureDiagnostic('capture.requested', 'local', payload);
          return Promise.resolve(capture.capture(baseUrl, payload)).then(function (png) {
            captureDiagnostic('capture.completed', 'local', payload, png);
            if (!png || !png.length) throw new Error('native capture returned an empty image');
            if (url.pathname === CAPTURE_IMAGE_PATH) {
              sendImage(res, png, payload.format);
              return null;
            }
            var file = saveShot(root, payload.name, png, payload.format);
            onShot(file);
            return { file: file };
          });
        })
        .then(function (result) {
          if (result === null) return;
          send(res, 200, JSON.stringify(Object.assign({ ok: true }, result)), TYPES['.json']);
        })
        .catch(function (err) {
          send(res, 503, JSON.stringify({ ok: false, error: String(err.message || err) }), TYPES['.json']);
        });
      return;
    }

    if (url.pathname === SHOT_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST a PNG or JPEG here.');
        return;
      }
      readBody(req, MAX_SHOT)
        .then(function (body) {
          if (!body.length) throw new Error('that shot was empty');
          var format = (req.headers['content-type'] || '').split(';')[0].trim() === 'image/jpeg' ? 'jpeg' : 'png';
          var file = saveShot(root, url.searchParams.get('name'), body, format);
          onShot(file);
          send(res, 200, JSON.stringify({ ok: true, file: file }), TYPES['.json']);
        })
        .catch(function (err) {
          send(res, 500, JSON.stringify({ ok: false, error: String(err.message || err) }), TYPES['.json']);
        });
      return;
    }

    if (url.pathname === WORKBENCH_PREFIX + 'canvas/review') {
      if (req.method !== 'POST') { send(res, 405, 'POST an artboard review.'); return; }
      readBody(req, MAX_HANDOFF).then(function (body) {
        var payload = JSON.parse(body.toString('utf8'));
        if (!payload || typeof payload.src !== 'string') throw new Error('Invalid artboard review.');
        return docsHandoff(payload).then(function () { json(res, 200, { ok: true, payload: payload }); });
      }).catch(function (error) { trouble(res, error, 400); });
      return;
    }

    /* The shot is already on disk by the time this arrives — all that's left
       is to say it in words and pass it to the editor. */
    if (url.pathname === HANDOFF_PATH) {
      if (req.method === 'GET' || req.method === 'HEAD') {
        send(res, 200, JSON.stringify({ ok: true, available: hasHandoff }), TYPES['.json']);
        return;
      }
      if (req.method !== 'POST') {
        send(res, 405, 'POST the canvas here.');
        return;
      }
      var handoffFile = null;
      readBody(req, MAX_HANDOFF)
        .then(function (body) {
          var canvas = JSON.parse(body.toString('utf8'));
          handoffFile = canvas.file || null;
          return (canvas.version === 2 ? Promise.resolve() : docsHandoff(canvas)).then(function () {
            return onHandoff(handoff.prompt(canvas), canvas);
          }).then(function () {
            diagnostic('info', 'handoff.completed', { file: handoffFile, target: 'clipboard' });
            send(res, 200, JSON.stringify({ ok: true }), TYPES['.json']);
          });
        })
        .catch(function (err) {
          diagnostic('error', 'handoff.failed', {
            file: handoffFile,
            target: 'clipboard',
            message: String(err.message || err),
          });
          send(res, 500, JSON.stringify({ ok: false, error: String(err.message || err) }), TYPES['.json']);
        });
      return;
    }

    if (req.method !== 'GET' && req.method !== 'HEAD') {
      send(res, 405, 'GET or HEAD only.');
      return;
    }

    /* Lucide is a dependency rather than another hand-maintained workbench
       asset. Give its browser bundle one stable URL beside the tool. */
    if (url.pathname === LUCIDE_URL) {
      serveFile(path.dirname(LUCIDE_PATH), '/' + path.basename(LUCIDE_PATH), res);
      return;
    }

    /* The space's workbench.yaml and workbench.local.yaml, wherever they
       are — for a space whose config isn't at the root this serves. Only
       those two files. */
    if (url.pathname.indexOf(MANIFEST_PATH) === 0) {
      var manifestName = url.pathname.slice(MANIFEST_PATH.length);
      if (manifestName !== config.FILE && manifestName !== config.LOCAL) { send(res, 404, 'Not a Workbench config file.'); return; }
      serveFile(where.dir, '/' + manifestName, res);
      return;
    }

    /* The tool, out of the extension. Its own paths are relative, so keeping
       the leading slash is what makes /_workbench/ mean the folder's root. */
    if (url.pathname.indexOf(WORKBENCH_PREFIX) === 0) {
      var workbenchFile = url.pathname === WORKBENCH_PREFIX ? '/index.html' : url.pathname.slice(WORKBENCH_PREFIX.length - 1);
      if (workbenchFile === '/index.html') diagnostic('info', 'canvas.served', {});
      serveFile(WORKBENCH_DIR, workbenchFile, res, { head: canvasHead });
      return;
    }

    serveFile(root, url.pathname, res, { inject: true });
  });

  server.on('upgrade', function (req, socket) {
    if (!windowVideo.accept(req, socket)) socket.destroy();
  });

  /* Loopback only — this serves a whole folder, and it isn't anybody else's
     business. */
  function listen(ports) {
    return new Promise(function (resolve, reject) {
      var port = ports[0];

      function onError(err) {
        server.removeListener('error', onError);
        if (err.code === 'EADDRINUSE' && ports.length > 1) {
          resolve(listen(ports.slice(1)));
          return;
        }
        reject(err);
      }

      server.once('error', onError);
      server.listen(port, '127.0.0.1', function () {
        server.removeListener('error', onError);
        resolve(server.address().port);
      });
    });
  }

  return listen(PORTS).then(function (port) {
    diagnostic('info', 'session.started', { port: port });
    try { shown.announce('http://127.0.0.1:' + port + '/'); } catch (error) {
      diagnostic('warn', 'view.announce.failed', { message: String(error.message || error) });
    }
    if (options.eagerPreviews) warmDocs();
    if (options.eagerCapture) {
      captureReady = Promise.resolve().then(function () {
        return capture.warm('http://127.0.0.1:' + port + '/');
      }).catch(function (error) {
        diagnostic('warn', 'capture.warm.failed', { message: String(error.message || error) });
        console.info('[canonic] capture warm-up unavailable:', String(error.message || error));
      });
    }
    return {
      port: port,
      url: 'http://127.0.0.1:' + port + WORKBENCH_PATH,
      config: resolvedWithCatalogs,

      /* Points .canonic/.workbench/server.json at this server again. Spaces
         that share a root share that file; the one being shown claims it, so
         agents in that folder read the view the user has open. */
      announce: function () {
        try { shown.announce('http://127.0.0.1:' + port + '/'); } catch (error) {
          diagnostic('warn', 'view.announce.failed', { message: String(error.message || error) });
        }
      },

      close: function () {
        diagnostic('info', 'session.stopping', { port: port });
        shown.close();
        var stops = Object.keys(simulatorSessions).map(function (udid) {
          return Promise.resolve(simulatorSessions[udid]).then(function (session) {
            return runIosCli([
              'stop', '--session', session.sessionName, '--state-dir', simulatorStateDir, '--teardown',
            ], 30000);
          }).catch(function () {});
        });
        return Promise.all(stops).then(function () {
          return Promise.all([
            options.captureShared ? Promise.resolve() : Promise.resolve(capture.close()).catch(function () {}),
            Promise.resolve(windowVideo.close()).catch(function () {}),
            proxies.close(),
            previews ? previews.close() : Promise.resolve(),
          ]);
        }).then(function () {
          return new Promise(function (resolve) {
            server.close(resolve);
          });
        });
      },
    };
  });
}

module.exports = {
  start: start,
  capturePayload: capturePayload,
  pagePayload: pagePayload,
  exportCapturePlan: exportCapturePlan,
  captureExportReferences: captureExportReferences,
  safeName: safeName,
  CAPTURE_PATH: CAPTURE_PATH,
  CAPTURE_IMAGE_PATH: CAPTURE_IMAGE_PATH,
  CAPTURE_PAGE_PATH: CAPTURE_PAGE_PATH,
  CAPTURE_PAGE_IMAGE_PATH: CAPTURE_PAGE_IMAGE_PATH,
  CAPTURE_PAGE_PREPARE_PATH: CAPTURE_PAGE_PREPARE_PATH,
  CAPTURE_PREPARE_PATH: CAPTURE_PREPARE_PATH,
  CAPTURE_WARM_PATH: CAPTURE_WARM_PATH,
  HANDOFF_PATH: HANDOFF_PATH,
  CONFIG_PATH: CONFIG_PATH,
  CONFIG_FILE_PATH: CONFIG_FILE_PATH,
  SIZES_PATH: SIZES_PATH,
  LOG_PATH: LOG_PATH,
  OPEN_PATH: OPEN_PATH,
  SHOT_PATH: SHOT_PATH,
  STORIES_PATH: STORIES_PATH,
  SPACES_PATH: SPACES_PATH,
  SPACES_OPEN_PATH: SPACES_OPEN_PATH,
  createCapture: createCapture,
  EXPORT_PATH: EXPORT_PATH,
  SIMULATOR_PATH: SIMULATOR_PATH,
  SIMULATOR_INPUT_PATH: SIMULATOR_INPUT_PATH,
  SIMULATOR_STREAM_PATH: SIMULATOR_STREAM_PATH,
  WINDOW_STREAM_PATH: WINDOW_STREAM_PATH,
  storybookPorts: storybookPorts,
  simulatorCatalog: simulatorCatalog,
  withPreviewScripts: withPreviewScripts,
};

/* `node server.js [folder...]` — the same server the extension runs, for when
   you want it without the editor. Each folder's workbench.yaml is one
   space, or one per entry of its `spaces`. One space is one workbench,
   as in the editor; several are several workbenches the canvas switches
   between, each on its own server, all started here and sharing one
   screenshot service. */
if (require.main === module) {
  var dirs = process.argv.slice(2);
  if (!dirs.length) dirs = [process.cwd()];
  var onShot = function (file) {
    console.log('saved ' + file);
  };
  var failed = function (err) {
    console.error(String(err.message || err));
    process.exit(1);
  };
  var found = [];
  dirs.forEach(function (dir) {
    config.list(dir).forEach(function (space) { found.push(Object.assign({ dir: path.resolve(dir) }, space)); });
  });

  if (found.length <= 1) {
    var only = found[0] || { dir: path.resolve(dirs[0]), key: null, root: path.resolve(dirs[0]) };
    start({ root: only.root, config: { dir: only.dir, key: only.key }, eagerCapture: true, eagerPreviews: true, onShot: onShot }).then(function (running) {
      console.log('workbench on ' + running.url);
    }, failed);
  } else {
    var missing = dirs.filter(function (dir) { return !spaces.hasWorkbench(dir); });
    if (missing.length) failed(new Error('There’s no ' + config.FILE + ' in ' + missing.join(', ') + '.'));
    var shared = createCapture();
    var hub = spaces.create({
      start: function (space) {
        return start({
          root: space.root, config: { dir: space.dir, key: space.key },
          capture: shared, captureShared: true, eagerCapture: true, eagerPreviews: true,
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
