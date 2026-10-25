import { defineOnce } from '@canonic2/lattice/elements';
import { WbPreviewControls } from './element.js';
export function registerPreviewControls() {
  defineOnce(customElements, 'wb-preview-controls', WbPreviewControls);
}
