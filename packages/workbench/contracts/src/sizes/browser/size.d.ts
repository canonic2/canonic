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
export interface Dimensions {
    width: number;
    height: number;
}
/** The smallest and largest length a size or an artboard can have. */
export declare const MIN_LENGTH = 1;
export declare const MAX_LENGTH = 8192;
/** A filled axis, like Fit, never shrinks below this. */
export declare const FILL_FLOOR = 320;
/** Resizable's size before it is first dragged. */
export declare const RESIZABLE_START: Dimensions;
/** What a filled axis, and Fit, measure in an export: there is no canvas there. */
export declare const EXPORT_FILL: Dimensions;
export declare const DEFAULT_SIZES: readonly ResolvedSize[];
/** A copy of the default sizes, for a space that declares none. */
export declare function defaultSizes(): ResolvedSize[];
export declare function defaultSize(key: string): ResolvedSize | undefined;
/** A size key: kebab-case, and not all digits, so it can't be read as a width. */
export declare function isSizeKey(value: unknown): value is string;
export declare function isLength(value: unknown): value is Length;
/** "small-phone" -> "Small phone": a size's label when it gives none, as implementation labels are made. */
export declare function labelOf(key: string): string;
