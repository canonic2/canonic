import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { indexPages } from './index.ts';
const require = createRequire(import.meta.url);
const { Compiler } = require('../../../preview/compiler.cjs');

test('native listing matches the compiler for literals, computed definitions, docs, errors and custom globs', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'acme-page-index-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.writeFile(path.join(root, 'page.html'), '<p>Acme</p>');
  await fs.writeFile(path.join(root, 'page.md'), '# Acme');
  await fs.writeFile(path.join(root, 'title.ts'), "export const title = 'Acme/Computed';");
  const helper = "import {definePreview, defineDocs} from '@canonic2/workbench';\n";
  const literal = (id: string, fields = '') => helper + `export default definePreview({id:'${id}',adapter:'html',source:{entry:'./page.html'},${fields}});`;
  const definitions: { [name: string]: string } = {
    'a.workbench.ts': literal('a', "states:{ready:{label:'Ready'},empty:{}},controls:{color:{type:'select',options:['red']}},sizes:['mobile'],links:{'/next':{state:'ready'}},docs:'./page.md',"),
    'A-.workbench.ts': literal('a'),
    'a_.workbench.ts': literal('underscore'),
    'é.workbench.ts': literal('accent'),
    '10.workbench.ts': literal('ten'),
    '2.workbench.ts': literal('two'),
    'computed.workbench.ts': "import {title} from './title';\n" + literal('computed', 'title,'),
    'docs.workbench.ts': helper + "export default defineDocs({id:'docs',docs:'./page.md',lenses:{html:{adapter:'html',examples:'./page.html'}}});",
    'invalid.workbench.ts': literal('invalid', "icon:'BAD',"),
    'missing.workbench.ts': helper + "export default definePreview({id:'missing',adapter:'html',source:{entry:'./missing.html'}});",
    'broken.workbench.ts': 'export default {{{',
    'custom.preview.ts': literal('custom'),
  };
  await Promise.all(Object.entries(definitions).map(([name, body]) => fs.writeFile(path.join(root, name), body)));
  for (const include of [undefined, ['**/*.{workbench,preview}.{ts,tsx}'], ['!**/broken.workbench.ts']]) {
    const options = include ? { include } : {};
    const list = () => indexPages({ root, previews: options, executable: async () => process.execPath,
      fallback: async () => { throw new Error('Native parity must not fall back'); }, diagnostic: () => {} });
    const expected = JSON.parse(JSON.stringify(await new Compiler(root, options).index()));
    assert.deepEqual((await list()).answer, expected);
    assert.deepEqual((await list()).answer, expected);
  }
  // An imported list field invalidates only its consumer; parse failures
  // deliberately retry, because their dependency graph is incomplete.
  await fs.rm(path.join(root, 'broken.workbench.ts'));
  const list = () => indexPages({ root, executable: async () => process.execPath, fallback: async () => { throw new Error('fallback'); }, diagnostic: () => {} });
  await list();
  await fs.writeFile(path.join(root, 'title.ts'), "export const title = 'Acme/Changed title';");
  const changed = await list();
  assert.deepEqual(changed.answer, JSON.parse(JSON.stringify(await new Compiler(root).index())));
  assert.equal(changed.answer.previews.find(entry => entry.id === 'computed')?.title, 'Acme/Changed title');
  assert.equal(changed.timings.evaluated, 1);
  assert.equal(changed.timings.parsed, 0);
});

test('literal-only listing needs no JavaScript executable; missing and failing binaries fall back', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'acme-index-literal-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.writeFile(path.join(root, 'page.html'), '<p>Acme</p>');
  await fs.writeFile(path.join(root, 'page.workbench.ts'), "import {definePreview} from '@canonic2/workbench'; export default definePreview({id:'page',adapter:'html',source:{entry:'page.html'}});");
  const result = await indexPages({ root, executable: async () => { throw new Error('must not start Node'); }, fallback: async () => { throw new Error('fallback'); }, diagnostic: () => {} });
  assert.equal(result.answer.previews.length, 1);
  const digest = (await import('node:crypto')).createHash('sha256').update(path.resolve(root)).digest('hex');
  const answerFile = path.join(os.tmpdir(), 'canonic-workbench-index', digest, 'answer.json');
  await fs.writeFile(answerFile, '{partial');
  const rebuilt = await indexPages({ root, executable: async () => { throw new Error('must not start Node'); }, fallback: async () => { throw new Error('fallback'); }, diagnostic: () => {} });
  assert.deepEqual(rebuilt.answer, result.answer);
  const expected = { previews: [], docs: [], errors: ['fallback'] };
  for (const binary of [path.join(root, 'missing'), process.execPath]) {
    const events: string[] = [];
    const result = await indexPages({ root, binary, executable: async () => process.execPath, fallback: async () => expected, diagnostic: (_level, event) => events.push(event) });
    assert.deepEqual(result.answer, expected);
    assert.deepEqual(events, ['previews.index.fallback']);
  }
  await fs.writeFile(path.join(root, 'page.workbench.ts'), 'while (true) {}\nexport default {};');
  const diagnostics: string[] = [];
  const timedOut = await indexPages({ root, timeoutMs: 100, executable: async () => process.execPath, fallback: async () => expected, diagnostic: (_level, _event, details) => diagnostics.push(String(details.message)) });
  assert.deepEqual(timedOut.answer, expected);
  assert.ok(diagnostics.some(message => message.includes('timed out')));
});

test('source aliases and JavaScript whitespace validation keep compiler parity', async t => {
  const base = await fs.mkdtemp(path.join(os.tmpdir(), 'acme-index-source-'));
  const root = path.join(base, 'project');
  t.after(() => fs.rm(base, { recursive: true, force: true }));
  await fs.mkdir(root);
  await fs.symlink(root, path.join(base, 'alias'), 'junction');
  for (const entry of ['page.html', '\ufeff', '\u0085']) await fs.writeFile(path.join(root, entry), '<p>Acme</p>');
  for (const entry of ['../alias/page.html', '\ufeff', '\u0085']) {
    await fs.writeFile(path.join(root, 'page.workbench.ts'), "import {definePreview} from '@canonic2/workbench'; export default definePreview({id:'page',adapter:'html',source:{entry:" + JSON.stringify(entry) + '}});');
    const actual = await indexPages({ root, executable: async () => process.execPath, diagnostic() {}, fallback: async () => { throw new Error('unexpected fallback'); } });
    assert.deepEqual(actual.answer, JSON.parse(JSON.stringify(await new Compiler(root).index())));
  }
});

test('native listing and the compiler leave out the same .gitignore paths', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'acme-page-index-gitignore-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const write = async (file: string, body: string) => {
    await fs.mkdir(path.dirname(path.join(root, file)), { recursive: true });
    await fs.writeFile(path.join(root, file), body);
  };
  const literal = (id: string) => `import {definePreview} from '@canonic2/workbench';\nexport default definePreview({id:'${id}',adapter:'html',source:{entry:'../page.html'}});`;
  await write('page.html', '<p>Acme</p>');
  await write('.gitignore', 'target/\ngenerated/*.workbench.ts\n!generated/kept.workbench.ts\nignored/\n!ignored/back.workbench.ts\n# a comment\n');
  await write('sub/.gitignore', 'local.workbench.ts\n');
  for (const [file, id] of [['demo/a.workbench.ts', 'demo-a'], ['target/debug/a.workbench.ts', 'target-a'], ['generated/x.workbench.ts', 'generated-x'],
    ['generated/kept.workbench.ts', 'generated-kept'], ['ignored/back.workbench.ts', 'ignored-back'], ['sub/local.workbench.ts', 'sub-local'],
    ['sub/keep.workbench.ts', 'sub-keep'], ['sub/deep/local.workbench.ts', 'sub-deep-local']]) await write(file, literal(id));
  const ids = (answer: { previews: { id: string }[] }) => answer.previews.map(entry => entry.id).sort();
  for (const include of [undefined, ['generated/*.workbench.ts'], ['**/*.{workbench,preview}.ts']]) {
    const options = include ? { include } : {};
    const native = await indexPages({ root, previews: options, executable: async () => process.execPath,
      fallback: async () => { throw new Error('Native parity must not fall back'); }, diagnostic: () => {} });
    const expected = JSON.parse(JSON.stringify(await new Compiler(root, options).index()));
    assert.deepEqual(native.answer, expected);
    if (!include || include[0].startsWith('**')) assert.deepEqual(ids(native.answer), ['demo-a', 'generated-kept', 'sub-keep']);
    else assert.deepEqual(ids(native.answer), ['generated-kept']);
  }
  assert.throws(() => new Compiler(root).declared('target/debug/a.workbench.ts'), /declared/);
  assert.throws(() => new Compiler(root).declared('sub/deep/local.workbench.ts'), /declared/);
  assert.doesNotThrow(() => new Compiler(root).declared('generated/kept.workbench.ts'));
});
