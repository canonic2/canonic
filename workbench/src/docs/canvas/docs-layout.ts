/* Where a docs page sits on the canvas. A docs page has no artboard of its
   own: its frame fills the canvas. The page lays out at the canvas's width
   whatever the zoom, so zooming scales it without reflowing it; its height is
   the canvas's height in page pixels, so zooming out shows more of the page.
   The page scrolls vertically inside its frame; horizontally it is centered
   while it fits and pans when zoomed in past the canvas's width. */

export interface Size {
  width: number;
  height: number;
}

export interface DocsLayout {
  /** The frame's layout size, in page pixels. */
  width: number;
  height: number;
  /** Where its top-left corner sits on the canvas, in canvas pixels. */
  x: number;
  y: number;
}

/** The page's frame at zoom `zoom`, with the horizontal pan `x` kept in bounds. */
export function docsLayout(canvas: Size, zoom: number, x: number): DocsLayout {
  const width = Math.max(1, Math.round(canvas.width));
  const height = Math.max(1, Math.round(canvas.height / zoom));
  const scaled = width * zoom;
  const left = scaled <= canvas.width ? (canvas.width - scaled) / 2 : Math.min(0, Math.max(canvas.width - scaled, x));
  return { width, height, x: left, y: 0 };
}

/** The horizontal pan that centers the page at `zoom`. */
export function centeredX(canvas: Size, zoom: number): number {
  return (canvas.width - Math.max(1, Math.round(canvas.width)) * zoom) / 2;
}

/**
 * The page's new vertical scroll after zooming from `from` to `to` about a
 * point `pointY` canvas pixels from the top, so the page content under the
 * point stays under it.
 */
export function scrollAfterZoom(scrollY: number, pointY: number, from: number, to: number): number {
  return Math.max(0, scrollY + pointY / from - pointY / to);
}
