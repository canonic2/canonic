// Observe debugger/actionability order through the unchanged pinned SDK.
const playwright = require(process.argv[2]);
const pinnedUtils = require(process.argv[2]+'/lib/coreBundle.js').utils;
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function tracked(promise) { promise.catch(()=>{}); return promise; }
async function paused(debugger_) {
  const until = Date.now() + 5000;
  while (!debugger_.pausedDetails()) {
    if (Date.now() >= until) throw new Error('Acme debugger pause unavailable');
    await delay(10);
  }
}
(async () => {
  const browser = await playwright[process.argv[3]].launch({headless:process.argv[4] === 'headless'});
  const report = {};
  try {
    // Without call collection, requestPause/next stop before auto-waiting.
    for (const next of [false,true]) {
      const context = await browser.newContext();
      const page = await context.newPage();
      await page.setContent('<button id=first>Acme</button><script>globalThis.acmeClicks=0</script>');
      const debugger_ = context.debugger;
      await debugger_.requestPause();
      let first;
      if (next) {
        first = tracked(page.locator('#first').click());
        await paused(debugger_);
        await debugger_.next();
        await first;
      }
      const click = tracked(page.locator('#missing').click());
      await paused(debugger_);
      report[next?'next':'pause'] = {pausedBeforeAttachment:true};
      await page.evaluate(() => {
        const button=document.createElement('button');button.id='missing';button.hidden=true;
        button.onclick=()=>globalThis.acmeClicks++;document.body.append(button);
      });
      await debugger_.resume();
      await delay(50);
      report[next?'next':'pause'].hiddenClicks=await page.evaluate(()=>globalThis.acmeClicks);
      await page.evaluate(()=>{document.querySelector('#missing').hidden=false;});
      await click;
      report[next?'next':'pause'].clicks=await page.evaluate(()=>globalThis.acmeClicks);
      await context.close();
    }
    // Matching source locations stop before auto-waiting even with collection.
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.setContent('<button id=first>Acme</button><script>globalThis.acmeClicks=0</script>');
    const debugger_ = context.debugger;
    await debugger_._enable();
    await debugger_.requestPause();
    const first = tracked(page.locator('#first').click());
    await paused(debugger_);
    await debugger_.runTo({file:__filename});
    await first;
    const click = tracked(page.locator('#missing').click());
    await paused(debugger_);
    report.runTo = {pausedBeforeAttachment:true};
    await page.evaluate(() => {
      const button=document.createElement('button');button.id='missing';button.hidden=true;
      button.onclick=()=>globalThis.acmeClicks++;document.body.append(button);
    });
    await debugger_.resume();
    await delay(50);
    report.runTo.hiddenClicks=await page.evaluate(()=>globalThis.acmeClicks);
    await page.evaluate(()=>{document.querySelector('#missing').hidden=false;});
    await click;
    report.runTo.clicks=await page.evaluate(()=>globalThis.acmeClicks);
    await context.close();
    process.stdout.write(JSON.stringify(report));
  } finally {
    try { await browser.close(); }
    finally {
      // Headed pauses open owned Inspector apps; close those through the
      // unchanged SDK's graceful process owner as well as the test browser.
      await pinnedUtils.gracefullyCloseAll();
      if (pinnedUtils.gracefullyCloseSet.size) throw new Error('Owned SDK browser cleanup remains pending');
    }
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
