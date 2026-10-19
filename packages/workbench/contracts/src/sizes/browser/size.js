/* Artboard sizes: the shape every reader of a size shares
   ------------------------------------------------------
   A size is either a kind with no dimensions of its own — Fit fills the
   canvas, Resizable is dragged by its edges — or fixed, with a width and a
   height in CSS pixels, either of which may fill the canvas. A space lists
   its sizes in order; a page supports some of them and may add its own.
   See specs/sizes.md. Pure data and constants: no DOM, no fs. */

/** A fixed size's length on one axis: CSS pixels, or the canvas's length. */

/** A page's sizes as the server resolves them. */

/** The smallest and largest length a size or an artboard can have. */
export const MIN_LENGTH = 1;
export const MAX_LENGTH = 8192;
/** A filled axis, like Fit, never shrinks below this. */
export const FILL_FLOOR = 320;
/** Resizable's size before it is first dragged. */
export const RESIZABLE_START             = { width: 1024, height: 768 };
/** What a filled axis, and Fit, measure in an export: there is no canvas there. */
export const EXPORT_FILL             = { width: 1440, height: 900 };

export const DEFAULT_SIZES                          = Object.freeze([
  { key: 'fit', label: 'Fit', icon: 'minimize-2', button: true, kind: 'fit', width: null, height: null },
  { key: 'laptop', label: 'Laptop', icon: 'monitor', button: true, kind: 'fixed', width: 1512, height: 982 },
  { key: 'mobile', label: 'Mobile', icon: 'smartphone', button: true, kind: 'fixed', width: 393, height: 852 },
  { key: 'resizable', label: 'Resizable', icon: 'scaling', button: true, kind: 'resizable', width: null, height: null },
].map(size => Object.freeze(size                )));

/** A copy of the default sizes, for a space that declares none. */
export function defaultSizes()                 {
  return DEFAULT_SIZES.map(size => ({ ...size }));
}

export function defaultSize(key        )                           {
  return DEFAULT_SIZES.find(size => size.key === key);
}

const KEY = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** A size key: kebab-case, and not all digits, so it can't be read as a width. */
export function isSizeKey(value         )                  {
  return typeof value === 'string' && KEY.test(value) && !/^[0-9-]+$/.test(value);
}

export function isLength(value         )                  {
  return value === 'fill' || (typeof value === 'number' && Number.isInteger(value) && value >= MIN_LENGTH && value <= MAX_LENGTH);
}

/** "small-phone" -> "Small phone": a size's label when it gives none, as implementation labels are made. */
export function labelOf(key        )         {
  const words = key.split('-').join(' ');
  return words.charAt(0).toUpperCase() + words.slice(1);
}
