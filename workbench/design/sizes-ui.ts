/* The size components' design examples: the shipping elements, with the
   canvas's icons and sample sizes as the server resolves them. */

import { icon } from './icons.ts';
import { registerSizeComponents } from '../src/components/register.ts';
import { defaultSizes } from '../src/sizes/browser/size.ts';
import type { ResolvedSize } from '../src/sizes/browser/size.ts';
import type { WbSizeSwitcher } from '../src/components/size-switcher/element.ts';

registerSizeComponents();

export { icon };

/** The defaults, a sidebar that fills the canvas's height, and two sizes in More sizes. */
export const sampleSizes: ResolvedSize[] = [
  ...defaultSizes(),
  { key: 'sidebar', label: 'Sidebar', icon: 'panel-left', button: true, kind: 'fixed', width: 340, height: 'fill' },
  { key: 'tablet', label: 'Tablet', icon: 'tablet', button: false, kind: 'fixed', width: 1024, height: 1366 },
  { key: 'small-phone', label: 'Small phone', icon: 'smartphone', button: false, kind: 'fixed', width: 375, height: 667, local: true },
];

/** A stretch of the top bar to draw into, tall enough for an open menu. */
export function topBar(canvas: HTMLElement, height?: number): HTMLElement {
  const area = document.createElement('div');
  area.className = 'design-surface design-top-bar';
  if (height) area.style.minHeight = `${height}px`;
  canvas.append(area);
  return area;
}

export function sizeSwitcher(options: { current: string; supported?: string[] | null; allowEdit?: boolean; reason?: string }) {
  const element = document.createElement('wb-size-switcher') as WbSizeSwitcher;
  element.iconRenderer = icon;
  element.sizes = sampleSizes;
  element.supported = options.supported ?? null;
  element.current = options.current;
  element.allowEdit = !!options.allowEdit;
  if (options.reason) element.setAttribute('unsupported-reason', options.reason);
  /* An example stands in for the canvas controller: it takes what is picked. */
  element.addEventListener('wb-size-pick', event => { element.current = event.detail.key; });
  return element;
}

/** A button that opens a dialog, which closes on its own close event. */
export function opener(canvas: HTMLElement, text: string, dialog: HTMLElement & { open: boolean }, close: string) {
  const area = topBar(canvas);
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'design-open';
  button.textContent = text;
  button.addEventListener('click', () => { dialog.open = true; });
  dialog.addEventListener(close, () => { dialog.open = false; });
  area.append(button, dialog);
  return area;
}
