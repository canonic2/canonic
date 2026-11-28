// Pinned SDK concurrency over one owned request context and loopback server.
const assert=require('node:assert/strict'),http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const pending=new Map(),seen=[];
const server=http.createServer((request,response)=>{
  const name=request.url.slice(1);seen.push(name);
  assert(!pending.has(name));pending.set(name,response);
});
const grant=name=>{
  const response=pending.get(name);assert(response);pending.delete(name);
  const status={A:201,B:202,C:203}[name];
  response.writeHead(status,{'X-Acme':'/'+name,'Set-Cookie':'Acme'+name+'=one; Path=/','Content-Length':2,'Connection':'close'});
  response.end('/'+name);
};
async function held(names){
  const until=Date.now()+2000;
  while(!names.every(name=>pending.has(name))){
    assert(Date.now()<until,'Both requests must dispatch before a response is granted');
    await new Promise(resolve=>setTimeout(resolve,1));
  }
}
async function project(response){
  const result={status:response.status(),body:await response.text(),
    header:response.headers()['x-acme']};
  await response.dispose();return result;
}
async function exercise(client,url){
  const before=seen.length;
  const a=client.get(url+'/A'),b=client.get(url+'/B');a.catch(()=>{});b.catch(()=>{});
  await held(['A','B']);
  const order=process.argv[4]==='headless'?['B','A']:['A','B'];
  const promises={A:a,B:b};let otherSettled=false;
  promises[order[1]].then(()=>{otherSettled=true;},()=>{otherSettled=true;});
  grant(order[0]);
  const first=await project(await promises[order[0]]);
  assert.equal(otherSettled,false,'The ungranted request must remain pending');
  grant(order[1]);const second=await project(await promises[order[1]]);
  const c=client.get(url+'/C');c.catch(()=>{});await held(['C']);grant('C');
  const next=await project(await c);
  const cookies=(await client.cookies()).map(cookie=>cookie.name).sort();
  assert.equal(seen.length-before,3);assert.deepEqual(cookies,['AcmeA','AcmeB','AcmeC']);
  return {order,first,second,next,cookies,requests:seen.length-before};
}
(async()=>{
  const report={};let browser,failure;
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const url='http://127.0.0.1:'+server.address().port;
  try{
    const isolated=await playwright.request.newContext();
    try{report.isolated=await exercise(isolated,url);}finally{await isolated.dispose();}
    const chrome=process.argv[3]==='chrome';
    browser=await playwright[chrome?'chromium':process.argv[3]].launch({headless:process.argv[4]==='headless',
      ...(chrome?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    const context=await browser.newContext();
    try{report.associated=await exercise(context.request,url);}finally{await context.close();}
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
