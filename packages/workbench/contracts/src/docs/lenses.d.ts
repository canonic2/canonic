export declare const DOCS_KIND = "docs";
/** The key that names a page's own design lens, in the address and the top bar's choice. */
export declare const DESIGN_KEY = "design";
/** The lens of a page with Markdown that maps no docs implementation. */
export declare const BUILT_IN_DOCS: Readonly<{
    key: "docs";
    label: "Docs";
    kind: "docs";
}>;
export interface Lens {
    key: string;
    label: string;
    kind: string;
}
export interface LensPage {
    src: string;
    /** The page's Markdown, relative to the project root; its src for a Markdown page. */
    markdown?: string;
    /** A Markdown page's default lens. */
    lens?: string;
    /** A page imported from one implementation, which is its only lens. */
    implementationOnly?: string;
    implementations?: Record<string, unknown>;
    /** The docs lenses a defineDocs definition declares, in its order. */
    docsLenses?: readonly {
        key: string;
        label: string;
    }[];
}
export type Implementations = Readonly<Record<string, Lens>>;
/** A page whose src is its Markdown: it has docs lenses and no design lens. */
export declare function isMarkdownPage(page: LensPage | null | undefined): boolean;
export declare function isDocsLens(lens: Lens | null | undefined): boolean;
/** 'docs' when the lens fills the canvas with a docs page; 'default' for an artboard. */
export declare function canvasMode(lens: Lens | null | undefined): 'docs' | 'default';
/** Whether the page has its own design lens: every page but a Markdown page or an imported one. */
export declare function hasDesignLens(page: LensPage | null | undefined): boolean;
/**
 * A page's lenses besides its design, in order: its docs lenses (a
 * definition's, then those it maps in the order the config declares them, or
 * the built-in Docs lens when it has Markdown and no docs lens), then its
 * other implementations in the order the config declares them. The top bar
 * puts the design between the two.
 */
export declare function pageLenses(page: LensPage | null | undefined, implementations: Implementations): Lens[];
export declare function docsLenses(page: LensPage | null | undefined, implementations: Implementations): Lens[];
/** The lens a page shows by default: a Markdown page's `lens`, else its first docs lens, else null for its design. */
export declare function defaultLens(page: LensPage | null | undefined, implementations: Implementations): Lens | null;
/** The lens a page shows: `wanted` when the page has it, else its default. Null is its design, which DESIGN_KEY asks for. */
export declare function chooseLens(page: LensPage | null | undefined, implementations: Implementations, wanted: string | null | undefined): Lens | null;
/** A link between docs opens the target in a docs lens: `wanted` when it has it, else its default docs lens. */
export declare function docsLinkLens(page: LensPage | null | undefined, implementations: Implementations, wanted: string | null | undefined): Lens | null;
/** The lens key the address names: null for the page's default lens, which is left out; DESIGN_KEY for a design that isn't its default. */
export declare function addressLens(page: LensPage | null | undefined, implementations: Implementations, lens: Lens | null | undefined): string | null;
/** Whether a key names a lens the page has, a docs lens and its design included. */
export declare function hasLens(page: LensPage | null | undefined, implementations: Implementations, key: string | null | undefined): boolean;
