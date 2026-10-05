import { create } from './source.ts';
import type { ExportOptions, ExportResult, ExportView, PortableExport } from './types.ts';
import type { ExportRequest } from './request.ts';

export interface PortableBuilder {
  export(selection?: { pages: string[]; states: string[]; sizes: string[] }): Promise<PortableExport>;
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
      const request: ExportRequest | undefined = options.request;
      portable = await previews.export(request ? { pages: Object.keys(view.pages || {}), states: request.states, sizes: request.sizes } : undefined);
    } catch (error) {
      portable = null;
      warnings.push('Portable browser previews: ' + String(error instanceof Error ? error.message : error));
    }
  }
  return create(root, view, { ...options, portable, warnings });
}
