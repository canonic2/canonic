// Browser launch defaults and explicit context/client precedence in the pinned SDK.
const path = require('node:path');
const {origin, proxy, basic} = require('./browser-proxy-wire.cjs');
const playwright = require(process.argv[2]);
const utils = require(process.argv[2] + '/lib/coreBundle.js').utils;
(async () => {
  const site = await origin();
  const global = await proxy(site.port, basic('Acme global','global example'));
  const override = await proxy(site.port, basic('Acme context','context example'));
  let browser;
  const cases = [];
  try {
    const chrome = process.argv[3] === 'chrome';
    const launch = bypass => playwright[chrome ? 'chromium' : process.argv[3]].launch({
      headless: process.argv[4] === 'headless',
      proxy:{server:global.url.replace('http://','http://AcmeIgnored:example@') + '/ignored?Acme=1',username:'Acme global',password:'global example',...(bypass ? {bypass} : {})},
      ...(chrome ? {executablePath:path.join(process.argv[5], 'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')} : {}),
    });
    browser = await launch();
    for (const name of ['initial','override','bypass','later','global_bypass']) {
      if (name === 'global_bypass') {await browser.close();browser = await launch(' 127.0.0.1 , localhost ');}
      const before = {origin:site.requests.length,global:global.requests.length,override:override.requests.length};
      const options = {httpCredentials:{username:'Acme origin',password:'origin example'},extraHTTPHeaders:{'X-Acme':'context'}};
      if (name === 'override' || name === 'bypass') options.proxy = {
        server:override.url,username:'Acme context',password:'context example',
        ...(name === 'bypass' ? {bypass:' 127.0.0.1 , localhost '} : {}),
      };
      let context;
      let explicit;
      let phase = 'context_create';
      const row = {name};
      try {
        context = await browser.newContext(options);
        phase = 'page_navigate';
        const page = await context.newPage();
        row.pageStatus = (await page.goto(site.url + '/page')).status();
        phase = 'inherited_http';
        const response = await context.request.get(site.url + '/auth');
        row.fetchStatus = response.status();await response.dispose();
        phase = 'explicit_http_override';
        const inherited = context._initializer.options;
        explicit = await playwright.request.newContext({
          proxy:name === 'initial' || name === 'later' || name === 'global_bypass'
            ? {server:override.url,username:'Acme context',password:'context example'}
            : {server:global.url,username:'Acme global',password:'global example'},
          httpCredentials:inherited.httpCredentials,
          extraHTTPHeaders:Object.fromEntries((inherited.extraHTTPHeaders || []).map(header => [header.name,header.value])),
        });
        const changed = await explicit.get(site.url + '/auth');
        row.overrideStatus = changed.status();await changed.dispose();
        await explicit.dispose();explicit = undefined;
        const closed = context;
        await context.close();context = undefined;
        phase = 'closed_source';
        try {
          const unexpected = await closed.request.get(site.url + '/auth');
          await unexpected.dispose();row.closedRejected = false;
        } catch {row.closedRejected = true;}
      } catch (error) {row.error = {phase,message:error.message};}
      finally {
        if (explicit) await explicit.dispose();
        if (context) await context.close();
        row.origin = site.requests.slice(before.origin);
        row.globalProxy = global.requests.slice(before.global);
        row.contextProxy = override.requests.slice(before.override);
      }
      cases.push(row);
    }
  } finally {
    try {if (browser) await browser.close();}
    finally {
      await utils.gracefullyCloseAll();
      await override.close();await global.close();await site.close();
    }
  }
  const ownedProcesses = utils.gracefullyCloseSet.size;
  if (ownedProcesses) throw new Error('Owned SDK browser cleanup remains pending');
  process.stdout.write(JSON.stringify({cases,ownedProcesses}));
})().catch(error => {console.error(error);process.exitCode = 1;});
