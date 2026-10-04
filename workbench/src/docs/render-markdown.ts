/* A docs page's Markdown as HTML: GitHub Flavored Markdown, with each
   ```example <id>``` block replaced by that example's panel. Headings get
   GitHub-style anchors, relative images and links resolve against the
   Markdown file, links to other docs pages are marked so the canvas can open
   them, and fenced code is highlighted. What each panel shows is the caller's
   decision (see placement.ts); this module only lays it out. */

import { Marked, type Tokens } from 'marked';
import { escapeHtml, highlight } from './code-highlight.ts';
import { headingAnchor, readDocsMarkdown, type DocsHeading, type DocsProblem, type ExamplePlacement } from './docs-markdown.ts';

export type PanelStatus = 'ready' | 'missing' | 'invalid' | 'unavailable';

export interface PanelView {
  status: PanelStatus;
  /** Why the example isn't shown, for every status but `ready`. */
  note?: string;
}

export interface RenderInput {
  /** The Markdown text. */
  source: string;
  /** The Markdown file, relative to the project root, with `/` separators. */
  file: string;
  /** The project-relative sources of every declared docs page, for links between them. */
  docsPages: ReadonlySet<string>;
  /** What one placement's panel shows. */
  panel(placement: ExamplePlacement): PanelView;
}

export interface RenderedDocs {
  html: string;
  /** The first level-1 heading's text, for the document title. */
  title: string | null;
  headings: DocsHeading[];
  placements: ExamplePlacement[];
  problems: DocsProblem[];
}

const SCHEME = /^[a-z][a-z0-9+.-]*:/i;

/* Lucide's code and copy icons. */
const CODE_ICON = '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m16 18 6-6-6-6"/><path d="m8 6-6 6 6 6"/></svg>';
const COPY_ICON = '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>';

export function renderDocs(input: RenderInput): RenderedDocs {
  const model = readDocsMarkdown(input.source);
  const waiting = new Map<string, ExamplePlacement[]>();
  for (const placement of model.placements) {
    const queue = waiting.get(placement.id) ?? [];
    queue.push(placement);
    waiting.set(placement.id, queue);
  }
  const anchors = new Map<string, number>();
  let title: string | null = null;

  const marked = new Marked({
    gfm: true,
    renderer: {
      heading({ tokens, depth, text }: Tokens.Heading): string {
        const inner = this.parser.parseInline(tokens);
        const base = headingAnchor(text) || 'section';
        const count = anchors.get(base) ?? 0;
        anchors.set(base, count + 1);
        const anchor = count ? `${base}-${count}` : base;
        if (depth === 1 && title === null) title = inner.replace(/<[^>]+>/g, '');
        return `<h${depth} id="${escapeHtml(anchor)}">${inner}</h${depth}>\n`;
      },
      code({ text, lang }: Tokens.Code): string {
        const info = (lang ?? '').trim().split(/\s+/);
        if (info[0] === 'example') {
          const placement = waiting.get(info[1] ?? '')?.shift() ?? waiting.get('')?.shift();
          if (placement) return panelHtml(placement, input.panel(placement));
        }
        return `<pre class="wb-docs-code"><code${info[0] ? ` class="language-${escapeHtml(info[0])}"` : ''}>${highlight(text, info[0])}</code></pre>\n`;
      },
      image({ href, title: imageTitle, text }: Tokens.Image): string {
        return `<img src="${escapeHtml(resolveAgainst(input.file, href))}" alt="${escapeHtml(text)}"${imageTitle ? ` title="${escapeHtml(imageTitle)}"` : ''}>`;
      },
      link({ href, title: linkTitle, tokens }: Tokens.Link): string {
        const inner = this.parser.parseInline(tokens);
        const resolved = resolveAgainst(input.file, href);
        const page = docsPageOf(resolved, input.docsPages);
        return `<a href="${escapeHtml(resolved)}"${page ? ` data-wb-docs-page="${escapeHtml(page)}"` : ''}${linkTitle ? ` title="${escapeHtml(linkTitle)}"` : ''}>${inner}</a>`;
      },
    },
  });

  const html = marked.parse(model.body, { async: false });
  return { html, title, headings: model.headings, placements: model.placements, problems: model.problems };
}

function panelHtml(placement: ExamplePlacement, view: PanelView): string {
  const id = escapeHtml(placement.id);
  const parts = [`<figure class="wb-docs-example" id="example-${id}" data-wb-example="${id}" data-status="${view.status}">`];
  if (view.status === 'ready') {
    parts.push('<div class="wb-docs-example-stage" data-wb-example-stage></div>');
    parts.push('<div class="wb-docs-example-tools">' +
      `<button type="button" class="wb-docs-tool" data-wb-show-code aria-expanded="false" aria-label="Show code" title="Show code">${CODE_ICON}</button>` +
      `<button type="button" class="wb-docs-tool" data-wb-copy-code aria-label="Copy code" title="Copy code">${COPY_ICON}</button>` +
      '</div>');
    parts.push('<div class="wb-docs-example-code" data-wb-example-code hidden></div>');
  } else {
    parts.push(`<p class="wb-docs-example-note">${escapeHtml(view.note ?? 'This example isn’t available.')}</p>`);
  }
  parts.push('</figure>');
  if (placement.caption) parts.push(`<p class="wb-docs-caption">${escapeHtml(placement.caption)}</p>`);
  return parts.join('') + '\n';
}

/* A relative URL in the Markdown, as an address on the project server. Absolute
   URLs, root paths, and in-page fragments stay as they are. */
export function resolveAgainst(file: string, href: string): string {
  if (!href || href.startsWith('#') || href.startsWith('/') || SCHEME.test(href)) return href;
  const [pathPart, suffix = ''] = href.split(/(?=[?#])/);
  const segments = file.split('/').slice(0, -1);
  for (const segment of (pathPart ?? '').split('/')) {
    if (segment === '..') {
      if (!segments.length) return href;
      segments.pop();
    } else if (segment && segment !== '.') segments.push(segment);
  }
  return '/' + segments.map(segment => encodeURIComponent(decodeSafely(segment))).join('/') + suffix;
}

function decodeSafely(segment: string): string {
  try { return decodeURIComponent(segment); } catch { return segment; }
}

function docsPageOf(href: string, pages: ReadonlySet<string>): string | null {
  if (!href.startsWith('/') || href.startsWith('//')) return null;
  const pathPart = decodeSafely(href.split(/[?#]/)[0]!.slice(1));
  return pages.has(pathPart) ? pathPart : null;
}
