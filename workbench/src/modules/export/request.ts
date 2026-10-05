export type ExportFormat = 'zip' | 'pdf' | 'images' | 'browser';
export interface CurrentView {
  page: string; state?: string | null; lens?: string | null;
  size?: string; width: number; height: number;
}
export interface ExportRequest {
  format: ExportFormat;
  scope: 'space' | 'page' | 'pages' | 'collection';
  pages: string[];
  collection?: string;
  variants: 'all' | 'current' | 'custom';
  states: string[];
  sizes: string[];
  lens: string | null;
  current?: CurrentView;
  paper: 'A4' | 'Letter';
  imageFormat: 'png' | 'jpeg';
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Export options must be an object.');
  return value as Record<string, unknown>;
}
function strings(value: unknown, label: string): string[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 10000 || value.some(s => typeof s !== 'string' || !s || s.length > 2000)) throw new Error('Invalid export ' + label + '.');
  return [...new Set(value as string[])].sort();
}
function choice<T extends string>(value: unknown, choices: readonly T[], fallback: T): T {
  if (value === undefined) return fallback;
  if (!choices.includes(value as T)) throw new Error('Invalid export option: ' + String(value));
  return value as T;
}
export function readRequest(value: unknown = {}): ExportRequest {
  const input = record(value);
  const format = choice(input.format, ['zip', 'pdf', 'images', 'browser'], 'zip');
  const scope = choice(input.scope, ['space', 'page', 'pages', 'collection'], 'space');
  const variants = choice(input.variants, ['all', 'current', 'custom'], scope === 'page' && (format === 'pdf' || format === 'images') ? 'current' : 'all');
  if (format === 'browser' && variants === 'current') throw new Error('Portable browser export uses declared variants.');
  let current: CurrentView | undefined;
  if (input.current !== undefined) {
    const view = record(input.current);
    if (typeof view.page !== 'string' || !view.page || view.page.length > 2000 ||
        ![view.width, view.height].every(n => Number.isInteger(n) && Number(n) >= 1 && Number(n) <= 8192)) throw new Error('Invalid current export view.');
    for (const key of ['state', 'lens', 'size']) if (view[key] != null && (typeof view[key] !== 'string' || String(view[key]).length > 2000)) throw new Error('Invalid current export ' + key + '.');
    current = { page: view.page, width: Number(view.width), height: Number(view.height),
      state: typeof view.state === 'string' ? view.state : null, lens: typeof view.lens === 'string' ? view.lens : null,
      size: typeof view.size === 'string' ? view.size : undefined };
  }
  const pages = strings(input.pages, 'pages');
  if (scope === 'page' && !pages.length && current) pages.push(current.page);
  if ((scope === 'page' && pages.length !== 1) || (scope === 'pages' && !pages.length)) throw new Error('Choose the pages to export.');
  if (variants === 'current' && (scope !== 'page' || !current || current.page !== pages[0])) throw new Error('Current view export requires its selected page.');
  const collection = input.collection;
  if (scope === 'collection' && (typeof collection !== 'string' || !collection)) throw new Error('Choose a collection to export.');
  if (input.lens != null && (typeof input.lens !== 'string' || input.lens.length > 2000)) throw new Error('Invalid export lens.');
  return { format, scope, pages, variants, states: strings(input.states, 'states'), sizes: strings(input.sizes, 'sizes'),
    collection: typeof collection === 'string' ? collection : undefined,
    lens: typeof input.lens === 'string' && input.lens ? input.lens : null, current,
    paper: choice(input.paper, ['A4', 'Letter'], 'A4'), imageFormat: choice(input.imageFormat, ['png', 'jpeg'], 'png') };
}

export interface CatalogItem { src?: string; items?: CatalogItem[]; [key: string]: unknown }
export interface CatalogCollection { name: string; items: CatalogItem[]; [key: string]: unknown }
export interface SelectableView {
  name?: string; pages?: Record<string, { label: string }>;
  collections?: CatalogCollection[]; catalogCollections?: CatalogCollection[];
}
export function pageIds(items: readonly CatalogItem[]): string[] {
  return items.flatMap(item => item.src ? [item.src] : pageIds(item.items || []));
}
export function selectView<T extends SelectableView>(view: T, request: ExportRequest): T {
  const all = view.pages || {};
  let selected = Object.keys(all);
  if (request.scope === 'page' || request.scope === 'pages') selected = request.pages;
  if (request.scope === 'collection') {
    const collections = [...(view.collections || []), ...(view.catalogCollections || [])].filter(c => c.name === request.collection);
    if (!collections.length) throw new Error('That export collection does not exist.');
    selected = collections.flatMap(c => pageIds(c.items));
  }
  if (selected.some(id => !Object.hasOwn(all, id))) throw new Error('An export page does not belong to this space.');
  const ids = new Set(selected);
  if (!ids.size) throw new Error('There are no pages to export.');
  const filterItems = (items: CatalogItem[]): CatalogItem[] => items.flatMap(item => {
    if (item.src) return ids.has(item.src) ? [item] : [];
    const children = filterItems(item.items || []);
    return children.length ? [{ ...item, items: children }] : [];
  });
  const filterCollections = (collections: CatalogCollection[] = []) => collections.map(c => ({ ...c, items: filterItems(c.items) })).filter(c => c.items.length);
  return { ...view, pages: Object.fromEntries(Object.entries(all).filter(([id]) => ids.has(id))),
    collections: filterCollections(view.collections), catalogCollections: filterCollections(view.catalogCollections) };
}

/** Only identical requests can share a background job. */
export function requestKey(request: ExportRequest): string { return JSON.stringify(request); }
