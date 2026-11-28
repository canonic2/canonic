// Exact caller header selection and string/binary serialization in the pinned SDK.
const assert=require('node:assert/strict'),http=require('node:http'),path=require('node:path');
const pw=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const records=[],server=http.createServer(async(req,res)=>{
  const chunks=[];for await(const chunk of req)chunks.push(chunk);
  const body=Buffer.concat(chunks);assert(body.length<=65536);
  const value={url:req.url,method:req.method,headers:req.headers,bodyBase64:body.toString('base64')};records.push(value);
  res.sendDate=false;res.writeHead(200,{'Content-Type':req.url==='/fixture'?'text/html':'application/json','Connection':'close'});
  res.end(req.url==='/fixture'?'<title>Acme JSON bodies</title>Acme':JSON.stringify(value));
});
const json={'Content-Type':'application/json'},plain={'Content-Type':'text/plain'};
const invalid='Acme invalid\nline',valid=' { "Acme": 1, "Acme": 2 } \n';
const escapedSurrogate='"'+String.fromCharCode(92)+'u'+'d800'+'"';
const cases=[
  ['exact-invalid',invalid,json],['exact-valid',valid,json],['exact-overflow','1e9999',json],
  ['exact-surrogate',escapedSurrogate,json],['exact-deep','['.repeat(512)+'0'+']'.repeat(512),json],
  ['exact-empty','',json],['exact-space',' \t ',json],['plain-invalid',invalid,plain],
  ['case-invalid',invalid,{'Content-Type':'Application/JSON'}],
  ['charset-invalid',invalid,{'Content-Type':'application/json; charset=utf-8'}],
  ['padded-invalid',invalid,{'Content-Type':' application/json '}],
  ['binary-json',Buffer.from(invalid),json],['empty-binary-json',Buffer.alloc(0),json],
  ['default-json',invalid,undefined,'defaults'],['default-json-empty','',undefined,'defaults'],
  ['mixed-first-json',invalid,{'Content-Type':'application/json','content-type':'text/plain'}],
  ['mixed-first-plain',invalid,{'Content-Type':'text/plain','content-type':'application/json'}],
  ['no-headers',invalid],
  ['no-headers-empty',''],['no-headers-binary',Buffer.from([0,255,128])],
  ['no-headers-empty-binary',Buffer.alloc(0)],['empty-content-type',invalid,{'Content-Type':''}],
  ['source-json-invalid',invalid,undefined,'plain','json'],
  ['source-json-valid',valid,undefined,'plain','json'],
  ['source-json-binary',Buffer.from(invalid),undefined,'plain','json'],
  ['source-json-clear-headers',invalid,{},'defaults','json'],
  ['source-json-explicit-plain',invalid,plain,'plain','json'],
  ['source-plain-default-json',invalid,undefined,'defaults','plain'],
];
(async()=>{
  const report={cases:{}};let browser,clients={},failure;
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const base='http://127.0.0.1:'+server.address().port;
  try{
    const chrome=process.argv[3]==='chrome';
    browser=await pw[chrome?'chromium':process.argv[3]].launch({headless:process.argv[4]==='headless',...(chrome?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    clients.plain=await pw.request.newContext();clients.defaults=await pw.request.newContext({extraHTTPHeaders:json});
    const context=await browser.newContext(),page=await context.newPage(),sources={};await page.goto(base+'/fixture');
    for(const kind of ['json','plain']){
      const url=base+'/source-'+kind,pending=page.waitForRequest(request=>request.url()===url);
      await page.evaluate(async({url,contentType})=>{const r=await fetch(url,{method:'POST',headers:{'Content-Type':contentType,'X-Acme':'source'},body:new Uint8Array([65,99,109,101,0,255])});if(r.status!==200)throw Error('Acme source status');await r.text();},{url,contentType:kind==='json'?'application/json':'text/plain'});
      sources[kind]=await pending;
    }
    for(const [name,data,headers,client='plain',source] of cases){
      const input={client,source,headers:headers===undefined?undefined:Object.entries(headers).map(([name,value])=>({name,value})),body:Buffer.isBuffer(data)?{kind:'base64',data:data.toString('base64')}:{kind:'text',text:data}};
      const response=await clients[client].fetch(source?sources[source]:base+'/case-'+name,{method:'POST',data,...(headers===undefined?{}:{headers})});
      assert.equal(response.status(),200);report.cases[name]={input,value:await response.json()};await response.dispose();
    }
    assert.equal(Object.keys(report.cases).length,28);await context.close();report.records=records;
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally{
    try{for(const client of Object.values(clients))await client.dispose();await browser?.close();}finally{await utils.gracefullyCloseAll();}
    server.closeAllConnections();await new Promise(resolve=>server.close(resolve));report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
