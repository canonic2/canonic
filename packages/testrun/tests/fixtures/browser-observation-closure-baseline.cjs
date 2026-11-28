// Unchanged SDK Request/Response observation for the 4.5 closure against the
// caller's server: a redirect chain, raw headers, HTTP version, status text,
// sizes and frame ownership, plus Page.requestGC().
// Arguments: playwright-core, engine, mode, browsers, origin.
const path=require('node:path');
const playwright=require(process.argv[2]);
(async()=>{
  const engine=process.argv[3],headless=process.argv[4]==='headless',origin=process.argv[6];
  const report={};let browser;
  try {
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    const context=await browser.newContext();const page=await context.newPage();
    await page.goto(origin+'/page');
    const finished=page.waitForEvent('requestfinished',request=>request.url().endsWith('/final'));
    await page.evaluate(()=>fetch('/redirect',{headers:{'X-Acme':'request'}}).then(response=>response.text()));
    const last=await finished;
    const response=await last.response();
    const first=last.redirectedFrom();
    const firstResponse=await first.response();
    const pick=(headers,names)=>headers.filter(header=>names.includes(header.name.toLowerCase())).map(header=>({name:header.name.toLowerCase(),value:header.value}));
    const sizes=await last.sizes();
    report.last={method:last.method(),resourceType:last.resourceType(),navigation:last.isNavigationRequest(),mainFrame:last.frame()===page.mainFrame(),
      redirectedTo:last.redirectedTo()===null,status:response.status(),statusText:response.statusText(),httpVersion:await response.httpVersion(),
      fromServiceWorker:response.fromServiceWorker(),requestHeaders:pick(await last.headersArray(),['x-acme']),
      responseHeaders:pick(await response.headersArray(),['x-acme-response','content-length']),
      sizes:{responseBodySize:sizes.responseBodySize,responseHeadersSize:sizes.responseHeadersSize,requestBodySize:sizes.requestBodySize}};
    report.first={redirectedTo:first.redirectedTo()===last,status:firstResponse.status(),statusText:firstResponse.statusText(),
      responseHeaders:pick(await firstResponse.headersArray(),['location'])};
    await page.evaluate(()=>{globalThis.acmeWeak=new WeakRef({acme:new Array(1000).fill('Acme')});});
    await page.requestGC();
    report.collected=await page.evaluate(()=>globalThis.acmeWeak.deref()===undefined);
    await context.close();
  } finally { if(browser)await browser.close(); }
  process.stdout.write(JSON.stringify(report),()=>process.exit(0));
})().catch(error=>{process.stderr.write(String(error.stack||error));process.exit(1);});
