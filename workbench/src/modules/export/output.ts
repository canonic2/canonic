import { PDFDocument } from 'pdf-lib';
import { splitArchives, zip } from './archive.ts';
import type { ExportReport, ExportResult, ZipEntry, PortableExport } from './types.ts';
import type { ExportRequest } from './request.ts';

export interface CapturedReference { page: string; variant?: string; label: string; body: Buffer }
export function outputReport(name: string, request: ExportRequest, warnings: string[]): ExportReport {
  return { name, generatedAt: new Date().toISOString(), selection: request.scope + ' export',
    files: [], pages: [], dependencies: [], warnings: [...new Set(warnings)], captureWarnings: [], browser: null, request };
}
function stem(name: string) { return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'workbench'; }
function packageFiles(name: string, entries: ZipEntry[], report: ExportReport, maxArchiveBytes?: number): ExportResult {
  const archives = splitArchives(name, entries, '# ' + report.name + ' export\n\nSee canonic-export.json for selection and warnings.\n', report, maxArchiveBytes);
  const download = archives.length === 1 ? archives[0] : { filename: name + '-parts.zip', body: zip(archives.map(a => ({ name: a.filename, body: a.body }))) };
  return { filename: archives.length === 1 ? download.filename : null, body: archives.length === 1 ? download.body : null, archives, download, report };
}
export async function pdfOutput(name: string, request: ExportRequest, references: CapturedReference[], warnings: string[]): Promise<ExportResult> {
  if (!references.length) throw new Error('No pages could be rendered for this PDF. ' + warnings.join(' '));
  const document = await PDFDocument.create();
  for (const reference of references) {
    const source = await PDFDocument.load(reference.body);
    for (const page of await document.copyPages(source, source.getPageIndices())) document.addPage(page);
  }
  document.setTitle(name); document.setSubject('Workbench ' + request.scope + ' export');
  const archive = { filename: stem(name) + '.pdf', body: Buffer.from(await document.save()), contentType: 'application/pdf' };
  return { filename: archive.filename, body: archive.body, archives: [archive], download: archive, report: outputReport(name, request, warnings) };
}
export function imagesOutput(name: string, request: ExportRequest, references: CapturedReference[], warnings: string[], maxArchiveBytes?: number): ExportResult {
  if (!references.length) throw new Error('No reference images could be captured. ' + warnings.join(' '));
  const prefix = stem(name) + '-images';
  const entries = references.map((r, i) => ({ name: prefix + '/' + String(i + 1).padStart(3, '0') + '-' + stem(r.page) + '-' + stem(r.variant || r.label) + '.' + (request.imageFormat === 'png' ? 'png' : 'jpg'), body: r.body }));
  const report = outputReport(name, request, warnings); report.files = entries.map(e => e.name.slice(prefix.length + 1));
  return packageFiles(prefix, entries, report, maxArchiveBytes);
}
export function browserOutput(name: string, request: ExportRequest, portable: PortableExport | null | undefined, maxArchiveBytes?: number): ExportResult {
  if (!portable?.files.length || !portable.previews.length && !portable.docs?.length) throw new Error('No portable browser previews could be built for this selection.');
  const prefix = stem(name) + '-browser';
  const report = outputReport(name, request, portable.warnings);
  const entries = portable.files.map(file => {
    if (!file.path.startsWith('browser/') || file.path.split('/').some(part => part === '..' || part === '.')) throw new Error('Invalid portable export path');
    return { name: prefix + '/' + file.path, body: Buffer.from(file.data, 'base64') };
  });
  report.files = portable.files.map(f => f.path);
  return packageFiles(prefix, entries, report, maxArchiveBytes);
}
