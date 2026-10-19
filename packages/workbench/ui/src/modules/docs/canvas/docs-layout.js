/* Where a docs page sits on the canvas. A docs page has no artboard of its
   own: its frame fills the canvas. The page lays out at the canvas's width
   whatever the zoom, so zooming scales it without reflowing it; its height is
   the canvas's height in page pixels, so zooming out shows more of the page.
   The page scrolls vertically inside its frame; horizontally it is centered
   while it fits and pans when zoomed in past the canvas's width. */

/** The page's frame at zoom `zoom`, with the horizontal pan `x` kept in bounds. */
export function docsLayout(canvas, zoom, x) {
  const width = Math.max(1, Math.round(canvas.width));
  const height = Math.max(1, Math.round(canvas.height / zoom));
  const scaled = width * zoom;
  const left =
    scaled <= canvas.width
      ? (canvas.width - scaled) / 2
      : Math.min(0, Math.max(canvas.width - scaled, x));
  return { width, height, x: left, y: 0 };
}

/** The horizontal pan that centers the page at `zoom`. */
export function centeredX(canvas, zoom) {
  return (canvas.width - Math.max(1, Math.round(canvas.width)) * zoom) / 2;
}

/**
 * The page's new vertical scroll after zooming from `from` to `to` about a
 * point `pointY` canvas pixels from the top, so the page content under the
 * point stays under it.
 */
export function scrollAfterZoom(scrollY, pointY, from, to) {
  return Math.max(0, scrollY + pointY / from - pointY / to);
}
