// Run the unchanged pinned public SDK to distinguish engine limits from adapters.
const browserType = require(process.argv[2])[process.argv[3]];
(async () => {
  const browser = await browserType.launch({headless:true});
  const context = await browser.newContext();
  const page = await context.newPage();
  let handle;
  try {
    handle = await page.evaluateHandle(() => 123456789012345678901234567890n);
    const value = await handle.evaluate(value => String(value));
    process.stdout.write(JSON.stringify({ok:true,value}));
  } catch (error) {
    process.stdout.write(JSON.stringify({ok:false,name:error.name,message:error.message}));
  } finally {
    if (handle) await handle.dispose();
    await context.close();
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
