// Compare caller execution while an unchanged SDK input owns its paused call.
const playwright = require(process.argv[2]);
const utils = require(process.argv[2] + '/lib/coreBundle.js').utils;
const path = require('node:path');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
(async () => {
  const ordinary = process.argv[5] === 'ordinary';
  const chrome = process.argv[3] === 'chrome';
  const browser = await playwright[chrome ? 'chromium' : process.argv[3]].launch({
    headless: process.argv[4] === 'headless',
    ...(chrome ? {executablePath:path.join(process.argv[6], 'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')} : {}),
  });
  try {
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.setContent('<button id=click>Acme</button><script>globalThis.acmeClicks=0;document.querySelector("button").onclick=()=>acmeClicks++;</script><iframe id=child srcdoc="<button>Acme child</button>"></iframe>');
    if (ordinary) await page.evaluate(() => {document.querySelector('#click').hidden = true;});
    const frame = page.frames().find(frame => frame !== page.mainFrame());
    const source = await page.evaluateHandle(() => ({label:'Acme owner'}));
    if (!ordinary) await context.debugger.requestPause();
    let settled = false;
    const click = page.locator('#click').click().finally(() => { settled = true; });
    click.catch(() => {});
    const until = Date.now() + 5000;
    while (!ordinary && !context.debugger.pausedDetails()) {
      if (Date.now() >= until) throw new Error('Acme input pause unavailable');
      await delay(10);
    }
    const pageValue = await page.evaluate(() => { globalThis.acmeSequence = ['page']; return acmeClicks; });
    const frameValue = await frame.evaluate(() => document.querySelector('button').textContent);
    const retained = await source.evaluateHandle(owner => ({label:owner.label+' retained'}));
    const handleValue = await retained.evaluate(owner => owner.label);
    const readValue = (await retained.jsonValue()).label;
    const selectorValue = await page.locator('#click').evaluate(button => button.textContent);
    const first = await page.evaluate(() => { acmeSequence.push('first'); return acmeSequence.length; });
    const second = await page.evaluate(() => { acmeSequence.push('second'); return acmeSequence.join('|'); });
    if (settled || !ordinary && !context.debugger.pausedDetails()) throw new Error('Caller execution released the input');
    await retained.dispose();
    await source.dispose();
    if (ordinary) await page.evaluate(() => {document.querySelector('#click').hidden = false;});
    else await context.debugger.resume();
    await click;
    const clicks = await page.evaluate(() => acmeClicks);
    await context.close();
    process.stdout.write(JSON.stringify({pageValue,frameValue,handleValue,readValue,selectorValue,first,second,clicks,held:true}));
  } finally {
    try { await browser.close(); }
    finally {
      await utils.gracefullyCloseAll();
      if (utils.gracefullyCloseSet.size) throw new Error('Owned SDK browser cleanup remains pending');
    }
  }
})().catch(error => {console.error(error);process.exitCode=1;});
