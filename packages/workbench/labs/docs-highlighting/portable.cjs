// Portable-build correctness, kept outside all latency/memory cells.
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const http = require('node:http');
const { promisify } = require('node:util');
const { Browser } = require('../../scripts/chrome.cjs');
const execFile = promisify(cp.execFile);
async function portable(product, fixture, dir) {
  const output = path.join(dir, 'portable');
  fs.mkdirSync(output, { recursive: true });
  const binary = path.join(
    product,
    'server/bin',
    process.platform + '-' + process.arch,
    'workbench-server',
  );
  const built = await execFile(
    binary,
    ['build', fixture, output, '--node', process.execPath, '--product', product],
    {
      env: { ...process.env, TMPDIR: path.join(dir, 'tmp') },
      timeout: 120000,
      maxBuffer: 8 * 1024 * 1024,
    },
  );
  const files = [];
  const walk = (folder) => {
    for (const ent of fs.readdirSync(folder, { withFileTypes: true })) {
      const file = path.join(folder, ent.name);
      ent.isDirectory() ? walk(file) : files.push(file);
    }
  };
  walk(output);
  const pages = files.filter(
    (file) =>
      file.endsWith('.html') && fs.readFileSync(file, 'utf8').includes('window.__workbenchDocs'),
  );
  if (!pages.length) throw Error('Portable build produced no docs pages');
  const server = http.createServer((req, res) => {
    const pathname = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
    const file = path.resolve(output, '.' + pathname);
    if (!file.startsWith(output + path.sep)) {
      res.writeHead(403).end();
      return;
    }
    const extension = path.extname(file);
    const mime =
      {
        '.html': 'text/html',
        '.js': 'text/javascript',
        '.css': 'text/css',
        '.json': 'application/json',
        '.svg': 'image/svg+xml',
        '.woff2': 'font/woff2',
      }[extension] || 'application/octet-stream';
    try {
      res.setHeader('Content-Type', mime);
      res.end(fs.readFileSync(file));
    } catch {
      res.writeHead(404).end('Not found');
    }
  });
  let browser, target;
  try {
    await new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', resolve);
    });
    browser = new Browser({ profile: path.join(dir, 'portable-profile') });
    await browser.launch();
    target = await browser.createTarget();
    const errors = [],
      external = [];
    target.on('Runtime.exceptionThrown', (e) =>
      errors.push(e.exceptionDetails.exception?.description || e.exceptionDetails.text),
    );
    await target.send('Network.enable');
    target.on('Network.requestWillBeSent', (e) => {
      if (/^https?:/.test(e.request.url) && new URL(e.request.url).hostname !== '127.0.0.1')
        external.push(e.request.url);
    });
    const checked = [];
    for (const page of pages) {
      await target.navigate(
        `http://127.0.0.1:${server.address().port}/` +
          path.relative(output, page).split(path.sep).join('/'),
      );
      checked.push(
        await target.evaluate(
          `(async()=>{const end=performance.now()+10000;while(!window.__workbenchReady){if(window.__workbenchError)throw Error(window.__workbenchError);if(performance.now()>end)throw Error('Portable readiness timeout');await new Promise(r=>setTimeout(r,10));}const before=[...document.querySelectorAll('pre code')].map(n=>({text:n.textContent,coloured:!!n.children.length}));document.querySelector('[data-wb-show-code]')?.click();while(document.querySelector('[data-wb-example-code]')&&!document.querySelector('[data-wb-example-code] code')){if(performance.now()>end)throw Error('Portable source timeout');await new Promise(r=>setTimeout(r,10));}const source=document.querySelector('[data-wb-example-code] code');return {page:location.pathname,liveHighlightUrl:!!window.__workbenchDocs.highlightUrl,before,source:source&&{text:source.textContent,coloured:!!source.children.length},fonts:document.fonts.status};})()`,
        ),
      );
    }
    if (errors.length || external.length) throw Error(JSON.stringify({ errors, external }));
    if (checked.some((page) => page.liveHighlightUrl || page.fonts !== 'loaded'))
      throw Error('Portable page depends on live highlighting or unfinished fonts');
    return {
      built: true,
      pages: checked,
      files: files.length,
      warnings: built.stderr,
      errors,
      external,
    };
  } finally {
    if (target) await target.close();
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
}
if (require.main === module)
  portable(...process.argv.slice(2).map((f) => path.resolve(f))).then(
    (value) => console.log(JSON.stringify(value, null, 2)),
    (error) => {
      console.error(error);
      process.exitCode = 1;
    },
  );
module.exports = { portable };
