/**
 * Rspack loaders for HTML previews and Astro renders. The page becomes a
 * module whose default export is its HTML:
 *
 * - linked local stylesheets and inline `<style>` blocks become CSS imports,
 *   so they enter the build, resolve their `url()` assets and update in place;
 * - inline scripts and local `<script src>` become modules of the build, so
 *   they can import local modules, packages and CSS, and their top-level
 *   declarations stay private. Each is replaced by a marker the HTML adapter
 *   runs on every render, in document order, with its imports evaluated once;
 * - local images and media become asset URLs;
 * - `<title>`, `<meta>` and `<base>` are dropped.
 *
 * JSON data blocks and external scripts stay in the HTML. `wbfile:<absolute path>` names a
 * file the worker generated, such as an Astro resource. The parts are served
 * by `htmlPartLoader` through resource queries on the same file.
 */
import crypto from 'node:crypto';
import path from 'node:path';

interface LoaderContext { rootContext: string; resourcePath: string; resourceQuery: string }

const external = (value: string): boolean => /^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i.test(value) && !value.startsWith('wbfile:');
const INLINE_STYLE = /<style\b[^>]*>([\s\S]*?)<\/style>/gi;
const SCRIPT = /<script\b([^>]*)>([\s\S]*?)<\/script>/gi;
const isModule = (attributes: string): boolean => /\btype\s*=\s*["']module["']/i.test(attributes);
const srcOf = (attributes: string): string | null => (/\bsrc\s*=\s*(["'])([^"']+)\1/i.exec(attributes) || [])[2] || null;
const isData = (attributes: string): boolean => /\btype\s*=\s*["']application\/(?:ld\+)?json["']/i.test(attributes);
/** An Astro client script: a module script that only imports a generated resource. */
const ASTRO_SCRIPT = /^\s*import\s*["'](wbfile:[^"']+)["'];?\s*$/;

/** Inline scripts the build compiles, in document order, as both loaders count them. */
function inlineScripts(source: string): string[] {
  return Array.from(source.matchAll(SCRIPT)).filter(match => !srcOf(match[1]) && !isData(match[1]) && !(isModule(match[1]) && ASTRO_SCRIPT.test(match[2]))).map(match => match[2]);
}

function inlineStyles(source: string): string[] {
  return Array.from(source.matchAll(INLINE_STYLE)).map(match => match[1]);
}

/**
 * A script as a module that runs again on each render: its imports stay at
 * the top, evaluated once, and the rest becomes the default export.
 */
export function runnableScript(source: string): string {
  const imports: string[] = [];
  const leading = /^\s*import\s(?:[^;'"]*?\sfrom\s*)?["'][^"']+["']\s*;?/;
  let body = source;
  for (let match = leading.exec(body); match; match = leading.exec(body)) {
    imports.push(match[0].trim());
    body = body.slice(match[0].length);
  }
  return imports.join('\n') + '\nexport default async function workbenchScript() {\n' + body + '\n}\n';
}

export function transformHtml(source: string, resourcePath: string, root: string): string {
  const request = (value: string): string => {
    const clean = value.split(/[?#]/)[0];
    if (clean.startsWith('wbfile:')) return clean.slice('wbfile:'.length);
    if (clean.startsWith('/')) return path.join(root, clean.slice(1));
    return path.resolve(path.dirname(resourcePath), clean);
  };
  const id = crypto.createHash('sha256').update(resourcePath).digest('hex').slice(0, 12);
  const imports: string[] = [];
  const scripts: string[] = [];
  const urls: string[] = [];
  let styles = 0;
  let inline = 0;
  let html = source.replace(/<link\b[^>]*>/gi, tag => {
    if (!/\brel\s*=\s*["']?stylesheet/i.test(tag)) return tag;
    const href = /\bhref\s*=\s*(["'])([^"']+)\1/i.exec(tag);
    if (!href || external(href[2])) return tag;
    imports.push('import ' + JSON.stringify(request(href[2])) + ';');
    return '';
  });
  html = html.replace(INLINE_STYLE, () => {
    imports.push('import ' + JSON.stringify(resourcePath + '?workbench-style=' + styles++) + ';');
    return '';
  });
  html = html.replace(SCRIPT, (match, attributes: string, body: string) => {
    // Astro hoists its client scripts into module scripts that import a resource.
    const astro = ASTRO_SCRIPT.exec(body);
    if (isModule(attributes) && astro) { imports.push('import ' + JSON.stringify(request(astro[1])) + ';'); return ''; }
    if (isData(attributes)) return match;
    const src = srcOf(attributes);
    let target: string | null = null;
    if (src && !external(src)) target = request(src) + '?workbench-run';
    else if (!src) target = resourcePath + '?workbench-script=' + inline++;
    if (!target) return match;
    scripts.push('() => import(' + JSON.stringify(target) + ')');
    // Other attributes stay on the marker, for scripts that read them.
    const kept = attributes.replace(/\s(?:src|type)\s*=\s*(["'])[^"']*\1/gi, '');
    return '<script' + kept + ' data-workbench-script="' + id + ':' + (scripts.length - 1) + '"></script>';
  });
  const placeholder = (value: string): string => { urls.push(request(value)); return '__WB_URL_' + (urls.length - 1) + '__'; };
  // Attribute-like text in Markdown code samples is not an asset reference.
  html = html.replace(/<!--[\s\S]*?-->|<[a-z](?:[^"'<>]|"[^"]*"|'[^']*')*>/gi, tag => {
    if (tag.startsWith('<!--')) return tag;
    return tag.replace(/\b(src|poster)\s*=\s*(["'])([^"']+)\2/gi, (match, attribute: string, quote: string, value: string) =>
      external(value) ? match : attribute + '=' + quote + placeholder(value) + quote)
      .replace(/\bsrcset\s*=\s*(["'])([^"']+)\1/gi, (_match, quote: string, value: string) => 'srcset=' + quote + value.split(',').map(item => {
        const [url, ...size] = item.trim().split(/\s+/);
        return (external(url) ? url : placeholder(url)) + (size.length ? ' ' + size.join(' ') : '');
      }).join(', ') + quote);
  });
  html = html.replace(/<title>[\s\S]*?<\/title>|<meta\b[^>]*>|<base\b[^>]*>/gi, '');
  const pieces = html.split(/__WB_URL_(\d+)__/).map((part, index) => index % 2 ? 'url' + part : JSON.stringify(part));
  return imports.join('\n') + '\n' +
    urls.map((url, index) => 'import url' + index + ' from ' + JSON.stringify(url) + ';').join('\n') + '\n' +
    (scripts.length ? '(window.__wbHtmlScripts || (window.__wbHtmlScripts = {}))[' + JSON.stringify(id) + '] = [' + scripts.join(', ') + '];\n' : '') +
    'export default ' + pieces.join(' + ') + ';\n';
}

export default function htmlLoader(this: LoaderContext, source: string): string {
  return transformHtml(source, this.resourcePath, this.rootContext);
}

/** The inline style or script a query names, or a script file made runnable. */
export function htmlPart(source: string, query: string): string {
  const params = new URLSearchParams(query.replace(/^\?/, ''));
  if (params.has('workbench-style')) return inlineStyles(source)[Number(params.get('workbench-style'))] ?? '';
  if (params.has('workbench-script')) return runnableScript(inlineScripts(source)[Number(params.get('workbench-script'))] ?? '');
  if (params.has('workbench-run')) return runnableScript(source);
  throw new Error('Unknown HTML part: ' + query);
}

export function htmlPartLoader(this: LoaderContext, source: string): string {
  return htmlPart(source, this.resourceQuery);
}
