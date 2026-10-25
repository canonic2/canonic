const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { stageExtension } = require('./stage-extension.cjs');
const { Browser, findChrome } = require('./chrome.cjs');

test('a staged product serves unchanged Lattice and imports it in canvas and renderer documents', { skip: !findChrome() }, async t => {
  const product = path.resolve(__dirname, '..');
  const staged = stageExtension(product);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-lattice-delivery-'));
  const browser = new Browser();
  let running;
  t.after(async () => {
    await browser.close();await running?.close();
    fs.rmSync(staged, { recursive: true, force: true });fs.rmSync(root, { recursive: true, force: true });
  });
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\npreviews: false\ncollections:\n  - name: Pages\n    items:\n      - label: Form\n        src: form.html\n');
  fs.writeFileSync(path.join(root, 'form.html'), '<!doctype html><input value="Acme">');
  const lattice = 'node_modules/@canonic2/lattice';
  assert.equal(fs.lstatSync(path.join(staged, lattice)).isSymbolicLink(), false);
  for (const file of ['src/index.js', 'src/report.js', 'src/modules/store/store.js', 'src/modules/store/shallow.js', 'src/modules/elements/elements.js', 'src/modules/collection/collection.js']) {
    const at = path.join(lattice, file);
    assert.equal(fs.lstatSync(path.join(staged, at)).isSymbolicLink(), false);
    assert.deepEqual(fs.readFileSync(path.join(staged, at)), fs.readFileSync(path.join(product, at)), file);
  }
  assert.equal(fs.existsSync(path.join(staged, lattice, 'node_modules')), false);
  assert.equal(fs.existsSync(path.join(staged, lattice, 'src/modules/store/store.test.js')), false);
  // Load the staged host, whose product root and every served resource are
  // outside the checkout. The linked development dependency cannot satisfy it.
  const server = require(path.join(staged, 'server/node/server.js'));
  running = await server.start({ root, capture: { close() {} } });
  for (const file of ['src/index.js', 'src/modules/elements/elements.js']) {
    const response = await fetch(new URL(lattice + '/' + file, running.url));
    assert.equal(response.headers.get('content-type'), 'text/javascript; charset=utf-8');
    assert.deepEqual(Buffer.from(await response.arrayBuffer()), fs.readFileSync(path.join(product, lattice, file)));
  }
  await browser.launch();
  const target = await browser.createTarget();
  const errors = [];
  target.on('Runtime.exceptionThrown', event => errors.push(event.exceptionDetails.exception?.description || event.exceptionDetails.text));
  await target.navigate(running.url);
  const deadline = Date.now() + 10000;
  while (!await target.evaluate(`document.getElementById('artboards')?.state.artboards[0]?.view?.ready`)) {
    if (Date.now() > deadline) throw Error('Staged canvas did not initialize: ' + errors.join('\n'));
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  const resources = await target.evaluate(`(()=>{
    const paths=win=>win.performance.getEntriesByType('resource').map(entry=>new URL(entry.name).pathname);
    return [paths(window), paths(document.querySelector('wb-canvas > iframe').contentWindow)];
  })()`);
  for (const paths of resources) {
    assert.ok(paths.includes('/_workbench/' + lattice + '/src/index.js'), JSON.stringify(paths));
    assert.ok(paths.includes('/_workbench/' + lattice + '/src/modules/elements/elements.js'), JSON.stringify(paths));
    assert.equal(paths.some(value => value.includes('/testing/') || value.endsWith('.test.js')), false);
  }
  assert.deepEqual(errors, []);
});
