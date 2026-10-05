/* Hands the docs lens rules and the docs canvas geometry to the canvas's
   classic scripts. workbench.js waits for this module before it applies the
   config, since every page it shows needs a lens; zoom.js lays out a docs
   page with the geometry. */

import { centeredX, docsLayout, scrollAfterZoom } from './docs-layout.ts';
import { addressLens, canvasMode, chooseLens, docsLinkLens, hasDesignLens, hasLens, isDocsLens, isMarkdownPage, pageLenses } from './lenses.ts';

const lenses = { addressLens, canvasMode, chooseLens, docsLinkLens, hasDesignLens, hasLens, isDocsLens, isMarkdownPage, pageLenses };

declare global {
  interface Window {
    wbDocsLayout?: { docsLayout: typeof docsLayout; centeredX: typeof centeredX; scrollAfterZoom: typeof scrollAfterZoom };
    wbDocsLenses?: typeof lenses;
    wbZoom?: { update(): void };
  }
}

window.wbDocsLayout = { docsLayout, centeredX, scrollAfterZoom };
window.wbDocsLenses = lenses;
window.dispatchEvent(new Event('wb-docs-ready'));
window.wbZoom?.update();
