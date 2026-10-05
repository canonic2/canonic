/* Space switcher examples, one per named export: the shipping switcher as
   the top of the sidebar draws it, from the sample spaces. */

import { designSpaces, surface, spaceSwitcher, type Space } from './workbench-ui.ts';

function switcher(canvas: HTMLElement, spaces: Space[], open: boolean) {
  const area = surface(canvas, open ? 264 : undefined);
  area.classList.add('is-sidebar');
  const drawn = spaceSwitcher(spaces, designSpaces.current);
  area.append(drawn);
  drawn.open = open;
  return area;
}

export const closed = (canvas: HTMLElement) => {
  switcher(canvas, designSpaces.spaces, false);
};

export const open = (canvas: HTMLElement) => {
  switcher(canvas, designSpaces.spaces, true);
};

// Hover or keyboard-focus a removable row to reveal its remove action.
export const removable = (canvas: HTMLElement) => {
  switcher(canvas, designSpaces.spaces, true);
};

export const oneSpace = (canvas: HTMLElement) => {
  switcher(canvas, designSpaces.spaces.slice(0, 1), true);
};
