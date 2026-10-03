/* Workbench server
   ----------------
   Serves the project and the workbench over one http origin, and takes the
   workbench's screenshots.

   Two reasons this exists rather than a static server:
   - The native Chromium capture surface and its preview have to share an
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

var nativeCapture = require('./capture');
var electronCapture = require('./electron-capture');
var handoff = require('./handoff');
var config = require('./config');
var remote = require('./remote');
var designExport = require('./export');
var previewService = require('./preview-service');
var previewScripts = require('./preview-scripts');
var previewCompiler = require('./preview/compiler.cjs');
var windowStream = require('./window-stream');
var manifest = require('./workbench/manifest');

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
var OPEN_PATH = '/_workbench/open';
var STORIES_PATH = '/_workbench/stories';
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
var MAX_CAPTURE = 16 * 1024 * 1024; /* live DOM, styles, media state and marks */
var MAX_HANDOFF = 1024 * 1024; /* a canvas full of marks is a few kB */
var MAX_LOG = 16 * 1024; /* diagnostics are metadata, never screenshots or prompts */
var MAX_LOG_SESSION = 2 * 1024 * 1024; /* one noisy workbench cannot grow forever */
var REMOTE_TIMEOUT = 5000; /* a dev server that isn't running says so quickly */
var EXPORT_CAPTURE_WORKERS = 4;
var EXPORT_VIEWPORTS = {
  fit: { id: 'fit', label: 'Fit', width: 1440, height: 900 },
  desktop: { id: 'desktop', label: 'Desktop', width: 1512, height: 982 },
  mobile: { id: 'mobile', label: 'Mobile', width: 393, height: 852 },
};

/* What the marks are dressed in when they're laid over a page the workbench
   doesn't serve — see capture.js. The tokens markup.css leans on come from
   the workbench's own stylesheet, scoped to the layer so the page underneath
   never sees them; the hit areas and handles are the canvas's, not the
   picture's. */
var OVERLAY_CSS = [
  '#__wb_markup{--wb-font-sans:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif;',
  '--wb-space-2:2px;--wb-space-4:4px;--wb-space-8:8px;--wb-space-12:12px;--wb-space-16:16px;--wb-space-24:24px;',
  '--wb-radius-sm:4px;--wb-radius-md:8px;--wb-radius-pill:999px;--wb-fg:#fff;--wb-fg-3:#8c8c8c;--wb-accent:#00a1ff}',
  fs.readFileSync(path.join(__dirname, 'workbench', 'markup.css'), 'utf8'),
  '#__wb_markup .wb-hit,#__wb_markup .wb-sel{display:none}',
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

/* Two shots of the same screen are two different notes, so the second one
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
    markup: typeof payload.markup === 'string' ? payload.markup : '',
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
    markup: typeof payload.markup === 'string' ? payload.markup : '',
    anchors: anchorsOf(payload.anchors),
    css: OVERLAY_CSS,
    name: payload.name,
    format: captureFormat(payload.format),
    mirror: mirrorPayload(payload.mirror),
    revision: typeof payload.revision === 'string' ? payload.revision.slice(0, 128) : '',
  };
}

function screenItems(sections, out) {
  out = out || [];
  (sections || []).forEach(function (section) {
    (section.items || []).forEach(function visit(entry) {
      if (entry.folder) (entry.items || []).forEach(visit);
      else if (entry && entry.src) out.push(entry);
    });
  });
  return out;
}

/* Stable reference viewports for every declared state or Storybook story.
   Responsive screens produce both desktop and mobile references. The
   background renderer loads these directly, so exporting never drives or
   annotates the visible workbench canvas. */
function exportCapturePlan(view, baseUrl) {
  var items = screenItems((view && view.sections) || []).concat(screenItems((view && view.catalogSections) || []));
  var captures = [];
  var warnings = [];
  var seen = new Set();
  items.forEach(function (item) {
    if (seen.has(item.src)) return;
    seen.add(item.src);
    var implementationKey = item.implementationOnly;
    var implementation = implementationKey && view.implementations[implementationKey];
    var variants = item.states && item.states.length ? item.states : [{ id: 'default', label: 'Default' }];
    var declaredViewports = item.viewports || ['fit', 'desktop', 'mobile', 'responsive'];
    var viewports = [];
    declaredViewports.forEach(function (viewport) {
      var expanded = viewport === 'responsive'
        ? [EXPORT_VIEWPORTS.desktop, EXPORT_VIEWPORTS.mobile]
        : [EXPORT_VIEWPORTS[viewport] || EXPORT_VIEWPORTS.fit];
      expanded.forEach(function (size) {
        if (!viewports.some(function (candidate) { return candidate.id === size.id; })) viewports.push(size);
      });
    });
    var preview = view.screens[item.src] && view.screens[item.src].preview;
    if (preview) {
      variants = preview.states;
      variants.forEach(function (state) {
        viewports.forEach(function (size) {
          var target = new URL(preview.file, baseUrl);
          target.searchParams.set('state', state.id);
          captures.push({ screen: item.src, state: state.id, variant: state.id + '-' + size.id, label: state.label,
            viewport: size.id, viewportLabel: size.label, url: target.href, external: false, width: size.width, height: size.height });
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
        var story = view.screens[item.src] && (view.screens[item.src].stories || []).find(function (candidate) {
          return candidate.state === state.id;
        });
        if (!story) {
          warnings.push(item.label + ' — ' + state.label + ': Storybook did not provide a story id.');
          return;
        }
        viewports.forEach(function (size) {
          captures.push({
            screen: item.src, state: state.id, variant: state.id + '-' + size.id, label: state.label,
            viewport: size.id, viewportLabel: size.label,
            url: implementation.url + '/iframe.html?id=' + encodeURIComponent(story.id) + '&viewMode=story',
            external: true, width: size.width, height: size.height,
          });
        });
      });
      return;
    }
    var resolvedScreen = view.screens && view.screens[item.src];
    if (resolvedScreen && resolvedScreen.design && !fs.existsSync(resolvedScreen.design)) {
      warnings.push(item.label + ': the design file is missing, so no reference screenshot was captured.');
      return;
    }
    variants.forEach(function (state, index) {
      var target = new URL(item.src, baseUrl);
      target.searchParams.set('actions', 'off');
      if (index > 0) target.searchParams.set('state', state.id);
      viewports.forEach(function (size) {
        captures.push({
          screen: item.src, state: state.id, variant: state.id + '-' + size.id, label: state.label,
          viewport: size.id, viewportLabel: size.label,
          url: target.toString(), external: false, width: size.width, height: size.height,
        });
      });
    });
  });
  return { captures: captures, warnings: warnings };
}

function captureExportReferences(capture, plan, baseUrl, progress) {
  var screenshots = new Array(plan.captures.length);
  var warnings = plan.warnings.slice();
  // Keep every viewport of a page/story on the same renderer. The first shot
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
            scroll: { x: 0, y: 0 }, markup: '', anchors: [], format: 'jpeg', revision: '',
            reuse: reference.external ? 'storybook' : '',
          };
          try {
            var shot = reference.external ? await worker.captureExportPage(payload) : await worker.capture(baseUrl, payload);
            var body = shot && (shot.image || shot.png) ? (shot.image || shot.png) : shot;
            if (!body || !body.length) throw new Error('the renderer returned an empty image');
            screenshots[index] = Object.assign({}, reference, { body: body });
          } catch (error) {
            warnings.push(reference.screen + ' — ' + reference.label + ' · ' + reference.viewportLabel + ': ' + String(error.message || error));
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

/* The points under the marks, one per mark, null where a mark has no
   element to name (a freeform note). */
function anchorsOf(raw) {
  if (!Array.isArray(raw)) return [];
  return raw.map(function (point) {
    if (!point || !Number.isFinite(Number(point.x)) || !Number.isFinite(Number(point.y))) return null;
    return { x: Number(point.x), y: Number(point.y) };
  });
}

/* What runs in every page the browsers open: the element-describing helper
   the markup layer uses, plus one entry point DevTools can call by name. */
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
   first title segment is a section, any middle segments are one joined folder,
   and the leaf is the screen. Each story under that title is one state. The
   src is deliberately synthetic and stable; imported screens have no design
   page and always open through their Storybook implementation. */
function catalogIcon(impl, title) {
  var found = impl.catalogIcon || 'book-open';
  var length = -1;
  Object.keys(impl.catalogIcons || {}).forEach(function (prefix) {
    if ((title === prefix || title.indexOf(prefix + '/') === 0) && prefix.length > length) {
      found = impl.catalogIcons[prefix];
      length = prefix.length;
    }
  });
  return found;
}

function storybookCatalog(index, key, impl) {
  var byTitle = {};
  storyEntries(index, impl).forEach(function (entry) {
    if (!byTitle[entry.title]) byTitle[entry.title] = [];
    byTitle[entry.title].push(entry);
  });

  var sections = [];
  var bySection = {};
  var screens = {};

  Object.keys(byTitle).forEach(function (title) {
    var entries = byTitle[title];
    var parts = title.split('/').filter(Boolean);
    var sectionName = parts.length > 1 ? parts.shift() : impl.label;
    var label = parts.length ? parts.pop() : title;
    var folderName = parts.join(' / ');
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

    var section = bySection[sectionName];
    if (!section) {
      section = { group: sectionName, icon: catalogIcon(impl, sectionName), items: [] };
      bySection[sectionName] = section;
      sections.push(section);
    }
    if (folderName) {
      var folder = section.items.find(function (candidate) { return candidate.folder === folderName; });
      if (!folder) {
        folder = { folder: folderName, items: [] };
        section.items.push(folder);
      }
      folder.items.push(item);
    } else {
      section.items.push(item);
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
    screens[src] = {
      label: label, design: null, code: code,
      stories: entries.map(function (entry) { return { id: entry.id, state: manifest.storyState(entry.id), label: entry.name || manifest.storyState(entry.id) }; }),
    };
  });

  return { sections: sections, screens: screens };
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
  var section = { group: impl.label, icon: impl.catalogIcon || 'smartphone', items: [] };
  var screens = {};
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
    section.items.push(item);
    screens[src] = { label: device.name, design: null, code: [], simulator: device };
  });
  return { sections: selected.length ? [section] : [], screens: screens };
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
    send(res, 200, body, TYPES[ext] || 'application/octet-stream');
  });
}

/* Starts the server. `onShot` is called with the workspace-relative path of
   every screenshot written, so the editor can say so. `onHandoff` is given the
   composed prompt and the canvas it came from so the editor can copy it.
   `onOpen` is handed an absolute path the config resolves to — a screen's
   design file, or where its implementation's code is — and puts it in front
   of the user. */
function start(options) {
  var root = path.resolve(options.root);
  var createCapture = electronCapture.available() ? electronCapture.createWithFallback : nativeCapture.create;
  var capture = options.capture || createCapture({ chromePath: options.chromePath, inject: DESCRIBE_SOURCE, storage: options.captureStorage });
  var captureReady = Promise.resolve();
  var previews = null;
  var previewSettings = null;
  var windowVideo = options.windowStream || windowStream.create({
    storage: path.join(options.captureStorage || path.join(os.tmpdir(), 'canonic-workbench-capture'), 'window-capture'),
    path: WINDOW_STREAM_PATH,
    permissionOwner: options.screenCapturePermissionOwner,
  });
  var onShot = options.onShot || function () {};
  var onLog = options.onLog || function () {};
  var logLimit = options.logLimit || MAX_LOG_SESSION;
  var loggedBytes = 0;
  var logLimitReported = false;
  var hasHandoff = typeof options.onHandoff === 'function';
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
    return config.resolve(root, config.read(root));
  }

  /* Catalog imports are the one asynchronous part of configuration: the
     manifest names the Storybook, then its live index supplies the screens.
     A stopped catalog leaves manual screens usable and reports one problem. */
  function resolvedWithCatalogs() {
    var view = resolved();
    if (!view) return Promise.resolve(null);
    var sections = [];
    var screens = Object.assign({}, view.screens);
    var problems = view.problems.slice();
    var implementations = Object.assign({}, view.implementations);
    var autoKeys = Object.keys(implementations).filter(function (key) {
      return implementations[key].kind === 'storybook' && implementations[key].auto;
    });
    return importPreviews(view, sections, screens, problems, implementations).then(function () {
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
          imported.sections.forEach(function (section) {
            var existing = sections.find(function (candidate) { return candidate.group === section.group; });
            if (existing) existing.items = existing.items.concat(section.items);
            else sections.push(section);
          });
          Object.assign(screens, imported.screens);
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
          sections = sections.concat(imported.sections);
          Object.assign(screens, imported.screens);
          if (!imported.sections.length) {
            problems.push('implementations › ' + key + ': no matching booted iOS Simulator was found.');
          }
        });
      }).catch(function (error) {
        problems.push('iOS Simulator: ' + String(error.message || error));
      });
    }).then(function () {
      return Object.assign({}, view, {
        implementations: implementations,
        catalogSections: sections,
        screens: screens,
        problems: problems,
      });
    });
    });
  }

  async function importPreviews(view, sections, screens, problems, implementations) {
    if (view.previews === false) {
      if (previews) { await previews.close(); previews = null; previewSettings = null; }
      return;
    }
    if (options.isTrusted === false) {
      if (previewCompiler.discover(root, view.previews && view.previews.include).length) problems.push('Workbench previews require a trusted workspace.');
      return;
    }
    if (!previewCompiler.discover(root, view.previews && view.previews.include).length) return;
    try {
      var settings = JSON.stringify(view.previews || {});
      if (previews && settings !== previewSettings) { await previews.close(); previews = null; }
      if (!previews) {
        previews = previewService.create(root, { previews: view.previews, storage: options.captureStorage,
          diagnostic: diagnostic,
          log: function (message) { diagnostic('warn', 'preview.worker', { message: message }); } });
        previewSettings = settings;
      }
      var index = await previews.index();
      problems.push.apply(problems, index.errors);
      var byId = {};
      index.previews.forEach(function (preview) {
        byId[preview.id] = preview;
        var parts = preview.title.split('/').filter(Boolean);
        var group = parts.length > 1 ? parts.shift() : 'Previews';
        var label = parts.pop() || preview.id;
        var item = { src: preview.file, label: label, states: preview.states, workbench: true, icon: 'component' };
        if (preview.viewports) item.viewports = preview.viewports;
        var section = sections.find(function (section) { return section.group === group; });
        if (!section) { section = { group: group, icon: 'component', items: [] }; sections.push(section); }
        var folderName = parts.join(' / ');
        if (folderName) {
          var folder = section.items.find(function (item) { return item.folder === folderName; });
          if (!folder) { folder = { folder: folderName, items: [] }; section.items.push(folder); }
          folder.items.push(item);
        } else section.items.push(item);
        var previous = screens[preview.file];
        screens[preview.file] = Object.assign({}, previous, { label: previous && previous.label || label,
          design: path.join(root, preview.file), preview: preview, code: (previous && previous.code || []).concat([
            { implementation: 'workbench', path: path.join(root, preview.source), relative: preview.source, exists: true },
          ]) });
      });
      Object.keys(implementations).forEach(function (key) {
        if (implementations[key].kind !== 'workbench') return;
        implementations[key].base = '';
        if (implementations[key].root !== root) problems.push('Workbench implementations use this project root; use previews.config to configure adapters.');
        screenItems(view.sections).forEach(function (item) {
          var reference = item.implementations && item.implementations[key];
          if (!reference) return;
          var preview = byId[reference.preview];
          if (!preview) { problems.push(item.label + ': unknown Workbench preview “' + reference.preview + '”.'); return; }
          reference.path = '/' + preview.file;
          reference.states = {};
          preview.states.forEach(function (state) { reference.states[state.id] = '/' + preview.file + '?state=' + encodeURIComponent(state.id); });
          screens[item.src].code.push({ implementation: key, path: path.join(root, preview.source), relative: preview.source, exists: true });
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
    }).filter(Boolean);
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
      return captureExportReferences(capture, plan, baseUrl, function (completed) {
        job.completed = completed;
        if (completed === 1 || (completed > 0 && completed % 100 === 0)) {
          diagnostic('info', 'export.progress', { completed: completed, total: job.total });
        }
      });
    }).then(async function (references) {
      var portable = previews ? await previews.export() : null;
      job.warnings = references.warnings.length + (portable ? portable.warnings.length : 0);
      job.archive = designExport.create(root, view, {
        portable: portable,
        screenshots: references.screenshots,
        captureWarnings: references.warnings,
        maxArchiveBytes: options.exportMaxArchiveBytes,
      });
      job.completed = job.total;
      job.current = null;
      job.status = 'complete';
      diagnostic('info', 'export.completed', {
        files: job.archive.report.files.length,
        screenshots: references.screenshots.length,
        archives: job.archive.archives.length,
        warnings: job.archive.report.warnings.length + references.warnings.length,
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
    if (url.pathname === WORKBENCH_PREFIX + 'preview-runtime.js') {
      serveFile(path.join(__dirname, 'preview'), '/browser.js', res);
      return;
    }

    if (url.pathname.indexOf('/_workbench/previews/') === 0 || /\.workbench\.tsx?$/.test(url.pathname)) {
      if (req.method !== 'GET') { send(res, 405, 'GET a Workbench preview here.'); return; }
      // Assets belong to an already compiled preview. Recheck configuration
      // and trust, but don't rebuild every catalog for each CSS/image/script.
      if (url.pathname.indexOf('/_workbench/previews/') === 0 && previews) {
        try {
          var previewView = config.read(root);
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
        if (!view.screens[relative] || !view.screens[relative].preview) throw refused(404, 'This preview is not in the catalog.');
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

    /* The form editor changes only the committed sections block. The rest of
       the file, including implementations and its comments, stays untouched. */
    if (url.pathname === CONFIG_FILE_PATH) {
      if (req.method === 'GET') {
        try {
          var configSource = config.source(root);
          if (!configSource) json(res, 404, { ok: false, error: 'There is no ' + config.FILE + ' at the project root.' });
          else json(res, 200, { ok: true, sections: configSource.raw.sections || [] });
        } catch (error) { trouble(res, error, 400); }
        return;
      }
      if (req.method !== 'POST') { send(res, 405, 'GET the page form or POST its sections here.'); return; }
      readBody(req, MAX_HANDOFF)
        .then(function (body) {
          var ask = JSON.parse(body.toString('utf8'));
          var saved = config.updateSections(root, ask.sections);
          json(res, 200, { ok: true, sections: saved.raw.sections || [] });
        })
        .catch(function (error) { trouble(res, error, 400); });
      return;
    }

    /* What this machine makes of the config: every path absolute, every
       problem named. The workbench reads it for the source menu and the
       handoff; `curl` reads it to check a setup. */
    if (url.pathname === CONFIG_PATH) {
      Promise.resolve()
        .then(resolvedWithCatalogs)
        .then(function (view) {
          if (!view) json(res, 404, { ok: false, error: 'There is no ' + config.FILE + ' at the project root.' });
          else json(res, 200, Object.assign({ ok: true }, view));
        })
        .catch(function (err) { trouble(res, err, 500); });
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

    /* A window lens: stream the window a screen names from the app its
       implementation declares. The page only says which screen; the app and
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
            var screen = view && Object.prototype.hasOwnProperty.call(view.screens, src) ? view.screens[src] : null;
            var title = screen && screen.windows && screen.windows[key];
            if (!impl || impl.kind !== 'window' || !title) {
              throw refused(403, 'that window isn’t declared by this workbench');
            }
            if (ask.stop) {
              return Promise.resolve(windowVideo.stop()).then(function () {
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
          return resolvedWithCatalogs().then(function (view) {
            var key = String(ask.implementation || '');
            var udid = String(ask.udid || '');
            var impl = view && view.implementations[key];
            var allowed = view && Object.keys(view.screens).some(function (src) {
              return src.indexOf('__ios-simulator/' + key + '/') === 0
                && view.screens[src].simulator
                && view.screens[src].simulator.udid === udid;
            });
            if (!impl || impl.kind !== 'ios-simulator' || !allowed) {
              throw refused(403, 'that Simulator isn’t declared by this workbench');
            }
            if (url.pathname === SIMULATOR_STREAM_PATH) {
              if (ask.stop) {
                return Promise.resolve(windowVideo.stop()).then(function () {
                  json(res, 200, { ok: true, stopped: true });
                });
              }
              return Promise.resolve(windowVideo.start({
                source: view.screens[Object.keys(view.screens).find(function (src) {
                  return src.indexOf('__ios-simulator/' + key + '/') === 0
                    && view.screens[src].simulator && view.screens[src].simulator.udid === udid;
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

    /* Open a screen's source in the editor: its design file, or one of the
       places its implementation lives. Only paths the config resolves to —
       a page in the frame could POST here, and it is not getting to name a
       file on this machine. */
    if (url.pathname === OPEN_PATH) {
      if (req.method !== 'POST') {
        send(res, 405, 'POST the screen here.');
        return;
      }
      readBody(req, MAX_HANDOFF)
        .then(function (body) {
          var ask = JSON.parse(body.toString('utf8'));
          return resolvedWithCatalogs().then(function (view) {
            var screen = view && view.screens[String(ask.src || '')];
            if (!screen) throw refused(404, 'that screen isn’t in the workbench');
            var file = null;
            if (!ask.path) file = screen.design;
            else {
              screen.code.forEach(function (entry) {
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
              sendImage(res, png, payload.format);
              return;
            }
            var file = saveShot(root, payload.name, png, payload.format);
            onShot(file);
            json(res, 200, { ok: true, file: file, targets: (shot && shot.targets) || null });
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
          var text = handoff.prompt(canvas);
          return Promise.resolve(onHandoff(text, canvas)).then(function () {
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

    /* The tool, out of the extension. Its own paths are relative, so keeping
       the leading slash is what makes /_workbench/ mean the folder's root. */
    if (url.pathname.indexOf(WORKBENCH_PREFIX) === 0) {
      serveFile(WORKBENCH_DIR, url.pathname.slice(WORKBENCH_PREFIX.length - 1), res);
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

      close: function () {
        diagnostic('info', 'session.stopping', { port: port });
        var stops = Object.keys(simulatorSessions).map(function (udid) {
          return Promise.resolve(simulatorSessions[udid]).then(function (session) {
            return runIosCli([
              'stop', '--session', session.sessionName, '--state-dir', simulatorStateDir, '--teardown',
            ], 30000);
          }).catch(function () {});
        });
        return Promise.all(stops).then(function () {
          return Promise.all([
            Promise.resolve(capture.close()).catch(function () {}),
            Promise.resolve(windowVideo.close()).catch(function () {}),
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
  LOG_PATH: LOG_PATH,
  OPEN_PATH: OPEN_PATH,
  SHOT_PATH: SHOT_PATH,
  STORIES_PATH: STORIES_PATH,
  EXPORT_PATH: EXPORT_PATH,
  SIMULATOR_PATH: SIMULATOR_PATH,
  SIMULATOR_INPUT_PATH: SIMULATOR_INPUT_PATH,
  SIMULATOR_STREAM_PATH: SIMULATOR_STREAM_PATH,
  WINDOW_STREAM_PATH: WINDOW_STREAM_PATH,
  storybookPorts: storybookPorts,
  simulatorCatalog: simulatorCatalog,
  withPreviewScripts: withPreviewScripts,
};

/* `node server.js <folder>` — the same server the extension runs, for when
   you want it without the editor. */
if (require.main === module) {
  start({
    root: process.argv[2] || process.cwd(),
    eagerCapture: true,
    onShot: function (file) {
      console.log('saved ' + file);
    },
  }).then(
    function (running) {
      console.log('workbench on ' + running.url);
    },
    function (err) {
      console.error(String(err.message || err));
      process.exit(1);
    }
  );
}
