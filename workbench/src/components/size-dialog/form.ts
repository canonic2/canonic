/* What the Custom size… form says, checked
   ----------------------------------------
   The dialog's fields as typed, turned into the size the controller sends,
   or the first reason it can't be sent yet. The server checks again; this
   keeps Add disabled and says why while the fields are wrong. */

import { isLength, MAX_LENGTH, MIN_LENGTH } from '../../sizes/browser/size.ts';
import type { Length } from '../../sizes/browser/size.ts';

export interface SizeDraft {
  name: string;
  width: string;
  widthFill: boolean;
  height: string;
  heightFill: boolean;
  icon: string;
  button: boolean;
  pageOnly: boolean;
}

/** What wb-size-add carries: the size as the server's add route takes it. */
export interface SizeRequest {
  name: string;
  width: Length;
  height: Length;
  icon?: string;
  button: boolean;
  pageOnly: boolean;
}

export const EMPTY_DRAFT: SizeDraft = { name: '', width: '', widthFill: false, height: '', heightFill: false, icon: '', button: false, pageOnly: false };

const ICON = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** A typed length, or null when it isn't one. */
export function lengthOf(text: string, fill: boolean): Length | null {
  if (fill) return 'fill';
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return isLength(value) ? value : null;
}

const LENGTH = `a whole number from ${MIN_LENGTH} to ${MAX_LENGTH}, or Fill`;

/** The size to send, or the first problem with the fields. */
export function readDraft(draft: SizeDraft): { size: SizeRequest } | { problem: string } {
  const name = draft.name.trim();
  if (!name) return { problem: 'Give the size a name.' };
  const width = lengthOf(draft.width, draft.widthFill);
  if (width === null) return { problem: `Width must be ${LENGTH}.` };
  const height = lengthOf(draft.height, draft.heightFill);
  if (height === null) return { problem: `Height must be ${LENGTH}.` };
  if (width === 'fill' && height === 'fill') return { problem: 'Width and height can’t both fill the canvas; use Fit.' };
  const icon = draft.icon.trim();
  if (icon && !ICON.test(icon)) return { problem: 'The icon is a Lucide name in kebab-case, such as panel-left.' };
  return { size: { name, width, height, ...(icon ? { icon } : {}), button: draft.button && !draft.pageOnly, pageOnly: draft.pageOnly } };
}
