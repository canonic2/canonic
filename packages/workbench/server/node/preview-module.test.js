// The preview module the compiler serves, run against a scripted document:
// chunks preload during the stylesheet wait, then run in order.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const compilerFixture = require('./compiler-fixture.cjs');

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-preview-module-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.symlinkSync(path.join(__dirname, '../../node_modules'), path.join(root, 'node_modules'), 'junction');
  fs.writeFileSync(path.join(root, 'card.workbench.ts'), `import { definePreview } from '@canonic2/workbench';
export default definePreview({ id: 'components/card', adapter: 'html', source: { entry: './card.ts' } });`);
  fs.writeFileSync(path.join(root, 'card.ts'), 'import "./card.css"; export default function (canvas) { canvas.textContent = "Acme"; }');
  fs.writeFileSync(path.join(root, 'card.css'), '.card { color: purple; }');
  return root;
}

// The served module's body, without its imports, as an async function of a document.
function runnable(source) {
  const body = source.replace(/^import .*;$/gm, '').replace(/^export .*$/gm, '');
  const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
  return new AsyncFunction('window', 'document', 'runtime', 'location', body);
}

for (const fails of [false, true]) {
  test(`preview chunks preload during the stylesheet wait and clean up after ${fails ? 'failure' : 'success'}`, async t => {
    const root = fixture(t);
    const compiler = await compilerFixture.open(root);
    t.after(() => compiler.close());
    const base = 'http://127.0.0.1:' + compiler.port;
    const descriptor = await (await fetch(base + '/_workbench/previews/descriptor?file=card.workbench.ts')).json();
    const source = await (await fetch(base + descriptor.module)).text();
    const scripts = JSON.parse(/const scripts = (\[.*\]);/.exec(source)[1]);
    assert.ok(scripts.length >= 2, 'the runtime and the entry are separate chunks');
    assert.match(source, /link\.rel = 'stylesheet'/, 'the preview has a stylesheet to wait for');
    const nodes = [];
    const executed = [];
    const window = { __wbRspack: {}, addEventListener() {}, removeEventListener() {} };
    const last = scripts[scripts.length - 1];
    const slug = JSON.parse(/const slug = (".*");/.exec(source)[1]);
    const document = {
      querySelector: () => null,
      createElement: tag => ({ tag, setAttribute() {}, remove() { nodes.splice(nodes.indexOf(this), 1); } }),
      head: { append(node) {
        nodes.push(node);
        if (node.tag !== 'script') return;
        executed.push(node.src);
        if (node.src === last) window.__wbRspack[slug] = { mount() {} };
        queueMicrotask(() => (fails ? node.onerror() : node.onload()));
      } },
    };
    const pending = runnable(source)(window, document, {}, { replace() {}, reload() {} });
    assert.deepEqual(nodes.filter(node => node.rel === 'preload').map(node => node.href), scripts);
    assert.deepEqual(executed, [], 'project code cannot run before styles are available');
    for (const node of nodes.filter(item => item.rel === 'stylesheet')) node.onload();
    if (fails) await assert.rejects(pending, new RegExp('Failed to load ' + scripts[0].replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
    else {
      await pending;
      assert.deepEqual(executed, scripts);
    }
    assert.equal(nodes.some(node => node.rel === 'preload'), false);
  });
}
