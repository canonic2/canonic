// Independent unchanged pinned SDK evidence for embedding-element ownership.
const assert = require('node:assert/strict');
const browserType = require(process.argv[2])[process.argv[3]];
(async () => {
  const options = {headless: process.argv[4] === 'headless'};
  if (process.argv[3] === 'chromium') options.executablePath = process.argv[5];
  const browser = await browserType.launch(options);
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.setContent('<!doctype html><iframe id="child" srcdoc="<body>Acme child</body>"></iframe>');
    const main = page.mainFrame();
    const child = page.frames().find(frame => frame.parentFrame() === main);
    const element = await child.frameElement();
    assert.equal(await element.ownerFrame(), main);
    assert.equal(await element.contentFrame(), child);
    assert.equal(await element.evaluate(node => node.id), 'child');
    await assert.rejects(main.frameElement());
    await child.setContent('<body>Acme replacement</body>');
    assert.equal(await element.evaluate(node => node.id), 'child');
    const second = await child.frameElement();
    assert.equal(await second.evaluate(node => node.id), 'child');
    await element.evaluate(node => node.remove());
    assert.equal(child.isDetached(), true);
    assert.equal(await second.evaluate(node => node.isConnected), false);
    assert.equal(await second.ownerFrame(), main);
    await assert.rejects(child.frameElement());
    await page.setContent('<body>Acme new parent document</body>');
    // setContent can preserve an SDK execution realm. A real navigation must
    // invalidate the old realm's ElementHandle independently of DOM detachment.
    await page.goto('data:text/html,<body>Acme parent navigation</body>');
    await assert.rejects(second.evaluate(node => node.id));
    await element.dispose();
    await second.dispose();
    process.stdout.write(JSON.stringify({embedding:true,parentRealm:true,childReplacement:true,detachedElement:true,parentNavigation:true}));
  } finally {
    await context.close();
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
