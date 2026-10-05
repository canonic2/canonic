/* Reading `sizes` from workbench.yaml
   -----------------------------------
   A space's `sizes` is an ordered map of key to size; a page's `sizes` lists
   the space's keys it supports and may define sizes of its own. Anything
   malformed is reported, in the form every problem takes, and left out; the
   rest is used. See specs/sizes.md. Pure: the config reader hands it the
   parsed YAML and keeps the problems. */

import { defaultSize, defaultSizes, isLength, isSizeKey, labelOf } from './browser/size.ts';
import type { Length, PageSizes, ResolvedSize } from './browser/size.ts';

const FIELDS = ['width', 'height', 'label', 'icon', 'button'];
const ICON = /^[a-z0-9-]+$/;

function isMap(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export { labelOf };

/**
 * One size from its key and YAML value, or null when it can't be used.
 * `where` names it in problems; `page` is true for a page's own size, which
 * can't be a button.
 */
export function readSize(key: string, raw: unknown, where: string, problems: string[], page = false): ResolvedSize | null {
  const base = defaultSize(key);
  if (raw === true) {
    if (base) return { ...base, button: page ? false : base.button };
    problems.push(where + ': true only includes a default size (fit, laptop, mobile, or resizable); give this size a width and height.');
    return null;
  }
  if (!isMap(raw)) {
    problems.push(where + ': must be true or a map of width, height, label, icon, and button.');
    return null;
  }
  for (const field of Object.keys(raw)) {
    if (!FIELDS.includes(field)) problems.push(where + ': “' + field + '” isn’t a size field (width, height, label, icon, button).');
  }
  const kind = base?.kind ?? 'fixed';
  if (kind !== 'fixed' && (raw.width !== undefined || raw.height !== undefined)) {
    problems.push(where + ': ' + key + (kind === 'fit' ? ' fills the canvas' : ' is dragged to any size') + ', so it takes no width or height.');
    return null;
  }
  let width: Length | null = null;
  let height: Length | null = null;
  if (kind === 'fixed') {
    const lengths: Record<'width' | 'height', Length | null> = { width: null, height: null };
    for (const name of ['width', 'height'] as const) {
      const value = raw[name] === undefined && base ? base[name] : raw[name];
      if (!isLength(value)) {
        problems.push(where + ': ' + name + ' must be a whole number from 1 to 8192, or fill.');
        return null;
      }
      lengths[name] = value;
    }
    width = lengths.width;
    height = lengths.height;
    if (width === 'fill' && height === 'fill') {
      problems.push(where + ': width and height can’t both be fill; use fit.');
      return null;
    }
  }
  const label = typeof raw.label === 'string' && raw.label.trim() ? raw.label.trim() : base?.label ?? labelOf(key);
  let icon = base?.icon ?? 'frame';
  if (raw.icon !== undefined) {
    if (typeof raw.icon === 'string' && ICON.test(raw.icon.trim())) icon = raw.icon.trim();
    else problems.push(where + ': icon “' + String(raw.icon) + '” must be a kebab-case Lucide icon name.');
  }
  let button = page ? false : base ? base.button : false;
  if (raw.button !== undefined) {
    if (page) problems.push(where + ': button doesn’t apply to a page’s own size; the size switcher’s buttons belong to the space.');
    else if (typeof raw.button === 'boolean') button = raw.button;
    else problems.push(where + ': button must be true or false.');
  }
  return { key, label, icon, button, kind, width, height };
}

/**
 * A space's sizes, in order, or the default sizes when it declares none or
 * none of them can be used. `local` names the keys workbench.local.yaml set.
 */
export function readSpaceSizes(raw: unknown, problems: string[], local: readonly string[] = []): ResolvedSize[] {
  if (raw === undefined || raw === null) return defaultSizes();
  if (!isMap(raw)) {
    problems.push('Sizes: must be a map of size keys to sizes.');
    return defaultSizes();
  }
  const out: ResolvedSize[] = [];
  for (const [key, value] of Object.entries(raw)) {
    if (!isSizeKey(key)) {
      problems.push('Sizes: “' + key + '” must be a kebab-case key that isn’t all digits.');
      continue;
    }
    const size = readSize(key, value, 'Sizes › ' + key, problems);
    if (size) out.push(local.includes(key) ? { ...size, local: true } : size);
  }
  if (!out.length) {
    problems.push('Sizes: no size can be used, so the default sizes are.');
    return defaultSizes();
  }
  return out;
}

/**
 * A page's `sizes`: the space's keys it supports and its own sizes, or null
 * when it supports every size of the space.
 */
export function readPageSizes(raw: unknown, space: readonly ResolvedSize[], where: string, problems: string[]): PageSizes | null {
  if (raw === undefined || raw === null || raw === '') return null;
  const entries = Array.isArray(raw) ? raw : [raw];
  const sizes: string[] = [];
  const ownSizes: ResolvedSize[] = [];
  for (const entry of entries) {
    if (typeof entry === 'string') {
      const key = entry.trim();
      if (!space.some(size => size.key === key)) {
        problems.push(where + ': size “' + key + '” isn’t one of the space’s sizes.');
        continue;
      }
      if (!sizes.includes(key)) sizes.push(key);
      continue;
    }
    if (!isMap(entry) || Object.keys(entry).length !== 1) {
      problems.push(where + ': each size is a key of the space’s sizes, or one key with the fields of a size of the page’s own.');
      continue;
    }
    const [key, value] = Object.entries(entry)[0]!;
    if (!isSizeKey(key)) {
      problems.push(where + ': size “' + key + '” must be a kebab-case key that isn’t all digits.');
      continue;
    }
    if (space.some(size => size.key === key)) {
      problems.push(where + ': size “' + key + '” is one of the space’s sizes; a page can list it but not change it.');
      continue;
    }
    if (sizes.includes(key)) {
      problems.push(where + ': size “' + key + '” is listed twice.');
      continue;
    }
    const size = readSize(key, value, where + ' › ' + key, problems, true);
    if (!size) continue;
    sizes.push(key);
    ownSizes.push(size);
  }
  if (!sizes.length) return null;
  return ownSizes.length ? { sizes, ownSizes } : { sizes };
}

/**
 * A new size's key from the name someone typed: kebab-case, `size-` in front
 * when that leaves nothing or only digits, and a numeric suffix until it
 * isn't one of `taken`.
 */
export function keyForName(name: string, taken: readonly string[]): string {
  let key = name.toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '')
    .replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');
  if (!isSizeKey(key)) key = key ? 'size-' + key : 'size';
  let candidate = key;
  for (let n = 2; taken.includes(candidate); n += 1) candidate = key + '-' + n;
  return candidate;
}
