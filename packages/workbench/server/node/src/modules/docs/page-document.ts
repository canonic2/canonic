/* The HTML document a docs page is served as: Workbench's docs stylesheet,
   the lens's example styles, the rendered Markdown, and the page script that
   mounts the examples. Everything the script needs travels in
   window.__workbenchDocs. */

import { escapeHtml } from './code-highlight.ts';

import type { DocsPageOptions } from '../../../../../contracts/src/docs/protocol.ts';
export type { DocsPageOptions } from '../../../../../contracts/src/docs/protocol.ts';

export interface PageDocumentInput {
  title: string;
  /** Where the page's script and stylesheet are: the server's by default, beside the page in a portable export. */
  script?: string;
  stylesheet?: string;
  body: string;
  stylesheets: string[];
  options: DocsPageOptions;
}

export const PAGE_STYLESHEET = '/_workbench/ui/src/modules/docs/page/docs-page.css';
export const PAGE_SCRIPT = '/_workbench/ui/src/modules/docs/page/docs-page.ts';

export function pageDocument(input: PageDocumentInput): string {
  const options = JSON.stringify(input.options).replace(/</g, '\\u003c');
  return '<!doctype html><html lang="en"><head><meta charset="utf-8">' +
    '<meta name="viewport" content="width=device-width,initial-scale=1">' +
    `<title>${escapeHtml(input.title)}</title>` +
    `<link rel="stylesheet" href="${escapeHtml(input.stylesheet ?? PAGE_STYLESHEET)}">` +
    input.stylesheets.map(href => `<link rel="stylesheet" href="${escapeHtml(href)}">`).join('') +
    '</head><body class="wb-docs-body">' +
    `<main class="wb-docs">${input.body}</main>` +
    `<script>window.__workbenchDocs=${options}</script>` +
    `<script type="module" src="${escapeHtml(input.script ?? PAGE_SCRIPT)}"></script>` +
    '</body></html>';
}
