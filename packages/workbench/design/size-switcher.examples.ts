/* Size switcher examples, one per named export: the shipping
   wb-size-switcher as the canvas's top bar draws it, from sample sizes. */

import { sizeSwitcher, topBar } from './sizes-ui.ts';

export const buttons = (canvas: HTMLElement) => {
  topBar(canvas).append(sizeSwitcher({ current: 'laptop' }));
};

// The menu open on a size that isn't a button: it shows in the menu button.
export const moreSizes = (canvas: HTMLElement) => {
  const switcher = sizeSwitcher({ current: 'tablet', allowEdit: true });
  topBar(canvas, 470).append(switcher);
  switcher.open = true;
};

export const limitedByPage = (canvas: HTMLElement) => {
  topBar(canvas).append(sizeSwitcher({ current: 'sidebar', supported: ['sidebar', 'resizable'] }));
};

export const docsPage = (canvas: HTMLElement) => {
  topBar(canvas).append(sizeSwitcher({ current: '', supported: [], allowEdit: true, reason: 'a docs page fills the canvas' }));
};
