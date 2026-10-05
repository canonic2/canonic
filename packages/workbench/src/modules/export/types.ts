import type { ExportRequest } from './request.ts';
/** The resolved-space subset consumed by the source exporter. */
export interface ExportView {
  name?: string;
  implementations?: Record<string, { root?: string | null; kind?: string }>;
  pages?: Record<string, { label: string; design?: string | null; code?: { path?: string | null; exists?: boolean; implementation: string }[] }>;
}
export interface PortablePreview {
  id: string;
  file: string;
  directory: string;
  states: { id: string; label: string }[];
  files: string[];
  packages?: { name: string; version?: string | null }[];
}
export interface PortableExport {
  docs?: unknown[];
  files: { path: string; data: string }[];
  previews: PortablePreview[];
  warnings: string[];
}
export interface ReferenceScreenshot {
  page: string;
  state?: string | null;
  variant?: string;
  label?: string;
  size?: string;
  sizeLabel?: string;
  width: number;
  height: number;
  body: Buffer;
}
export interface ExportOptions {
  request?: ExportRequest;
  /** Build warnings recorded alongside unresolved source references. */
  warnings?: string[];
  manifest?: string;
  portable?: PortableExport | null;
  screenshots?: ReferenceScreenshot[];
  captureWarnings?: string[];
  maxArchiveBytes?: number;
}
export interface PageEntry { kind: 'design' | 'source'; path: string; implementation?: string }
export interface PageReport {
  id: string;
  label: string;
  hash: string;
  readme: string;
  entries: PageEntry[];
  files: string[];
  screenshots: { state: string | null; label: string; size: string; sizeLabel?: string; path: string; width: number; height: number }[];
}
export interface ExportReport {
  request?: ExportRequest;
  name?: string;
  generatedAt: string;
  selection: string;
  files: string[];
  pages: PageReport[];
  dependencies: { name: string; version: string | null }[];
  warnings: string[];
  captureWarnings: string[];
  browser: { entry: string; manifest: string; previews: { id: string; source: string; entry: string; states: PortablePreview['states'] }[]; warnings: string[] } | null;
  parts?: { number: number; filename: string; files: number }[];
}
export interface ZipEntry { name: string; body: string | Buffer }
export interface Archive { filename: string; body: Buffer; contentType?: string }
export interface ExportResult {
  filename: string | null;
  body: Buffer | null;
  archives: Archive[];
  download: Archive;
  report: ExportReport;
}

/** Internal filesystem and package graph records. */
export interface SourceOwner { key: string; root: string; destination: string }
export interface SourceFile { source: string; destination: string; owner: SourceOwner }
export type PackageExport = string | PackageExport[] | { [key: string]: PackageExport };
export interface PackageManifest {
  name?: string;
  scripts?: Record<string, string>;
  exports?: PackageExport;
  module?: string;
  main?: string;
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
}
export interface PackageInfo { manifest: string; root: string; package: PackageManifest }
