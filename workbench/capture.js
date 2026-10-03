/* Chromium, driven
   ----------------
   The screenshot fallback uses this private Chrome pipe. Packaged desktop
   screenshots use electron-capture.js instead; interactive previews use
   ordinary iframes in the workbench.

   `Capture` keeps one headless page warm and reads its compositor surface.
   It speaks the DevTools protocol over Chrome's private pipe. `Browser` is
   the process and its profile, `Target` is one page inside it, `Pipe` is the
   wire; `Capture` puts the first two together for the screenshot case and
   keeps the interface the server has always used.

   A missing or incompatible browser rejects cleanly; the caller keeps the
   in-page renderer as its fallback. */

var childProcess = require('child_process');
var crypto = require('crypto');
var fs = require('fs');
var os = require('os');
var path = require('path');

var COMMAND_TIMEOUT = 15000;
var CAPTURE_PAGE = '/_workbench/capture.html';

function firstFile(candidates) {
  for (var i = 0; i < candidates.length; i++) {
    if (candidates[i] && fs.existsSync(candidates[i])) return candidates[i];
  }
  return null;
}

function chromeCandidates(env, platform) {
  env = env || process.env;
  platform = platform || process.platform;
  var configured = env.CANONIC_CHROME_PATH;

  if (platform === 'darwin') {
    return [
      configured,
      '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
      '/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary',
      '/Applications/Chromium.app/Contents/MacOS/Chromium',
      '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
    ];
  }

  if (platform === 'win32') {
    return [
      configured,
      env.PROGRAMFILES && path.join(env.PROGRAMFILES, 'Google/Chrome/Application/chrome.exe'),
      env['PROGRAMFILES(X86)'] && path.join(env['PROGRAMFILES(X86)'], 'Google/Chrome/Application/chrome.exe'),
      env.LOCALAPPDATA && path.join(env.LOCALAPPDATA, 'Google/Chrome/Application/chrome.exe'),
      env.PROGRAMFILES && path.join(env.PROGRAMFILES, 'Microsoft/Edge/Application/msedge.exe'),
      env['PROGRAMFILES(X86)'] && path.join(env['PROGRAMFILES(X86)'], 'Microsoft/Edge/Application/msedge.exe'),
    ];
  }

  return [
    configured,
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/usr/bin/microsoft-edge',
    '/usr/bin/microsoft-edge-stable',
  ];
}

function findChrome(options) {
  options = options || {};
  return firstFile(
    options.chromePath
      ? [options.chromePath]
      : chromeCandidates(options.env, options.platform)
  );
}

function messageOf(error) {
  if (!error) return 'unknown browser error';
  return String(error.message || error);
}

function removeProfile(profile) {
  if (!profile) return;
  try {
    fs.rmSync(profile, { recursive: true, force: true });
  } catch (error) {}
}

function stopProcess(child, profile) {
  if (!child || child.exitCode !== null) {
    removeProfile(profile);
    return Promise.resolve();
  }

  return new Promise(function (resolve) {
    var done = false;
    function finish() {
      if (done) return;
      done = true;
      clearTimeout(timer);
      resolve();
    }
    child.once('exit', finish);
    var timer = setTimeout(function () {
      if (!child.killed) child.kill('SIGKILL');
      finish();
    }, 1500);
    if (!child.killed) child.kill();
  }).then(function () {
    removeProfile(profile);
  });
}

/* Chrome's --remote-debugging-pipe uses fd 3 for commands, fd 4 for replies,
   and a NUL between JSON messages. Session ids let one pipe carry both the
   browser commands and each page's Page/Runtime domains. */
function Pipe(child) {
  this.child = child;
  this.input = child.stdio[3];
  this.output = child.stdio[4];
  this.buffer = '';
  this.nextId = 1;
  this.pending = new Map();
  this.listeners = [];
  this.watchers = [];
  this.closed = false;

  var self = this;
  this.output.setEncoding('utf8');
  this.output.on('data', function (chunk) {
    self.read(chunk);
  });
  this.output.on('error', function (error) {
    self.fail(error);
  });
  child.once('exit', function (code, signal) {
    self.fail(new Error('Chromium exited' + (signal ? ' with ' + signal : ' with code ' + code)));
  });
  child.once('error', function (error) {
    self.fail(error);
  });
}

Pipe.prototype.read = function (chunk) {
  this.buffer += chunk;
  var at = this.buffer.indexOf('\0');
  while (at > -1) {
    var raw = this.buffer.slice(0, at);
    this.buffer = this.buffer.slice(at + 1);
    if (raw) {
      try {
        this.receive(JSON.parse(raw));
      } catch (error) {
        this.fail(new Error('Chromium sent an invalid DevTools message'));
      }
    }
    at = this.buffer.indexOf('\0');
  }
};

Pipe.prototype.receive = function (message) {
  if (message.id) {
    var pending = this.pending.get(message.id);
    if (!pending) return;
    this.pending.delete(message.id);
    clearTimeout(pending.timer);
    if (message.error) pending.reject(new Error(message.error.message || 'DevTools command failed'));
    else pending.resolve(message.result || {});
    return;
  }

  this.listeners.slice().forEach(function (listener) {
    if (listener.method !== message.method) return;
    if (listener.sessionId && listener.sessionId !== message.sessionId) return;
    listener.finish(message.params || {});
  });
  this.watchers.slice().forEach(function (watcher) {
    if (watcher.method !== message.method) return;
    if (watcher.sessionId && watcher.sessionId !== message.sessionId) return;
    watcher.fn(message.params || {});
  });
};

/* `options.timeout` for commands that must answer quickly or not at all —
   input a page is ignoring shouldn't hold a queue for fifteen seconds. */
Pipe.prototype.send = function (method, params, sessionId, options) {
  if (this.closed) return Promise.reject(new Error('Chromium pipe is closed'));
  var id = this.nextId++;
  var message = { id: id, method: method };
  if (params) message.params = params;
  if (sessionId) message.sessionId = sessionId;
  var timeout = (options && options.timeout) || COMMAND_TIMEOUT;

  var self = this;
  return new Promise(function (resolve, reject) {
    var timer = setTimeout(function () {
      self.pending.delete(id);
      reject(new Error('Chromium timed out during ' + method));
    }, timeout);
    self.pending.set(id, { resolve: resolve, reject: reject, timer: timer });
    self.input.write(JSON.stringify(message) + '\0', function (error) {
      if (!error) return;
      clearTimeout(timer);
      self.pending.delete(id);
      reject(error);
    });
  });
};

/* The next such event, once. */
Pipe.prototype.waitFor = function (method, sessionId) {
  var self = this;
  return new Promise(function (resolve, reject) {
    var listener = {
      method: method,
      sessionId: sessionId,
      finish: function (params) {
        clearTimeout(timer);
        self.listeners.splice(self.listeners.indexOf(listener), 1);
        resolve(params);
      },
      reject: reject,
    };
    var timer = setTimeout(function () {
      self.listeners.splice(self.listeners.indexOf(listener), 1);
      reject(new Error('Chromium timed out waiting for ' + method));
    }, COMMAND_TIMEOUT);
    self.listeners.push(listener);
  });
};

/* Every such event, until told to stop — a screencast is nothing but. When
   the pipe dies the watcher hears about it once, as `error`, and is dropped.
   Answers with the way to stop. */
Pipe.prototype.on = function (method, sessionId, fn, onError) {
  var self = this;
  var watcher = { method: method, sessionId: sessionId, fn: fn, onError: onError || null };
  this.watchers.push(watcher);
  return function () {
    var at = self.watchers.indexOf(watcher);
    if (at > -1) self.watchers.splice(at, 1);
  };
};

Pipe.prototype.fail = function (error) {
  if (this.closed) return;
  this.closed = true;
  this.pending.forEach(function (pending) {
    clearTimeout(pending.timer);
    pending.reject(error);
  });
  this.pending.clear();
  this.listeners.splice(0).forEach(function (listener) {
    listener.reject(error);
  });
  this.watchers.splice(0).forEach(function (watcher) {
    if (watcher.onError) watcher.onError(error);
  });
};

/* One page in the browser, with its own session on the pipe. */
function Target(browser, targetId, sessionId) {
  this.browser = browser;
  this.pipe = browser.pipe;
  this.targetId = targetId;
  this.sessionId = sessionId;
  this.pageUrl = null;
}

Target.prototype.send = function (method, params, options) {
  return this.pipe.send(method, params, this.sessionId, options);
};

Target.prototype.waitFor = function (method) {
  return this.pipe.waitFor(method, this.sessionId);
};

Target.prototype.on = function (method, fn, onError) {
  return this.pipe.on(method, this.sessionId, fn, onError);
};

Target.prototype.alive = function () {
  return !!this.pipe && !this.pipe.closed;
};

/* Page and Runtime, plus reduced motion so nothing is caught mid-animation. */
Target.prototype.enable = function () {
  return Promise.all([
    this.send('Page.enable'),
    this.send('Runtime.enable'),
    this.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] }),
  ]);
};

/* Loaded, meaning the load event has fired. The same address twice is a
   no-op; `pageUrl = null` first forces a fresh load. */
Target.prototype.navigate = function (url) {
  if (this.pageUrl === url) return Promise.resolve();
  var loaded = this.waitFor('Page.loadEventFired');
  var self = this;
  return this.send('Page.navigate', { url: url })
    .then(function () {
      return loaded;
    })
    .then(function () {
      self.pageUrl = url;
    });
};

Target.prototype.evaluate = function (expression) {
  return this.send('Runtime.evaluate', { expression: expression, awaitPromise: true, returnByValue: true })
    .then(function (result) {
      if (result.exceptionDetails) {
        var exception = result.exceptionDetails.exception;
        throw new Error(
          (exception && exception.description) || result.exceptionDetails.text || 'The page threw'
        );
      }
      return result.result && result.result.value;
    });
};

Target.prototype.screenshot = function (format) {
  var self = this;
  var options = { format: format === 'jpeg' ? 'jpeg' : 'png', fromSurface: true, captureBeyondViewport: false, optimizeForSpeed: true };
  if (options.format === 'jpeg') options.quality = 90;
  return this.send('Page.captureScreenshot', options).catch(function (error) {
    /* optimizeForSpeed is newer than the rest of the command. Older Chromium
       builds still provide native capture without it. */
    if (!/optimizeForSpeed|Invalid parameters/i.test(messageOf(error))) throw error;
    delete options.optimizeForSpeed;
    return self.send('Page.captureScreenshot', options);
  }).then(function (result) {
    if (!result.data) throw new Error('Chromium returned an empty screenshot');
    return Buffer.from(result.data, 'base64');
  });
};

Target.prototype.close = function () {
  var self = this;
  if (!this.alive()) return Promise.resolve();
  return this.pipe.send('Target.closeTarget', { targetId: this.targetId }).catch(function () {}).then(function () {
    self.pageUrl = null;
  });
};

/* The process. `options.profile` names a folder to keep between runs — a
   browser that remembers its sign-ins — and is left alone on close; without
   one a temporary profile is made and removed. `options.headless` is on
   unless said otherwise; `options.spawn` stands in for child_process in
   tests. */
function Browser(options) {
  this.options = options || {};
  this.child = null;
  this.pipe = null;
  this.profile = null;
  this.ownsProfile = false;
  this.launching = null;
  this.closed = false;
  this.stderr = '';
}

Browser.prototype.alive = function () {
  return !!this.pipe && !this.pipe.closed;
};

Browser.prototype.reset = function () {
  this.child = null;
  this.pipe = null;
  this.profile = null;
  this.ownsProfile = false;
};

Browser.prototype.arguments = function () {
  var args = [];
  if (this.options.headless !== false) args.push('--headless=new');
  args.push(
    '--remote-debugging-pipe',
    '--user-data-dir=' + this.profile,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-extensions'
  );
  if (typeof process.getuid === 'function' && process.getuid() === 0) args.push('--no-sandbox');
  args.push('about:blank');
  return args;
};

Browser.prototype.launch = function () {
  if (this.alive()) return Promise.resolve();
  if (this.launching) return this.launching;
  if (this.closed) return Promise.reject(new Error('the browser is closed'));

  /* A crashed browser leaves its object and temporary profile behind until
     the next request. Clear both before replacing it. */
  if (this.child || this.profile) {
    if (this.child && !this.child.killed) this.child.kill();
    if (this.ownsProfile) removeProfile(this.profile);
    this.reset();
  }

  var executable = findChrome(this.options);
  if (!executable) {
    return Promise.reject(
      new Error('Chrome or Chromium was not found; set CANONIC_CHROME_PATH to enable native capture')
    );
  }

  if (this.options.profile) {
    fs.mkdirSync(this.options.profile, { recursive: true });
    this.profile = this.options.profile;
    this.ownsProfile = false;
  } else {
    this.profile = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-capture-'));
    this.ownsProfile = true;
  }
  this.stderr = '';

  var spawn = this.options.spawn || childProcess.spawn;
  this.child = spawn(executable, this.arguments(), {
    stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'],
  });
  this.pipe = new Pipe(this.child);

  var self = this;
  if (this.child.stderr) {
    this.child.stderr.setEncoding('utf8');
    this.child.stderr.on('data', function (chunk) {
      self.stderr = (self.stderr + chunk).slice(-8192);
    });
  }

  /* The first exchange is the test that the browser is there and talking. */
  this.launching = this.pipe
    .send('Target.getTargets')
    .then(function () {})
    .catch(function (error) {
      var detail = self.stderr.trim();
      var child = self.child;
      var profile = self.ownsProfile ? self.profile : null;
      self.reset();
      return stopProcess(child, profile).then(function () {
        throw new Error(messageOf(error) + (detail ? ': ' + detail : ''));
      });
    })
    .finally(function () {
      self.launching = null;
    });

  return this.launching;
};

Browser.prototype.createTarget = function (url) {
  var self = this;
  var targetId = null;
  return this.pipe
    .send('Target.createTarget', { url: url || 'about:blank' })
    .then(function (created) {
      targetId = created.targetId;
      return self.pipe.send('Target.attachToTarget', { targetId: created.targetId, flatten: true });
    })
    .then(function (attached) {
      var target = new Target(self, targetId, attached.sessionId);
      return target.enable().then(function () {
        return target;
      });
    });
};

Browser.prototype.close = function () {
  this.closed = true;
  var child = this.child;
  var pipe = this.pipe;
  var profile = this.ownsProfile ? this.profile : null;
  this.reset();

  var closing = pipe && !pipe.closed
    ? pipe.send('Browser.close').catch(function () {})
    : Promise.resolve();

  return closing.finally(function () {
    return stopProcess(child, profile);
  });
};

/* Native screenshots
   ------------------
   One headless browser on a throwaway profile, one page kept warm on the
   workbench's capture surface. The browser is lazy: the workbench's prepare
   request starts it while the user is looking at a screen. Everything goes
   through one queue, so a capture never lands while a prepare is halfway. */
function Capture(options) {
  this.options = options || {};
  this.browser = new Browser({
    chromePath: this.options.chromePath,
    env: this.options.env,
    platform: this.options.platform,
  });
  this.target = null;
  this.closed = false;
  this.queue = Promise.resolve();
}

Capture.prototype.serial = function (work) {
  var next = this.queue.catch(function () {}).then(work);
  this.queue = next.catch(function () {});
  return next;
};

/* A browser and a page in it, made when there isn't one. The element-describing
   helper is installed here, once per page, for screenshot handoffs. */
Capture.prototype.launch = function () {
  if (this.closed) return Promise.reject(new Error('Chromium capture is closed'));
  var self = this;
  return this.browser.launch().then(function () {
    if (self.target && self.target.alive()) return;
    self.target = null;
    return self.browser.createTarget('about:blank').then(function (target) {
      self.target = target;
      if (!self.options.inject) return;
      return target.send('Page.addScriptToEvaluateOnNewDocument', { source: self.options.inject });
    });
  });
};

Capture.prototype.metrics = function (width, height) {
  return this.target.send('Emulation.setDeviceMetricsOverride', {
    width: width, height: height, deviceScaleFactor: 1, mobile: false, screenWidth: width, screenHeight: height,
  });
};

Capture.prototype.restorePointer = function (payload) {
  if (!payload.mirror) return Promise.resolve();
  var point = payload.mirror.pointer;
  var x = point ? point.x : payload.width + 1;
  var y = point ? point.y : payload.height + 1;
  var self = this;
  return this.target.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x, y: y }).then(function () {
    return self.target.evaluate('new Promise(function (resolve) { requestAnimationFrame(function () { requestAnimationFrame(resolve); }); })');
  });
};

Capture.prototype.prepareNow = function (baseUrl, payload) {
  var width = Math.max(1, Math.round(Number(payload.width) || 0));
  var height = Math.max(1, Math.round(Number(payload.height) || 0));
  var page = new URL(CAPTURE_PAGE, baseUrl).toString();
  var self = this;

  return this.launch()
    .then(function () {
      return self.metrics(width, height);
    })
    .then(function () {
      return self.target.navigate(page);
    })
    .then(function () {
      return self.target.evaluate('window.wbCapture.prepare(' + JSON.stringify(payload) + ')');
    }).then(function (details) {
      return self.restorePointer(payload).then(function () { return details; });
    });
};

Capture.prototype.prepare = function (baseUrl, payload) {
  var self = this;
  return this.serial(function () {
    return self.prepareNow(baseUrl, payload);
  });
};

Capture.prototype.warm = function (baseUrl) {
  var self = this;
  return this.serial(function () {
    return self.launch().then(function () {
      return self.target.navigate(new URL(CAPTURE_PAGE, baseUrl).toString());
    });
  });
};

Capture.prototype.capture = function (baseUrl, payload) {
  var self = this;
  return this.serial(function () {
    return self.prepareNow(baseUrl, payload).then(function (details) {
      return self.target.screenshot(payload.format).then(function (image) {
        Object.defineProperty(image, 'captureDetails', { value: details });
        return image;
      });
    });
  });
};

/* A page from somewhere else
   --------------------------
   An implementation lens shows a page the workbench doesn't serve — a dev
   server's, a Storybook's, staging's — and the same-origin routes above
   can't read into it. So the browser goes there itself: same viewport as the
   frame, the same scroll, and the marks laid over the top by a layer put into
   the page for the moment of the shot. `css` is the markup stylesheet, which
   is what makes the marks paint the way they do on the canvas.

   A prepared revision reuses its page; a fresh revision or an API request
   without one reloads. Whatever session
   the temporary profile lacks, it lacks — a page behind sign-in comes back as
   its sign-in page.

   `anchors` are the points under the marks; the answer names the element at
   each, by way of the helper put into the page at launch, so the handoff can
   say what a mark is on even here. */
var OVERLAY_ID = '__wb_markup';

function overlayScript(payload) {
  return '(function () {' +
    'var s = ' + JSON.stringify(payload.scroll || null) + ';' +
    'if (s) {' +
    '  var r = document.documentElement, v = r.style.getPropertyValue("scroll-behavior"), p = r.style.getPropertyPriority("scroll-behavior");' +
    '  r.style.setProperty("scroll-behavior", "auto", "important");' +
    '  window.scrollTo(s.x, s.y);' +
    '  if (v) r.style.setProperty("scroll-behavior", v, p); else r.style.removeProperty("scroll-behavior");' +
    '}' +
    'var old = document.getElementById(' + JSON.stringify(OVERLAY_ID) + ');' +
    'if (old) old.remove();' +
    'var o = document.createElement("div");' +
    'o.id = ' + JSON.stringify(OVERLAY_ID) + ';' +
    'o.className = "wb wb-markup-layer";' +
    'o.style.cssText = "position:fixed;inset:0;overflow:hidden;pointer-events:none;z-index:2147483647";' +
    'o.innerHTML = ' + JSON.stringify('<style>' + (payload.css || '') + '</style>' + (payload.markup || '')) + ';' +
    'document.documentElement.appendChild(o);' +
    'var fonts = document.fonts && document.fonts.ready ? document.fonts.ready : Promise.resolve();' +
    'return fonts.catch(function () {}).then(function () {' +
    '  return new Promise(function (done) {' +
    '    requestAnimationFrame(function () { requestAnimationFrame(function () {' +
    '      done({ scroll: { requestedX: s ? Number(s.x) || 0 : 0, requestedY: s ? Number(s.y) || 0 : 0,' +
    '        appliedX: window.scrollX || window.pageXOffset || 0, appliedY: window.scrollY || window.pageYOffset || 0 } });' +
    '    }); });' +
    '  });' +
    '});' +
    '})()';
}

var REMOVE_OVERLAY = '(function () { var o = document.getElementById(' + JSON.stringify(OVERLAY_ID) + '); if (o) o.remove(); return true; })()';

function describeScript(anchors) {
  return '(function () {' +
    'var at = ' + JSON.stringify(anchors || []) + ';' +
    'return at.map(function (p) {' +
    '  if (!p || typeof window.__wbDescribeAt !== "function") return null;' +
    '  try { return window.__wbDescribeAt(p.x, p.y); } catch (e) { return null; }' +
    '});' +
    '})()';
}

/* A load event only means Storybook's preview shell arrived. Its story is
   rendered later, while #storybook-root is hidden behind .sb-loader. Wait for
   that hand-off and then for a short quiet window so async components, fonts,
   and visible images have painted before pixels are read. */
function settleScript(options) {
  options = options || {};
  var imageTimeout = Number.isFinite(options.imageTimeout) ? Math.max(0, options.imageTimeout) : 2000;
  var quiet = options.quiet !== false;
  var quietScript = quiet ? `
    var changed = Date.now();
    var observer = new MutationObserver(function () { changed = Date.now(); });
    observer.observe(document.documentElement, { attributes: true, childList: true, characterData: true, subtree: true });
    var quietDeadline = Date.now() + 1500;
    while (Date.now() - changed < 250 && Date.now() < quietDeadline) await sleep(50);
    observer.disconnect();` : '';
  return `(async function () {
    function sleep(ms) { return new Promise(function (resolve) { setTimeout(resolve, ms); }); }
    function visible(node) {
      if (!node || node.hidden) return false;
      var style = getComputedStyle(node);
      if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0) return false;
      var box = node.getBoundingClientRect();
      return box.width > 0 && box.height > 0;
    }
    var storybook = location.pathname.replace(/\\/+$/, '').endsWith('/iframe.html') || location.pathname === '/iframe.html';
    if (/\\.workbench\\.tsx?$/.test(location.pathname) || window.__workbenchOptions) {
      var previewDeadline = Date.now() + 8000;
      while (!window.__workbenchReady) {
        if (window.__workbenchError) throw new Error(window.__workbenchError);
        if (Date.now() >= previewDeadline) throw new Error('Workbench preview did not finish rendering before capture');
        await sleep(50);
      }
    }
    if (storybook) {
      var deadline = Date.now() + 8000;
      while (true) {
        var loader = Array.from(document.querySelectorAll('.sb-loader, .sb-preparing-story, .sb-preparing-docs')).some(visible);
        var failure = Array.from(document.querySelectorAll('.sb-errordisplay, .sb-nopreview')).some(visible);
        var main = document.body.classList.contains('sb-show-main');
        if (failure || (main && !loader)) break;
        if (Date.now() >= deadline) throw new Error('Storybook did not finish rendering before capture');
        await sleep(50);
      }
    }
    if (document.fonts && document.fonts.ready) await document.fonts.ready.catch(function () {});
    var images = Promise.all(Array.from(document.images).filter(function (image) {
      return visible(image) && !image.complete;
    }).map(function (image) {
      return image.decode ? image.decode().catch(function () {}) : Promise.resolve();
    }));
    await Promise.race([images, sleep(${imageTimeout})]);
    ${quietScript}
    await new Promise(function (resolve) { requestAnimationFrame(function () { requestAnimationFrame(resolve); }); });
    return { storybook: storybook };
  })()`;
}

function storybookPreview(value) {
  try {
    var url = new URL(value);
    if (url.pathname.replace(/\/+$/, '') !== '/iframe.html' && !url.pathname.endsWith('/iframe.html')) return null;
    return { document: url.origin + url.pathname, id: url.searchParams.get('id') || '' };
  } catch (_) { return null; }
}

function canSwitchStorybook(from, to) {
  var current = storybookPreview(from);
  var next = storybookPreview(to);
  return !!(current && next && current.document === next.document);
}

function storybookSwitchScript(url) {
  var story = storybookPreview(url);
  if (!story) return 'false';
  return `(function () {
    var root = document.getElementById('storybook-root');
    var docs = document.getElementById('storybook-docs');
    if (root) root.hidden = true;
    if (docs) docs.hidden = true;
    document.body.classList.remove('sb-show-main');
    document.body.classList.add('sb-show-preparing-story');
    history.replaceState(null, '', ${JSON.stringify(url)});
    window.postMessage(JSON.stringify({ key: 'storybook-channel', event: {
      type: 'setCurrentStory', args: [{ storyId: ${JSON.stringify(story.id)}, viewMode: 'story' }],
      from: 'canonic-export'
    } }), location.origin);
    return true;
  })()`;
}

function prepareExternalPage(target, payload) {
  var reuse = payload.reuse === 'storybook' && canSwitchStorybook(target.pageUrl, payload.url);
  var exact = reuse && target.pageUrl === payload.url;
  var navigation = reuse
    ? (exact ? Promise.resolve() : target.evaluate(storybookSwitchScript(payload.url)).then(function () { target.pageUrl = payload.url; }))
    : target.navigate(payload.url);
  return navigation.then(function () {
    return target.evaluate(exact ? settleScript({ quiet: false, imageTimeout: 250 }) : settleScript());
  });
}

/* The marks over a page, then the picture: element names first, since the
   overlay is what a point would otherwise hit. Answers { png, targets }. */
function shootWithMarks(target, payload) {
  var targets = null;
  var details = null;
  return Promise.resolve()
    .then(function () {
      if (!payload.anchors || !payload.anchors.length) return;
      return target.evaluate(describeScript(payload.anchors)).then(function (found) {
        targets = found;
      }, function () {});
    })
    .then(function () {
      return target.evaluate(overlayScript(payload)).then(function (value) { details = value; });
    })
    .then(function () {
      return target.screenshot(payload.format);
    })
    .then(function (png) {
      return target.evaluate(REMOVE_OVERLAY).catch(function () {}).then(function () {
        var result = payload.format === 'jpeg' ? { image: png, targets: targets } : { png: png, targets: targets };
        if (details) result.details = details;
        return result;
      });
    });
}

Capture.prototype.capturePage = function (payload) {
  var width = Math.max(1, Math.round(Number(payload.width) || 0));
  var height = Math.max(1, Math.round(Number(payload.height) || 0));
  var self = this;

  return this.serial(function () {
    return self.launch()
      .then(function () {
        return self.metrics(width, height);
      })
      .then(function () {
        if (!payload.revision || self.pageRevision !== payload.revision) self.target.pageUrl = null;
        self.pageRevision = payload.revision;
        return self.target.navigate(payload.url);
      })
      .then(function () {
        return shootWithMarks(self.target, payload);
      });
  });
};

Capture.prototype.preparePage = function (payload) {
  var self = this;
  return this.serial(function () {
    return self.launch().then(function () {
      return self.metrics(payload.width, payload.height);
    }).then(function () {
      if (!payload.revision || self.pageRevision !== payload.revision) self.target.pageUrl = null;
      self.pageRevision = payload.revision;
      return self.target.navigate(payload.url);
    });
  });
};

Capture.prototype.captureExportPage = function (payload) {
  var width = Math.max(1, Math.round(Number(payload.width) || 0));
  var height = Math.max(1, Math.round(Number(payload.height) || 0));
  var self = this;
  return this.serial(function () {
    return self.launch()
      .then(function () { return self.metrics(width, height); })
      .then(function () {
        self.pageRevision = payload.revision;
        return prepareExternalPage(self.target, payload);
      })
      .then(function () { return shootWithMarks(self.target, payload); });
  });
};

Capture.prototype.close = function () {
  this.closed = true;
  this.target = null;
  return this.browser.close();
};

function create(options) {
  return new Capture(options);
}

function token() {
  return crypto.randomBytes(16).toString('hex');
}

module.exports = {
  Capture: Capture,
  Browser: Browser,
  Target: Target,
  Pipe: Pipe,
  chromeCandidates: chromeCandidates,
  create: create,
  findChrome: findChrome,
  overlayScript: overlayScript,
  describeScript: describeScript,
  settleScript: settleScript,
  canSwitchStorybook: canSwitchStorybook,
  storybookSwitchScript: storybookSwitchScript,
  removeOverlayScript: REMOVE_OVERLAY,
  shootWithMarks: shootWithMarks,
  stopProcess: stopProcess,
  token: token,
};
