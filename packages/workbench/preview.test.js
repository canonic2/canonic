const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Compiler, discover } = require('./preview/compiler.cjs');
const server = require('./server');
const exporter = require('./src/modules/export/index.ts');
const manifest = require('./workbench/manifest');
const portable = require('./preview/portable.cjs');

function fixture(t, files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-preview-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const [file, contents] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), contents);
  }
  fs.symlinkSync(path.join(__dirname, 'node_modules'), path.join(root, 'node_modules'), 'junction');
  return root;
}
// Portable builds: the Rspack production compiler the export and the CLI use.
async function build(t, root, file, options) {
  const builder = portable.portableBuilder(new Compiler(root, options));
  t.after(() => builder.close());
  return builder.compile(file);
}
async function buildDocs(t, root, request) {
  const builder = portable.portableBuilder(new Compiler(root));
  t.after(() => builder.close());
  return builder.compileDocs(request);
}
const definition = (adapter, entry, extra = '') => `import { definePreview } from '@canonic2/workbench';
export default definePreview({ id: 'components/button', title: 'Components/Button', adapter: '${adapter}', source: { entry: '${entry}' },
inputs: { label: 'Continue' }, states: { default: {}, disabled: { inputs: { label: 'Disabled' } } }, ${extra} });`;

test('the server supplies discovered lens labels while retaining per-page overrides', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\npreviews:\n  lensLabel: Design\ncollections:\n  - name: Components\n    items:\n      - label: My button\n        src: button.workbench.ts\n        lensLabel: Reference\n',
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const resolved = await running.config();
  assert.deepEqual(resolved.problems, []);
  assert.equal(resolved.catalogCollections[0].items[0].lensLabel, 'Design');
  assert.equal(resolved.collections[0].items[0].lensLabel, 'Reference');
  const merged = manifest.mergeCollections(resolved.collections, resolved.catalogCollections);
  assert.equal(merged[0].items.length, 1);
  assert.equal(merged[0].items[0].label, 'My button');
  assert.equal(manifest.authoredLensLabel(merged[0].items[0]), 'Reference');
});

test('preview icons pass through discovery, override maps, and isolate invalid names', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\npreviews:\n  icon: boxes\n  icons:\n    Components: app-window\ncollections:\n  - name: Components\n    icon: star\n',
    'button.workbench.ts': definition('html', './button.ts', "icon: 'monitor'"),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
    'bad.workbench.ts': definition('html', './button.ts', "icon: 'Bad Icon'"),
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const resolved = await running.config();
  assert.equal(resolved.catalogCollections[0].icon, 'app-window');
  assert.equal(resolved.catalogCollections[0].items[0].icon, 'monitor');
  assert.equal(resolved.pages['button.workbench.ts'].preview.icon, 'monitor');
  assert.match(resolved.problems.join('\n'), /bad.workbench.ts: .*icon must be a kebab-case Lucide icon name/);
  const manual = resolved.collections.map(collection => ({ name: collection.name, icon: collection.icon, items: collection.items }));
  assert.equal(manifest.mergeCollections(manual, resolved.catalogCollections)[0].icon, 'star');
});

test('preview and live Storybook collection icons respect mappings and configured fallbacks', async t => {
  const http = require('node:http');
  const catalog = http.createServer((req, res) => {
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ entries: { customers: { id: 'customers', type: 'story', title: 'Web App/Pages/Customers', name: 'Default' } } }));
  });
  await new Promise(resolve => catalog.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => catalog.close(resolve)));
  const root = fixture(t, {
    'jobs.workbench.ts': definition('html', './jobs.ts').replace('Components/Button', 'Web App/Pages/Jobs'),
    'jobs.ts': 'export default function() {}',
  });
  const cases = [
    ['', 'true', 'component', 'component'],
    ['', '\n      icons:\n        Web App: monitor', 'monitor', 'component'],
    ['previews:\n  icon: boxes\n', '\n      icon: book-open', 'boxes', 'boxes'],
    ['previews:\n  icon: boxes\n', '\n      icons:\n        Web App: monitor', 'monitor', 'boxes'],
    ['previews:\n  icons:\n    Web App: app-window\n    Web App/Pages: monitor\n', '\n      icons:\n        Web App: palette', 'app-window', 'monitor'],
  ];
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n');
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  for (const [previews, icons, collectionIcon, pageIcon] of cases) {
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\n' + previews + 'implementations:\n  storybook:\n    kind: storybook\n    url: http://127.0.0.1:' + catalog.address().port + '\n    catalog: ' + icons + '\n');
    const resolved = await running.config();
    assert.deepEqual(resolved.problems, []);
    assert.equal(resolved.catalogCollections.length, 1);
    assert.equal(resolved.catalogCollections[0].icon, collectionIcon);
    assert.equal(resolved.catalogCollections[0].items[0].items[0].icon, pageIcon);
  }
});

test('discovers TypeScript previews, validates IDs and states, and isolates invalid definitions', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
    'broken.workbench.ts': 'export default { id: "INVALID" };',
    'dist/ignored.workbench.ts': definition('html', './button.ts'),
  });
  assert.equal(discover(root).length, 2);
  const index = await new Compiler(root).index();
  assert.equal(index.previews.length, 1);
  assert.deepEqual(index.previews[0].states.map(state => state.id), ['default', 'disabled']);
  assert.equal(index.errors.length, 1);
  assert.match(index.errors[0], /kebab-case/);
  assert.throws(() => new Compiler(root, { include: ['src/**'] }).declared('button.workbench.ts'), /declared/);
});

test('lists every definition from one build, follows helper edits and isolates broken definitions', async t => {
  const page = (id, extra = '') => `import { definePreview } from '@canonic2/workbench';
import { section } from '../titles';
export default definePreview({ id: 'pages/${id}', title: section + '/${id}', adapter: 'html', source: { entry: '../button.ts' } });${extra}`;
  const root = fixture(t, {
    'titles.ts': "export const section: string = 'Acme';",
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
    'pages/alpha.workbench.ts': page('alpha'),
    'pages/beta.workbench.tsx': page('beta'),
    'pages/gamma.workbench.ts': page('gamma'),
  });
  // Definitions evaluated one build at a time: only those the shared build could not.
  const compiler = new Compiler(root);
  const alone = [];
  const evaluate = compiler.evaluate.bind(compiler);
  compiler.evaluate = file => { alone.push(path.relative(root, file)); return evaluate(file); };
  let index = await compiler.index();
  assert.deepEqual(index.previews.map(preview => preview.title), ['Acme/alpha', 'Acme/beta', 'Acme/gamma']);
  assert.deepEqual(index.errors, []);
  assert.deepEqual(alone, []);
  fs.writeFileSync(path.join(root, 'titles.ts'), "export const section: string = 'Acme Pages';\n");
  index = await compiler.index();
  assert.deepEqual(index.previews.map(preview => preview.title), ['Acme Pages/alpha', 'Acme Pages/beta', 'Acme Pages/gamma']);
  assert.deepEqual(alone, []);

  fs.writeFileSync(path.join(root, 'pages/beta.workbench.tsx'), page('beta', '\nthrow new Error("beta failed");'));
  fs.writeFileSync(path.join(root, 'pages/gamma.workbench.ts'), page('gamma', '\nconst = ;'));
  index = await compiler.index();
  assert.deepEqual(index.previews.map(preview => preview.id), ['pages/alpha']);
  assert.equal(index.errors.length, 2);
  assert.match(index.errors.join('\n'), /pages\/beta\.workbench\.tsx: beta failed/);
  assert.match(index.errors.join('\n'), /pages\/gamma\.workbench\.ts: /);
  assert.deepEqual(alone.sort(), ['pages/beta.workbench.tsx', 'pages/gamma.workbench.ts']);
});

test('callers listing at the same time share one definitions build', async t => {
  const page = id => `import { definePreview } from '@canonic2/workbench';
import { section } from '../titles';
export default definePreview({ id: 'pages/${id}', title: section + '/${id}', adapter: 'html', source: { entry: '../button.ts' } });`;
  const root = fixture(t, {
    'titles.ts': "export const section: string = 'Acme';",
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
    'pages/alpha.workbench.ts': page('alpha'),
    'pages/beta.workbench.ts': page('beta'),
  });
  const compiler = new Compiler(root);
  const builds = [];
  const loadBatch = compiler.loadBatch.bind(compiler);
  compiler.loadBatch = files => { builds.push(files.length); return loadBatch(files); };
  const [first, second] = await Promise.all([compiler.index(), compiler.index()]);
  assert.deepEqual(builds, [2]);
  assert.deepEqual(second.previews, first.previews);

  // After an edit, the next callers share one build again and see it.
  fs.writeFileSync(path.join(root, 'titles.ts'), "export const section: string = 'Acme Pages';\n");
  const again = await Promise.all([compiler.index(), compiler.index()]);
  assert.deepEqual(builds, [2, 2]);
  for (const index of again) assert.deepEqual(index.previews.map(preview => preview.title), ['Acme Pages/alpha', 'Acme Pages/beta']);
});

test('indexes a definition’s size keys and rejects entries that aren’t keys', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts', "sizes: ['laptop', 'sidebar']"),
    'wide.workbench.ts': definition('html', './button.ts', "sizes: ['1024']").replace('components/button', 'components/wide').replace('Components/Button', 'Components/Wide'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
  });
  const index = await new Compiler(root).index();
  assert.deepEqual(index.previews.map(preview => [preview.id, preview.sizes]), [['components/button', ['laptop', 'sidebar']]]);
  assert.match(index.errors.join('\n'), /wide\.workbench\.ts: sizes must be a list of size keys/);
});

test('the server resolves a definition’s sizes against the space and reports unknown keys', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\nsizes:\n  fit: true\n  sidebar:\n    width: 340\n    height: fill\n',
    'button.workbench.ts': definition('html', './button.ts', "sizes: ['sidebar', 'tablet']"),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Button"; }',
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const resolved = await running.config();
  assert.deepEqual(resolved.sizes.map(size => size.key), ['fit', 'sidebar']);
  assert.deepEqual(resolved.pages['button.workbench.ts'].sizes, ['sidebar']);
  assert.match(resolved.problems.join('\n'), /button\.workbench\.ts: size “tablet” isn’t one of the space’s sizes\./);
});

test('builds HTML scripts, inline modules, CSS, srcset and assets into portable output', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.html'),
    'button.html': '<!doctype html><html><head><style>button{background:url(./dot.svg)}</style></head><body><a href="/account">Account</a><button>Continue</button><img srcset="./dot.svg 1x, ./dot.svg 2x"><script type="module">import "./button.css"; import { label } from "./label.ts"; document.querySelector("button").textContent=label;</script></body></html>',
    'button.css': 'button { color: purple; }',
    'label.ts': 'export const label: string = "Bundled";',
    'dot.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>',
  });
  const output = await build(t, root, 'button.workbench.ts');
  assert.ok(Array.from(output.outputs.keys()).some(name => name.endsWith('.css')));
  assert.ok(Array.from(output.outputs.keys()).some(name => name.endsWith('.svg')));
  assert.ok(output.localFiles.includes(path.join(root, 'label.ts')));
  const html = output.outputs.get('index.html').toString();
  assert.doesNotMatch(html, /<base|\/_workbench\/actions|\/_workbench\/keys/);
  assert.match(output.outputs.get('preview.js').toString(), /srcset/);
  assert.match(output.outputs.get('preview.js').toString(), /href=["']\/account/);
  assert.ok(output.localFiles.includes(path.join(root, 'button.css')));
  const css = Array.from(output.outputs.keys()).find(name => name.endsWith('.css') && output.outputs.get(name).includes(Buffer.from('purple')));
  assert.ok(css);
  // The script's own chunk carries the CSS it imports, loaded when the script runs.
  assert.ok(output.outputs.get('preview.js').toString().includes(path.basename(css, '.css')));
  // The inline module script is bundled with its imports and runs on each render.
  assert.match(output.outputs.get('preview.js').toString(), /data-workbench-script/);
  assert.ok(Array.from(output.outputs).some(([name, body]) => name.endsWith('.js') && body.includes(Buffer.from('Bundled'))));
  fs.writeFileSync(path.join(root, 'label.ts'), 'export const label = "Changed";');
  const changed = await build(t, root, 'button.workbench.ts');
  assert.notEqual(changed.revision, output.revision);
});

test('an inline script keeps attributes that look like replacement patterns', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.html'),
    'button.html': '<!doctype html><html><body><button>Continue</button><script data-price="$&amp;$1"> document.body.dataset.ready = "yes";</script></body></html>',
  });
  const output = await build(t, root, 'button.workbench.ts');
  const preview = output.outputs.get('preview.js').toString();
  // The script's attributes stay in the page; its code is bundled.
  assert.ok(preview.includes('data-price=') && preview.includes('$&amp;$1'));
  assert.ok(Array.from(output.outputs).some(([name, body]) => name.endsWith('.js') && body.includes(Buffer.from('document.body.dataset.ready'))));
});

test('bundles React and Vue SFCs with scoped styles using project framework versions', async t => {
  for (const [adapter, entry, source] of [
    ['react', './Button.tsx', 'export default function Button({label}:{label:string}) { return <button>{label}</button>; }'],
    ['vue', './Button.vue', '<script setup lang="ts">defineProps<{label:string}>()</script><template><button>{{label}}</button></template><style scoped>button{color:red}</style>'],
  ]) {
    const root = fixture(t, { 'button.workbench.ts': definition(adapter, entry), [entry.slice(2)]: source });
    const built = await build(t, root, 'button.workbench.ts');
    assert.ok(built.outputs.get('preview.js').length > 1000);
    assert.ok(built.localFiles.includes(path.join(root, entry)));
    // Vue style blocks are injected by the bundle, scoped to the component.
    if (adapter === 'vue') assert.match(built.outputs.get('preview.js').toString(), /button\[data-v-[0-9a-f]+\]/);
  }
});

test('preview mounts fill flex-body artboards and React Native Web fills frames and docs stages', {
  timeout: 60000, skip: require('./electron-runtime').unavailable() || false,
}, async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'flex.workbench.ts': `import { definePreview } from '@canonic2/workbench';
      export default definePreview({ id:'pages/jobs', title:'Pages/Jobs', adapter:'react',
        source:{entry:'./Flex.tsx'}, states:{loading:{inputs:{label:'Loading'}},
          empty:{inputs:{label:'No jobs'}}, 'load-error':{inputs:{label:'Load error'}}} });`,
    'Flex.tsx': `import './flex.css';
      export default function Jobs({label}) { return <section data-testid="screen" style={{width:'100%'}}>
        <div style={{width:827,maxWidth:'100%'}}>{label}</div></section>; }`,
    'flex.css': 'body { display:flex; } #app { width:100%; }',
    'native.workbench.ts': definition('react-native-web', './Screen.tsx'),
    'web.workbench.ts': definition('react', './Web.tsx'),
    'Screen.tsx': `import { View, Text } from 'react-native';
      export default function Screen() { return <View testID="screen" style={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}><Text>Acme modal</Text></View>; }`,
    'Web.tsx': 'export default function Web() { return <div data-testid="screen">Acme web</div>; }',
    'examples.ts': 'export { default as screen } from "./Screen.tsx";',
    'web-examples.ts': 'export { default as screen } from "./Web.tsx";',
  });
  const routes = new Map();
  for (const name of ['native', 'web', 'flex']) {
    const built = await build(t, root, name + '.workbench.ts');
    for (const [file, contents] of built.outputs) routes.set('/' + name + '/' + file, contents);
    if (name === 'flex') continue;
    const docs = await buildDocs(t, root, { adapter: name === 'native' ? 'react-native-web' : 'react', examples: name === 'native' ? 'examples.ts' : 'web-examples.ts' });
    for (const [file, contents] of docs.outputs) routes.set('/' + name + '-docs/' + file, contents);
    routes.set('/' + name + '-docs/index.html', Buffer.from(`<link rel="stylesheet" href="./examples.css">
      <style>.stage { min-height:48px; padding:32px; } .sized { height:200px; }</style>
      <div class="stage sized"></div><div class="stage intrinsic"></div>
      <script type="module">import { adapter, examples, environment } from './examples.js';
      window.stops = []; for (const stage of document.querySelectorAll('.stage')) {
        window.stops.push(await adapter.mount(stage, examples.screen, { inputs:{}, error:console.error }, environment));
      } window.ready = true;</script>`));
  }
  const http = require('node:http');
  const listener = http.createServer((req, res) => {
    const contents = routes.get(req.url.split('?')[0]);
    if (!contents) { res.writeHead(404); res.end(); return; }
    res.setHeader('Content-Type', req.url.endsWith('.js') ? 'text/javascript' : req.url.endsWith('.css') ? 'text/css' : 'text/html');
    res.end(contents);
  });
  await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => listener.close(resolve)));
  const base = 'http://127.0.0.1:' + listener.address().port;
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  t.after(() => running.close());
  const runner = path.join(root, 'check.cjs');
  fs.writeFileSync(runner, `const { app, BrowserWindow } = require('electron');
    const assert = require('node:assert/strict');
    app.whenReady().then(async () => {
      const win = new BrowserWindow({ show:false, width:393, height:852, useContentSize:true });
      const evaluate = code => win.webContents.executeJavaScript(code);
      const load = async path => {
        await win.loadURL(${JSON.stringify(base)} + path);
        await evaluate(\`new Promise((resolve, reject) => { const start=Date.now(); const poll=()=> {
          if (window.ready || document.querySelector('[data-testid="screen"]')) resolve();
          else if(Date.now()-start>10000) reject(new Error('Mount timed out')); else setTimeout(poll,20);
        }; poll(); })\`);
      };
      for (const surface of ['portable','live']) {
        if (surface === 'live') await win.loadURL(${JSON.stringify(new URL(running.url).origin)} + '/_workbench/preview-host.html');
        for (const [state,label] of [['loading','Loading'],['empty','No jobs'],['load-error','Load error']]) {
          if (surface === 'portable') await load('/flex/index.html?state=' + state);
          else await evaluate('window.wbPreviewHost.load(' + JSON.stringify(${JSON.stringify(new URL(running.url).origin)} + '/flex.workbench.ts?state=' + state) + ')');
          assert.equal(await evaluate('document.querySelector("[data-testid=screen]").textContent.trim()'),label);
          for (const [width,height] of [[1512,982],[393,852],[800,600]]) {
            win.setContentSize(width,height);
            await evaluate('new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))');
            const box = await evaluate(\`(() => { const mount=document.querySelector('#workbench-preview').getBoundingClientRect();
              const screen=document.querySelector('[data-testid=screen]').getBoundingClientRect();
              return {mount:mount.width,screen:screen.width,width:innerWidth,height:mount.height,viewportHeight:innerHeight}; })()\`);
            assert.equal(box.mount,box.width,surface + ' ' + state);
            assert.equal(box.screen,box.width,surface + ' ' + state);
            assert.ok(box.height >= box.viewportHeight);
          }
        }
      }
      await load('/native/index.html');
      for (const [width,height] of [[393,852],[800,600],[320,480]]) {
        win.setContentSize(width,height);
        await evaluate('new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))');
        const box = await evaluate(\`(() => { const root=document.querySelector('[data-testid="screen"]');
          const r=root.getBoundingClientRect(), text=root.firstElementChild.getBoundingClientRect();
          return {height:r.height,width:r.width,viewport:innerHeight,viewportWidth:innerWidth,
            centered:Math.abs(text.top+text.height/2-innerHeight/2)<1}; })()\`);
        assert.equal(box.height,box.viewport); assert.equal(box.width,box.viewportWidth); assert.ok(box.centered);
      }
      await evaluate(\`(() => { const style=document.createElement('style'); style.textContent='#workbench-preview { display:block; }'; document.head.append(style); })()\`);
      assert.equal(await evaluate('getComputedStyle(document.querySelector("#workbench-preview")).display'), 'block');
      await load('/web/index.html');
      assert.equal(await evaluate('getComputedStyle(document.querySelector("#workbench-preview")).display'), 'block');
      assert.ok(await evaluate('document.querySelector("[data-testid=screen]").getBoundingClientRect().height < innerHeight'));
      await load('/native-docs/index.html');
      assert.equal(await evaluate('document.querySelector(".sized [data-testid=screen]").getBoundingClientRect().height'),200);
      assert.equal(await evaluate('getComputedStyle(document.querySelector(".sized")).paddingTop'),'32px');
      assert.ok(await evaluate('document.querySelector(".intrinsic").getBoundingClientRect().height < innerHeight'));
      await evaluate('window.stops.forEach(stop => stop())');
      assert.equal(await evaluate('document.querySelectorAll(".wb-react-native-root").length'),0);
      await load('/web-docs/index.html');
      assert.equal(await evaluate('getComputedStyle(document.querySelector(".stage")).display'),'block');
      win.destroy(); app.exit(0);
    }).catch(error => { console.error(error); app.exit(1); });`);
  // Use a test-owned copy of the packaged app so its entry can run assertions.
  const binary = await require('./electron-runtime').prepare(path.join(root, 'runtime'));
  const appMain = process.platform === 'darwin'
    ? path.resolve(path.dirname(binary), '../Resources/app/main.cjs')
    : path.join(path.dirname(binary), 'resources/app/main.cjs');
  fs.copyFileSync(runner, appMain);
  const env = { ...process.env }; delete env.ELECTRON_RUN_AS_NODE;
  const child = require('node:child_process').spawn(binary, ['--user-data-dir=' + path.join(root, 'browser-profile'), runner], { env });
  t.after(() => { if (child.exitCode === null) child.kill(); });
  let output = '';
  child.stdout.on('data', data => { output += data; });
  child.stderr.on('data', data => { output += data; });
  const code = await new Promise((resolve, reject) => { child.on('error', reject); child.on('exit', resolve); });
  assert.equal(code, 0, output);
});

test('builds run the project’s Rspack plugins and rules, and esbuild plugins are reported', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('acme', './button.acme'),
    'button.acme': 'Custom adapter',
    'adapter.ts': 'export function mount(canvas, source, ctx) { canvas.textContent = source + ctx.inputs.label + BUILD_LABEL; }',
    'workbench.config.ts': `import { defineConfig } from '@canonic2/workbench';
export default defineConfig({
  adapters: { acme: { runtime: './adapter.ts', rules: [{ test: /\\.acme$/, type: 'asset/source' }] } },
  plugins: [{ apply(compiler) { new compiler.rspack.DefinePlugin({ BUILD_LABEL: JSON.stringify('ACME_PLUGIN') }).apply(compiler); } }],
});`,
  });
  const built = await build(t, root, 'button.workbench.ts');
  const js = built.outputs.get('preview.js').toString();
  assert.match(js, /Custom adapter/);
  assert.match(js, /ACME_PLUGIN/);
  assert.ok(built.localFiles.includes(path.join(root, 'workbench.config.ts')));
  const { projectExtensions } = require('./src/modules/preview-pipeline/index.ts');
  const checked = projectExtensions({ plugins: [{ name: 'acme-old', setup() {} }, { apply() {} }], adapters: { 'react-native-web': { plugins: [{ apply() {} }] } } }, 'web');
  assert.equal(checked.plugins.length, 1, 'an adapter’s plugins apply only to its own compiler');
  assert.match(checked.problems[0], /“acme-old” is an esbuild plugin/);
});

test('builds follow the evaluated configuration and only discovered definitions build', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas) { canvas.textContent = CONFIG_LABEL; }',
    'label.txt': 'ACME_FIRST_CONFIG',
    'workbench.config.ts': `import fs from 'node:fs'; export default { define: {
      CONFIG_LABEL: JSON.stringify(fs.readFileSync(__dirname + '/label.txt', 'utf8')) } };`,
    '.hidden/button.workbench.ts': definition('html', '../button.ts'),
    'dist/button.workbench.ts': definition('html', '../button.ts'),
  });
  const first = await build(t, root, 'button.workbench.ts');
  assert.match(first.outputs.get('preview.js').toString(), /ACME_FIRST_CONFIG/);
  fs.writeFileSync(path.join(root, 'label.txt'), 'ACME_CHANGED_CONFIG');
  const changed = await build(t, root, 'button.workbench.ts');
  assert.notEqual(changed.revision, first.revision);
  assert.match(changed.outputs.get('preview.js').toString(), /ACME_CHANGED_CONFIG/);
  assert.throws(() => new Compiler(root, { include: ['src/**'] }).declared('button.workbench.ts'), /declared/);
  assert.throws(() => new Compiler(root).declared('.hidden/button.workbench.ts'), /declared/);
  assert.throws(() => new Compiler(root).declared('dist/button.workbench.ts'), /declared/);
  fs.symlinkSync(path.join(root, 'button.workbench.ts'), path.join(root, 'linked.workbench.ts'));
  assert.throws(() => new Compiler(root).declared('linked.workbench.ts'), /declared/);
  await assert.rejects(build(t, root, 'linked.workbench.ts'), /declared/);
  fs.unlinkSync(path.join(root, 'button.ts'));
  await assert.rejects(build(t, root, 'button.workbench.ts'), /source must exist/);
});

// Reads the server's event stream until `test` accepts an event, or the deadline.
function nextEvent(url, test, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const request = require('node:http').get(url, response => {
      let buffer = '';
      response.on('data', chunk => {
        buffer += chunk;
        for (let at; (at = buffer.indexOf('\n\n')) > -1;) {
          const block = buffer.slice(0, at); buffer = buffer.slice(at + 2);
          const data = block.split('\n').find(line => line.startsWith('data: '));
          if (!data) continue;
          const event = JSON.parse(data.slice(6));
          if (test(event)) { clearTimeout(timer); request.destroy(); resolve(event); return; }
        }
      });
    });
    const timer = setTimeout(() => { request.destroy(); reject(new Error('No matching event')); }, timeoutMs);
    request.on('error', error => { if (!request.destroyed) reject(error); });
  });
}

test('the server serves previews from the live pipeline, pushes builds, exports runnable output and closes its worker', async t => {
  const diagnostics = [];
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'button.workbench.ts': definition('html', './button.ts', "sizes: ['mobile'], controls: { label: { type: 'text' } },"),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
  });
  const running = await server.start({ root, onLog: entry => diagnostics.push(entry), capture: { capture: async () => Buffer.from('jpeg'), close: () => Promise.resolve() } });
  t.after(() => running.close());
  const base = `http://127.0.0.1:${running.port}`;
  const config = await (await fetch(base + '/_workbench/config')).json();
  assert.deepEqual(config.problems, []);
  const preview = config.catalogCollections[0].items[0];
  assert.equal(preview.src, 'button.workbench.ts');
  assert.equal(preview.workbench, true);
  const page = await fetch(base + '/button.workbench.ts?state=disabled');
  const html = await page.text();
  assert.match(page.headers.get('content-type'), /text\/html/);
  assert.match(html, /<head><script src="\/_workbench\/preview-compat\.js"><\/script>/);
  assert.equal((html.match(/preview-compat\.js/g) || []).length, 1);
  assert.doesNotMatch(html, /<script src="\/_workbench\/(keys|actions|states)\.js"/);
  assert.match(html, /"push":true/);
  const compatibility = await fetch(base + '/_workbench/preview-compat.js');
  assert.equal(compatibility.status, 200);
  assert.equal(await compatibility.text(), require('./preview-scripts').source);
  // The preview module names the build's scripts; they are content-hashed and immutable.
  const modulePath = /import \{ mount \} from "([^"]+)"/.exec(html)[1];
  const scriptsOf = async () => {
    const answer = await fetch(base + modulePath);
    assert.equal(answer.status, 200);
    assert.match(answer.headers.get('content-type'), /javascript/);
    return JSON.parse(/const scripts = (\[[^\]]*\]);/.exec(await answer.text())[1]);
  };
  const scripts = await scriptsOf();
  assert.ok(scripts.length >= 2);
  const script = await fetch(base + scripts[scripts.length - 1]);
  assert.equal(script.status, 200);
  assert.match(script.headers.get('cache-control'), /immutable/);
  // An edit is built and pushed; the preview module then names new scripts.
  const pushed = nextEvent(base + '/_workbench/previews/rspack/events', event => event.type === 'built');
  await new Promise(resolve => setTimeout(resolve, 300));
  fs.writeFileSync(path.join(root, 'button.ts'), 'export default function(canvas) { canvas.textContent = "Changed"; }');
  const built = await pushed;
  assert.equal(built.profile, 'web');
  assert.deepEqual(built.errors, {});
  assert.notDeepEqual(await scriptsOf(), scripts);
  const timing = diagnostics.find(entry => entry.event === 'preview.request.completed' && entry.details.route === '/page');
  assert.ok(timing);
  assert.equal(timing.details.file, 'button.workbench.ts');
  assert.ok(timing.details.elapsedMs >= timing.details.queueMs);
  const bundle = await fetch(base + '/_workbench/export');
  assert.equal(bundle.status, 200);
  assert.ok((await bundle.arrayBuffer()).byteLength > 1000);
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\npreviews: false\n');
  const disabled = await (await fetch(base + '/_workbench/config')).json();
  assert.equal(disabled.catalogCollections.length, 0);
  assert.equal((await fetch(base + '/button.workbench.ts')).status, 404);
  assert.equal((await fetch(base + modulePath)).status, 404);
});

test('untrusted and disabled workspaces never execute preview definitions', async t => {
  for (const disabled of [true, false]) {
    const root = fixture(t, {
      'workbench.yaml': 'name: Acme\n' + (disabled ? 'previews: false\n' : ''),
      'button.workbench.ts': 'import fs from "node:fs"; fs.writeFileSync(__dirname + "/executed", "yes"); export default {};',
    });
    const running = await server.start({ root, isTrusted: disabled, capture: { close: () => Promise.resolve() } });
    try {
      const config = await running.config();
      assert.equal(fs.existsSync(path.join(root, 'executed')), false);
      assert.equal(config.catalogCollections.length, 0);
    } finally { await running.close(); }
  }
});

test('manual preview placement receives definition states and stays unique', () => {
  const original = [{ name: 'Authored', items: [{ label: 'My button', src: 'button.workbench.ts', implementations: { storybook: { title: 'Button' } } }] }];
  const imported = [{ name: 'Components', items: [{ label: 'Button', src: 'button.workbench.ts', workbench: true, states: [{ id: 'default', label: 'Default' }] }] }];
  const merged = manifest.mergeCollections(original, imported);
  assert.equal(merged.length, 1);
  assert.equal(merged[0].items[0].label, 'My button');
  assert.equal(merged[0].items[0].workbench, true);
  assert.ok(merged[0].items[0].implementations.storybook);
  assert.equal(original[0].items[0].workbench, undefined);
});

test('a Workbench implementation maps a design to managed preview states', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\nimplementations:\n  built:\n    kind: workbench\ncollections:\n  - name: Pages\n    items:\n      - label: Design\n        src: design.html\n        implementations:\n          built: components/button\n',
    'design.html': '<button>Design</button>',
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  try {
    const config = await running.config();
    assert.deepEqual(config.problems, []);
    assert.equal(config.collections[0].items[0].implementations.built.states.disabled, '/button.workbench.ts?state=disabled');
    assert.equal(config.pages['design.html'].code[0].relative, 'button.ts');
    const plan = server.exportCapturePlan(config, `http://127.0.0.1:${running.port}/`);
    assert.ok(plan.captures.some(capture => capture.state === 'default' && capture.url.includes('state=default')));
  } finally { await running.close(); }
});

test('the project environment applies to every preview, or per adapter name', async t => {
  const files = (config) => ({
    'workbench.config.ts': `export default ${config};`,
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, ctx) { canvas.textContent = ctx.inputs.label; }',
    'environment.ts': 'export function setup() { document.title = "ACME_PROJECT_ENVIRONMENT"; }',
  });
  const compiled = async config => (await build(t, fixture(t, files(config)), 'button.workbench.ts')).outputs.get('preview.js').toString();
  assert.match(await compiled(`{ environment: './environment.ts' }`), /ACME_PROJECT_ENVIRONMENT/);
  assert.match(await compiled(`{ environment: { html: './environment.ts' } }`), /ACME_PROJECT_ENVIRONMENT/);
  assert.doesNotMatch(await compiled(`{ environment: { react: './environment.ts' } }`), /ACME_PROJECT_ENVIRONMENT/);
  await assert.rejects(build(t, fixture(t, files(`{ environment: './missing.ts' }`)), 'button.workbench.ts'), /must be a file inside the project/);
});

test('preview links are validated and name previews and states that exist', async t => {
  const page = (id, links) => `import { definePreview } from '@canonic2/workbench';
export default definePreview({ id: '${id}', adapter: 'html', source: { entry: './page.ts' }, states: { default: {}, sent: {} }, links: ${links} });`;
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'page.ts': 'export default function(canvas) { canvas.textContent = "Acme"; }',
    'start.workbench.ts': page('pages/start', `{ '/next/': 'pages/next', '/reset/': { state: 'sent' }, '/gone/': 'pages/gone', '/lost/': { preview: 'pages/next', state: 'lost' } }`),
    'next.workbench.ts': page('pages/next', `{ '/': 'pages/start' }`),
    'broken.workbench.ts': page('pages/broken', `{ '/next/': { label: 'Next' } }`),
  });
  const running = await server.start({ root, capture: { close: () => Promise.resolve() } });
  try {
    const config = await running.config();
    assert.deepEqual(config.pages['start.workbench.ts'].preview.links['/reset/'], { preview: 'pages/start', state: 'sent' });
    assert.equal(config.problems.length, 3);
    assert.match(config.problems[0], /^broken\.workbench\.ts: .*link \/next\/ must name a preview ID, or \{ preview, state \}\.$/);
    assert.deepEqual(config.problems.slice(1), [
      'start.workbench.ts: link /gone/ names unknown Workbench preview “pages/gone”.',
      'start.workbench.ts: link /lost/ names unknown state “lost” of pages/next.',
    ]);
  } finally { await running.close(); }
});

test('portable compiler source closure is hashed and browser files are included in the archive', async t => {
  const root = fixture(t, {
    'workbench.yaml': 'name: Acme\n',
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'import { label } from "./fixture.ts"; export default function(canvas) { canvas.textContent = label; }',
    'fixture.ts': 'export const label = "Hello";',
  });
  const built = await build(t, root, 'button.workbench.ts');
  const view = { name: 'Acme', pages: { 'button.workbench.ts': { label: 'Button', design: path.join(root, 'button.workbench.ts'), code: [] } }, implementations: {} };
  const portable = { previews: [{ id: built.id, file: built.file, files: built.localFiles, packages: built.packages, directory: 'browser/' + built.slug, states: [{ id: 'default' }] }],
    files: Array.from(built.outputs, ([name, body]) => ({ path: 'browser/' + built.slug + '/' + name, data: Buffer.from(body).toString('base64') })), warnings: [] };
  const output = exporter.create(root, view, { portable });
  assert.ok(output.report.pages[0].files.includes('fixture.ts'));
  assert.ok(!output.report.dependencies.some(pkg => pkg.name === '@canonic2/workbench'));
  assert.equal(output.report.browser.previews[0].source, 'button.workbench.ts');
  assert.ok(output.body.includes(Buffer.from('browser/' + built.slug + '/index.html')));
  const first = output.report.pages[0].hash;
  fs.writeFileSync(path.join(root, 'fixture.ts'), 'export const label = "Changed";');
  assert.notEqual(exporter.create(root, view, { portable }).report.pages[0].hash, first);
});

test('authoring declarations infer input types and reject invalid state inputs', t => {
  const cp = require('node:child_process');
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas: HTMLElement) { canvas.textContent = "Hello"; }',
    'tsconfig.json': JSON.stringify({ compilerOptions: { strict: true, noEmit: true, target: 'ES2022', module: 'ESNext', moduleResolution: 'Bundler', types: [] }, include: ['*.ts'] }),
  });
  cp.execFileSync(process.execPath, [path.join(__dirname, 'preview', 'cli.cjs'), 'init', root]);
  const tsc = path.join(path.dirname(require.resolve('typescript')), '..', 'bin', 'tsc');
  const check = () => cp.spawnSync(process.execPath, [tsc, '-p', root], { encoding: 'utf8' });
  const valid = check();
  assert.equal(valid.status, 0, valid.stdout + valid.stderr);
  fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts').replace("label: 'Disabled'", 'label: 123'));
  assert.notEqual(check().status, 0);
});

test('the published package types definitions in installed projects', t => {
  const cp = require('node:child_process');
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-preview-package-'));
  t.after(() => fs.rmSync(work, { recursive: true, force: true }));
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const [packed] = JSON.parse(cp.execFileSync(npm, ['pack', '--json', '--pack-destination', work], { cwd: path.join(__dirname, 'preview'), encoding: 'utf8', shell: process.platform === 'win32' }));
  assert.deepEqual(packed.files.map(file => file.path).sort(), ['LICENSE', 'README.md', 'api.d.ts', 'api.js', 'package.json']);
  const tsc = path.join(path.dirname(require.resolve('typescript')), '..', 'bin', 'tsc');
  for (const [type, resolution] of [['module', 'Bundler'], ['module', 'NodeNext'], ['commonjs', 'NodeNext']]) {
    const root = path.join(work, type + '-' + resolution);
    const installed = path.join(root, 'node_modules', '@canonic2', 'workbench');
    fs.mkdirSync(installed, { recursive: true });
    require('tar').x({ file: path.join(work, packed.filename), cwd: installed, strip: 1, sync: true });
    const module = resolution === 'NodeNext' ? 'NodeNext' : 'ESNext';
    fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ type }));
    fs.writeFileSync(path.join(root, 'tsconfig.json'), JSON.stringify({ compilerOptions: { strict: true, noEmit: true, target: 'ES2022', module, moduleResolution: resolution, types: [] }, include: ['*.ts'] }));
    fs.writeFileSync(path.join(root, 'button.ts'), 'export default function(canvas: HTMLElement) { canvas.textContent = "Hello"; }');
    fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts'));
    const check = () => cp.spawnSync(process.execPath, [tsc, '-p', root], { encoding: 'utf8' });
    const valid = check();
    assert.equal(valid.status, 0, type + ' ' + resolution + ': ' + valid.stdout + valid.stderr);
    fs.writeFileSync(path.join(root, 'button.workbench.ts'), definition('html', './button.ts').replace("label: 'Disabled'", 'label: 123'));
    assert.match(check().stdout, /button\.workbench\.ts.*TS2322/);
  }
});

test('portable build includes the interactive viewer and shared controls while retaining successful previews', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas, context) { canvas.textContent = context.inputs.label; }',
    'broken.workbench.ts': "export default { id: 'broken', adapter: 'unknown', source: { entry: './button.ts' } }",
  });
  const result = await portable.create(new Compiler(root), { name: 'Acme' });
  assert.equal(result.previews.length, 1);
  assert.equal(result.warnings.length, 1);
  const contents = name => Buffer.from(result.files.find(file => file.path === 'browser/' + name).data, 'base64');
  const catalog = JSON.parse(contents('workbench.json'));
  assert.equal(catalog.name, 'Acme');
  assert.deepEqual(catalog.previews[0].states.map(state => state.id), ['default', 'disabled']);
  assert.equal(catalog.previews[0].files, undefined);
  assert.ok(result.files.some(file => file.path === result.previews[0].directory + '/index.html'));
  assert.deepEqual(contents('preview-controls.js'), fs.readFileSync(path.join(__dirname, 'workbench/preview-controls.js')));
  assert.deepEqual(contents('preview-controls.css'), fs.readFileSync(path.join(__dirname, 'workbench/preview-controls.css')));
  assert.deepEqual(contents('CANONIC-LICENSE.txt'), fs.readFileSync(path.join(__dirname, 'LICENSE')));
  const exported = exporter.create(root, { name: 'Acme', pages: {}, implementations: {} }, { portable: result });
  assert.ok(exported.body.includes(Buffer.from('browser/CANONIC-LICENSE.txt')));
  assert.ok(exported.body.includes(contents('CANONIC-LICENSE.txt')));
});

test('portable export compiles only selected previews and filters declared variants', async t => {
  const root = fixture(t, {
    'button.workbench.ts': definition('html', './button.ts'),
    'other.workbench.ts': definition('html', './other.ts').replace('components/button', 'components/other'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Selected"; }',
    'other.ts': 'export default function(canvas) { canvas.textContent = "Unselected"; }',
  });
  const result = await portable.create(new Compiler(root), { selection: { pages: ['button.workbench.ts'], states: ['disabled'], sizes: ['mobile'] } });
  assert.deepEqual(result.previews.map(p => p.file), ['button.workbench.ts']);
  assert.deepEqual(result.previews[0].states.map(s => s.id), ['disabled']);
  assert.deepEqual(result.previews[0].sizes.map(s => s.key), ['mobile']);
  const catalog = JSON.parse(Buffer.from(result.files.find(f => f.path === 'browser/workbench.json').data, 'base64'));
  assert.equal(catalog.previews.length, 1);
});

test('CLI builds a static viewer without starting a server and refuses to overwrite existing content', t => {
  const cp = require('node:child_process');
  const root = fixture(t, { 'workbench.yaml': 'name: Acme\n', 'button.workbench.ts': definition('html', './button.ts'),
    'button.ts': 'export default function(canvas) { canvas.textContent = "Hello"; }' });
  const output = path.join(root, 'output');
  const args = [path.join(__dirname, 'preview/cli.cjs'), 'build', root, output];
  const result = cp.spawnSync(process.execPath, args, { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(JSON.parse(fs.readFileSync(path.join(output, 'workbench.json'))).name, 'Acme');
  assert.ok(fs.existsSync(path.join(output, 'index.html')));
  fs.writeFileSync(path.join(output, 'preserve.txt'), 'User content');
  const repeat = cp.spawnSync(process.execPath, args, { encoding: 'utf8' });
  assert.notEqual(repeat.status, 0);
  assert.match(repeat.stderr, /must be empty/);
  assert.equal(fs.readFileSync(path.join(output, 'preserve.txt'), 'utf8'), 'User content');
});
