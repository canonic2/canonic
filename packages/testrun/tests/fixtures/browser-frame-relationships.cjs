// An unchanged pinned public SDK supplies independent element relationship evidence.
const assert = require('node:assert/strict');
const browserType = require(process.argv[2])[process.argv[3]];
(async () => {
  const launch = {headless: process.argv[4] === 'headless'};
  if (process.argv[3] === 'chromium') {
    if (!process.argv[5]) throw new Error('Select the prepared owned Chrome executable');
    launch.executablePath = process.argv[5];
  }
  const browser = await browserType.launch(launch);
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.setContent('<!doctype html><button id="adopt">Acme</button><iframe id="child" srcdoc="<body>Acme child</body>"></iframe>');
    const main = page.mainFrame();
    const button = await page.$('#adopt');
    const iframe = await page.$('#child');
    const document = (await page.evaluateHandle(() => window.document)).asElement();
    const child = await iframe.contentFrame();
    assert.ok(child);
    assert.equal(await button.ownerFrame(), main);
    assert.equal(await button.contentFrame(), null);
    assert.equal(await document.ownerFrame(), main);
    assert.equal(await document.contentFrame(), null);
    assert.equal(await iframe.ownerFrame(), main);
    assert.equal(child.parentFrame(), main);
    const inner = await child.$('body');
    assert.equal(await inner.ownerFrame(), child);
    await button.evaluate(node => node.remove());
    assert.equal(await button.ownerFrame(), main);
    await button.evaluate(node => window.document.body.append(node));
    const popupPromise = page.waitForEvent('popup');
    await page.evaluate(() => { globalThis.acmePopup = window.open('about:blank'); });
    const popup = await popupPromise;
    await page.evaluate(() => acmePopup.document.body.append(acmePopup.document.adoptNode(document.querySelector('#adopt'))));
    assert.equal(await button.ownerFrame(), popup.mainFrame());
    assert.equal(await button.contentFrame(), null);
    assert.equal(await popup.locator('#adopt').count(), 1);
    for (const handle of [button, iframe, document, inner]) await handle.dispose();
    await popup.close();
    await page.close();
    process.stdout.write(JSON.stringify({owner:true,content:true,document:true,detached:true,crossPageAdoption:true}));
  } finally {
    await context.close();
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
