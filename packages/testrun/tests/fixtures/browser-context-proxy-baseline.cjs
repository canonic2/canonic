// Observe context proxy/default inheritance through the unchanged pinned SDK.
const {origin, proxy, basic} = require('./browser-proxy-wire.cjs');
const path = require('node:path');
const playwright = require(process.argv[2]);
const utils = require(process.argv[2] + '/lib/coreBundle.js').utils;
// A rejected WebKit credential initializer leaves a partial dispatcher whose
// disposal throws. Preserve that SDK failure while completing owned cleanup.
process.once('uncaughtException', error => {
  process.stderr.write(error.stack + '\n');
  utils.gracefullyCloseAll().then(() => {
    process.stderr.write('Acme SDK fatal cleanup ' + JSON.stringify({ownedProcesses:utils.gracefullyCloseSet.size}) + '\n');
    process.exitCode = 1;
  }, cleanup => {
    process.stderr.write('Acme SDK fatal cleanup failure ' + cleanup.stack + '\n');
    process.exitCode = 1;
  });
});
(async () => {
  const chrome = process.argv[3] === 'chrome';
  const browser = await playwright[chrome ? 'chromium' : process.argv[3]].launch({
    headless: process.argv[4] === 'headless',
    ...(chrome ? {executablePath:path.join(process.argv[5], 'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')} : {}),
  });
  const report = [];
  try {
    for (const name of ['plain','auth','bypass','normalized','username_only','empty_password']) {
      const site = await origin();
      const endpoint = await proxy(site.port, name === 'auth' ? basic('Acme proxy','proxy example') : undefined);
      const override = await proxy(site.port, basic('Acme override','override example'));
      let context;
      let explicit;
      let phase = 'context_create';
      const row = {name,pageStatus:null,fetchStatus:null,overrideStatus:null,closedRejected:null};
      try {
        const settings = {server:endpoint.url};
        if (['auth','bypass','username_only','empty_password'].includes(name)) {
          settings.username = 'Acme proxy';
          if (name !== 'username_only') settings.password = name === 'empty_password' ? '' : 'proxy example';
        }
        if (name === 'bypass') settings.bypass = ' 127.0.0.1 , localhost ';
        if (name === 'normalized') settings.server = endpoint.url.replace('http://','http://AcmeIgnored:example@') + '/ignored?Acme=1#fragment';
        context = await browser.newContext({
          proxy:settings,
          httpCredentials:{username:'Acme origin',password:'origin example'},
          extraHTTPHeaders:{'X-Acme':'context'},
        });
        phase = 'page_create';
        const page = await context.newPage();
        phase = 'page_navigate';
        row.pageStatus = (await page.goto(site.url + '/page')).status();
        phase = 'associated_fetch';
        const response = await context.request.get(site.url + '/auth');
        row.fetchStatus = response.status();
        await response.dispose();
        phase = 'explicit_proxy_override';
        // These are actual server-backed creation defaults, not caller guesses.
        const inherited = context._initializer.options;
        explicit = await playwright.request.newContext({
          proxy:{server:override.url,username:'Acme override',password:'override example'},
          httpCredentials:inherited.httpCredentials,
          extraHTTPHeaders:Object.fromEntries((inherited.extraHTTPHeaders || []).map(header => [header.name,header.value])),
        });
        const overridden = await explicit.get(site.url + '/auth');
        row.overrideStatus = overridden.status();
        await overridden.dispose();
        await explicit.dispose();explicit = undefined;
        const closed = context;
        await context.close();context = undefined;
        phase = 'closed_source';
        try {
          const unexpected = await closed.request.get(site.url + '/auth');
          await unexpected.dispose();row.closedRejected = false;
        } catch {row.closedRejected = true;}
      } catch (error) {
        row.error = {phase,message:error.message};
      } finally {
        if (explicit) await explicit.dispose();
        if (context) await context.close();
        row.origin = site.requests;
        row.proxy = endpoint.requests;
        row.overrideProxy = override.requests;
        await override.close();
        await endpoint.close();
        await site.close();
      }
      report.push(row);
    }
    process.stdout.write(JSON.stringify(report));
  } finally {
    try {await browser.close();}
    finally {
      await utils.gracefullyCloseAll();
      if (utils.gracefullyCloseSet.size) throw new Error('Owned SDK browser cleanup remains pending');
    }
  }
})().catch(error => {console.error(error);process.exitCode=1;});
