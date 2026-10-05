/* Space mark examples, one per named export: each draws marks with the
   shipping wb-space-mark in the sidebar's colours. */

import { surface, spaceMark, type Space } from './workbench-ui.ts';

function board(canvas: HTMLElement, variants: { label: string; space: Space }[]) {
  const grid = document.createElement('div');
  grid.className = 'board-grid';
  for (const variant of variants) {
    const cell = document.createElement('div');
    cell.className = 'board-cell';
    const label = document.createElement('span');
    label.textContent = variant.label;
    cell.append(spaceMark(variant.space), label);
    grid.append(cell);
  }
  surface(canvas).append(grid);
}

/* The extension's own icon, inlined the way the server sends a space's image. */
const image = 'data:image/svg+xml;base64,' + btoa('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><path fill="#fff" d="M12 4L19 4L19 29L44 29L44 36L12 36ZM22 12L36 12L36 26L29 26L29 19L22 19ZM4 12L9 12L9 19L4 19ZM29 39L36 39L36 44L29 44Z"/></svg>');
const framed = 'data:image/svg+xml;base64,' + btoa('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32" rx="8" fill="#111"/><path d="M8 24 16 7l8 17h-4.2L16 15.6 12.2 24z" fill="#ff5a36"/></svg>');

export const namedColors = (canvas: HTMLElement) => {
  board(canvas, [
    { label: 'blue', space: { initial: 'A', color: 'blue' } },
    { label: 'green', space: { initial: 'F', color: 'green' } },
    { label: 'orange', space: { initial: 'B', color: 'orange' } },
    { label: 'purple', space: { initial: 'D', color: 'purple' } },
    { label: 'pink', space: { initial: 'E', color: 'pink' } },
    { label: 'teal', space: { initial: 'M', color: 'teal' } },
    { label: 'red', space: { initial: 'R', color: 'red' } },
    { label: 'yellow', space: { initial: 'Y', color: 'yellow' } },
    { label: 'gray', space: { initial: 'G', color: 'gray' } },
  ]);
};

export const hexColors = (canvas: HTMLElement) => {
  board(canvas, [
    { label: '#2f7d55', space: { initial: 'H', color: '#2f7d55' } },
    { label: '#f5d76e (light)', space: { initial: 'L', color: '#f5d76e' } },
  ]);
};

export const icons = (canvas: HTMLElement) => {
  board(canvas, [
    { label: 'palette', space: { icon: 'palette', color: 'purple' } },
    { label: 'hammer on light hex', space: { icon: 'hammer', color: '#f5d76e' } },
  ]);
};

export const images = (canvas: HTMLElement) => {
  board(canvas, [
    { label: 'no color', space: { image: framed } },
    { label: 'on blue', space: { image: image, color: 'blue' } },
  ]);
};
