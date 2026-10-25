/* macOS host acceptance in an owned VS Code profile and staged extension.
   Resizes only the window belonging to the process this runner launches. */
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const { once } = require('node:events');
const { stageExtension } = require('./stage-extension.cjs');
const { createFixture, verifyResize } = require('./smoke-resize.cjs');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

async function connect(url) {
  const socket = new WebSocket(url);
  await once(socket, 'open');
  let sequence = 0;
  const pending = new Map();
  const listeners = [];
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    if (!message.id) { for (const listener of listeners) listener(message); return; }
    const job = pending.get(message.id);
    if (!job) return;
    pending.delete(message.id);
    clearTimeout(job.timer);
    if (message.error) job.reject(Error(JSON.stringify(message.error)));
    else job.resolve(message.result);
  });
  socket.addEventListener('close', () => {
    for (const job of pending.values()) { clearTimeout(job.timer); job.reject(Error('VS Code debugger closed')); }
    pending.clear();
  });
  return {
    listeners,
    close: () => socket.close(),
    send(method, params = {}, sessionId) {
      return new Promise((resolve, reject) => {
        const id = ++sequence;
        const timer = setTimeout(() => { pending.delete(id); reject(Error(method + ' timed out')); }, 15000);
        pending.set(id, { resolve, reject, timer });
        socket.send(JSON.stringify({ id, method, params, sessionId }));
      });
    },
  };
}

async function main() {
  if (process.platform !== 'darwin') throw Error('This host acceptance runner requires macOS.');
  const executable = process.env.VSCODE_PATH || '/Applications/Visual Studio Code.app/Contents/MacOS/Code';
  if (!fs.existsSync(executable)) throw Error('VS Code was not found; set VSCODE_PATH.');
  // Keep Unix-domain socket paths short; never use the developer's profile.
  const root = fs.mkdtempSync('/private/tmp/wbr-');
  let staged, child, connection, log;
  try {
    const fixture = path.join(root, 'project');
    const profile = path.join(root, 'profile');
    const extensions = path.join(root, 'extensions');
    fs.mkdirSync(fixture);
    fs.mkdirSync(extensions);
    fs.mkdirSync(path.join(profile, 'User'), { recursive: true });
    fs.writeFileSync(path.join(profile, 'User/settings.json'), JSON.stringify({
      'workbench.startupEditor': 'none', 'security.workspace.trust.enabled': false,
      'extensions.autoUpdate': false, 'extensions.autoCheckUpdates': false,
      'update.mode': 'none', 'telemetry.telemetryLevel': 'off',
    }));
    createFixture(fixture);
    const product = path.resolve(__dirname, '..');
    staged = stageExtension(product);
    const version = require('../extension/package.json').version;
    fs.renameSync(staged, path.join(extensions, 'canonic.canonic-workbench-' + version));
    staged = undefined;
    const env = { ...process.env };
    delete env.ELECTRON_RUN_AS_NODE;
    log = fs.openSync(path.join(root, 'code.log'), 'a');
    child = cp.spawn(executable, ['--user-data-dir', profile, '--extensions-dir', extensions,
      '--remote-debugging-port=0', '--disable-workspace-trust', '--skip-welcome',
      '--skip-release-notes', '--new-window', fixture], { env, detached: true, stdio: ['ignore', log, log] });
    let launchError;
    child.on('error', error => { launchError = error; });
    async function until(test, label) {
      const deadline = Date.now() + 40000;
      while (Date.now() < deadline) {
        if (launchError) throw launchError;
        if (child.exitCode !== null) throw Error('VS Code exited: ' + child.exitCode);
        const result = await test();
        if (result) return result;
        await delay(100);
      }
      throw Error('VS Code setup did not settle: ' + label);
    }
    await until(() => fs.existsSync(path.join(profile, 'DevToolsActivePort')), 'debugger');
    const [port, endpoint] = fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').trim().split('\n');
    const c = connection = await connect('ws://127.0.0.1:' + port + endpoint);
    const sessions = new Map();
    c.listeners.push(event => {
      if (event.method !== 'Target.attachedToTarget') return;
      const { sessionId, targetInfo } = event.params;
      sessions.set(targetInfo.targetId, sessionId);
      if (['page', 'iframe'].includes(targetInfo.type))
        void c.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true }, sessionId).catch(error => console.warn('Nested debugger attachment:', error.message));
    });
    await c.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true });
    async function session(target) {
      return sessions.get(target.targetId) || (await c.send('Target.attachToTarget', { targetId: target.targetId, flatten: true })).sessionId;
    }
    async function evaluate(expression, sessionId) {
      const response = await c.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, sessionId);
      if (response.exceptionDetails) throw Error(response.exceptionDetails.exception?.description || response.exceptionDetails.text);
      return response.result?.value;
    }
    const code = await until(async () => (await c.send('Target.getTargets')).targetInfos.find(t => t.type === 'page'), 'editor window');
    const editor = await session(code);
    await until(() => evaluate(`!!document.querySelector('.monaco-workbench') && document.readyState==='complete'`, editor), 'editor layout');
    cp.execFileSync('osascript', ['-e', `tell application "System Events" to set frontmost of first application process whose unix id is ${child.pid} to true`]);
    async function key(key, code, value) {
      for (const type of ['keyDown', 'keyUp']) await c.send('Input.dispatchKeyEvent', { type, key, code, windowsVirtualKeyCode: value }, editor);
    }
    await key('F1', 'F1', 112);
    await until(() => evaluate(`!!document.querySelector('.quick-input-widget input')?.getClientRects().length`, editor), 'command palette');
    await c.send('Input.insertText', { text: 'Workbench: Open Canvas' }, editor);
    await until(() => evaluate(`document.querySelector('.quick-input-widget')?.innerText.includes('Open Canvas')`, editor), 'Open Canvas command');
    await key('Enter', 'Enter', 13);
    const canvas = await until(async () => (await c.send('Target.getTargets')).targetInfos.find(t => /^http:\/\/127\.0\.0\.1:\d+\/_workbench\/$/.test(t.url)), 'Workbench webview');
    const content = await session(canvas);
    const evaluateCanvas = expression => evaluate(expression, content);
    const resize = async (width, height) => {
      cp.execFileSync('osascript', ['-e', `tell application "System Events" to set size of front window of (first application process whose unix id is ${child.pid}) to {${width}, ${height}}`]);
      await evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))', editor);
    };
    const results = await verifyResize(evaluateCanvas, resize);
    console.log(JSON.stringify({ host: 'VS Code on macOS', version: await evaluate('navigator.userAgent', editor), results }));
  } catch (error) {
    if (fs.existsSync(path.join(root, 'code.log'))) console.error(fs.readFileSync(path.join(root, 'code.log'), 'utf8'));
    throw error;
  } finally {
    connection?.close();
    if (child && child.exitCode === null && child.signalCode === null) {
      const exited = once(child, 'exit');
      child.kill('SIGTERM');
      let timer;
      await Promise.race([exited, new Promise(resolve => { timer = setTimeout(resolve, 8000); })]);
      clearTimeout(timer);
      if (child.exitCode === null && child.signalCode === null) {
        process.kill(-child.pid, 'SIGKILL');
        await exited;
        console.warn('VS Code required forced cleanup.');
      }
    }
    if (log !== undefined) fs.closeSync(log);
    if (staged) fs.rmSync(staged, { recursive: true, force: true });
    fs.rmSync(root, { recursive: true, force: true });
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
