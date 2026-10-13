// Render parity: mounts each variant's output for each preview and state in
// headless Chrome through a minimal host page, and returns the rendered HTML
// of the preview canvas with generated asset names normalized.
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { createRequire } from 'node:module';

const MIME = { '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.woff': 'font/woff', '.json': 'application/json', '.html': 'text/html' };

function serve(workbench, outputs) {
  const runtime = { '/_workbench/preview-runtime.js': path.join(workbench, 'preview', 'browser.js'), '/_workbench/preview-requests.js': path.join(workbench, 'preview', 'requests.js') };
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://127.0.0.1');
    let file = runtime[url.pathname];
    const host = /^\/host\/([^/]+)\/([^/]+)\.html$/.exec(url.pathname);
    if (host) {
      const [, variant, slug] = host;
      const output = outputs[variant];
      const entry = output.entry(slug);
      const css = output.css(slug);
      res.setHeader('Content-Type', 'text/html');
      res.end(`<!doctype html><html><head><meta charset="utf-8">${css ? `<link rel="stylesheet" href="/out/${variant}/${css}">` : ''}
<script>window.__workbenchOptions = { development: false };</script>
<script ${output.classic ? '' : 'type="module" '}src="/out/${variant}/${entry}"></script></head><body></body></html>`);
      return;
    }
    const out = /^\/out\/([^/]+)\/(.+)$/.exec(url.pathname);
    if (out && outputs[out[1]]) {
      const root = outputs[out[1]].dir;
      file = path.join(root, decodeURIComponent(out[2]));
      if (!file.startsWith(root + path.sep)) file = null;
    }
    if (!file || !fs.existsSync(file) || !fs.statSync(file).isFile()) { res.statusCode = 404; res.end('Not found'); return; }
    res.setHeader('Content-Type', MIME[path.extname(file)] || 'application/octet-stream');
    res.setHeader('Cache-Control', 'no-store');
    fs.createReadStream(file).pipe(res);
  });
  return new Promise(resolve => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// Asset hashes follow a hyphen (esbuild, Rolldown, Bun, Rspack) or a dot
// (Utoo); both are generated names, not rendering differences.
const normalize = html => html
  .replace(/(src|href)="[^"]*?([^/".]+?)(?:[-.][A-Za-z0-9_-]{6,})?\.(svg|png|woff2?|jpg)"/g, '$1="ASSET:$2.$3"')
  .replace(/\s+/g, ' ');

/**
 * outputs: { [variant]: { dir, entry(slug) -> relative path, css(slug) -> relative path | null } }
 * cases: [{ slug, states: [...] }]
 */
export async function renderAll({ workbench, outputs, cases, timeoutMs = 20000 }) {
  const { Browser } = createRequire(import.meta.url)(path.join(workbench, 'scripts', 'chrome.cjs'));
  const server = await serve(workbench, outputs);
  const browser = new Browser({});
  const results = [];
  try {
    await browser.launch();
    const base = 'http://127.0.0.1:' + server.address().port;
    for (const [variant] of Object.entries(outputs)) {
      for (const item of cases) {
        for (const state of item.states) {
          const target = await browser.createTarget('about:blank');
          try {
            await target.navigate(`${base}/host/${variant}/${item.slug}.html?state=${encodeURIComponent(state)}`);
            const started = Date.now();
            let previous = null, stable = 0, snapshot;
            // Ready, then unchanged across consecutive reads: data, locale and
            // effects settle after mount.
            while (Date.now() - started < timeoutMs) {
              snapshot = await target.evaluate(`(() => ({ ready: window.__workbenchReady === true, error: window.__workbenchError || null,
                html: document.getElementById('workbench-preview')?.innerHTML ?? null,
                seen: document.documentElement.dataset.acmeSeenPreviews || null }))()`);
              if (snapshot.error) break;
              if (snapshot.ready && snapshot.html === previous) { if (++stable >= 2) break; } else stable = 0;
              previous = snapshot.html;
              await new Promise(resolve => setTimeout(resolve, 250));
            }
            results.push({ variant, slug: item.slug, state, ready: !!snapshot?.ready, error: snapshot?.error,
              html: snapshot?.html == null ? null : normalize(snapshot.html), timedOut: !snapshot?.error && stable < 2 });
          } finally { await target.close().catch(() => {}); }
        }
      }
    }
  } finally {
    await browser.close().catch(() => {});
    await new Promise(resolve => server.close(resolve));
  }
  return results;
}

export function compare(results, control = 'shipping') {
  const reference = new Map(results.filter(item => item.variant === control).map(item => [item.slug + '|' + item.state, item]));
  return results.filter(item => item.variant !== control).map(item => {
    const expected = reference.get(item.slug + '|' + item.state);
    const ok = !!expected?.html && item.ready && !item.error && item.html === expected.html;
    let firstDifference = null;
    if (!ok && expected?.html && item.html) {
      let index = 0;
      while (index < item.html.length && item.html[index] === expected.html[index]) index++;
      firstDifference = { at: index, expected: expected.html.slice(Math.max(0, index - 80), index + 120), actual: item.html.slice(Math.max(0, index - 80), index + 120) };
    }
    return { variant: item.variant, slug: item.slug, state: item.state, ok, error: item.error, ready: item.ready, timedOut: item.timedOut, firstDifference };
  });
}
