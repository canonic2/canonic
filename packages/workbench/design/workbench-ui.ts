/* Design examples use the shipping elements and explicit icon input. */

import { icon } from './icons.ts';
import { registerSpaceComponents } from '../src/components/register.ts';
import type { WbSpaceMark } from '../src/components/space-mark/element.ts';
import type { WbSpaceSwitcher } from '../src/components/space-switcher/element.ts';
import type { Space } from '../src/components/space-mark/space.ts';
import './spaces.js';

export type { Space };

const host = window as unknown as { designSpaces: { current: string; spaces: Space[] } };

registerSpaceComponents();

export function spaceMark(space: Space) {
  const element = document.createElement('wb-space-mark') as WbSpaceMark;
  element.iconRenderer = icon;
  element.space = space;
  return element;
}

export function spaceSwitcher(spaces: Space[], currentId: string) {
  const element = document.createElement('wb-space-switcher') as WbSpaceSwitcher;
  element.iconRenderer = icon;
  element.spaces = spaces;
  element.currentId = currentId;
  element.allowAdd = element.allowRemove = true;
  return element;
}

/* The sample spaces the Sidebar design page uses too. */
export const designSpaces = host.designSpaces;

/* An area in the sidebar's colours to draw into. */
export function surface(canvas: HTMLElement, height?: number): HTMLElement {
  const area = document.createElement('div');
  area.className = 'design-surface';
  if (height) area.style.minHeight = `${height}px`;
  canvas.append(area);
  return area;
}
