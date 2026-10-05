/* Actions switch examples, one per named export: the shipping
   wb-actions-switch as the canvas's top bar draws it. */

import { registerActionsComponents } from '../src/components/register.ts';
import type { WbActionsSwitch } from '../src/components/actions-switch/element.ts';
import { icon, sizeSwitcher, topBar } from './sizes-ui.ts';

registerActionsComponents();

function actionsSwitch(options: { checked?: boolean; disabled?: boolean; hint?: string } = {}) {
  const element = document.createElement('wb-actions-switch') as WbActionsSwitch;
  element.iconRenderer = icon;
  element.checked = !!options.checked;
  element.disabled = !!options.disabled;
  if (options.hint) element.hint = options.hint;
  /* An example stands in for the canvas controller: it takes what is asked. */
  element.addEventListener('wb-actions-toggle', event => { element.checked = event.detail.checked; });
  return element;
}

export const off = (canvas: HTMLElement) => {
  topBar(canvas).append(actionsSwitch());
};

export const on = (canvas: HTMLElement) => {
  topBar(canvas).append(actionsSwitch({ checked: true }));
};

export const throughALens = (canvas: HTMLElement) => {
  topBar(canvas).append(actionsSwitch({
    checked: true, disabled: true,
    hint: 'always on here: Storybook serves this page, not the workbench, so nothing inside it is switched off',
  }));
};

export const inTheTopBar = (canvas: HTMLElement) => {
  const area = topBar(canvas);
  area.classList.add('design-top-bar-group');
  area.append(sizeSwitcher({ current: 'laptop' }), actionsSwitch());
};
