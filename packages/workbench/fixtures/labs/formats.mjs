// Deterministic Acme project with every native format, for labs that change
// how previews compile and update: React, Vue, Astro, HTML, React Native Web,
// a custom adapter and a docs page, with project plugins in esbuild and Rspack
// form and named edit targets. See README.md.
import fs from 'node:fs';
import path from 'node:path';

// The framework versions Workbench's own tests use, published before 2026-09-28.
export const DEPENDENCIES = {
  react: '19.3.0',
  'react-dom': '19.3.0',
  vue: '3.5.43',
  astro: '7.3.5',
  'react-native-web': '0.21.3',
};

// Previews that leave residue behind on purpose and report what other
// previews left: a listener, a timer, a portal, a body class, a window global
// and a request mock.
const OWNERS = ['page-a', 'page-b', 'vue-counter', 'native-counter', 'canvas-badge', 'landing', 'card'];

const definition = ({ id, title, adapter, entry, exported, owner, extra = '', requests = '', up = '../' }) => `import { definePreview } from '@canonic2/workbench';
import { enter, probe } from '${up}shared/sentinels';

export default definePreview({
  id: '${id}',
  title: '${title}',
  adapter: '${adapter}',
  source: { entry: '${entry}'${exported ? `, export: '${exported}'` : ''} },
${extra}  requests: {
${requests}    'GET /api/only-${owner}': { body: { owner: '${owner}' } },
  },
  setup: () => enter('${owner}'),
  ready: () => probe('${owner}'),
});
`;

const files = {
  'workbench.yaml': `name: Acme formats lab
previews:
  include:
    - src/**/*.workbench.ts
    - src/**/*.workbench.tsx
`,
  'workbench.config.ts': `import { defineConfig } from '@canonic2/workbench';
import { rspackPlugins, rspackRules } from './workbench/plugins';

const config = {
  environment: {
    react: './src/workbench/react-environment.tsx',
    vue: './src/workbench/vue-environment.ts',
    'react-native-web': './src/workbench/native-environment.tsx',
  },
  adapters: { 'acme-canvas': { runtime: './src/adapters/acme-canvas.ts' } },
  define: { __ACME_CONFIG_REVISION__: JSON.stringify('config 0') },
  // Rspack plugins and loader rules; the esbuild forms in plugins.ts are the
  // shape Workbench reports as a problem.
  plugins: rspackPlugins,
  rules: rspackRules,
};

export default defineConfig(config);
`,
  // Plain objects with apply(compiler): the config is bundled with packages
  // external, so Rspack's own classes come from the compiler it is given.
  'workbench/plugins.ts': `import { readFile } from 'node:fs/promises';
import path from 'node:path';

const TOKEN = '__ACME_GREETING__';
const GREETING = 'Hello from the Acme transform';
const BUILD_INFO = "export const buildInfo = 'Acme build 42';";
const SOURCE = /[\\\\/]src[\\\\/].*\\.tsx?$/;
const NATIVE = /[\\\\/]src[\\\\/]native[\\\\/]/;
const platformFile = (root: string, importer: string) =>
  path.join(root, 'src', 'platform', NATIVE.test(importer) ? 'native.ts' : 'web.ts');

interface PluginBuild {
  initialOptions: { absWorkingDir?: string };
  onResolve(options: { filter: RegExp }, callback: (args: { path: string; importer: string }) => { path: string; namespace?: string } | undefined): void;
  onLoad(options: { filter: RegExp; namespace?: string }, callback: (args: { path: string }) => unknown): void;
}

export const esbuildPlugins = [
  {
    name: 'acme-build-info',
    setup(build: PluginBuild) {
      build.onResolve({ filter: /^virtual:acme-build-info$/ }, args => ({ path: args.path, namespace: 'acme-build-info' }));
      build.onLoad({ filter: /.*/, namespace: 'acme-build-info' }, () => ({ contents: BUILD_INFO, loader: 'js' }));
    },
  },
  {
    name: 'acme-greeting',
    setup(build: PluginBuild) {
      build.onLoad({ filter: SOURCE }, async args => {
        const source = await readFile(args.path, 'utf8');
        if (!source.includes(TOKEN)) return undefined;
        return { contents: source.split(TOKEN).join(JSON.stringify(GREETING)), loader: args.path.endsWith('.tsx') ? 'tsx' : 'ts' };
      });
    },
  },
  {
    name: 'acme-platform',
    setup(build: PluginBuild) {
      const root = build.initialOptions.absWorkingDir || process.cwd();
      build.onResolve({ filter: /^@acme\\/platform$/ }, args => ({ path: platformFile(root, args.importer) }));
    },
  },
];

interface ResolveData { request: string; contextInfo?: { issuer?: string } }
interface RspackCompiler {
  context: string;
  rspack?: { NormalModuleReplacementPlugin: new (pattern: RegExp, replace: (data: ResolveData) => void) => { apply(compiler: RspackCompiler): void } };
  webpack?: RspackCompiler['rspack'];
}
const replace = (compiler: RspackCompiler, pattern: RegExp, rewrite: (data: ResolveData) => void) => {
  const { NormalModuleReplacementPlugin } = (compiler.rspack || compiler.webpack)!;
  new NormalModuleReplacementPlugin(pattern, rewrite).apply(compiler);
};

export const rspackPlugins = [
  { name: 'acme-build-info', apply: (compiler: RspackCompiler) => replace(compiler, /^virtual:acme-build-info$/, data => {
    data.request = 'data:text/javascript,' + encodeURIComponent(BUILD_INFO);
  }) },
  { name: 'acme-platform', apply: (compiler: RspackCompiler) => replace(compiler, /^@acme\\/platform$/, data => {
    data.request = platformFile(compiler.context, data.contextInfo?.issuer || '');
  }) },
];

// Rule loaders resolve from the compiler's context, the project root.
export const rspackRules = [
  { test: SOURCE, exclude: /node_modules/, enforce: 'pre', loader: './workbench/greeting-loader.cjs' },
];
`,
  'workbench/greeting-loader.cjs': `// Rspack form of the acme-greeting plugin: the same token, the same text.
module.exports = function greeting(source) {
  return source.split('__ACME_GREETING__').join(JSON.stringify('Hello from the Acme transform'));
};
`,
  'src/types.d.ts': `declare const __ACME_GREETING__: string;
declare const __ACME_CONFIG_REVISION__: string;
declare module 'virtual:acme-build-info' { export const buildInfo: string; }
declare module '@acme/platform' { export const platform: string; }
declare module '*.module.css' { const classes: Record<string, string>; export default classes; }
`,
  'src/platform/web.ts': "export const platform = 'Acme web platform';\n",
  'src/platform/native.ts': "export const platform = 'Acme native platform';\n",
  'src/shared/format.ts': "export const formatTitle = (title: string) => 'Acme: ' + title;\n",
  'src/shared/constants.ts': "export const LABEL = 'Limit';\nexport const LIMIT = 10;\n",
  'src/shared/sentinels.ts': `// Each preview leaves residue behind on purpose, and on entering records in
// <html data-acme-leaks> what any other preview left. A fresh document per
// preview keeps it empty. Nothing here runs at import, so definitions can
// import it in Node.
const OWNERS = ${JSON.stringify(OWNERS)};
const camel = (owner: string) => owner.replace(/-(\\w)/g, (_, char: string) => char.toUpperCase());
const tickKey = (owner: string) => 'acmeTick' + camel(owner).replace(/^\\w/, char => char.toUpperCase());

function leak(found: string) {
  const html = document.documentElement;
  const list = new Set((html.dataset.acmeLeaks || '').split(' ').filter(Boolean));
  list.add(found);
  html.dataset.acmeLeaks = Array.from(list).join(' ');
}

export function enter(owner: string) {
  const html = document.documentElement;
  if (html.dataset.acmeLeaks === undefined) html.dataset.acmeLeaks = '';
  const heard: string[] = [];
  window.dispatchEvent(new CustomEvent('acme-leak-probe', { detail: heard }));
  for (const other of heard) if (other !== owner) leak('listener:' + other);
  for (const node of Array.from(document.querySelectorAll('[data-acme-portal]'))) {
    const other = node.getAttribute('data-acme-portal');
    if (other !== owner) leak('portal:' + other);
  }
  for (const name of Array.from(document.body.classList)) {
    if (name.startsWith('acme-owner-') && name !== 'acme-owner-' + owner) leak('class:' + name.slice('acme-owner-'.length));
  }
  for (const other of OWNERS) {
    if (other !== owner && (window as unknown as Record<string, unknown>)['acmeOwner' + camel(other)]) leak('global:' + other);
  }
  window.addEventListener('acme-leak-probe', event => { (event as CustomEvent<string[]>).detail.push(owner); });
  window.setInterval(() => { document.documentElement.dataset[tickKey(owner)] = '1'; }, 25);
  const portal = document.createElement('div');
  portal.dataset.acmePortal = owner;
  portal.hidden = true;
  document.body.append(portal);
  document.body.classList.add('acme-owner-' + owner);
  (window as unknown as Record<string, unknown>)['acmeOwner' + camel(owner)] = true;
}

// Runs as the preview's ready hook, so its result is in place before readiness.
export async function probe(owner: string) {
  const html = document.documentElement;
  for (const other of OWNERS) delete html.dataset[tickKey(other)];
  await new Promise(resolve => setTimeout(resolve, 80));
  for (const other of OWNERS) if (other !== owner && html.dataset[tickKey(other)]) leak('timer:' + other);
  for (const other of OWNERS) {
    if (other === owner) continue;
    try {
      const response = await fetch('/api/only-' + other);
      const body = await response.json();
      if (body && body.owner === other) leak('request:' + other);
    } catch (_) { /* Not mocked: the request reached the server. */ }
  }
}
`,
  'src/styles/global.css': `:root { --acme-global: rgb(51, 85, 221); }
body { margin: 0; font-family: system-ui, sans-serif; color: #1d2433; }
.acme-global-probe { color: var(--acme-global); }
`,
  // The sentinels acme-app.mjs uses: preview IDs this module instance has
  // rendered, and its live window listeners.
  'src/workbench/react-environment.tsx': `import type { ReactNode } from 'react';
import '../styles/global.css';

const seenPreviews = new Set<string>();
let liveListeners = 0;

export function setup(context: { id: string }) {
  seenPreviews.add(context.id);
  document.documentElement.dataset.acmeSeenPreviews = String(seenPreviews.size);
  const listener = () => {};
  window.addEventListener('acme-ping', listener);
  liveListeners++;
  document.documentElement.dataset.acmeListeners = String(liveListeners);
  return () => {
    window.removeEventListener('acme-ping', listener);
    liveListeners--;
    document.documentElement.dataset.acmeListeners = String(liveListeners);
  };
}

export function wrap(tree: ReactNode) {
  return <div className="acme-root" data-acme-environment="0">{tree}</div>;
}
`,
  'src/workbench/vue-environment.ts': `import { h, type VNode } from 'vue';

export function wrap(tree: VNode) {
  return h('div', { class: 'acme-vue-root' }, [tree]);
}
`,
  'src/workbench/native-environment.tsx': `import type { ReactNode } from 'react';
import { View } from 'react-native';

export function wrap(tree: ReactNode) {
  return <View testID="native-root" style={{ flex: 1 }}>{tree}</View>;
}
`,
  'src/adapters/acme-canvas.ts': `// A project adapter: draws a badge from a plain object source.
export function mount(canvas: HTMLElement, source: { label: string }, context: { inputs: Record<string, unknown> }) {
  const badge = document.createElement('strong');
  badge.dataset.acme = 'canvas-badge';
  badge.textContent = source.label + ': ' + String(context.inputs.text);
  canvas.append(badge);
  return () => badge.remove();
}
`,
  'src/canvas/badge.ts': "export const badge = { label: 'Acme canvas badge' };\n",
  'src/canvas/badge.workbench.ts': definition({ id: 'canvas/badge', title: 'Acme/Custom/Badge', adapter: 'acme-canvas', entry: './badge.ts', exported: 'badge', owner: 'canvas-badge',
    extra: "  inputs: { text: 'Revision 0' },\n" }),
  'src/vue/Counter.vue': `<script setup lang="ts">
import { ref } from 'vue';

const label = 'Count';
const count = ref(0);
const note = ref('');
</script>

<template>
  <section class="counter" data-acme="interactive">
    <button type="button" data-acme="increment" @click="count++">{{ label }} {{ count }}</button>
    <input v-model="note" data-acme="note" aria-label="Note" />
    <span data-acme="marker">Revision 0</span>
    <p class="probe" data-acme-probe="vue">Scoped style</p>
  </section>
</template>

<style scoped>
.counter { display: grid; gap: 8px; padding: 24px; }
.probe { --acme-vue: rgb(136, 51, 170); color: var(--acme-vue); }
</style>
`,
  'src/vue/counter.workbench.ts': definition({ id: 'vue/counter', title: 'Acme/Vue/Counter', adapter: 'vue', entry: './Counter.vue', owner: 'vue-counter' }),
  'src/native/Counter.tsx': `import { useState } from 'react';
import { Pressable, Text, View } from 'react-native';
import { platform } from '@acme/platform';

export function Counter() {
  const [count, setCount] = useState(0);
  return (
    <View style={{ padding: 24, gap: 8 }}>
      <Text testID="platform">{platform}</Text>
      <Pressable testID="increment" accessibilityRole="button" onPress={() => setCount(value => value + 1)}>
        <Text>Count {count}</Text>
      </Pressable>
      <Text testID="marker">Revision 0</Text>
    </View>
  );
}
`,
  'src/native/counter.workbench.tsx': definition({ id: 'native/counter', title: 'Acme/Native/Counter', adapter: 'react-native-web', entry: './Counter.tsx', exported: 'Counter', owner: 'native-counter' }),
  'src/html/landing.html': `<!doctype html>
<html>
<head>
  <link rel="stylesheet" href="./landing.css">
</head>
<body class="acme-landing">
  <main>
    <h1 data-acme="marker">Revision 0</h1>
    <img src="./hero.svg" alt="Acme hero" width="320" height="160">
    <p class="probe" data-acme-probe="html">Linked style</p>
    <p data-acme="script">Script pending</p>
  </main>
  <script>document.querySelector('[data-acme="script"]').textContent = 'Script ran';</script>
</body>
</html>
`,
  'src/html/landing.css': `main { padding: 24px; }
.probe { --acme-html: rgb(221, 119, 0); color: var(--acme-html); }
`,
  'src/html/hero.svg': '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="160" viewBox="0 0 320 160"><rect width="320" height="160" fill="#d7e0fb"/><circle cx="160" cy="80" r="48" fill="#3355dd"/></svg>\n',
  'src/html/landing.workbench.ts': definition({ id: 'html/landing', title: 'Acme/HTML/Landing', adapter: 'html', entry: './landing.html', owner: 'landing' }),
  'src/astro/Card.astro': `---
interface Props { title: string; tone?: string }
const { title, tone = 'neutral' } = Astro.props;
---
<article class={\`card card-\${tone}\`} data-acme="card">
  <h2 data-acme="title">{title}</h2>
  <span data-acme="marker">Revision 0</span>
  <p class="probe" data-acme-probe="astro">Astro style</p>
</article>
<style>
  .card { padding: 24px; }
  .card-accent { border: 2px solid currentColor; }
  .probe { --acme-astro: rgb(0, 136, 136); color: var(--acme-astro); }
</style>
`,
  'src/astro/card.workbench.ts': definition({ id: 'astro/card', title: 'Acme/Astro/Card', adapter: 'astro', entry: './Card.astro', owner: 'card',
    extra: "  inputs: { title: 'Acme card' },\n  scenarios: { default: {}, accent: { inputs: { tone: 'accent' } } },\n" }),
  'src/docs/buttons.workbench.ts': `import { defineDocs } from '@canonic2/workbench';

export default defineDocs({
  id: 'docs/buttons',
  title: 'Acme/Docs/Buttons',
  docs: './buttons.md',
  lenses: {
    web: { label: 'Web', adapter: 'react', examples: './examples/' },
    native: { label: 'React Native Web', adapter: 'react-native-web', examples: './native-examples/' },
  },
  lens: 'web',
});
`,
  'src/docs/buttons.md': `# Buttons

<p data-acme="docs-marker">Revision 0</p>

## Primary

\`\`\`example primary
\`\`\`

## Secondary

\`\`\`example secondary
\`\`\`
`,
  'src/docs/examples/primary.tsx': 'export default function Primary() {\n  return <button type="button" data-acme="example-primary">Primary, revision 0</button>;\n}\n',
  'src/docs/examples/secondary.tsx': 'export default function Secondary() {\n  return <button type="button" data-acme="example-secondary">Secondary</button>;\n}\n',
  'src/docs/native-examples/primary.tsx': "import { Pressable, Text } from 'react-native';\n\nexport default function Primary() {\n  return <Pressable testID=\"example-primary\" accessibilityRole=\"button\"><Text>Primary, revision 0</Text></Pressable>;\n}\n",
  'src/docs/native-examples/secondary.tsx': "import { Pressable, Text } from 'react-native';\n\nexport default function Secondary() {\n  return <Pressable testID=\"example-secondary\" accessibilityRole=\"button\"><Text>Secondary</Text></Pressable>;\n}\n",
  'src/pages/broken-compile/Page.tsx': "import { Missing } from './does-not-exist';\n\nexport function Page() {\n  return <Missing />;\n}\n",
  'src/pages/broken-compile/index.workbench.tsx': "import { definePreview } from '@canonic2/workbench';\n\nexport default definePreview({\n  id: 'pages/broken-compile',\n  title: 'Acme/Broken/Compile',\n  adapter: 'react',\n  source: { entry: './Page.tsx', export: 'Page' },\n});\n",
  'src/pages/broken-mount/Page.tsx': "export function Page(): never {\n  throw new Error('Acme broken mount');\n}\n",
  'src/pages/broken-mount/index.workbench.tsx': "import { definePreview } from '@canonic2/workbench';\n\nexport default definePreview({\n  id: 'pages/broken-mount',\n  title: 'Acme/Broken/Mount',\n  adapter: 'react',\n  source: { entry: './Page.tsx', export: 'Page' },\n});\n",
};

for (const [slug, name] of [['page-a', 'Page A'], ['page-b', 'Page B']]) {
  files[`src/pages/${slug}/Page.tsx`] = `import { useEffect, useState } from 'react';
import { buildInfo } from 'virtual:acme-build-info';
import { platform } from '@acme/platform';
import { formatTitle } from '../../shared/format';
import { LABEL, LIMIT } from '../../shared/constants';
import { Interactive } from './Interactive';
import styles from './page.module.css';

export function Page({ title }: { title: string }) {
  const [greeting, setGreeting] = useState('Loading');
  useEffect(() => {
    fetch('/api/greeting').then(response => response.json()).then(body => setGreeting(body.message));
  }, []);
  return (
    <main className={styles.page}>
      <h1 data-acme="title">{formatTitle(title)}</h1>
      <p data-acme="transform">{__ACME_GREETING__}</p>
      <p data-acme="virtual">{buildInfo}</p>
      <p data-acme="platform">{platform}</p>
      <p data-acme="constants">{LABEL} {LIMIT}</p>
      <p data-acme="config">{__ACME_CONFIG_REVISION__}</p>
      <p data-acme="request">{greeting}</p>
      <p className={styles.probe} data-acme-probe="module">Module style</p>
      <p className="acme-global-probe" data-acme-probe="global">Global style</p>
      <Interactive />
    </main>
  );
}
`;
  files[`src/pages/${slug}/Interactive.tsx`] = `import { useState } from 'react';

export function Interactive() {
  const [count, setCount] = useState(0);
  const [note, setNote] = useState('');
  return (
    <section data-acme="interactive">
      <button type="button" data-acme="increment" onClick={() => setCount(value => value + 1)}>Count {count}</button>
      <input data-acme="note" value={note} onChange={event => setNote(event.target.value)} aria-label="Note" />
      <span data-acme="marker">Revision 0</span>
    </section>
  );
}
`;
  files[`src/pages/${slug}/page.module.css`] = `.page { padding: 24px; }
.probe { --acme-module: rgb(34, 136, 68); color: var(--acme-module); }
`;
  files[`src/pages/${slug}/index.workbench.tsx`] = definition({ id: `pages/${slug}`, title: `Acme/Pages/${name}`, adapter: 'react', entry: './Page.tsx', exported: 'Page', owner: slug, up: '../../',
    extra: `  inputs: { title: '${name}' },\n`, requests: `    'GET /api/greeting': { body: { message: 'Hello from the ${name} mock' } },\n` });
}

const PREVIEWS = [
  { id: 'pages/page-a', file: 'src/pages/page-a/index.workbench.tsx', adapter: 'react', source: 'src/pages/page-a/Page.tsx', owner: 'page-a', interactive: 'react' },
  { id: 'pages/page-b', file: 'src/pages/page-b/index.workbench.tsx', adapter: 'react', source: 'src/pages/page-b/Page.tsx', owner: 'page-b', interactive: 'react' },
  { id: 'vue/counter', file: 'src/vue/counter.workbench.ts', adapter: 'vue', source: 'src/vue/Counter.vue', owner: 'vue-counter', interactive: 'vue' },
  { id: 'astro/card', file: 'src/astro/card.workbench.ts', adapter: 'astro', source: 'src/astro/Card.astro', owner: 'card', scenarios: ['default', 'accent'] },
  { id: 'html/landing', file: 'src/html/landing.workbench.ts', adapter: 'html', source: 'src/html/landing.html', owner: 'landing' },
  { id: 'native/counter', file: 'src/native/counter.workbench.tsx', adapter: 'react-native-web', source: 'src/native/Counter.tsx', owner: 'native-counter' },
  { id: 'canvas/badge', file: 'src/canvas/badge.workbench.ts', adapter: 'acme-canvas', source: 'src/canvas/badge.ts', owner: 'canvas-badge' },
  { id: 'pages/broken-compile', file: 'src/pages/broken-compile/index.workbench.tsx', adapter: 'react', source: 'src/pages/broken-compile/Page.tsx', broken: 'compile' },
  { id: 'pages/broken-mount', file: 'src/pages/broken-mount/index.workbench.tsx', adapter: 'react', source: 'src/pages/broken-mount/Page.tsx', broken: 'mount' },
];

// A docs page opens on the canvas at its Markdown address, `~<lens>` for a lens.
const DOCS = [{ id: 'docs/buttons', file: 'src/docs/buttons.workbench.ts', markdown: 'src/docs/buttons.md', address: 'src/docs/buttons.md', lenses: { web: 'react', native: 'react-native-web' } }];

// What each plugin makes visible on a React page, and on the native screen.
const PLUGIN_CHECKS = [
  { plugin: 'acme-build-info', preview: 'pages/page-a', selector: '[data-acme="virtual"]', text: 'Acme build 42' },
  { plugin: 'acme-greeting', preview: 'pages/page-a', selector: '[data-acme="transform"]', text: 'Hello from the Acme transform' },
  { plugin: 'acme-platform', preview: 'pages/page-a', selector: '[data-acme="platform"]', text: 'Acme web platform' },
  { plugin: 'acme-platform', preview: 'native/counter', selector: '[data-testid="platform"]', text: 'Acme native platform' },
];

// Stage edits: replace `find` with `replace` in `file`, then read `observe` in
// `preview`. `state` names what an interactive preview keeps when an edit
// applies in place: click `increment`, type into `note`, and read both back.
const color = (selector, from, to) => ({ find: from, replace: to, observe: { selector, style: 'color', before: from, expect: to } });
const text = (selector, before, expect) => ({ observe: { selector, text: expect, before } });
const EDITS = {
  cssGlobal: { preview: 'pages/page-a', file: 'src/styles/global.css', ...color('[data-acme-probe="global"]', 'rgb(51, 85, 221)', 'rgb(204, 34, 68)') },
  cssModule: { preview: 'pages/page-a', file: 'src/pages/page-a/page.module.css', ...color('[data-acme-probe="module"]', 'rgb(34, 136, 68)', 'rgb(204, 34, 68)') },
  vueScoped: { preview: 'vue/counter', file: 'src/vue/Counter.vue', ...color('[data-acme-probe="vue"]', 'rgb(136, 51, 170)', 'rgb(204, 34, 68)') },
  reactComponent: { preview: 'pages/page-a', file: 'src/pages/page-a/Interactive.tsx', find: 'Revision 0', replace: 'Revision 1', ...text('[data-acme="marker"]', 'Revision 0', 'Revision 1') },
  nonComponentModule: { preview: 'pages/page-a', file: 'src/shared/constants.ts', find: 'LIMIT = 10', replace: 'LIMIT = 11', ...text('[data-acme="constants"]', 'Limit 10', 'Limit 11') },
  vueTemplate: { preview: 'vue/counter', file: 'src/vue/Counter.vue', find: 'Revision 0', replace: 'Revision 1', ...text('[data-acme="marker"]', 'Revision 0', 'Revision 1') },
  vueScript: { preview: 'vue/counter', file: 'src/vue/Counter.vue', find: "const label = 'Count';", replace: "const label = 'Clicks';", ...text('[data-acme="increment"]', 'Count', 'Clicks') },
  htmlMarkup: { preview: 'html/landing', file: 'src/html/landing.html', find: 'Revision 0', replace: 'Revision 1', ...text('[data-acme="marker"]', 'Revision 0', 'Revision 1') },
  htmlStylesheet: { preview: 'html/landing', file: 'src/html/landing.css', ...color('[data-acme-probe="html"]', 'rgb(221, 119, 0)', 'rgb(204, 34, 68)') },
  astroComponent: { preview: 'astro/card', file: 'src/astro/Card.astro', find: 'Revision 0', replace: 'Revision 1', ...text('[data-acme="marker"]', 'Revision 0', 'Revision 1') },
  docsMarkdown: { docs: 'docs/buttons', address: 'src/docs/buttons.md~web', file: 'src/docs/buttons.md', find: 'Revision 0', replace: 'Revision 1', ...text('[data-acme="docs-marker"]', 'Revision 0', 'Revision 1') },
  docsExample: { docs: 'docs/buttons', address: 'src/docs/buttons.md~web', file: 'src/docs/examples/primary.tsx', find: 'Primary, revision 0', replace: 'Primary, revision 1', ...text('[data-acme="example-primary"]', 'Primary, revision 0', 'Primary, revision 1') },
  definition: { preview: 'pages/page-a', file: 'src/pages/page-a/index.workbench.tsx', find: "inputs: { title: 'Page A' }", replace: "inputs: { title: 'Page A edited' }", ...text('[data-acme="title"]', 'Acme: Page A', 'Acme: Page A edited') },
  environment: { preview: 'pages/page-a', file: 'src/workbench/react-environment.tsx', find: 'data-acme-environment="0"', replace: 'data-acme-environment="1"', observe: { selector: '.acme-root', attribute: 'data-acme-environment', before: '0', expect: '1' } },
  config: { preview: 'pages/page-a', file: 'workbench.config.ts', find: "'config 0'", replace: "'config 1'", ...text('[data-acme="config"]', 'config 0', 'config 1') },
  shared: { preview: 'pages/page-a', file: 'src/shared/format.ts', find: "'Acme: '", replace: "'Acme / '", ...text('[data-acme="title"]', 'Acme: Page A', 'Acme / Page A') },
  syntaxError: { preview: 'pages/page-a', file: 'src/pages/page-a/Interactive.tsx',
    break: { find: '<section data-acme="interactive">', replace: '<section data-acme="interactive"' },
    fix: { find: '<section data-acme="interactive"\n', replace: '<section data-acme="interactive">\n' },
    ...text('[data-acme="marker"]', 'Revision 0', 'Revision 0') },
};

const STATE = {
  react: { increment: '[data-acme="increment"]', count: '[data-acme="increment"]', note: '[data-acme="note"]' },
  vue: { increment: '[data-acme="increment"]', count: '[data-acme="increment"]', note: '[data-acme="note"]' },
};

/**
 * Writes the formats project into `root`, which must be empty or missing.
 * Returns a manifest describing what it generated.
 */
export function generate(root, { seed = 1 } = {}) {
  if (fs.existsSync(root) && fs.readdirSync(root).length) throw new Error('Fixture directory is not empty: ' + root);
  let bytes = 0;
  const all = { 'package.json': JSON.stringify({ name: 'acme-lab-formats', private: true, type: 'module', dependencies: DEPENDENCIES }, null, 2) + '\n', ...files };
  for (const [file, contents] of Object.entries(all)) {
    const target = path.join(root, file);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, contents);
    bytes += Buffer.byteLength(contents);
  }
  const manifest = { size: 'formats', seed, files: Object.keys(all).length, sourceBytes: bytes, dependencies: DEPENDENCIES,
    previews: PREVIEWS, docs: DOCS, owners: OWNERS, pluginChecks: PLUGIN_CHECKS, state: STATE, edits: EDITS };
  fs.writeFileSync(path.join(root, 'acme-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
  return manifest;
}
