// Observe page/context precedence through the unchanged pinned SDK and real wire.
const playwright = require(process.argv[2]);
(async () => {
  const browser = await playwright[process.argv[3]].launch({headless:process.argv[4] === 'headless'});
  try {
    const context = await browser.newContext({extraHTTPHeaders:{'X-Acme':'context'}});
    const page = await context.newPage();
    await page.goto(`${process.argv[5]}/header-page`);
    const observed = async tag => page.evaluate(async tag => {
      const response = await fetch(`/echo?baseline=${tag}`);
      if (response.status !== 200) throw new Error('Header baseline fixture refused');
      const record = await response.json();
      return record.headers['x-acme'] ?? null;
    }, tag);
    const report = {initial:await observed('initial')};
    await page.setExtraHTTPHeaders({'x-aCmE':'page'});
    report.page = await observed('page');
    await context.setExtraHTTPHeaders({'X-Acme':'changed'});
    report.contextChanged = await observed('context-changed');
    await page.setExtraHTTPHeaders({'X-Acme':''});
    report.empty = await observed('empty');
    await page.setExtraHTTPHeaders({});
    report.reset = await observed('reset');
    process.stdout.write(JSON.stringify(report));
  } finally {
    await browser.close();
  }
})().catch(error => {console.error(error);process.exitCode=1;});
