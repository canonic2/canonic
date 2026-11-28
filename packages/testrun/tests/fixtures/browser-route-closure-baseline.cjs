// Pinned SDK observations for interception closure: abort codes, JSON/file
// fulfill, unrouteAll without waiting, worker routing, worker HAR capture and
// worker loss while a route is paused. Usage:
// node this.cjs <playwright-core> <engine> <mode> <browsers> <workspace>
const fs = require('node:fs'), http = require('node:http'), path = require('node:path');
const playwright = require(process.argv[2]);
const [engine, mode, browsers, workspace] = process.argv.slice(3);
const worker = `self.onmessage=async e=>{try{const r=await fetch(e.data);postMessage({status:r.status,type:r.headers.get('content-type'),acme:r.headers.get('x-acme'),text:await r.text()})}catch(error){postMessage({error:String(error.message||error)})}}`;
const shared = `onconnect=e=>{const port=e.ports[0];port.onmessage=async m=>{try{const r=await fetch(m.data);port.postMessage({status:r.status,text:await r.text()})}catch(error){port.postMessage({error:String(error.message||error)})}}}`;
const server = http.createServer((request, response) => {
  const url = new URL(request.url, 'http://localhost');
  if (url.pathname === '/page') { response.writeHead(200, {'content-type': 'text/html'}); return response.end('<!doctype html><title>Acme route closure</title>'); }
  if (url.pathname === '/worker.js') { response.writeHead(200, {'content-type': 'text/javascript'}); return response.end(worker); }
  if (url.pathname === '/shared.js') { response.writeHead(200, {'content-type': 'text/javascript'}); return response.end(shared); }
  response.writeHead(200, {'content-type': 'text/plain'}); response.end('network ' + url.pathname);
});
const phase = async (name, action) => {
  process.stderr.write(JSON.stringify({phase: name, at: Date.now()}) + '\n');
  try { return await action(); } catch (error) { return {error: String(error.message || error).split('\n')[0]}; }
};
const fetchIn = (page, url) => page.evaluate(async url => {
  try { const r = await fetch(url); return {status: r.status, type: r.headers.get('content-type'), acme: r.headers.get('x-acme'), text: await r.text()}; }
  catch (error) { return {error: String(error.message || error)}; }
}, url);
const viaWorker = (page, url) => page.evaluate(url => new Promise(resolve => {
  const w = new Worker('/worker.js'); w.onmessage = e => { resolve(e.data); w.terminate(); }; w.postMessage(url);
}), url);
(async () => {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const chrome = engine === 'chrome';
  const browser = await playwright[chrome ? 'chromium' : engine].launch({headless: mode === 'headless',
    ...(chrome ? {executablePath: path.join(browsers, 'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')} : {})});
  const report = {};
  try {
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.goto(base + '/page');
    report.abort = {};
    await page.route('**/abort/*', route => route.abort(route.request().url().split('/').pop()));
    for (const code of ['failed', 'aborted', 'namenotresolved']) {
      report.abort[code] = await phase('abort.' + code, async () => {
        const failed = page.waitForEvent('requestfailed');
        await fetchIn(page, base + '/abort/' + code);
        return (await failed).failure().errorText;
      });
    }
    await page.route('**/json', route => route.fulfill({json: {acme: [1, true]}, headers: {'X-Acme': 'one'}}));
    report.json = await phase('json', () => fetchIn(page, base + '/json'));
    const file = path.join(workspace, 'route-closure.html');
    fs.writeFileSync(file, '<b>Acme file</b>');
    await page.route('**/file', route => route.fulfill({path: file}));
    report.file = await phase('file', () => fetchIn(page, base + '/file'));
    report.unrouteDefault = await phase('unroute.default', async () => {
      let held, arrived;
      const reached = new Promise(resolve => (arrived = resolve));
      await page.route('**/held', route => { held = route; arrived(); });
      const pending = fetchIn(page, base + '/held');
      await reached;
      await page.unrouteAll({behavior: 'default'});
      const after = await fetchIn(page, base + '/net/after');
      await held.fallback();
      return {after, held: await pending};
    });
    report.unrouteDefaultFulfill = await phase('unroute.default.fulfill', async () => {
      let held, arrived;
      const reached = new Promise(resolve => (arrived = resolve));
      await page.route('**/held-fulfill', route => { held = route; arrived(); });
      const pending = fetchIn(page, base + '/held-fulfill');
      await reached;
      await page.unrouteAll({behavior: 'default'});
      let fulfilled = 'ok';
      try { await held.fulfill({body: 'late reply'}); }
      catch (error) { fulfilled = String(error.message || error).split('\n')[0]; }
      return {fulfilled, held: await pending};
    });
    await page.unrouteAll({behavior: 'wait'});
    await page.route('**/worker-data', route => route.fulfill({body: 'routed dedicated'}));
    report.dedicatedWorker = await phase('worker.dedicated', () => viaWorker(page, base + '/worker-data'));
    await page.unrouteAll({behavior: 'wait'});
    await context.route('**/shared-data', route => route.fulfill({body: 'routed shared'}));
    report.sharedWorker = await phase('worker.shared', () => page.evaluate(url => new Promise(resolve => {
      if (typeof SharedWorker !== 'function') return resolve({error: 'SharedWorker unavailable'});
      const w = new SharedWorker('/shared.js'); w.port.onmessage = e => resolve(e.data); w.port.postMessage(url);
      setTimeout(() => resolve({error: 'timeout'}), 5000);
    }), base + '/shared-data'));
    await context.unrouteAll({behavior: 'wait'});
    report.workerLoss = await phase('worker.loss', async () => {
      let held, arrived;
      const reached = new Promise(resolve => (arrived = resolve));
      await page.route('**/worker-held', route => { held = route; arrived(); });
      await page.evaluate(url => { globalThis.acmeWorker = new Worker('/worker.js'); acmeWorker.postMessage(url); }, base + '/worker-held');
      await Promise.race([reached, new Promise(resolve => setTimeout(resolve, 5000))]);
      if (!held) return {routed: false};
      await page.evaluate(() => acmeWorker.terminate());
      try { await held.continue(); return {routed: true, continued: 'ok'}; }
      catch (error) { return {routed: true, continued: String(error.message || error).split('\n')[0]}; }
    });
    await page.unrouteAll({behavior: 'ignoreErrors'});
    report.removalWhilePaused = await phase('worker.removal', async () => {
      let held, arrived;
      const reached = new Promise(resolve => (arrived = resolve));
      const handler = route => { held = route; arrived(); };
      await page.route('**/worker-late', handler);
      const pending = viaWorker(page, base + '/worker-late');
      await Promise.race([reached, new Promise(resolve => setTimeout(resolve, 5000))]);
      if (!held) return {routed: false, result: await pending};
      await page.unroute('**/worker-late', handler);
      let fulfilled = 'ok';
      try { await held.fulfill({body: 'late reply'}); }
      catch (error) { fulfilled = String(error.message || error).split('\n')[0]; }
      return {routed: true, fulfilled, result: await pending};
    });
    await context.close();
    const harPath = path.join(workspace, 'route-closure-worker.har');
    const recorded = await browser.newContext({recordHar: {path: harPath, content: 'embed'}});
    const recordedPage = await recorded.newPage();
    await recordedPage.goto(base + '/page');
    report.workerHarFetch = await phase('worker.har', () => viaWorker(recordedPage, base + '/net/worker-har'));
    await recorded.close();
    report.workerHarUrls = JSON.parse(fs.readFileSync(harPath, 'utf8')).log.entries
      .map(entry => new URL(entry.request.url).pathname).filter(name => name !== '/favicon.ico').sort();
  } finally {
    await browser.close();
    server.close();
  }
  process.stdout.write(JSON.stringify(report));
  process.exit();
})().catch(error => { console.error(error); process.exit(1); });
