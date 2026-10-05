/* Custom size… examples: the shipping wb-size-dialog, opened by a button
   that stands in for the size switcher's menu. */

import type { WbSizeDialog } from '../src/components/size-dialog/element.ts';
import { icon, opener } from './sizes-ui.ts';

function dialog(options: { pageOnly?: boolean; notice?: string; error?: string }) {
  const element = document.createElement('wb-size-dialog') as WbSizeDialog;
  element.iconRenderer = icon;
  element.allowPageOnly = !!options.pageOnly;
  element.notice = options.notice || '';
  /* An example stands in for the controller: Add closes the dialog, or shows an error. */
  element.addEventListener('wb-size-add', () => {
    if (options.error) element.error = options.error;
    else element.open = false;
  });
  return element;
}

export const forASpace = (canvas: HTMLElement) => {
  opener(canvas, 'Custom size…', dialog({}), 'wb-size-dialog-close');
};

// From a page workbench.yaml lists: the size can be that page's own.
export const forAPage = (canvas: HTMLElement) => {
  opener(canvas, 'Custom size…', dialog({ pageOnly: true }), 'wb-size-dialog-close');
};

export const withAnError = (canvas: HTMLElement) => {
  opener(canvas, 'Custom size…', dialog({ error: 'workbench.yaml has changed on disk; try again.' }), 'wb-size-dialog-close');
};

export const previewDefinition = (canvas: HTMLElement) => {
  opener(canvas, 'Custom size…', dialog({ notice: 'This page’s preview definition lists its sizes; add the new size’s key there to use it on this page.' }), 'wb-size-dialog-close');
};
