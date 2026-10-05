/* Docs: a lens that reads a page as Markdown with live examples.

   The server and the portable export use this entry point. The canvas loads
   canvas/bootstrap.ts, and a docs page loads page/docs-page.ts; both are
   browser-safe and served from src/. See specs/docs-pages.md. */

export { BUNDLE_PATH, REVISION_PATH, SOURCE_PATH, chooseLens, createDocsService, lensRequest } from './docs-service.ts';
export type { BundleInfo, DocsBundle, DocsLens, DocsPageEntry, ListedExample } from './docs-service.ts';
export { docsPages } from './pages.ts';
export type { DocsConfig } from './pages.ts';
export { examplesInFolder, exportNameToId } from './example-ids.ts';
export { portableDocs } from './portable.ts';
export { DOCS_KIND, BUILT_IN_DOCS, isMarkdownPage } from './canvas/lenses.ts';
