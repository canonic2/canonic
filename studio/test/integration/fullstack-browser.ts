import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { eventually } from './fullstack.ts';
import { captureOptional, captureWorkspace } from './ide-smoke.ts';
import type { FullstackFixture } from './fullstack.ts';
import type { DesktopContext } from '../../app/desktop/bootstrap.ts';

export async function fullstackBrowser({
  manager,
  project,
  a,
  b,
  ta,
  tb,
  artifacts,
  window,
  views,
  handle,
}: FullstackFixture & Omit<DesktopContext, 'manager' | 'root'>) {
  for (const session of [a, b]) {
    await handle('select', { projectId: project.id, sessionId: session.id });
    const code = views.get(`${session.id}:code`)!.webContents;
    await eventually(
      () => code.executeJavaScript(`Boolean(document.querySelector('.monaco-workbench'))`),
      'Code OSS workbench',
      60000,
    );
    await handle('workspace', { sessionId: session.id, mode: 'preview' });
    const preview = views.get(`${session.id}:preview`)!.webContents;
    await eventually(
      () =>
        preview.executeJavaScript(`window.studioInfo?.sessionId === ${JSON.stringify(session.id)}`),
      'frontend mounted with own API identity',
    );
  }
  const pa = views.get(`${a.id}:preview`)!.webContents;
  const pb = views.get(`${b.id}:preview`)!.webContents;
  await pa.executeJavaScript(`document.querySelector('#increment').click()`);
  await eventually(
    () => pa.executeJavaScript(`window.studioInfo.counter === 3`),
    'counter click completed',
  );
  assert.equal(await pb.executeJavaScript('window.studioInfo.counter'), 1);
  const source = path.join(a.checkout, 'apps/frontend/src/main.ts');
  await writeFile(source, (await readFile(source, 'utf8')).replace('Vite frontend', 'Only A HMR'));
  await eventually(
    () =>
      pa.executeJavaScript(`document.querySelector('#counter').textContent.includes('Only A HMR')`),
    'Vite hot module replacement',
  );
  assert.equal(
    await pb.executeJavaScript(
      `document.querySelector('#counter').textContent.includes('Only A HMR')`,
    ),
    false,
  );
  const backend = path.join(a.checkout, 'apps/backend/server.ts');
  const previousPid = (await (await fetch(`${ta.url}/api/health`)).json()).pid;
  const neighborPid = (await (await fetch(`${tb.url}/api/health`)).json()).pid;
  await writeFile(
    backend,
    (await readFile(backend, 'utf8')).replace('EXPRESS_READY', 'EXPRESS_RELOADED'),
  );
  await eventually(async () => {
    try {
      return (await (await fetch(`${ta.url}/api/health`)).json()).pid !== previousPid;
    } catch {
      return false;
    }
  }, 'Node watch restarted only A backend');
  assert.equal((await (await fetch(`${tb.url}/api/health`)).json()).pid, neighborPid);
  // Use the visible session switcher, then the Preview control.
  await window.webContents.executeJavaScript(
    `[...document.querySelectorAll('.session-button')].find(button => button.textContent.startsWith('Checkout A')).click()`,
  );
  await eventually(
    async () =>
      manager.snapshot().projects[0].activeSessionId === a.id &&
      (await window.webContents.executeJavaScript(`document.querySelector('#loading').hidden`)),
    'session switcher selected A',
  );
  await window.webContents.executeJavaScript(
    `document.querySelector('[data-mode="preview"]').click()`,
  );
  await eventually(
    () =>
      window.webContents.executeJavaScript(
        `document.querySelector('[data-mode="preview"]').classList.contains('selected') && document.querySelector('#loading').hidden`,
      ),
    'preview control',
  );
  assert.equal(views.get(`${a.id}:preview`)!.webContents, pa);
  assert.equal(await pa.executeJavaScript('window.studioInfo.counter'), 3);
  // Display capture depends on the host compositor; DOM/API assertions above
  // remain the pass/fail contract even when a graphical surface is unavailable.
  const captures: Record<string, string> = {};
  for (const [name, operation] of [
    [
      'fullstack-studio.png',
      () =>
        captureWorkspace(
          window,
          views.get(`${a.id}:preview`)!,
          path.join(artifacts, 'fullstack-studio.png'),
        ),
    ],
    [
      'fullstack-a.png',
      async () =>
        writeFile(path.join(artifacts, 'fullstack-a.png'), (await pa.capturePage()).toPNG()),
    ],
  ] as [string, () => Promise<void>][]) {
    await captureOptional(path.join(artifacts, name), operation, captures);
  }
  await writeFile(
    path.join(artifacts, 'fullstack-captures.json'),
    JSON.stringify(captures, null, 2),
  );
}
