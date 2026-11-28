// node <helper> <pinned-playwright-package> <chrome|firefox|webkit> <headless|headed> <private-browser-cache>
// Independent SDK browser-associated state, using only an owned loopback server.
const fs = require('node:fs');
const path = require('node:path');
if (!process.argv[5] || !path.isAbsolute(process.argv[5])) throw new Error('Select the absolute private browser cache');
process.env.PLAYWRIGHT_BROWSERS_PATH = process.argv[5];
const playwright = require(process.argv[2]);
const http = require('node:http');
const seed = fs.readFileSync(path.join(__dirname, 'browser-associated-state-seed.js'), 'utf8');
(async () => {
  const server = http.createServer((request, response) => {
    response.writeHead(200, {'Content-Type':'text/html', 'Connection':'close'});
    response.end('<!doctype html><title>Acme associated state</title><h1>Acme</h1>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  let childServer;
  try {
    const origin = `http://127.0.0.1:${server.address().port}`;
    const chrome = process.argv[3] === 'chrome';
    browser = await playwright[chrome ? 'chromium' : process.argv[3]].launch({
      headless:process.argv[4] === 'headless',
      ...(chrome ? {executablePath:path.join(process.argv[5], 'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')} : {}),
    });
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.goto(origin);
    const seeded = await page.evaluate('(' + seed + ')(' + (process.argv[3] !== 'webkit') + ')');
    if (seeded !== true) throw new Error('Acme OPFS API availability changed');
    const report = {origin,seeded,states:{}};
    for (const [name, options] of Object.entries({defaults:{},indexedDB:{indexedDB:true},opfs:{opfs:true},all:{indexedDB:true,opfs:true}})) {
      try { report.states[name] = {state:await context.request.storageState(options)}; }
      catch (error) {
        if (process.argv[3] !== 'webkit' || !options.opfs || !/OPFS/i.test(error.message)) throw error;
        report.states[name] = {refusal:{name:error.name,message:error.message}};
      }
    }
    if (process.argv[3] === 'webkit') {
      for (const name of ['opfs','all']) {
        if (!report.states[name].refusal) throw new Error('Acme WebKit OPFS refusal was not observed');
      }
    }
    if (chrome) {
      childServer = http.createServer(server.listeners('request')[0]);
      await new Promise(resolve => childServer.listen(0, '127.0.0.1', resolve));
      report.childOrigin = 'http://127.0.0.1:' + childServer.address().port;
      await page.evaluate(async url => {
        const frame = document.createElement('iframe');
        const loaded = new Promise(resolve => frame.onload = resolve);
        frame.src = url;
        document.body.append(frame);
        await loaded;
      }, report.childOrigin);
      const child = page.frames().find(frame => frame.url().startsWith(report.childOrigin + '/'));
      if (!child || await child.evaluate('(' + seed + ')(true)') !== true) throw new Error('Acme child-frame state was not seeded');
      await page.close();
      let privatePageEvents = 0;
      context.on('page', () => ++privatePageEvents);
      await context.addInitScript(() => localStorage.setItem('Acme-private-collector-leak', 'Acme-unexpected-init'));
      await context.route('**/*', () => { throw new Error('Private collection reached an application route'); });
      report.closedStates = {};
      for (const [name, options] of Object.entries({defaults:{},indexedDB:{indexedDB:true},opfs:{opfs:true},all:{indexedDB:true,opfs:true}})) {
        report.closedStates[name] = {state:await context.request.storageState(options)};
      }
      if (privatePageEvents !== 0 || context.pages().length !== 0) throw new Error('Private collection exposed a page');
    }
    process.stdout.write(JSON.stringify(report));
  } finally {
    try { if (browser) await browser.close(); }
    finally {
      if (childServer) await new Promise(resolve => childServer.close(resolve));
      await new Promise(resolve => server.close(resolve));
    }
  }
})().catch(error => {console.error(error);process.exitCode=1;});
