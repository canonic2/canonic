/* What the Edit sizes… rows say, checked and written back
   -------------------------------------------------------
   A row starts from a size as the space resolves it and is edited as typed.
   Saving turns each row into the value workbench.yaml holds for it: `true`
   for a default size left as it is, else only the fields that say something.
   The server checks again; see src/sizes/edit.ts. */

import { lengthOf } from '../size-dialog/form.ts';
import { defaultSize, labelOf } from '../../sizes/browser/size.ts';
import type { Length, ResolvedSize, SizeKind } from '../../sizes/browser/size.ts';

export interface SizeRowDraft {
  key: string;
  kind: SizeKind;
  name: string;
  width: string;
  widthFill: boolean;
  height: string;
  heightFill: boolean;
  icon: string;
  button: boolean;
  /** Set by workbench.local.yaml, so shown but not changed here. */
  local: boolean;
}

/** What wb-sizes-save carries for each row, as the server's update route takes it. */
export interface SizeRowValue { key: string; value: true | Record<string, unknown> }

const ICON = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function text(length: Length | null): string {
  return typeof length === 'number' ? String(length) : '';
}

export function draftOf(size: ResolvedSize): SizeRowDraft {
  return {
    key: size.key, kind: size.kind, name: size.label,
    width: text(size.width), widthFill: size.width === 'fill',
    height: text(size.height), heightFill: size.height === 'fill',
    icon: size.icon, button: size.button, local: !!size.local,
  };
}

/** The first problem with a row, or null. Local rows aren't checked: they aren't written. */
export function rowProblem(row: SizeRowDraft): string | null {
  if (row.local) return null;
  const name = row.name.trim() || row.key;
  if (!row.name.trim()) return name + ' needs a name.';
  if (row.kind === 'fixed') {
    const width = lengthOf(row.width, row.widthFill);
    const height = lengthOf(row.height, row.heightFill);
    if (width === null) return name + ': width must be a whole number from 1 to 8192, or Fill.';
    if (height === null) return name + ': height must be a whole number from 1 to 8192, or Fill.';
    if (width === 'fill' && height === 'fill') return name + ': width and height can’t both fill the canvas; use Fit.';
  }
  if (row.icon.trim() && !ICON.test(row.icon.trim())) return name + ': the icon is a Lucide name in kebab-case, such as panel-left.';
  return null;
}

/** The first problem across the rows, or null when they can be saved. */
export function rowsProblem(rows: readonly SizeRowDraft[]): string | null {
  if (!rows.length) return 'A space keeps at least one size.';
  for (const row of rows) {
    const problem = rowProblem(row);
    if (problem) return problem;
  }
  return null;
}

/** The row as workbench.yaml writes it. */
export function rowValue(row: SizeRowDraft): SizeRowValue {
  const base = defaultSize(row.key);
  const out: Record<string, unknown> = {};
  const label = row.name.trim();
  const icon = row.icon.trim() || (base ? base.icon : 'frame');
  if (label !== (base ? base.label : labelOf(row.key))) out.label = label;
  if (row.kind === 'fixed') {
    const width = lengthOf(row.width, row.widthFill);
    const height = lengthOf(row.height, row.heightFill);
    if (!base || width !== base.width) out.width = width;
    if (!base || height !== base.height) out.height = height;
  }
  if (icon !== (base ? base.icon : 'frame')) out.icon = icon;
  if (row.button !== (base ? base.button : false)) out.button = row.button;
  return { key: row.key, value: base && !Object.keys(out).length ? true : out };
}
