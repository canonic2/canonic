// Stands in for astro:content in Workbench previews (see workbench.config.ts).
// The docs pages read product guides through astro:content, which needs
// Astro's application pipeline, so this module renders every guide with
// Astro's own Markdown processor and the site's plugins, and answers
// getCollection and render. It runs in Node with the page's frontmatter.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createComponent, render as template, unescapeHTML } from 'astro/runtime/server/index.js';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const site = 'https://canonic.sh';

// Loaded by path at run time: the processor is a native module, and docs.js
// finds the guides from its own location.
const load = (file: string) => import(pathToFileURL(file).href);

interface Entry { id: string; collection: string; body: string; html: string; headings: unknown[] }

async function renderGuides(product: string): Promise<Entry[]> {
  const docs = path.resolve(root, `../${product}/docs`);
  const files = fs.readdirSync(docs).filter(name => name.endsWith('.md')).sort();
  const { docsMarkdownPlugins, shikiConfig } = await load(path.join(root, 'src/lib/docs.js'));
  const require = createRequire(path.join(root, 'package.json'));
  const { createSatteriMarkdownProcessor } = await load(require.resolve('@astrojs/markdown-satteri'));
  const processor = await createSatteriMarkdownProcessor({ shikiConfig, ...docsMarkdownPlugins({ site }) });
  const entries: Entry[] = [];
  for (const name of files) {
    const file = path.join(docs, name);
    const body = fs.readFileSync(file, 'utf8');
    const { code, metadata } = await processor.render(body, { fileURL: pathToFileURL(file) });
    // The same ids as the glob loader in src/content.config.ts.
    entries.push({ id: name.replace(/\.md$/, '').replace(/^README$/, 'index'), collection: `${product}Docs`, body, html: code, headings: metadata.headings });
  }
  return entries;
}

const entries = (await Promise.all(['workbench', 'shield'].map(renderGuides))).flat();

export async function getCollection(name: string) {
  return entries.filter(entry => entry.collection === name).map(({ id, collection, body }) => ({ id, collection, body }));
}

export async function render(entry: { id: string; collection: string }) {
  const found = entries.find(item => item.id === entry.id && item.collection === entry.collection)!;
  return { Content: createComponent(() => template`${unescapeHTML(found.html)}`), headings: found.headings };
}
