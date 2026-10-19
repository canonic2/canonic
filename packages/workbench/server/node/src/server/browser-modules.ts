/* Static browser assets. Responses contain the authored JavaScript/CSS bytes;
   no compiler or source transformation participates in UI delivery. */

import { promises as fs } from 'node:fs';
import path from 'node:path';

export const PREFIX = '/_workbench/';

export interface BrowserAsset {
  status: number;
  type: string;
  body: string;
}

const TYPES: Record<string, string> = {
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
};

/* Source ownership is also the browser asset boundary. */
const BROWSER = /^(ui\/src|contracts\/src)\//;

export function createBrowserModules(root: string) {
  return async function serve(pathname: string): Promise<BrowserAsset> {
    let relative: string;
    try { relative = decodeURIComponent(pathname.slice(PREFIX.length)); } catch { return missing(pathname); }
    if (relative.includes('\\') || relative.split('/').some(part => part === '.' || part === '..')) return missing(pathname);
    const extension = path.extname(relative);
    if (!TYPES[extension] || /\.test\.js$/.test(relative) || !BROWSER.test(relative)) return missing(pathname);
    const file = path.resolve(root, relative);
    if (!file.startsWith(root + path.sep)) return missing(pathname);
    let stat;
    try { stat = await fs.stat(file); } catch { return missing(pathname); }
    if (!stat.isFile()) return missing(pathname);
    const body = await fs.readFile(file, 'utf8');
    return { status: 200, type: TYPES[extension]!, body };
  };
}

function missing(pathname: string): BrowserAsset {
  return { status: 404, type: 'text/plain; charset=utf-8', body: 'Not a Workbench browser module: ' + pathname };
}
