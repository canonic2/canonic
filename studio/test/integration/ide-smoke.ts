import { errorMessage, errorCode } from '../../src/platform/errors.ts';
import assert from 'node:assert/strict';
import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { nativeImage } from 'electron';
import type { BrowserWindow, WebContentsView } from 'electron';
import type { DesktopContext } from '../../app/desktop/bootstrap.ts';

async function until(check: () => boolean | Promise<boolean>, message: string, timeout = 60000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  throw new Error(`Timed out: ${message}`);
}

export async function captureWorkspace(
  window: BrowserWindow,
  view: WebContentsView,
  destination: string,
) {
  const base = await window.webContents.capturePage();
  const size = base.getSize();
  const scale = size.width / window.getContentSize()[0];
  const bounds = view.getBounds();
  const capture = (await view.webContents.capturePage()).resize({
    width: Math.round(bounds.width * scale),
    height: Math.round(bounds.height * scale),
  });
  const pixels = base.toBitmap();
  const overlay = capture.toBitmap();
  const overlaySize = capture.getSize();
  const x = Math.round(bounds.x * scale);
  const y = Math.round(bounds.y * scale);
  for (let row = 0; row < overlaySize.height; row++) {
    overlay.copy(
      pixels,
      ((y + row) * size.width + x) * 4,
      row * overlaySize.width * 4,
      (row + 1) * overlaySize.width * 4,
    );
  }
  await writeFile(
    destination,
    nativeImage.createFromBitmap(pixels, { width: size.width, height: size.height }).toPNG(),
  );
}

export async function captureOptional(
  destination: string,
  operation: () => Promise<void>,
  captures: Record<string, string>,
) {
  await rm(destination, { force: true });
  let timer: NodeJS.Timeout | undefined;
  try {
    await Promise.race([
      operation(),
      new Promise((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error('Display capture timed out')), 3000);
      }),
    ]);
    captures[path.basename(destination)] = 'saved';
  } catch (error) {
    captures[path.basename(destination)] = errorMessage(error);
    console.log(`Optional capture ${path.basename(destination)}: ${errorMessage(error)}`);
  } finally {
    clearTimeout(timer);
  }
}

export async function runSmoke({ manager, window, views, handle, root }: DesktopContext) {
  const [store, site] = manager.snapshot().projects;
  const cart = store.sessions[1];
  const main = store.sessions[0];
  const artifacts = path.join(root, 'artifacts');
  const captures: Record<string, string> = {};
  const capture = (name: string, operation: () => Promise<void>) =>
    captureOptional(path.join(artifacts, name), operation, captures);
  await mkdir(artifacts, { recursive: true });
  console.log('Smoke: selecting real Code OSS session');
  await cp(
    path.join(root, 'test/fixtures/smoke-extension'),
    path.join(manager.root, 'sessions', cart.id, 'extensions', 'canonic.studio-smoke-0.0.1'),
    { recursive: true },
  );
  await cp(
    path.join(root, 'test/fixtures/studio-theme'),
    path.join(manager.root, 'sessions', cart.id, 'extensions', 'canonic.studio-theme-0.0.1'),
    { recursive: true },
  );
  const config = JSON.parse(await readFile(path.join(cart.checkout, 'studio.config.json'), 'utf8'));
  config.ide = {
    settings: {
      'workbench.colorTheme': 'Studio Midnight',
      'security.workspace.trust.enabled': false,
    },
  };
  await writeFile(path.join(cart.checkout, 'studio.config.json'), JSON.stringify(config));
  await writeFile(
    path.join(cart.checkout, '.studio-proof-request.json'),
    JSON.stringify({ sessionId: cart.id }),
  );
  await handle('select', { projectId: store.id, sessionId: cart.id });
  const code = views.get(`${cart.id}:code`)!.webContents;
  await until(
    () => code.executeJavaScript(`Boolean(document.querySelector('.monaco-workbench'))`),
    'IDE workbench loaded',
  );
  await until(
    () =>
      code.executeJavaScript(
        `getComputedStyle(document.querySelector('.monaco-workbench')).getPropertyValue('--vscode-editor-background').trim() === '#1c1e23'`,
      ),
    'bundled theme extension loaded',
  );
  console.log('Smoke: workspace extension edits and saves through the real IDE API');
  await until(
    () =>
      code.executeJavaScript(
        `[...document.querySelectorAll('.explorer-folders-view .monaco-list-row')].some(row => row.innerText.includes('title.txt'))`,
      ),
    'file explorer populated',
  );
  const edited = `Studio smoke — ${cart.id}`;
  await until(async () => {
    try {
      const proof = JSON.parse(await readFile(path.join(cart.checkout, 'ide-proof.json'), 'utf8'));
      if (proof.error) throw new Error(proof.error);
      return proof.saved && proof.sessionId === cart.id && proof.folder === cart.checkout;
    } catch (error) {
      if (errorCode(error) === 'ENOENT') return false;
      throw error;
    }
  }, 'workspace extension activated, edited, and saved');
  await until(
    async () => (await readFile(path.join(cart.checkout, 'title.txt'), 'utf8')).trim() === edited,
    'saved file contents',
  );
  assert.equal(
    (await readFile(path.join(main.checkout, 'title.txt'), 'utf8')).trim(),
    'Acme Store',
  );
  await capture('studio-ide.png', async () =>
    writeFile(path.join(artifacts, 'studio-ide.png'), (await code.capturePage()).toPNG()),
  );
  await capture('studio.png', () =>
    captureWorkspace(window, views.get(`${cart.id}:code`)!, path.join(artifacts, 'studio.png')),
  );
  console.log('Smoke: real integrated terminal');
  await until(
    () => code.executeJavaScript(`Boolean(document.querySelector('.terminal .xterm-screen'))`),
    'terminal loaded',
  );
  await until(async () => {
    try {
      return (
        (await readFile(path.join(cart.checkout, 'terminal-proof.txt'), 'utf8')) ===
        `${cart.id}\n${cart.checkout}`
      );
    } catch {
      return false;
    }
  }, 'terminal ran in the correct checkout with session environment');
  console.log('Smoke: switching projects retains editor and terminal');
  await handle('select', { projectId: site.id });
  await handle('select', { projectId: store.id });
  assert.equal(views.get(`${cart.id}:code`)!.webContents, code);
  assert.equal(
    await code.executeJavaScript(
      `document.querySelector('.tabs-container')?.textContent.includes('title.txt')`,
    ),
    true,
  );
  assert.equal(
    await code.executeJavaScript(`Boolean(document.querySelector('.terminal .xterm-screen'))`),
    true,
  );
  console.log('Smoke: switching sessions inside the project');
  const mainConfigFile = path.join(main.checkout, 'studio.config.json');
  const mainConfig = await readFile(mainConfigFile, 'utf8');
  await writeFile(mainConfigFile, '{invalid');
  await window.webContents.executeJavaScript(
    `[...document.querySelectorAll('.session-button')].find(button => button.textContent.startsWith('Main')).click()`,
  );
  await until(
    () =>
      views.has(`${main.id}:code`) &&
      window.webContents.executeJavaScript(`document.querySelector('#loading').hidden`),
    'Main session opened through the session switcher',
  );
  assert.match(manager.ide.errors.get(main.id)!, /studio.config.json/);
  await writeFile(mainConfigFile, mainConfig);
  await window.webContents.executeJavaScript(
    `[...document.querySelectorAll('.session-button')].find(button => button.textContent.startsWith('Cart redesign')).click()`,
  );
  await until(
    async () =>
      manager.snapshot().projects[0].activeSessionId === cart.id &&
      (await window.webContents.executeJavaScript(`document.querySelector('#loading').hidden`)),
    'Cart session restored through the switcher',
  );
  assert.equal(views.get(`${cart.id}:code`)!.webContents, code);
  assert.equal((await fetch(`${manager.runtimes.records.get(main.id)!.url}/health`)).status, 200);
  await handle('workspace', { sessionId: cart.id, mode: 'preview' });
  const preview = views.get(`${cart.id}:preview`)!.webContents;
  await until(
    () =>
      preview.executeJavaScript(`document.body.textContent.includes(${JSON.stringify(edited)})`),
    'preview reflects saved edit',
  );
  await capture('studio-preview.png', async () =>
    writeFile(path.join(artifacts, 'studio-preview.png'), (await preview.capturePage()).toPNG()),
  );
  await handle('workspace', { sessionId: cart.id, mode: 'logs' });
  await window.webContents.executeJavaScript(
    `document.querySelector('[data-mode="logs"]').click()`,
  );
  await until(
    () =>
      window.webContents.executeJavaScript(
        `document.querySelector('#logs').textContent.includes('READY')`,
      ),
    'logs view',
  );
  await capture('studio-shell.png', async () =>
    writeFile(
      path.join(artifacts, 'studio-shell.png'),
      (await window.webContents.capturePage()).toPNG(),
    ),
  );
  await handle('stopService', { sessionId: cart.id });
  assert.equal((await fetch(`${manager.runtimes.records.get(main.id)!.url}/health`)).status, 200);
  const mainURL = manager.runtimes.records.get(main.id)!.url;
  const siteURL = manager.runtimes.records.get(site.sessions[0].id)!.url;
  await window.webContents.executeJavaScript(
    `document.querySelector('[aria-label="Close Acme Store"]').click()`,
  );
  await until(
    async () =>
      manager.snapshot().projects.length === 1 &&
      (await window.webContents.executeJavaScript(`document.querySelector('#loading').hidden`)),
    'closing project through its tab',
  );
  await assert.rejects(fetch(`${mainURL}/health`));
  assert.equal((await fetch(`${siteURL}/health`)).status, 200);
  assert.equal((await manager.addProject(store.repository)).id, store.id);
  assert.equal((await readFile(path.join(cart.checkout, 'title.txt'), 'utf8')).trim(), edited);
  await writeFile(
    path.join(artifacts, 'smoke.json'),
    JSON.stringify(
      {
        passed: true,
        captures,
        checks: [
          'Code OSS loaded',
          'bundled theme extension loaded',
          'file edit/save',
          'checkout isolation',
          'integrated terminal',
          'project/session switching',
          'editor/terminal retention',
          'live preview',
          'logs',
          'independent service stop',
          'project close/reopen',
        ],
      },
      null,
      2,
    ),
  );
}
