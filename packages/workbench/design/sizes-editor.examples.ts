/* Edit sizes… examples: the shipping wb-sizes-editor, opened by a button
   that stands in for the size switcher's menu. */

import type { WbSizesEditor } from '../src/components/sizes-editor/element.ts';
import { icon, opener, sampleSizes } from './sizes-ui.ts';

function editor(sizes = sampleSizes) {
  const element = document.createElement('wb-sizes-editor') as WbSizesEditor;
  element.iconRenderer = icon;
  element.sizes = sizes;
  /* An example stands in for the controller: Save keeps what was saved. */
  element.addEventListener('wb-sizes-save', () => { element.open = false; });
  return element;
}

export const spaceSizes = (canvas: HTMLElement) => {
  opener(canvas, 'Edit sizes…', editor(), 'wb-sizes-close');
};

export const oneSize = (canvas: HTMLElement) => {
  opener(canvas, 'Edit sizes…', editor(sampleSizes.slice(0, 1)), 'wb-sizes-close');
};
