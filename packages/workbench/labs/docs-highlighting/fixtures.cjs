const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const product = path.resolve(__dirname, '../..');
const workspace = path.resolve(product, '../..');
const snippets = {
  bash: '# Acme note\nprintf "%s" "$ACME"\n',
  css: '/* Acme note */\n.card { color: red; width: 12px; }\n',
  diff: '@@ -1 +1 @@\n-old\n+new\n',
  javascript:
    'const card = `<Acme>\nλ & "quoted"\n' +
    'Acme long text '.repeat(24) +
    '`;\n/* Acme multi-line\n   comment */\nfunction show() { return 42; }\n',
  jsx: 'const card = <button title="Acme">Hello</button>;\n',
  json: '{\n  "name": "Acme",\n  "count": 42\n}\n',
  markdown: '# Acme\n**bold** and [link](https://example.com)\n',
  scss: '$accent: red;\n.card { color: $accent; }\n',
  typescript:
    'const card: string = `<Acme>\nλ & "quoted"\n' +
    'Acme long text '.repeat(24) +
    '`;\n/* Acme multi-line\n   comment */\nfunction show(): number { return 42; }\n',
  tsx: 'const card = <button title="Acme">Hello</button>;\n',
  html: '<button title="Acme">Hello</button>\n<!-- note -->\n',
  xml: '<card title="Acme">Hello</card>\n<!-- note -->\n',
  yaml: 'name: "Acme"\ncount: 42 # note\n',
};
const aliases = {
  bash: ['bash', 'sh', 'shell', 'zsh'],
  css: ['css'],
  diff: ['diff'],
  javascript: ['javascript', 'js', 'mjs', 'cjs'],
  jsx: ['jsx'],
  json: ['json'],
  markdown: ['markdown', 'md'],
  scss: ['scss'],
  typescript: ['typescript', 'ts', 'mts', 'cts'],
  tsx: ['tsx'],
  html: ['html', 'vue', 'astro'],
  xml: ['xml', 'svg'],
  yaml: ['yaml', 'yml'],
};
const roles = {
  bash: [['# Acme note', 'hljs-comment']],
  css: [['/* Acme note */', 'hljs-comment']],
  diff: [
    ['-old', 'hljs-deletion'],
    ['+new', 'hljs-addition'],
  ],
  javascript: [
    ['const', 'hljs-keyword'],
    ['42', 'hljs-number'],
  ],
  jsx: [['button', 'hljs-name']],
  json: [['42', 'hljs-number']],
  markdown: [['Acme', 'hljs-section']],
  scss: [['$accent', 'hljs-variable']],
  typescript: [
    ['const', 'hljs-keyword'],
    ['42', 'hljs-number'],
  ],
  tsx: [['button', 'hljs-name']],
  html: [
    ['button', 'hljs-name'],
    ['title', 'hljs-attr'],
  ],
  xml: [
    ['card', 'hljs-name'],
    ['title', 'hljs-attr'],
  ],
  yaml: [['42', 'hljs-number']],
};
function generate(root, shape, assets) {
  fs.mkdirSync(root, { recursive: true });
  const write = (name, body) => {
    const file = path.join(root, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, body);
  };
  fs.copyFileSync(path.join(assets, 'acme-mono.woff2'), path.join(root, 'font.woff2'));
  write(
    'image.svg',
    '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#6688aa"/></svg>',
  );
  write(
    'style.css',
    '@font-face{font-family:AcmeLocal;src:url("./font.woff2")} .lab-example{font-family:AcmeLocal,monospace}',
  );
  write('shared.ts', 'export const sentinel = "Acme shared original";\n');
  let first = 'first.md',
    second = 'second.md',
    example = 'basic';
  const expected = {};
  const page = (name, title, code, sourceLines = 0) => {
    const fence = code
      .map(([language, text]) => '```' + language + '\n' + text + '\n```')
      .join('\n\n');
    write(
      name,
      `# ${title}\n\nAcme local documentation.\n\n![Local image](/image.svg)\n\n\`\`\`example basic\n\`\`\`\n\n${fence}\n`,
    );
    expected[name] = code.map(([language, text]) => ({ language, text: text.trimEnd() }));
    write(
      name.replace('.md', '.examples.ts'),
      `import {sentinel} from './shared.ts';\nexport const basic = (canvas: HTMLElement) => {\n${Array.from({ length: sourceLines }, (_, i) => `  // Acme row ${i}: λ <tag> & "quote"`).join('\n')}\n  canvas.innerHTML = '<div class="lab-example">'+sentinel+'</div>';\n};\n`,
    );
  };
  if (shape === 'Overview') {
    for (const dir of ['assets', 'system'])
      fs.cpSync(path.join(workspace, 'demo', dir), path.join(root, dir), { recursive: true });
    const css = fs
      .readFileSync(path.join(root, 'assets/acme.css'), 'utf8')
      .replace(/@import url\([^;]+;/g, '');
    write(
      'assets/acme.css',
      css +
        '\n@font-face{font-family:Inter;src:url("../font.woff2")}\n@font-face{font-family:"JetBrains Mono";src:url("../font.woff2")}\n',
    );
    first = 'system/overview.md';
    second = 'system/colors.md';
    example = 'tokens-in-use';
    expected[first] = [
      {
        language: 'html',
        text: fs
          .readFileSync(path.join(root, first), 'utf8')
          .match(/```html\n([\s\S]*?)```/)[1]
          .trimEnd(),
      },
    ];
  } else {
    const keys = Object.keys(snippets);
    const heavy = () =>
      Array.from({ length: 20 }, (_, i) => {
        const language = keys[i % keys.length];
        const lines = snippets[language].trimEnd().split('\n');
        return [
          language,
          Array.from({ length: 160 }, (_, j) => lines[j % lines.length]).join('\n'),
        ];
      });
    page(first, 'Acme first', shape === 'Small' ? [] : heavy(), shape === 'Code-heavy' ? 997 : 0);
    page(
      second,
      'Acme second',
      shape === 'Small' ? [['json', snippets.json.trimEnd()]] : heavy(),
      shape === 'Code-heavy' ? 997 : 0,
    );
  }
  // More distinct docs destinations exercise the shipping three-idle-frame bound.
  for (let i = 0; i < 5; i++) page(`evict-${i}.md`, `Acme eviction ${i}`, []);
  page('supersede.md', 'Acme delayed destination', []);
  const corpus = Object.entries(aliases).flatMap(([key, names]) =>
    names.map((language) => ({ language, text: snippets[key].trimEnd(), roles: roles[key] })),
  );
  corpus.push(
    { language: ' TS ', text: snippets.typescript.trimEnd(), roles: roles.typescript },
    { language: 'unknown-acme', text: '<img src=x onerror="throw 1"> λ & "quoted"', roles: [] },
    { language: '', text: '<script>throw 1</script>', roles: [] },
  );
  page(
    'corpus.md',
    'Acme language corpus',
    corpus.map((b) => [b.language, b.text]),
  );
  // Markdown fence info trims surrounding whitespace; the highlighter itself is
  // also checked with untrimmed input by the worker corpus transport.
  expected['corpus.md'] = corpus.map((b) => ({ language: b.language.trim(), text: b.text }));
  const items = [
    first,
    second,
    ...Array.from({ length: 5 }, (_, i) => `evict-${i}.md`),
    'corpus.md',
    'supersede.md',
  ];
  write(
    'workbench.yaml',
    `name: Acme highlighting\nimplementations:\n  html:\n    kind: docs\n    label: Docs\n    adapter: html\n    styles:\n      - ${shape === 'Overview' ? 'assets/acme.css' : 'style.css'}\ncollections:\n  - name: Acme\n    items:\n${items.map((src) => `      - label: ${src}\n        src: ${src}\n        implementations:\n          html: ${src.replace('.md', '.examples.ts')}`).join('\n')}\n`,
  );
  let files = 0,
    bytes = 0;
  const hashes = {};
  const walk = (dir) => {
    for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
      const file = path.join(dir, ent.name);
      if (ent.isDirectory()) walk(file);
      else {
        const body = fs.readFileSync(file);
        files++;
        bytes += body.length;
        hashes[path.relative(root, file)] = crypto.createHash('sha256').update(body).digest('hex');
      }
    }
  };
  walk(root);
  return { shape, first, second, example, expected, corpus, files, bytes, hashes };
}
module.exports = { generate, snippets, aliases };
