// Pinned public HTTP client disposal, retained metadata and parent-context lifetime.
const assert=require('node:assert/strict'),http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const held=new Map();const requests=[];
const server=http.createServer((request,response)=>{
  requests.push(request.url);
  if(request.url.startsWith('/hold')) {held.set(request.url,response);return;}
  response.writeHead(201,'Acme',{'X-Acme':'one','Content-Length':4});response.end('Acme');
});
async function until(predicate) {
  const deadline=Date.now()+2000;
  while(!predicate()) {assert(Date.now()<deadline,'Owned request did not arrive');await new Promise(resolve=>setTimeout(resolve,1));}
}
async function outcome(action) {
  try {return {ok:true,value:await action()};}
  catch(error) {return {ok:false,name:error.name,message:error.message};}
}
async function metadata(response) {
  return {url:response.url(),status:response.status(),statusText:response.statusText(),
    ok:response.ok(),headers:response.headers(),headersArray:response.headersArray(),
    securityDetails:await response.securityDetails(),serverAddr:await response.serverAddr(),timing:response.timing()};
}
async function exercise(client,url,associated=false,report={}) {
  const response=await client.get(url+'/complete'),before=await metadata(response);
  assert.equal(await response.text(),'Acme');
  const separate=await client.get(url+'/response-dispose'),separateBefore=await metadata(separate);
  await separate.dispose();await separate.dispose();
  const separateRead=await outcome(()=>separate.text());
  assert.equal(separateRead.ok,false);assert(separateRead.message.includes('Response has been disposed'));
  assert.deepEqual(await metadata(separate),separateBefore);
  report.responseDisposal={bodyClosed:true,metadataSurvived:true};
  const fetchHeld=target=>outcome(async()=>{const value=await client.get(target);return {status:value.status(),body:await outcome(()=>value.text())};});
  const first=fetchHeld(url+'/hold-first'),second=fetchHeld(url+'/hold-second');
  await until(()=>held.has('/hold-first')&&held.has('/hold-second'));
  const started=Date.now();
  const disposed=await outcome(()=>client.dispose({reason:'Acme first disposal'}));
  report.disposeMs=Date.now()-started;
  report.socketsAfterDispose=[...held.values()].map(response=>response.socket?.destroyed??null);
  assert.equal(disposed.ok,true);
  if(associated) for(const response of held.values()) {response.writeHead(202,{'Content-Length':4});response.end('Acme');}
  const pending=await Promise.all([first,second]);
  report.pending=pending;
  if(!associated) assert(pending.every(result=>!result.ok&&result.message.includes('Acme first disposal')));
  else {
    assert.deepEqual(report.socketsAfterDispose,[false,false]);
    assert(pending.every(result=>result.ok&&result.value.status===202&&!result.value.body.ok&&result.value.body.message.includes('Response has been disposed')));
  }
  assert.deepEqual(await metadata(response),before);
  const reads={body:await outcome(()=>response.body()),text:await outcome(()=>response.text()),json:await outcome(()=>response.json())};
  assert(Object.values(reads).every(result=>!result.ok&&result.message.includes('Response has been disposed')));
  const after={
    fetch:await outcome(()=>client.get(url+'/never')),
    cookies:await outcome(()=>client.cookies()),
    addCookies:await outcome(()=>client.addCookies([])),
    clearCookies:await outcome(()=>client.clearCookies()),
    storageState:await outcome(()=>client.storageState()),
    responseDispose:await outcome(()=>response.dispose())
  };
  assert(Object.values(after).every(result=>!result.ok));
  assert(after.fetch.message.includes('Acme first disposal'));
  const repeated=await outcome(()=>client.dispose({reason:'Acme second disposal'}));
  assert.equal(repeated.ok,true);
  const afterRepeated=await outcome(()=>client.get(url+'/never-again'));
  assert.equal(afterRepeated.ok,false);assert(afterRepeated.message.includes('Acme second disposal'));
  held.clear();
  return Object.assign(report,{disposed,pending,reads,after,repeated,afterRepeated,metadataSurvived:true});
}
(async()=>{
  let browser,failure;const report={};
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const url='http://127.0.0.1:'+server.address().port;
  try {
    const isolated=await playwright.request.newContext();
    try {report.isolated=await exercise(isolated,url);} finally {await isolated.dispose();}
    const chrome=process.argv[3]==='chrome';
    browser=await playwright[chrome?'chromium':process.argv[3]].launch({
      headless:process.argv[4]==='headless',
      ...(chrome?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})
    });
    const context=await browser.newContext();
    try {
      report.associated={};
      await exercise(context.request,url,true,report.associated);
      const page=await context.newPage();await page.goto(url+'/parent');
      assert.equal(await page.textContent('body'),'Acme');
      report.parentContextSurvived=true;
    } finally {await context.close();}
    const closing=await browser.newContext();
    const pendingClose=outcome(()=>closing.request.get(url+'/hold-parent'));
    await until(()=>held.has('/hold-parent'));
    const parentResponse=held.get('/hold-parent');
    await closing.close();
    const closedRequest=await pendingClose;
    assert.equal(closedRequest.ok,false);assert(/closed|disposed/.test(closedRequest.message));
    await until(()=>parentResponse.destroyed);
    report.parentClosure={requestFailed:true,socketClosed:true};held.clear();
    assert.deepEqual(requests.filter(url=>url!=='/favicon.ico').sort(),['/complete','/complete','/hold-first','/hold-first','/hold-parent','/hold-second','/hold-second','/parent','/response-dispose','/response-dispose']);
    report.requests=requests;
  } catch(error) {failure=error;report.failure={name:error.name,message:error.message};}
  finally {
    try {await browser?.close();} finally {await utils.gracefullyCloseAll();}
    server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
    report.ownedProcesses=utils.gracefullyCloseSet.size;
    process.stdout.write(JSON.stringify(report));
  }
  if(failure) {console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
