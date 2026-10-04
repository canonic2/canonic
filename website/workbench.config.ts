// Workbench previews of this site. The docs pages read the Workbench guides
// through astro:content, which needs Astro's application pipeline, so this
// plugin stands in for it: it renders every guide with Astro's own Markdown
// processor and the site's plugins, and answers getCollection and render.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { defineConfig } from '@canonic2/workbench';

const root = __dirname;
const docs = path.resolve(root, '../workbench/docs');
const site = 'https://canonic.sh';

// Loaded by path at run time: the processor is a native module, and docs.js
// finds the guides from its own location.
const load = (file: string) => import(pathToFileURL(file).href);

let rendered: { stamp: string; entries: unknown[] } | null = null;
async function renderGuides() {
  const files = fs.readdirSync(docs).filter(name => name.endsWith('.md')).sort();
  const stamp = files.map(name => name + fs.statSync(path.join(docs, name)).mtimeMs).join();
  if (rendered?.stamp === stamp) return rendered.entries;
  const { docsMarkdownPlugins, shikiConfig } = await load(path.join(root, 'src/lib/docs.js'));
  const require = createRequire(path.join(root, 'package.json'));
  const { createSatteriMarkdownProcessor } = await load(require.resolve('@astrojs/markdown-satteri'));
  const processor = await createSatteriMarkdownProcessor({ shikiConfig, ...docsMarkdownPlugins({ site }) });
  const entries = [];
  for (const name of files) {
    const file = path.join(docs, name);
    const body = fs.readFileSync(file, 'utf8');
    const { code, metadata } = await processor.render(body, { fileURL: pathToFileURL(file) });
    // The same ids as the glob loader in src/content.config.ts.
    entries.push({ id: name.replace(/\.md$/, '').replace(/^README$/, 'index'), body, html: code, headings: metadata.headings });
  }
  rendered = { stamp, entries };
  return entries;
}

const content = {
  name: 'website-docs-content',
  setup(build) {
    build.onResolve({ filter: /^astro:content$/ }, () => ({ path: 'astro:content', namespace: 'website-docs' }));
    build.onLoad({ filter: /.*/, namespace: 'website-docs' }, async () => ({
      resolveDir: root,
      loader: 'js',
      contents: `
import { createComponent, render as template, unescapeHTML } from 'astro/runtime/server/index.js';
const entries = ${JSON.stringify(await renderGuides())};
export async function getCollection(name) {
  return name === 'workbenchDocs' ? entries.map(({ id, body }) => ({ id, body })) : [];
}
export async function render(entry) {
  const found = entries.find(item => item.id === entry.id);
  return { Content: createComponent(() => template\`\${unescapeHTML(found.html)}\`), headings: found.headings };
}`,
    }));
  },
};

export default defineConfig({ plugins: [content] });
