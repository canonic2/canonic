// Compatibility smoke in isolated VS Code profiles, never the user's window.
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const { once } = require('node:events');
const { overlay } = require('./overlay.cjs');
const { generate } = require('./fixtures.cjs');
const root = path.resolve(process.argv[2]);
const record = path.join(root, new Date().toISOString().replace(/[:.]/g, '-') + '-installed');
fs.mkdirSync(record);
// macOS Unix-domain sockets cap VS Code's user-data path length.
const dir = fs.mkdtempSync('/private/tmp/wbhi-');
const delay = (ms) => new Promise((r) => setTimeout(r, ms));
async function connect(url) {
  const socket = new WebSocket(url);
  await once(socket, 'open');
  let id = 0;
  const pending = new Map(),
    listeners = [];
  socket.addEventListener('message', (event) => {
    const value = JSON.parse(event.data);
    if (value.id) {
      const job = pending.get(value.id);
      if (!job) return;
      pending.delete(value.id);
      clearTimeout(job.timer);
      value.error ? job.reject(Error(JSON.stringify(value.error))) : job.resolve(value.result);
    } else for (const listener of listeners) listener(value);
  });
  socket.addEventListener('close', () => {
    for (const job of pending.values()) {
      clearTimeout(job.timer);
      job.reject(Error('DevTools closed'));
    }
    pending.clear();
  });
  return {
    socket,
    listeners,
    send(method, params = {}, sessionId) {
      return new Promise((resolve, reject) => {
        const key = ++id;
        const timer = setTimeout(() => {
          pending.delete(key);
          reject(Error(method + ' timeout'));
        }, 25000);
        pending.set(key, { resolve, reject, timer });
        socket.send(JSON.stringify({ id: key, method, params, sessionId }));
      });
    },
  };
}
async function check(variant) {
  const owned = path.join(dir, variant);
  fs.mkdirSync(owned);
  const extensions = path.join(owned, 'extensions');
  fs.mkdirSync(extensions);
  const product = overlay(
    path.join(extensions, 'canonic.canonic-workbench-0.14.0'),
    variant,
    path.join(root, 'assets'),
  );
  const manifest = JSON.parse(fs.readFileSync(path.join(product, 'extension/package.json')));
  manifest.main = './extension/extension.js';
  fs.writeFileSync(path.join(product, 'package.json'), JSON.stringify(manifest));
  for (const icon of ['icon.svg', 'icon.png'])
    fs.copyFileSync(path.join(product, 'extension', icon), path.join(product, icon));
  const fixture = path.join(owned, 'fixture');
  const fixtureInfo = generate(fixture, 'Overview', path.join(root, 'assets'));
  const profile = path.join(owned, 'profile');
  fs.mkdirSync(path.join(profile, 'User'), { recursive: true });
  fs.mkdirSync(path.join(owned, 'tmp'));
  fs.writeFileSync(
    path.join(profile, 'User/settings.json'),
    JSON.stringify({
      'workbench.trace': true,
      'workbench.startupEditor': 'none',
      'extensions.autoUpdate': false,
      'extensions.autoCheckUpdates': false,
      'update.mode': 'none',
      'telemetry.telemetryLevel': 'off',
      'security.workspace.trust.enabled': false,
    }),
  );
  const env = { ...process.env, TMPDIR: path.join(owned, 'tmp') };
  delete env.ELECTRON_RUN_AS_NODE;
  const log = fs.openSync(path.join(owned, 'code.log'), 'a');
  const child = cp.spawn(
    '/Applications/Visual Studio Code.app/Contents/MacOS/Code',
    [
      '--user-data-dir',
      profile,
      '--extensions-dir',
      extensions,
      '--remote-debugging-port=0',
      '--disable-workspace-trust',
      '--skip-welcome',
      '--skip-release-notes',
      '--new-window',
      fixture,
    ],
    { env, stdio: ['ignore', log, log], detached: true },
  );
  const result = {
    variant,
    kind: 'temporary installed source overlay; release server; compatibility only',
  };
  let c;
  const wait = async (test) => {
    const end = Date.now() + 40000;
    while (Date.now() < end) {
      const value = await test();
      if (value) return value;
      await delay(100);
    }
    throw Error('Installed setup/content timeout');
  };
  try {
    await wait(() => fs.existsSync(path.join(profile, 'DevToolsActivePort')));
    const [port, endpoint] = fs
      .readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8')
      .trim()
      .split('\n');
    c = await connect('ws://127.0.0.1:' + port + endpoint);
    const sessions = new Map();
    c.listeners.push((event) => {
      if (event.method === 'Target.attachedToTarget') {
        const { sessionId, targetInfo } = event.params;
        sessions.set(targetInfo.targetId, sessionId);
        if (['page', 'iframe'].includes(targetInfo.type))
          void c
            .send(
              'Target.setAutoAttach',
              { autoAttach: true, waitForDebuggerOnStart: false, flatten: true },
              sessionId,
            )
            .catch(() => {});
      }
    });
    await c.send('Target.setAutoAttach', {
      autoAttach: true,
      waitForDebuggerOnStart: false,
      flatten: true,
    });
    const code = await wait(async () => {
      const r = await c.send('Target.getTargets');
      return r.targetInfos.find((t) => t.type === 'page');
    });
    const codeSession =
      sessions.get(code.targetId) ||
      (await c.send('Target.attachToTarget', { targetId: code.targetId, flatten: true })).sessionId;
    const evaluate = async (expression, sessionId) => {
      const value = await c.send(
        'Runtime.evaluate',
        { expression, awaitPromise: true, returnByValue: true },
        sessionId,
      );
      if (value.exceptionDetails)
        throw Error(value.exceptionDetails.exception?.description || value.exceptionDetails.text);
      return value.result?.value;
    };
    await wait(() =>
      evaluate(
        `!!document.querySelector('.monaco-workbench')&&document.readyState==='complete'`,
        codeSession,
      ),
    );
    cp.execFileSync('osascript', [
      '-e',
      `tell application "System Events" to set frontmost of first application process whose unix id is ${child.pid} to true`,
    ]);
    await delay(2000);
    for (let attempt = 0; attempt < 3; attempt++) {
      result.paletteAttempts = attempt + 1;
      await c.send(
        'Input.dispatchKeyEvent',
        { type: 'keyDown', key: 'F1', code: 'F1', windowsVirtualKeyCode: 112 },
        codeSession,
      );
      await c.send(
        'Input.dispatchKeyEvent',
        { type: 'keyUp', key: 'F1', code: 'F1', windowsVirtualKeyCode: 112 },
        codeSession,
      );
      await delay(300);
      await c.send('Input.insertText', { text: '>Workbench: Open Canvas' }, codeSession);
      await delay(500);
      await c.send(
        'Input.dispatchKeyEvent',
        { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 },
        codeSession,
      );
      await c.send(
        'Input.dispatchKeyEvent',
        { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 },
        codeSession,
      );
      await delay(2000);
      if (
        (await c.send('Target.getTargets')).targetInfos.some((t) =>
          /^http:\/\/127\.0\.0\.1:\d+\/_workbench\/$/.test(t.url),
        )
      )
        break;
    }
    const canvas = await wait(async () => {
      const r = await c.send('Target.getTargets');
      return r.targetInfos.find((t) => /^http:\/\/127\.0\.0\.1:\d+\/_workbench\/$/.test(t.url));
    });
    const session =
      sessions.get(canvas.targetId) ||
      (await c.send('Target.attachToTarget', { targetId: canvas.targetId, flatten: true }))
        .sessionId;
    await c.send(
      'Page.addScriptToEvaluateOnNewDocument',
      { source: fs.readFileSync(path.join(__dirname, 'probe.js'), 'utf8') },
      session,
    );
    await evaluate(fs.readFileSync(path.join(__dirname, 'probe.js'), 'utf8'), session);
    await wait(() =>
      evaluate(
        `document.visibilityState==='visible'&&!!window.wbItem?.(${JSON.stringify(fixtureInfo.first)})`,
        session,
      ),
    );
    result.document = await evaluate(
      `__labSelect(${JSON.stringify(fixtureInfo.first)},'installed-compatibility')`,
      session,
    );
    result.source = await evaluate(
      `__labSource(${JSON.stringify(fixtureInfo.first)},${JSON.stringify(fixtureInfo.example)})`,
      session,
    );
    result.geometry = await evaluate(
      `__labCorrectness(${JSON.stringify(fixtureInfo.first)})`,
      session,
    );
    result.capture = await evaluate(`__labCapture(${JSON.stringify(fixtureInfo.first)})`, session);
    result.runtime = await evaluate(
      `({userAgent:navigator.userAgent,visibility:document.visibilityState})`,
      session,
    );
    if (
      !result.geometry.geometry ||
      result.geometry.workerErrors.length ||
      (variant !== 'D' && (!result.document.blocks[0].spans || !result.source.spans)) ||
      result.capture.bytes < 100
    )
      throw Error('Installed source/geometry/colour/capture check failed');
    result.status = 'passed';
  } catch (error) {
    result.status = 'failed';
    result.error = String(error.stack || error);
  } finally {
    c?.socket.close();
    if (child.exitCode === null && child.signalCode === null) {
      child.kill('SIGTERM');
      await Promise.race([once(child, 'exit'), delay(8000)]);
    }
    if (child.exitCode === null && child.signalCode === null) {
      process.kill(-child.pid, 'SIGKILL');
      result.cleanupFailure = 'VS Code needed SIGKILL';
    }
    fs.closeSync(log);
    fs.copyFileSync(path.join(owned, 'code.log'), path.join(record, variant + '-code.log'));
    for (const name of ['extensions', 'profile', 'fixture', 'tmp'])
      fs.rmSync(path.join(owned, name), { recursive: true, force: true });
  }
  return result;
}
const report = {
  kind: 'installed VS Code compatibility, not performance samples',
  started: new Date().toISOString(),
  variants: [],
};
(async () => {
  for (const variant of ['0', '1', '2', 'D']) {
    const result = await check(variant);
    report.variants.push(result);
    fs.writeFileSync(path.join(record, 'report.json'), JSON.stringify(report, null, 2));
    console.log(
      JSON.stringify({ variant, status: result.status, error: result.error?.split('\n')[0] }),
    );
  }
  report.finished = new Date().toISOString();
  fs.writeFileSync(path.join(record, 'report.json'), JSON.stringify(report, null, 2));
  fs.rmSync(dir, { recursive: true, force: true });
  console.log(JSON.stringify({ report: path.join(record, 'report.json') }));
})().catch((error) => {
  report.fatal = String(error);
  fs.writeFileSync(path.join(record, 'report.json'), JSON.stringify(report, null, 2));
  console.error(error);
  process.exitCode = 1;
});
