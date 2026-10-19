/* Which size a page shows, and how a size is named
   -----------------------------------------------
   The canvas keeps one current size. A page supports some of the space's
   sizes; when the address, the size the canvas was on, or the one remembered
   from last time isn't among them, the page's first size is shown instead.
   Pure: the canvas and the multiple-artboard runtime both decide with it. */

/** The sizes a page supports, in its order: its listed keys, mapped to the space's sizes or its own. */
export function supported(space                         , page                   )                 {
  const own = page?.ownSizes || [];
  if (!page?.sizes) return [...space, ...own];
  const out                 = [];
  for (const key of page.sizes) {
    const size = own.find(candidate => candidate.key === key) || space.find(candidate => candidate.key === key);
    if (size && !out.includes(size)) out.push(size);
  }
  return out.length ? out : [...space, ...own];
}

/**
 * The key to show: the first of `wanted` (address, current, remembered — in
 * that order, nulls skipped) the page supports, else the page's first size.
 * Null only when the page supports no size at all.
 */
export function choose(sizes                         , ...wanted                               )                {
  for (const key of wanted) if (key && sizes.some(size => size.key === key)) return key;
  return sizes[0]?.key ?? null;
}

function length(value                        )         {
  return value === 'fill' ? 'fill' : String(value);
}

/** "340 × fill" for a fixed size; empty for Fit and Resizable, which have no dimensions of their own. */
export function dimensionsText(size              )         {
  return size.kind === 'fixed' ? length(size.width) + ' × ' + length(size.height) : '';
}

/** "Sidebar, 340 × fill"; "Fit". What a handoff and a button title call the size. */
export function describe(size              )         {
  const text = dimensionsText(size);
  return text ? size.label + ', ' + text : size.label;
}
