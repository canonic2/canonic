/* Serves Workbench's own browser code from src/ under /_workbench/src/:
   TypeScript modules with their types stripped, and stylesheets as they are.
   Nothing is compiled ahead of time; a module is transformed when it is first
   asked for and again only after its file changes. Tests and server-only code
   are not served. */

import { promises as fs } from 'node:fs';
import path from 'node:path';

export const PREFIX = '/_workbench/src/';

export interface BrowserAsset {
  status: number;
  type: string;
  body: string;
}

/** Strips a module's types; esbuild's `transform` in the server. */
export type Transform = (code: string, file: string) => Promise<string>;

const TYPES: Record<string, string> = {
  '.ts': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
};

/* Browser code lives in these folders; the rest of src/ runs in Node. */
const BROWSER = /(^|\/)(page|canvas|browser)\//;

export function createBrowserModules(root: string, transform: Transform) {
  const cache = new Map<string, { stamp: string; body: string }>();

  return async function serve(pathname: string): Promise<BrowserAsset> {
    let relative: string;
    try { relative = decodeURIComponent(pathname.slice(PREFIX.length)); } catch { return missing(pathname); }
    const extension = path.extname(relative);
    if (!TYPES[extension] || /\.test\.ts$/.test(relative) || !BROWSER.test(relative)) return missing(pathname);
    const file = path.resolve(root, relative);
    if (!file.startsWith(root + path.sep)) return missing(pathname);
    let stat;
    try { stat = await fs.stat(file); } catch { return missing(pathname); }
    if (!stat.isFile()) return missing(pathname);
    const stamp = stat.mtimeMs + ':' + stat.size;
    const cached = cache.get(file);
    if (cached && cached.stamp === stamp) return { status: 200, type: TYPES[extension]!, body: cached.body };
    const source = await fs.readFile(file, 'utf8');
    const body = extension === '.ts' ? await transform(source, file) : source;
    cache.set(file, { stamp, body });
    return { status: 200, type: TYPES[extension]!, body };
  };
}

function missing(pathname: string): BrowserAsset {
  return { status: 404, type: 'text/plain; charset=utf-8', body: 'Not a Workbench browser module: ' + pathname };
}
