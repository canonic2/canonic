const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');

const SHARED_PREVIEW_RUNTIME = '/_workbench/preview-runtime.js';

test('the UI ships native JavaScript with a complete browser-ready module graph', async () => {
  const root = path.resolve(__dirname, '..');
  const ui = path.join(root, 'ui');
  const lattice = path.join(root, 'node_modules/@canonic2/lattice');
  const expectedImports = {
    '@canonic2/lattice': '/_workbench/node_modules/@canonic2/lattice/src/index.js',
    '@canonic2/lattice/elements':
      '/_workbench/node_modules/@canonic2/lattice/src/modules/elements/elements.js',
  };
  for (const document of [
    'workbench/index.html',
    'renderer/content.html',
    'sidebar/sidebar.html',
  ]) {
    const html = fs.readFileSync(path.join(ui, 'src/app', document), 'utf8');
    const map = /<script type="importmap">([\s\S]*?)<\/script>/.exec(html);
    assert.ok(map, document + ' supplies the shipping import map');
    assert.deepEqual(JSON.parse(map[1]).imports, expectedImports);
    assert.ok(
      map.index < html.indexOf('<script type="module"'),
      document + ' maps imports before modules',
    );
  }
  const files = fs.readdirSync(ui, { recursive: true });
  assert.deepEqual(
    files.filter((file) => /\.(?:[cm]?ts|tsx)$/.test(file)),
    [],
    'the UI contains no TypeScript or declarations',
  );
  const queue = files
    .filter((file) => file.startsWith('src/') && file.endsWith('.js') && !file.endsWith('.test.js'))
    .map((file) => path.join(ui, file))
    .filter((file) => fs.statSync(file).isFile());
  const { Linter } = await import('eslint');
  const linter = new Linter();
  const visited = new Set();
  for (const file of queue) {
    if (visited.has(file)) continue;
    visited.add(file);
    const imports = [];
    const source = fs.readFileSync(file, 'utf8');
    const messages = linter.verify(source, {
      languageOptions: { ecmaVersion: 'latest', sourceType: 'module' },
      plugins: {
        graph: {
          rules: {
            inspect: {
              create() {
                const read = (node) => {
                  if (node.source?.type === 'Literal') imports.push(node.source.value);
                };
                return {
                  ImportDeclaration: read,
                  ExportNamedDeclaration: read,
                  ExportAllDeclaration: read,
                  ImportExpression: read,
                };
              },
            },
          },
        },
      },
      rules: { 'graph/inspect': 'error' },
    });
    assert.deepEqual(messages, [], file + ' parses as native JavaScript');
    for (const specifier of imports) {
      // The preview runtime is served at one address so the preview host and
      // every compiled preview share a single instance of it.
      if (specifier === SHARED_PREVIEW_RUNTIME) continue;
      assert.ok(
        specifier.startsWith('.') || Object.hasOwn(expectedImports, specifier),
        file + ' has no runtime package/host dependency: ' + specifier,
      );
      assert.ok(
        specifier.endsWith('.js') || Object.hasOwn(expectedImports, specifier),
        file + ' imports browser-ready JavaScript: ' + specifier,
      );
      const dependency = Object.hasOwn(expectedImports, specifier)
        ? path.join(root, expectedImports[specifier].slice('/_workbench/'.length))
        : path.resolve(path.dirname(file), specifier);
      assert.ok(fs.existsSync(dependency), file + ' resolves ' + specifier);
      if (dependency.startsWith(lattice + path.sep) && !file.startsWith(lattice + path.sep)) {
        assert.ok(
          ['src/index.js', 'src/modules/elements/elements.js'].some(
            (entry) => dependency === path.join(lattice, entry),
          ),
          'UI imports only Lattice public entry files: ' + specifier,
        );
      }
      assert.ok(
        dependency.startsWith(ui + path.sep) ||
          dependency.startsWith(path.join(root, 'contracts') + path.sep) ||
          dependency.startsWith(lattice + path.sep),
        dependency,
      );
      queue.push(dependency);
    }
  }
  assert.ok(visited.size > 0);
});

test('UI lint enforces component and browser ownership boundaries', async () => {
  const { ESLint } = await import('eslint');
  const root = path.resolve(__dirname, '..');
  const eslint = new ESLint({ cwd: root });
  // Exercise the actual JavaScript component and runtime boundary rules.
  const component = path.join(root, 'ui/src/views/top-bar/components/actions-switch/element.js');
  const runtime = path.join(root, 'ui/src/modules/preview-sessions/runtime.js');
  async function rules(code, filePath) {
    const [result] = await eslint.lintText(code, { filePath });
    assert.ok(!result.fatalErrorCount, JSON.stringify(result.messages));
    return result.messages.map((message) => message.ruleId);
  }
  assert.ok((await rules("import 'node:fs';", runtime)).includes('no-restricted-imports'));
  assert.ok(
    (await rules("import '../../modules/navigation/index.ts';", component)).includes(
      'no-restricted-imports',
    ),
  );
  assert.ok(
    (await rules("void fetch('/_workbench/spaces');", component)).includes('no-restricted-globals'),
  );
  assert.ok(
    (await rules('new Promise(async () => {});', runtime)).includes('no-async-promise-executor'),
  );
  assert.ok(
    (
      await rules(
        'export class Example extends HTMLElement { constructor() { super(); this.attachShadow({mode:"closed"}); } }',
        component,
      )
    ).includes('wc/no-closed-shadow-root'),
  );
  assert.deepEqual(
    await rules(
      'export class Example extends HTMLElement { constructor() { super(); this.attachShadow({mode:"open"}); } }',
      component,
    ),
    [],
  );
});
