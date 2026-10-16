/* Which lens a page shows
   ----------------------
   Docs are a lens: a page with Markdown can be read as its docs beside its
   design and its implementations, and the Markdown is the same in each docs
   lens; the lens says what renders the examples. A page whose src is its
   Markdown has no design lens, so a docs lens is its default. A page with
   Markdown that maps no docs implementation still has one: the built-in Docs
   lens, with no examples.

   The lens the top bar is on carries across pages; a page that doesn't have
   it shows its default. Pure: the canvas and the multiple-artboard runtime
   decide with it, and Node tests check it. See specs/docs-pages.md. */

export const DOCS_KIND = 'docs';

/** The lens of a page with Markdown that maps no docs implementation. */
export const BUILT_IN_DOCS = Object.freeze({ key: 'docs', label: 'Docs', kind: DOCS_KIND });

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
  docsLenses?: readonly { key: string; label: string }[];
}

export type Implementations = Readonly<Record<string, Lens>>;

/** A page whose src is its Markdown: it has docs lenses and no design lens. */
export function isMarkdownPage(page: LensPage | null | undefined): boolean {
  return !!page && !!page.markdown && page.markdown === page.src;
}

export function isDocsLens(lens: Lens | null | undefined): boolean {
  return !!lens && lens.kind === DOCS_KIND;
}

/** 'docs' when the lens fills the canvas with a docs page; 'default' for an artboard. */
export function canvasMode(lens: Lens | null | undefined): 'docs' | 'default' {
  return isDocsLens(lens) ? 'docs' : 'default';
}

/** Whether the page has its own design lens: every page but a Markdown page or an imported one. */
export function hasDesignLens(page: LensPage | null | undefined): boolean {
  return !!page && !isMarkdownPage(page) && !page.implementationOnly;
}

/**
 * A page's lenses besides its design, in order: a definition's docs lenses,
 * then the implementations it maps in the order the config declares them,
 * then the built-in Docs lens when it has Markdown and no docs lens. A
 * Markdown page's docs lenses come first, where another page's design is.
 */
export function pageLenses(page: LensPage | null | undefined, implementations: Implementations): Lens[] {
  if (!page) return [];
  const out: Lens[] = (page.docsLenses ?? []).map(lens => ({ key: lens.key, label: lens.label, kind: DOCS_KIND }));
  for (const key of Object.keys(implementations)) {
    if (page.implementations?.[key] && !out.some(lens => lens.key === key)) out.push(implementations[key]!);
  }
  if (page.markdown && !out.some(isDocsLens) && !out.some(lens => lens.key === BUILT_IN_DOCS.key)) out.push({ ...BUILT_IN_DOCS });
  if (!isMarkdownPage(page)) return out;
  return out.filter(isDocsLens).concat(out.filter(lens => !isDocsLens(lens)));
}

export function docsLenses(page: LensPage | null | undefined, implementations: Implementations): Lens[] {
  return pageLenses(page, implementations).filter(isDocsLens);
}

/** The lens a page shows by default: null for its design; a Markdown page's `lens`, else its first docs lens. */
export function defaultLens(page: LensPage | null | undefined, implementations: Implementations): Lens | null {
  if (!page) return null;
  if (page.implementationOnly) return implementations[page.implementationOnly] ?? null;
  if (!isMarkdownPage(page)) return null;
  const docs = docsLenses(page, implementations);
  return docs.find(lens => lens.key === page.lens) ?? docs[0] ?? null;
}

/** The lens a page shows: `wanted` when the page has it, else its default. Null is its design. */
export function chooseLens(page: LensPage | null | undefined, implementations: Implementations, wanted: string | null | undefined): Lens | null {
  if (!page) return null;
  if (page.implementationOnly) return defaultLens(page, implementations);
  const found = wanted ? pageLenses(page, implementations).find(lens => lens.key === wanted) : undefined;
  return found ?? defaultLens(page, implementations);
}

/** A link between docs opens the target in a docs lens: `wanted` when it has it, else its default docs lens. */
export function docsLinkLens(page: LensPage | null | undefined, implementations: Implementations, wanted: string | null | undefined): Lens | null {
  const docs = docsLenses(page, implementations);
  return docs.find(lens => lens.key === wanted) ?? docs.find(lens => lens.key === page?.lens) ?? docs[0] ?? null;
}

/** The lens key the address names: null for the design and for a Markdown page's own docs lens, which are left out. */
export function addressLens(page: LensPage | null | undefined, implementations: Implementations, lens: Lens | null | undefined): string | null {
  if (!lens) return null;
  const own = isMarkdownPage(page) ? defaultLens(page, implementations) : null;
  return own && own.key === lens.key ? null : lens.key;
}

/** Whether a key names a lens the page has, a docs lens included. */
export function hasLens(page: LensPage | null | undefined, implementations: Implementations, key: string | null | undefined): boolean {
  return !!key && pageLenses(page, implementations).some(lens => lens.key === key);
}
