import { create } from './source.ts';
import type { ExportOptions, ExportResult, ExportView, PortableExport } from './types.ts';

export interface PortableBuilder {
  export(): Promise<PortableExport>;
}

/** Portable previews supplement sources and references; their builder can fail
 * independently. Archive errors remain fatal and preserve their original cause.
 */
export async function build(
  root: string,
  view: ExportView,
  options: ExportOptions,
  previews?: PortableBuilder | null,
): Promise<ExportResult> {
  let portable = options.portable;
  const warnings = [...(options.warnings || [])];
  if (previews) {
    try {
      portable = await previews.export();
    } catch (error) {
      portable = null;
      warnings.push('Portable browser previews: ' + String(error instanceof Error ? error.message : error));
    }
  }
  return create(root, view, { ...options, portable, warnings });
}
