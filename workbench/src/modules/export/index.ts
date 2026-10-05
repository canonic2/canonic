/** Public Node API for a whole space's sources, references and archives.
 * Job scheduling, capture and preview compilation are supplied by the host.
 */
export { create } from './source.ts';
export { build } from './build.ts';
export type { PortableBuilder } from './build.ts';
import path from 'node:path';
import { parseReferences } from './references.ts';
export { packageName } from './references.ts';
export function references(file: string, body: string): string[] {
  return parseReferences(path.extname(file).toLowerCase(), body);
}
export { zip, zipSize, MAX_ARCHIVE_BYTES } from './archive.ts';
export type { ExportView, ExportOptions, ExportResult, ExportReport, PageReport, ReferenceScreenshot, PortableExport, PortablePreview, Archive, ZipEntry } from './types.ts';
