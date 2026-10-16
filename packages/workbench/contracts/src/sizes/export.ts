/* The sizes an export captures a page at
   --------------------------------------
   Every size the page supports except Resizable, which has no size of its
   own; a filled axis, and Fit, take EXPORT_FILL. Sizes that come to the same
   dimensions are captured once, under the first of them. A page whose only
   size is Resizable is still captured once, at EXPORT_FILL. */

import { exportDimensions } from './browser/geometry.ts';
import { EXPORT_FILL } from './browser/size.ts';
import type { ResolvedSize } from './browser/size.ts';

export interface ExportSize { key: string; label: string; width: number; height: number }

export function exportSizes(sizes: readonly ResolvedSize[]): ExportSize[] {
  const out: ExportSize[] = [];
  for (const size of sizes) {
    const dimensions = exportDimensions(size);
    if (!dimensions) continue;
    if (out.some(seen => seen.width === dimensions.width && seen.height === dimensions.height)) continue;
    out.push({ key: size.key, label: size.label, ...dimensions });
  }
  if (!out.length) {
    const only = sizes[0];
    out.push({ key: only ? only.key : 'fit', label: only ? only.label : 'Fit', ...EXPORT_FILL });
  }
  return out;
}
