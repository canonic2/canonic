// Owned Chrome CDP body-retention reference; reports lengths/hashes, not payloads.
const assert=require('node:assert/strict'),http=require('node:http'),path=require('node:path'),crypto=require('node:crypto');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const hash=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
function material(size){return size===6?Buffer.from([65,99,109,101,0,255]):Buffer.from(Array.from({length:size},(_,index)=>index%256));}
const records=[];const server=http.createServer(async(request,response)=>{
  const parts=[];for await(const chunk of request)parts.push(chunk);const body=Buffer.concat(parts);assert(body.length<=1048576);
  if(request.url==='/fixture'){response.writeHead(200,{'Content-Type':'text/html'});response.end('<!doctype html><title>Acme owned Request</title>Acme');return;}
  records.push({url:request.url,bytes:body.length,sha256:hash(body)});response.writeHead(204);response.end();
});
(async()=>{
  const report={cases:[]};let browser,failure;
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const origin='http://127.0.0.1:'+server.address().port;
  try {
    browser=await playwright.chromium.launch({headless:process.argv[3]==='headless',executablePath:path.join(process.argv[4],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')});
    const context=await browser.newContext();const page=await context.newPage();await page.goto(origin+'/fixture');const cdp=await context.newCDPSession(page);
    for(const maxPostDataSize of [0,65536,1048576]){
      await cdp.send('Network.enable',{maxTotalBufferSize:1048576,maxResourceBufferSize:262144,maxPostDataSize});
      for(const size of [6,65537,1048576]){
        const url=origin+'/Acme-source?budget='+maxPostDataSize+'&size='+size;
        const pending=page.waitForRequest(request=>request.url()===url);
        let event;const listener=value=>{if(value.request.url===url)event=value;};cdp.on('Network.requestWillBeSent',listener);
        try {
          await page.evaluate(async({url,size})=>{const bytes=size===6?new Uint8Array([65,99,109,101,0,255]):Uint8Array.from({length:size},(_,index)=>index%256);const response=await fetch(url,{method:'POST',headers:{'Content-Type':'application/Acme','X-Acme':'source'},body:bytes});if(response.status!==204)throw Error('Acme source status');}, {url,size});
          const request=await pending;assert(event,'Owned CDP request event');
          const expected=material(size),selected=new URL(url),matching=records.filter(record=>record.url===selected.pathname+selected.search);assert.equal(matching.length,1,'Exactly one owned source request');const record=matching[0];assert.equal(record.bytes,size);assert.equal(record.sha256,hash(expected));
          const entries=event.request.postDataEntries;const captured=entries?Buffer.concat(entries.filter(entry=>typeof entry.bytes==='string').map(entry=>Buffer.from(entry.bytes,'base64'))):null;
          const sdk=request.postDataBuffer();let late;
          try{const value=await cdp.send('Network.getRequestPostData',{requestId:event.requestId});const bytes=Buffer.from(value.postData,'utf8');late={ok:true,bytes:bytes.length,sha256:hash(bytes)};}catch(error){late={ok:false,name:error.name,message:error.message};}
          report.cases.push({maxPostDataSize,size,hasPostData:event.request.hasPostData,postDataStringBytes:typeof event.request.postData==='string'?Buffer.byteLength(event.request.postData):null,entryCount:entries?.length??null,entryBytes:captured?.length??null,entrySha256:captured?hash(captured):null,sdkBytes:sdk?.length??null,sdkSha256:sdk?hash(sdk):null,expectedSha256:hash(expected),late});
        }finally{cdp.off('Network.requestWillBeSent',listener);}
      }
    }
    await cdp.detach();await context.close();assert.equal(records.filter(record=>record.url.startsWith('/Acme-source?')).length,9);assert(records.every(record=>record.url.startsWith('/Acme-source?')||record.url==='/favicon.ico'),'Unexpected browser request');
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally{try{await browser?.close();}finally{await utils.gracefullyCloseAll();}server.closeAllConnections();await new Promise(resolve=>server.close(resolve));report.records=records;report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));}
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
