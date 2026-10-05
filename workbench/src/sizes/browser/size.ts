/* Artboard sizes: the shape every reader of a size shares
   ------------------------------------------------------
   A size is either a kind with no dimensions of its own — Fit fills the
   canvas, Resizable is dragged by its edges — or fixed, with a width and a
   height in CSS pixels, either of which may fill the canvas. A space lists
   its sizes in order; a page supports some of them and may add its own.
   See specs/sizes.md. Pure data and constants: no DOM, no fs. */

export type SizeKind = 'fit' | 'resizable' | 'fixed';

/** A fixed size's length on one axis: CSS pixels, or the canvas's length. */
export type Length = number | 'fill';

export interface ResolvedSize {
  key: string;
  label: string;
  icon: string;
  /** Whether the size switcher shows it as a button. */
  button: boolean;
  kind: SizeKind;
  /** Set for a fixed size, null for Fit and Resizable. */
  width: Length | null;
  height: Length | null;
  /** True when workbench.local.yaml defines or changes it. */
  local?: boolean;
}

/** A page's sizes as the server resolves them. */
export interface PageSizes {
  /** The keys the page supports, in its order; absent when it supports every size of the space. */
  sizes?: string[];
  /** The sizes it defines for itself. */
  ownSizes?: ResolvedSize[];
}

export interface Dimensions { width: number; height: number }

/** The smallest and largest length a size or an artboard can have. */
export const MIN_LENGTH = 1;
export const MAX_LENGTH = 8192;
/** A filled axis, like Fit, never shrinks below this. */
export const FILL_FLOOR = 320;
/** Resizable's size before it is first dragged. */
export const RESIZABLE_START: Dimensions = { width: 1024, height: 768 };
/** What a filled axis, and Fit, measure in an export: there is no canvas there. */
export const EXPORT_FILL: Dimensions = { width: 1440, height: 900 };

export const DEFAULT_SIZES: readonly ResolvedSize[] = Object.freeze([
  { key: 'fit', label: 'Fit', icon: 'minimize-2', button: true, kind: 'fit', width: null, height: null },
  { key: 'laptop', label: 'Laptop', icon: 'monitor', button: true, kind: 'fixed', width: 1512, height: 982 },
  { key: 'mobile', label: 'Mobile', icon: 'smartphone', button: true, kind: 'fixed', width: 393, height: 852 },
  { key: 'resizable', label: 'Resizable', icon: 'scaling', button: true, kind: 'resizable', width: null, height: null },
].map(size => Object.freeze(size as ResolvedSize)));

/** A copy of the default sizes, for a space that declares none. */
export function defaultSizes(): ResolvedSize[] {
  return DEFAULT_SIZES.map(size => ({ ...size }));
}

export function defaultSize(key: string): ResolvedSize | undefined {
  return DEFAULT_SIZES.find(size => size.key === key);
}

const KEY = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** A size key: kebab-case, and not all digits, so it can't be read as a width. */
export function isSizeKey(value: unknown): value is string {
  return typeof value === 'string' && KEY.test(value) && !/^[0-9-]+$/.test(value);
}

export function isLength(value: unknown): value is Length {
  return value === 'fill' || (typeof value === 'number' && Number.isInteger(value) && value >= MIN_LENGTH && value <= MAX_LENGTH);
}

/** "small-phone" -> "Small phone": a size's label when it gives none, as implementation labels are made. */
export function labelOf(key: string): string {
  const words = key.split('-').join(' ');
  return words.charAt(0).toUpperCase() + words.slice(1);
}
