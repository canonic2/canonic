/* Changing a space's sizes in workbench.yaml
   ------------------------------------------
   What Custom size… and Edit sizes… ask for, worked out on the committed
   file's parsed values: the space's `sizes` map to write, and the space's
   `collections` when pages change too. Writing them is the config writer's
   job (config.js); this decides what to write and refuses what can't be
   written. See specs/sizes.md. */

import { DEFAULT_SIZES } from './browser/size.ts';
import type { Length } from './browser/size.ts';
import { keyForName, labelOf, readSize } from './schema.ts';

export type RawSize = true | Record<string, unknown>;
export type RawSizes = Record<string, RawSize>;
type RawEntry = Record<string, unknown>;

export interface SizeEdit {
  /** The space's `sizes` to write, when they changed. */
  sizes?: RawSizes;
  /** The space's `collections` to write, when a page changed. */
  collections?: unknown[];
}

export interface NewSize {
  name: string;
  width: Length;
  height: Length;
  icon?: string;
  button?: boolean;
  /** The page it was added from, by src; its list of sizes gains the key. */
  page?: string | null;
  /** Write the size into that page's own sizes rather than the space's. */
  pageOnly?: boolean;
}

export interface SizeRow {
  key: string;
  /** The size as it is written: `true` for a default size as it is, or its fields. */
  value: RawSize;
}

function isMap(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function clone<T>(value: T): T {
  return value === undefined ? value : JSON.parse(JSON.stringify(value));
}

/** The committed sizes, or the defaults spelled out when the space declares none. */
function committed(raw: unknown): RawSizes {
  if (isMap(raw)) return clone(raw) as RawSizes;
  const out: RawSizes = {};
  for (const size of DEFAULT_SIZES) out[size.key] = true;
  return out;
}

/** Every page entry in a collections list, groups included, in order. */
function pageEntries(collections: unknown): RawEntry[] {
  const out: RawEntry[] = [];
  const visit = (items: unknown) => {
    if (!Array.isArray(items)) return;
    for (const item of items) {
      if (!isMap(item)) continue;
      if (typeof item.group === 'string') visit(item.items);
      else out.push(item);
    }
  };
  if (Array.isArray(collections)) for (const collection of collections) if (isMap(collection)) visit(collection.items);
  return out;
}

function listOf(value: unknown): unknown[] | null {
  if (value === undefined || value === null || value === '') return null;
  return Array.isArray(value) ? value : [value];
}

/** Keys of the sizes pages define for themselves, so a new key avoids them. */
function ownKeys(collections: unknown): string[] {
  const keys: string[] = [];
  for (const page of pageEntries(collections)) {
    for (const entry of listOf(page.sizes) || []) if (isMap(entry)) keys.push(...Object.keys(entry));
  }
  return keys;
}

function check(key: string, value: RawSize, page: boolean) {
  const problems: string[] = [];
  readSize(key, value, labelOf(key), problems, page);
  if (problems.length) throw new Error(problems[0]!.replace(/^[^:]*: /, ''));
}

function fields(input: NewSize, key: string, page: boolean): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  const name = input.name.trim();
  if (name && name !== labelOf(key)) out.label = name;
  out.width = input.width;
  out.height = input.height;
  if (input.icon && input.icon.trim()) out.icon = input.icon.trim();
  if (input.button && !page) out.button = true;
  return out;
}

/** Custom size…: a new size, for the space or for one page. */
export function addSize(rawSizes: unknown, rawCollections: unknown, input: NewSize): SizeEdit & { key: string } {
  if (!input.name || !input.name.trim()) throw new Error('Give the size a name.');
  const sizes = committed(rawSizes);
  const collections = clone(Array.isArray(rawCollections) ? rawCollections : []);
  const key = keyForName(input.name, [...Object.keys(sizes), ...ownKeys(collections)]);
  const pageOnly = !!input.pageOnly;
  const value = fields(input, key, pageOnly);
  check(key, value, pageOnly);
  const page = input.page ? pageEntries(collections).find(entry => entry.src === input.page) : undefined;
  if (pageOnly) {
    if (!page) throw new Error('Only a page listed in workbench.yaml can have sizes of its own.');
    page.sizes = [...(listOf(page.sizes) || []), { [key]: value }];
    return { key, collections };
  }
  sizes[key] = value;
  const listed = page && listOf(page.sizes);
  if (page && listed) {
    page.sizes = [...listed, key];
    return { key, sizes, collections };
  }
  return { key, sizes };
}

/**
 * Edit sizes…: the space's sizes as the dialog left them, in order. A size
 * left out is removed, from the space and from every page that lists it.
 * Sizes workbench.local.yaml defines or changes can't be changed here: a
 * committed one keeps its committed value, whatever its row says, and one
 * only the local file defines isn't written.
 */
export function updateSizes(rawSizes: unknown, rawCollections: unknown, rows: readonly SizeRow[], local: readonly string[] = []): SizeEdit {
  if (!rows.some(row => !local.includes(row.key) || Object.prototype.hasOwnProperty.call(committed(rawSizes), row.key))) throw new Error('A space keeps at least one size.');
  const before = committed(rawSizes);
  const sizes: RawSizes = {};
  for (const row of rows) {
    if (Object.prototype.hasOwnProperty.call(sizes, row.key)) throw new Error('Size “' + row.key + '” is listed twice.');
    const committedRow = Object.prototype.hasOwnProperty.call(before, row.key);
    if (local.includes(row.key)) {
      if (committedRow) sizes[row.key] = clone(before[row.key]!);
      continue;
    }
    if (!committedRow) throw new Error('There is no size “' + row.key + '” to change; add it with Custom size….');
    check(row.key, row.value, false);
    sizes[row.key] = clone(row.value);
  }
  const removed = Object.keys(before).filter(key => !Object.prototype.hasOwnProperty.call(sizes, key));
  const blocked = removed.find(key => local.includes(key));
  if (blocked) throw new Error('Size “' + blocked + '” comes from workbench.local.yaml; remove it there.');
  if (!removed.length) return { sizes };
  const collections = clone(Array.isArray(rawCollections) ? rawCollections : []);
  let changed = false;
  for (const page of pageEntries(collections)) {
    const listed = listOf(page.sizes);
    if (!listed) continue;
    const kept = listed.filter(entry => !(typeof entry === 'string' && removed.includes(entry.trim())));
    if (kept.length === listed.length) continue;
    changed = true;
    if (kept.length) page.sizes = kept;
    else delete page.sizes;
  }
  return changed ? { sizes, collections } : { sizes };
}
