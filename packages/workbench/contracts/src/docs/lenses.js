/* Which lens a page shows
   ----------------------
   Docs are a lens: a page with Markdown can be read as its docs beside its
   design and its implementations, and the Markdown is the same in each docs
   lens; the lens says what renders the examples. Docs lenses come first, and
   a page with docs opens on them; its design is one choice away. A page whose
   src is its Markdown has no design lens. A page with Markdown that maps no
   docs implementation still has one: the built-in Docs lens, with no
   examples.

   The lens the top bar is on carries across pages; a page that doesn't have
   it shows its default. Null is a page's design, and DESIGN_KEY names it in
   the address and the top bar's choice. Pure: the canvas and the
   multiple-artboard runtime decide with it, and Node tests check it. See
   specs/pages/docs-pages.md. */

export const DOCS_KIND = 'docs';

/** The key that names a page's own design lens, in the address and the top bar's choice. */
export const DESIGN_KEY = 'design';

/** The lens of a page with Markdown that maps no docs implementation. */
export const BUILT_IN_DOCS = Object.freeze({ key: 'docs', label: 'Docs', kind: DOCS_KIND });

/** A page whose src is its Markdown: it has docs lenses and no design lens. */
export function isMarkdownPage(page                             )          {
  return !!page && !!page.markdown && page.markdown === page.src;
}

export function isDocsLens(lens                         )          {
  return !!lens && lens.kind === DOCS_KIND;
}

/** 'docs' when the lens fills the canvas with a docs page; 'default' for an artboard. */
export function canvasMode(lens                         )                     {
  return isDocsLens(lens) ? 'docs' : 'default';
}

/** Whether the page has its own design lens: every page but a Markdown page or an imported one. */
export function hasDesignLens(page                             )          {
  return !!page && !isMarkdownPage(page) && !page.implementationOnly;
}

/**
 * A page's lenses besides its design, in order: its docs lenses (a
 * definition's, then those it maps in the order the config declares them, or
 * the built-in Docs lens when it has Markdown and no docs lens), then its
 * other implementations in the order the config declares them. The top bar
 * puts the design between the two.
 */
export function pageLenses(page                             , implementations                 )         {
  if (!page) return [];
  const out         = (page.docsLenses ?? []).map(lens => ({ key: lens.key, label: lens.label, kind: DOCS_KIND }));
  for (const key of Object.keys(implementations)) {
    if (page.implementations?.[key] && !out.some(lens => lens.key === key)) out.push(implementations[key] );
  }
  if (page.markdown && !out.some(isDocsLens) && !out.some(lens => lens.key === BUILT_IN_DOCS.key)) out.push({ ...BUILT_IN_DOCS });
  return out.filter(isDocsLens).concat(out.filter(lens => !isDocsLens(lens)));
}

export function docsLenses(page                             , implementations                 )         {
  return pageLenses(page, implementations).filter(isDocsLens);
}

/** The lens a page shows by default: a Markdown page's `lens`, else its first docs lens, else null for its design. */
export function defaultLens(page                             , implementations                 )              {
  if (!page) return null;
  if (page.implementationOnly) return implementations[page.implementationOnly] ?? null;
  const docs = docsLenses(page, implementations);
  return (isMarkdownPage(page) ? docs.find(lens => lens.key === page.lens) : undefined) ?? docs[0] ?? null;
}

/** The lens a page shows: `wanted` when the page has it, else its default. Null is its design, which DESIGN_KEY asks for. */
export function chooseLens(page                             , implementations                 , wanted                           )              {
  if (!page) return null;
  if (page.implementationOnly) return defaultLens(page, implementations);
  if (wanted === DESIGN_KEY && hasDesignLens(page)) return null;
  const found = wanted ? pageLenses(page, implementations).find(lens => lens.key === wanted) : undefined;
  return found ?? defaultLens(page, implementations);
}

/** A link between docs opens the target in a docs lens: `wanted` when it has it, else its default docs lens. */
export function docsLinkLens(page                             , implementations                 , wanted                           )              {
  const docs = docsLenses(page, implementations);
  return docs.find(lens => lens.key === wanted) ?? docs.find(lens => lens.key === page?.lens) ?? docs[0] ?? null;
}

/** The lens key the address names: null for the page's default lens, which is left out; DESIGN_KEY for a design that isn't its default. */
export function addressLens(page                             , implementations                 , lens                         )                {
  /* An imported page's only lens is what its address names. */
  if (page?.implementationOnly) return lens ? lens.key : null;
  const own = defaultLens(page, implementations);
  if (!lens) return own ? DESIGN_KEY : null;
  return own && own.key === lens.key ? null : lens.key;
}

/** Whether a key names a lens the page has, a docs lens and its design included. */
export function hasLens(page                             , implementations                 , key                           )          {
  if (key === DESIGN_KEY) return hasDesignLens(page);
  return !!key && pageLenses(page, implementations).some(lens => lens.key === key);
}
