/* How big an artboard is at a size
   --------------------------------
   A fixed axis is its length; a filled axis, and both axes of Fit, take the
   room the canvas has for an artboard, never less than FILL_FLOOR; Resizable
   is whatever it was last dragged to. `available` is that room — the canvas
   less the space kept around an artboard — and is the caller's to measure. */

import { EXPORT_FILL, FILL_FLOOR, MAX_LENGTH, MIN_LENGTH } from './size.ts';
import type { Dimensions, Length, ResolvedSize } from './size.ts';

function filled(length: number): number {
  return Math.min(MAX_LENGTH, Math.max(FILL_FLOOR, Math.round(length)));
}

function axis(length: Length | null, room: number): number {
  return typeof length === 'number' ? length : filled(room);
}

/** Which axes follow the canvas: both for Fit, the `fill` ones of a fixed size. */
export function fills(size: ResolvedSize): { width: boolean; height: boolean } {
  if (size.kind === 'fit') return { width: true, height: true };
  if (size.kind === 'resizable') return { width: false, height: false };
  return { width: size.width === 'fill', height: size.height === 'fill' };
}

/** The artboard's CSS size at `size` on a canvas with `available` room. */
export function dimensions(size: ResolvedSize, available: Dimensions, resizable: Dimensions): Dimensions {
  if (size.kind === 'resizable') return clamp(resizable);
  if (size.kind === 'fit') return { width: filled(available.width), height: filled(available.height) };
  return { width: axis(size.width, available.width), height: axis(size.height, available.height) };
}

/** The size an export captures: a filled axis, and Fit, at EXPORT_FILL; Resizable has none. */
export function exportDimensions(size: ResolvedSize): Dimensions | null {
  if (size.kind === 'resizable') return null;
  return dimensions(size, EXPORT_FILL, EXPORT_FILL);
}

/** Whether an artboard of `actual` CSS size can be `size`: fixed axes exactly, filled ones any length. */
export function accepts(size: ResolvedSize, actual: Dimensions): boolean {
  if (size.kind !== 'fixed') return true;
  return (size.width === 'fill' || size.width === actual.width) && (size.height === 'fill' || size.height === actual.height);
}

/** A whole number of CSS pixels from MIN_LENGTH to MAX_LENGTH on each axis. */
export function clamp(value: Dimensions): Dimensions {
  const one = (n: number) => Math.min(MAX_LENGTH, Math.max(MIN_LENGTH, Math.round(Number.isFinite(n) ? n : MIN_LENGTH)));
  return { width: one(value.width), height: one(value.height) };
}

export function validDimensions(value: unknown): value is Dimensions {
  if (!value || typeof value !== 'object') return false;
  const { width, height } = value as Dimensions;
  return [width, height].every(n => Number.isFinite(n) && n >= MIN_LENGTH && n <= MAX_LENGTH);
}
