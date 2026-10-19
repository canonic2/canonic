import type { PageSizes, ResolvedSize } from './size.js';
/** The sizes a page supports, in its order: its listed keys, mapped to the space's sizes or its own. */
export declare function supported(space: readonly ResolvedSize[], page?: PageSizes | null): ResolvedSize[];
/**
 * The key to show: the first of `wanted` (address, current, remembered — in
 * that order, nulls skipped) the page supports, else the page's first size.
 * Null only when the page supports no size at all.
 */
export declare function choose(sizes: readonly ResolvedSize[], ...wanted: (string | null | undefined)[]): string | null;
/** "340 × fill" for a fixed size; empty for Fit and Resizable, which have no dimensions of their own. */
export declare function dimensionsText(size: ResolvedSize): string;
/** "Sidebar, 340 × fill"; "Fit". What a handoff and a button title call the size. */
export declare function describe(size: ResolvedSize): string;
