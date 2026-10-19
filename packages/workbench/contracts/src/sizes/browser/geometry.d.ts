import type { Dimensions, ResolvedSize } from './size.js';
/** Which axes follow the canvas: both for Fit, the `fill` ones of a fixed size. */
export declare function fills(size: ResolvedSize): {
    width: boolean;
    height: boolean;
};
/** The artboard's CSS size at `size` on a canvas with `available` room. */
export declare function dimensions(size: ResolvedSize, available: Dimensions, resizable: Dimensions): Dimensions;
/** The size an export captures: a filled axis, and Fit, at EXPORT_FILL; Resizable has none. */
export declare function exportDimensions(size: ResolvedSize): Dimensions | null;
/** Whether an artboard of `actual` CSS size can be `size`: fixed axes exactly, filled ones any length. */
export declare function accepts(size: ResolvedSize, actual: Dimensions): boolean;
/** A whole number of CSS pixels from MIN_LENGTH to MAX_LENGTH on each axis. */
export declare function clamp(value: Dimensions): Dimensions;
export declare function validDimensions(value: unknown): value is Dimensions;
