// Independent pinned SDK semantics; no wrapper or SDK source modifications.
const assert = require('node:assert/strict');
const type = require(process.argv[2])[process.argv[3]];
(async () => {
  const options = {headless: process.argv[4] === 'headless'};
  if (process.argv[3] === 'chromium') options.executablePath = process.argv[5];
  const browser = await type.launch(options);
  try {
    const page = await browser.newPage();
    const parent = await page.evaluateHandle(() => {
      globalThis.acmeReads = 0;
      globalThis.acmeThenReads = 0;
      globalThis.acmePromise = Promise.resolve(42);
      return Object.defineProperties({}, {
        release: {enumerable: true, get() { acmeReads++; acmeRelease(); return acmePromise; }},
        thenable: {get() { return {get then() { acmeThenReads++; throw Error('Acme then'); }}; }}
      });
    });
    const held = page.evaluate(() => new Promise(resolve => { globalThis.acmeRelease = () => resolve('Acme released'); }));
    await page.waitForFunction(() => typeof acmeRelease === 'function');
    const property = await parent.getProperty('release');
    assert.equal(await held, 'Acme released');
    assert.equal(await property.evaluate(value => value === acmePromise), true);
    const thenable = await parent.getProperty('thenable');
    assert.equal(await thenable.evaluate(value => typeof Object.getOwnPropertyDescriptor(value, 'then').get), 'function');
    assert.equal(await page.evaluate(() => acmeReads), 1);
    assert.equal(await page.evaluate(() => acmeThenReads), 0);
    await parent.dispose();
    assert.equal(await property.evaluate(value => value === acmePromise), true);
    await property.dispose();
    await thenable.dispose();
    // Testrun's names page deliberately remains a bounded projection. The SDK
    // getProperties map retains values. Firefox includes an undefined-valued
    // accessor entry; Chromium/WebKit skip it. None invokes this getter.
    const descriptors = await page.evaluateHandle(() => Object.defineProperties({Acme: 1}, {
      accessor: {enumerable: true, get() { throw Error('Acme accessor must not run'); }},
      hidden: {value: 2}
    }));
    const properties = await descriptors.getProperties();
    const accessorEntry = process.argv[3] === 'firefox';
    assert.deepEqual([...properties.keys()], accessorEntry ? ['Acme', 'accessor'] : ['Acme']);
    if (accessorEntry) assert.equal(await properties.get('accessor').jsonValue(), undefined);
    for (const value of properties.values()) await value.dispose();
    await descriptors.dispose();
    process.stdout.write(JSON.stringify({propertyUnblocks:true,promisePreserved:true,thenablePreserved:true,parentDisposal:true,sdkValueMap:true,accessorEntry}));
  } finally { await browser.close(); }
})().catch(error => {console.error(error); process.exitCode = 1;});
