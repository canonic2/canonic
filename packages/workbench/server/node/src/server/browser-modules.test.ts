import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createBrowserModules } from './browser-modules.ts';

function fixture(t: { after(fn: () => void): void }): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-src-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const write = (file: string, text: string) => {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), text);
  };
  write('ui/src/modules/docs/page/view.js', '/* unchanged */\nexport const label = "Acme";\n');
  write('ui/src/modules/docs/page/view.test.js', 'export {};');
  write('ui/src/modules/docs/page/view.ts', 'export const label: string = "Acme";');
  write('ui/src/modules/docs/page/view.css', '.acme { color: red; }');
  write('server/node/src/docs/service.ts', 'export const secret = 1;');
  write('ui/src/components/space-switcher/element.js', 'export const tag = "wb-space-switcher";');
  write('ui/src/components/space-switcher/element.test.js', 'export {};');
  write('ui/src/theme/defaults.css', ':root { --wb-fg: white; }');
  return root;
}

test('browser assets preserve authored bytes and reflect edits without compilation', async t => {
  const root = fixture(t);
  const serve = createBrowserModules(root);
  const module = await serve('/_workbench/ui/src/modules/docs/page/view.js');
  assert.deepEqual(module, { status: 200, type: 'text/javascript; charset=utf-8', body: fs.readFileSync(path.join(root, 'ui/src/modules/docs/page/view.js'), 'utf8') });
  fs.writeFileSync(path.join(root, 'ui/src/modules/docs/page/view.js'), 'export const label = "Acme Corp";');
  assert.equal((await serve('/_workbench/ui/src/modules/docs/page/view.js')).body, 'export const label = "Acme Corp";');
  assert.equal((await serve('/_workbench/ui/src/modules/docs/page/view.css')).type, 'text/css; charset=utf-8');
});

test('tests, server code, and anything outside src/ are not served', async t => {
  const root = fixture(t);
  const serve = createBrowserModules(root);
  for (const pathname of ['/_workbench/ui/src/modules/docs/page/view.ts', '/_workbench/ui/src/modules/docs/page/view.test.js', '/_workbench/server/node/src/docs/service.ts',
    '/_workbench/ui/src/components/space-switcher/element.test.js', '/_workbench/extension/src/webview-components.ts',
    '/_workbench/ui/src/components/../docs/service.ts', '/_workbench/ui/src/components/%2e%2e/docs/service.ts',
    '/_workbench/ui/src/components/../../etc/passwd.ts',
    '/_workbench/ui/src/modules/docs/page/../../../etc/passwd.ts', '/_workbench/ui/src/modules/docs/page/missing.ts', '/_workbench/ui/src/modules/docs/page/%E0%A4%A.ts']) {
    assert.equal((await serve(pathname)).status, 404, pathname);
  }
});

test('component source and shared theme assets are served for the browser host', async t => {
  const serve = createBrowserModules(fixture(t));
  assert.equal((await serve('/_workbench/ui/src/components/space-switcher/element.js')).body,
    'export const tag = "wb-space-switcher";');
  assert.equal((await serve('/_workbench/ui/src/theme/defaults.css')).type, 'text/css; charset=utf-8');
});
