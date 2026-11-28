// Pinned SDK reference for HTTP Request copies after routing overrides.
const assert=require('node:assert/strict'),http=require('node:http'),path=require('node:path');
for(const key of Object.keys(process.env))delete process.env[key];
process.env.PATH='/usr/bin:/bin';process.env.PW_LANG_NAME='javascript';process.env.PLAYWRIGHT_BROWSERS_PATH=process.argv[5];
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const records=[];
const server=http.createServer(async(request,response)=>{
  const chunks=[];for await(const chunk of request)chunks.push(chunk);
  const bytes=Buffer.concat(chunks);assert(bytes.length<=65536);
  const value={url:request.url,method:request.method,headers:request.headers,bodyBase64:bytes.toString('base64')};records.push(value);
  const fixture=request.url==='/fixture';const body=fixture?'<!doctype html><title>Acme Request overrides</title>Acme':JSON.stringify(value);
  response.writeHead(200,{'Content-Type':fixture?'text/html':'application/json','Connection':'close'});response.end(body);
});
(async()=>{
  const report={cases:{}};let browser,client,failure;
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const base='http://127.0.0.1:'+server.address().port;
  try {
    const chrome=process.argv[3]==='chrome';
    browser=await playwright[chrome?'chromium':process.argv[3]].launch({headless:process.argv[4]==='headless',...(chrome?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    client=await playwright.request.newContext();
    for(const kind of ['fallback','continue','empty','layered']) {
      const context=await browser.newContext(),page=await context.newPage();await page.goto(base+'/fixture');
      let retained,initial,intermediate;
      if(kind==='layered')await page.route('**/*',async route=>{
        const request=route.request();
        if(request.url()!==base+'/changed-layered'){await route.fallback();return;}
        intermediate={url:request.url(),method:request.method(),headers:request.headers(),bodyBase64:request.postDataBuffer()?.toString('base64')??null};
        await route.fallback({headers:JSON.parse('{"X-Acme":"first","x-acme":"last","Cookie":"AcmeLayer=private","__proto__":"Acme literal"}')});
      });
      await page.route('**/source-*',async route=>{
        retained=route.request();initial={url:retained.url(),method:retained.method(),bodyBase64:retained.postDataBuffer()?.toString('base64')??null};
        const options=kind==='empty'?{postData:Buffer.alloc(0),headers:{}}:{url:base+'/changed-'+kind,method:'PUT',headers:{'X-Acme':'routed','Cookie':'AcmeFallback=private'},postData:Buffer.from([0,255,128])};
        if(kind==='continue')await route.continue(options);else await route.fallback(options);
      });
      const browserValue=await page.evaluate(async url=>{const response=await fetch(url,{method:'POST',headers:{'X-Acme':'source'},body:new Uint8Array([65,99,109,101,0,255])});if(response.status!==200)throw Error('Acme source status');return response.json();},base+'/source-'+kind);
      assert(retained);const source={url:retained.url(),method:retained.method(),headers:retained.headers(),bodyBase64:retained.postDataBuffer()?.toString('base64')??null};
      await context.close();
      const response=await client.fetch(retained);assert.equal(response.status(),200);const copy=await response.json();await response.dispose();
      assert.equal(copy.method,source.method);assert.equal(copy.bodyBase64,source.bodyBase64??'');
      report.cases[kind]={initial,intermediate,source,browserValue,copy};
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally {
    try{await client?.dispose();await browser?.close();}finally{await utils.gracefullyCloseAll();}
    server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
    report.ownedProcesses=utils.gracefullyCloseSet.size;report.records=records;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
