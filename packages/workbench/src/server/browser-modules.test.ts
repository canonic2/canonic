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
  write('docs/page/view.ts', 'export const label: string = "Acme";');
  write('docs/page/view.test.ts', 'export {};');
  write('docs/page/view.css', '.acme { color: red; }');
  write('docs/service.ts', 'export const secret = 1;');
  write('components/space-switcher/element.ts', 'export const tag: string = "wb-space-switcher";');
  write('components/space-switcher/element.test.ts', 'export {};');
  write('theme/defaults.css', ':root { --wb-fg: white; }');
  return root;
}

test('browser modules are served with types stripped, and transformed again only after an edit', async t => {
  const root = fixture(t);
  const seen: string[] = [];
  const serve = createBrowserModules(root, async (code, file) => {
    seen.push(path.basename(file));
    return code.replace(': string', '');
  });
  const module = await serve('/_workbench/src/docs/page/view.ts');
  assert.deepEqual(module, { status: 200, type: 'text/javascript; charset=utf-8', body: 'export const label = "Acme";' });
  await serve('/_workbench/src/docs/page/view.ts');
  assert.deepEqual(seen, ['view.ts']);
  fs.writeFileSync(path.join(root, 'docs/page/view.ts'), 'export const label: string = "Acme Corp";');
  fs.utimesSync(path.join(root, 'docs/page/view.ts'), new Date(), new Date(Date.now() + 5000));
  assert.equal((await serve('/_workbench/src/docs/page/view.ts')).body, 'export const label = "Acme Corp";');
  assert.equal((await serve('/_workbench/src/docs/page/view.css')).type, 'text/css; charset=utf-8');
});

test('tests, server code, and anything outside src/ are not served', async t => {
  const root = fixture(t);
  const serve = createBrowserModules(root, async code => code);
  for (const pathname of ['/_workbench/src/docs/page/view.test.ts', '/_workbench/src/docs/service.ts',
    '/_workbench/src/components/space-switcher/element.test.ts', '/_workbench/src/server/webview-components.ts',
    '/_workbench/src/components/../docs/service.ts', '/_workbench/src/components/%2e%2e/docs/service.ts',
    '/_workbench/src/components/../../etc/passwd.ts',
    '/_workbench/src/docs/page/../../../etc/passwd.ts', '/_workbench/src/docs/page/missing.ts', '/_workbench/src/docs/page/%E0%A4%A.ts']) {
    assert.equal((await serve(pathname)).status, 404, pathname);
  }
});

test('component source and shared theme assets are served for the browser host', async t => {
  const serve = createBrowserModules(fixture(t), async code => code.replace(': string', ''));
  assert.equal((await serve('/_workbench/src/components/space-switcher/element.ts')).body,
    'export const tag = "wb-space-switcher";');
  assert.equal((await serve('/_workbench/src/theme/defaults.css')).type, 'text/css; charset=utf-8');
});
