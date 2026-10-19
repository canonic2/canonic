const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');

test('the UI ships native JavaScript with a complete browser-ready module graph', async () => {
  const root = path.resolve(__dirname, '..');
  const ui = path.join(root, 'ui');
  const files = fs.readdirSync(ui, { recursive: true });
  assert.deepEqual(
    files.filter((file) => /\.(?:[cm]?ts|tsx)$/.test(file)),
    [],
    'the UI contains no TypeScript or declarations',
  );
  const queue = files
    .filter((file) => file.startsWith('src/') && file.endsWith('.js') && !file.endsWith('.test.js'))
    .map((file) => path.join(ui, file));
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
      assert.ok(
        specifier.startsWith('.'),
        file + ' has no runtime package/host dependency: ' + specifier,
      );
      assert.ok(
        specifier.endsWith('.js'),
        file + ' imports browser-ready JavaScript: ' + specifier,
      );
      const dependency = path.resolve(path.dirname(file), specifier);
      assert.ok(fs.existsSync(dependency), file + ' resolves ' + specifier);
      assert.ok(
        dependency.startsWith(ui + path.sep) ||
          dependency.startsWith(path.join(root, 'contracts') + path.sep),
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
  const component = path.join(root, 'ui/src/components/actions-switch/element.js');
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
