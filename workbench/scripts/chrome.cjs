/* A headless Chrome, driven over the DevTools pipe, for repository tooling:
   the website screenshot script and the lens smoke test drive the workbench
   in it. The extension never uses it; screenshots come from the bundled
   capture helper (electron-capture.js).

   `Browser` is the process and its profile, `Target` is one page inside it,
   and `Pipe` is the wire. Set CHROME_PATH to choose the browser. */

var childProcess = require('child_process');
var fs = require('fs');
var os = require('os');
var path = require('path');

var COMMAND_TIMEOUT = 15000;

function firstFile(candidates) {
  for (var i = 0; i < candidates.length; i++) {
    if (candidates[i] && fs.existsSync(candidates[i])) return candidates[i];
  }
  return null;
}

function chromeCandidates(env, platform) {
  env = env || process.env;
  platform = platform || process.platform;
  var configured = env.CHROME_PATH;

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
      new Error('Chrome or Chromium was not found; set CHROME_PATH to choose one')
    );
  }

  if (this.options.profile) {
    fs.mkdirSync(this.options.profile, { recursive: true });
    this.profile = this.options.profile;
    this.ownsProfile = false;
  } else {
    this.profile = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-chrome-'));
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

module.exports = {
  Browser: Browser,
  Target: Target,
  Pipe: Pipe,
  chromeCandidates: chromeCandidates,
  findChrome: findChrome,
};
