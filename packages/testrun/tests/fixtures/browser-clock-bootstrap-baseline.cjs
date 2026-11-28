// Pinned SDK witness for timers scheduled by a new document's first script.
const http = require('node:http');
const path = require('node:path');
const [core, engine, mode, browsers] = process.argv.slice(2);
process.env.PLAYWRIGHT_BROWSERS_PATH = browsers;
const playwright = require(core);
const epoch = 1700000000000;
const html = '<!doctype html><script>globalThis.first={origin:performance.timeOrigin,date:Date.now()};globalThis.ticks=0;setTimeout(()=>ticks++,0)</script><body>Acme clock';
(async () => {
  const server = http.createServer((request, response) => response.end(html));
  let browser;
  try {
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    browser = await playwright[engine === 'chrome' ? 'chromium' : engine].launch({headless:mode === 'headless',
      ...(engine === 'chrome' ? {executablePath:path.join(browsers,'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')} : {})});
    const context = await browser.newContext();
    await context.clock.install({time:epoch});
    await context.clock.setFixedTime(epoch);
    await context.clock.pauseAt(epoch);
    const page = await context.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await new Promise(resolve => setTimeout(resolve, 200));
    process.stdout.write(JSON.stringify(await page.evaluate(() => ({first,ticks,date:Date.now()}))));
  } finally {
    if (browser) await browser.close();
    await new Promise(resolve => server.close(resolve));
  }
})().catch(error => {process.stderr.write(String(error.stack || error)); process.exitCode=1;});
