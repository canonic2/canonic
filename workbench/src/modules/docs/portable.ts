/* Docs pages for the portable export's browser/ folder: each page in each
   lens becomes a static page beside its production examples bundle, with the
   page script compiled once, every example's code for Show code as a JSON
   file, and the images its Markdown references. A static server opens them;
   nothing is rebuilt or fetched from Workbench. */

import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { highlight, languageOf } from './code-highlight.ts';
import { readDocsMarkdown } from './docs-markdown.ts';
import { createDocsService, lensRequest, type DocsBundle, type DocsLens, type DocsPageEntry, type ListedExample } from './docs-service.ts';
import { exportSource } from './export-source.ts';

const HERE = path.dirname(fileURLToPath(import.meta.url));

interface Compiled {
  revision: string;
  outputs: Map<string, Buffer>;
  stylesheet: boolean;
  examples: ListedExample[];
  problems: string[];
}

export interface PortableCompiler {
  root: string;
  compileDocs(request: object, development: boolean): Promise<Compiled>;
}

export interface PortableDocsPage {
  id: string;
  title: string;
  src: string;
  lens: string | null;
  lenses: { key: string; label: string; directory: string }[];
  examples: { id: string; label: string }[];
}

type Add = (name: string, body: string | Buffer) => void;

/** `docs/card.md` → `docs-card`: a folder name for one page's lenses. */
export function pageSlug(src: string): string {
  return src.replace(/\.md$/i, '').replace(/[^a-z0-9]+/gi, '-').replace(/^-+|-+$/g, '').toLowerCase() || 'page';
}

/* Images the Markdown references by project path are copied beside the page. */
function copyImages(html: string, root: string, directory: string, add: Add): string {
  return html.replace(/(<img\b[^>]*\ssrc=")(\/[^"]+)"/g, (whole, before: string, href: string) => {
    let relative: string;
    try { relative = decodeURIComponent(href.slice(1).split(/[?#]/)[0]!); } catch { return whole; }
    const file = path.resolve(root, relative);
    if (!file.startsWith(root + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) return whole;
    add(directory + '/files/' + relative, fs.readFileSync(file));
    return before + './files/' + relative.split('/').map(encodeURIComponent).join('/') + '"';
  });
}

async function pageScript(): Promise<string> {
  const esbuild = createRequire(import.meta.url)('../../../preview/engine.cjs');
  const result = await esbuild.build({ entryPoints: [path.join(HERE, 'page', 'docs-page.ts')], bundle: true, write: false,
    format: 'esm', platform: 'browser', target: 'es2020', logLevel: 'silent' });
  return result.outputFiles[0].text;
}

export async function portableDocs(compiler: PortableCompiler, entries: readonly DocsPageEntry[], add: Add): Promise<{ pages: PortableDocsPage[]; warnings: string[] }> {
  const pages: PortableDocsPage[] = [];
  const warnings: string[] = [];
  if (!entries.length) return { pages, warnings };
  const root = compiler.root;
  add('docs/page.js', await pageScript());
  add('docs/docs-page.css', fs.readFileSync(path.join(HERE, 'page', 'docs-page.css')));
  const docsPages = new Set(entries.map(entry => entry.src));
  const readFile = async (file: string) => {
    const target = path.resolve(root, file);
    if (!target.startsWith(root + path.sep) || !fs.existsSync(target)) return null;
    return fs.readFileSync(target, 'utf8');
  };

  for (const entry of entries) {
    const markdown = await readFile(entry.src);
    if (markdown === null) { warnings.push(entry.src + ': the Markdown file doesn’t exist.'); continue; }
    const placements = readDocsMarkdown(markdown).placements.filter(placement => placement.id && !placement.duplicate);
    const page: PortableDocsPage = { id: entry.src, title: entry.label, src: entry.src, lens: entry.lens,
      lenses: [], examples: placements.map(placement => ({ id: placement.id, label: placement.label })) };
    const lenses: (DocsLens | null)[] = entry.lenses.length ? entry.lenses : [null];
    for (const lens of lenses) {
      const directory = 'docs/' + pageSlug(entry.src) + '/' + (lens ? lens.key : 'page');
      let compiled: Compiled | null = null;
      if (lens) {
        try {
          compiled = await compiler.compileDocs(lensRequest(lens), false);
          for (const [name, body] of compiled.outputs) add(directory + '/' + name, body);
          for (const example of compiled.examples) {
            const file = await readFile(example.file);
            const text = file === null ? null : example.export === 'default' ? file : exportSource(file, example.export);
            if (text !== null) {
              add(directory + '/sources/' + example.id, JSON.stringify({ file: example.file, text, html: highlight(text, languageOf(example.file)) }));
            }
          }
        } catch (error) {
          warnings.push(entry.src + ' — ' + lens.label + ': ' + String((error as Error).message ?? error));
        }
      }
      const built = compiled;
      const docs = createDocsService({
        blocked: () => null,
        readFile,
        listExamples: async () => ({ examples: built?.examples ?? [], problems: [] }),
        bundle: async (): Promise<DocsBundle> => {
          if (!built) throw new Error('The examples could not be built for this export.');
          return { module: './examples.js', stylesheet: built.stylesheet ? './examples.css' : null, revision: built.revision,
            examples: built.examples, problems: built.problems };
        },
      });
      const html = await docs.page(entry, { lens: lens?.key ?? null }, docsPages,
        { script: '../../page.js', stylesheet: '../../docs-page.css', source: './sources/', revision: '' });
      add(directory + '/index.html', copyImages(html, root, directory, add));
      page.lenses.push({ key: lens?.key ?? 'page', label: lens?.label ?? 'Page', directory: 'browser/' + directory });
    }
    pages.push(page);
  }
  return { pages, warnings };
}
