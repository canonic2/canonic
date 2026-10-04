/* Hands the docs-page canvas geometry to the canvas's classic scripts.
   zoom.js lays out a docs page with it; until this module has run, a docs
   page waits for it rather than taking an artboard's layout. */

import { centeredX, docsLayout, scrollAfterZoom } from './docs-layout.ts';

declare global {
  interface Window {
    wbDocsLayout?: { docsLayout: typeof docsLayout; centeredX: typeof centeredX; scrollAfterZoom: typeof scrollAfterZoom };
    wbZoom?: { update(): void };
  }
}

window.wbDocsLayout = { docsLayout, centeredX, scrollAfterZoom };
window.wbZoom?.update();
