/* Design examples use the shipping elements and explicit icon input. */

import { icon } from './icons.ts';
import { registerSpaceComponents } from '../ui/src/bootstrap/register-space.js';
import type { WbSpaceMark } from '../ui/src/components/space-mark/element.js';
import type { WbSpaceSwitcher } from '../ui/src/components/space-switcher/element.js';
import './spaces.js';

export interface Space {
  id: string;
  name: string;
  initial?: string;
  color?: string;
  icon?: string | null;
  image?: string | null;
  removable?: boolean;
}

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
