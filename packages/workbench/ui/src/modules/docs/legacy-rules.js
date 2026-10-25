/* Hands the docs lens rules and the docs canvas geometry to the canvas's
   classic scripts. workbench.js waits for this module before it applies the
   config, since every page it shows needs a lens; zoom.js lays out a docs
   page with the geometry. */

import { centeredX, docsLayout, scrollAfterZoom } from './canvas/docs-layout.js';
import {
  DESIGN_KEY,
  addressLens,
  canvasMode,
  chooseLens,
  docsLinkLens,
  hasDesignLens,
  hasLens,
  isDocsLens,
  isMarkdownPage,
  pageLenses,
} from '../../../../contracts/src/docs/lenses.js';

const lenses = {
  DESIGN_KEY,
  addressLens,
  canvasMode,
  chooseLens,
  docsLinkLens,
  hasDesignLens,
  hasLens,
  isDocsLens,
  isMarkdownPage,
  pageLenses,
};

window.wbDocsLayout = { docsLayout, centeredX, scrollAfterZoom };
window.wbDocsLenses = lenses;
window.dispatchEvent(new Event('wb-docs-ready'));
window.wbZoom?.update();
