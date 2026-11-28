// Owned Request input/default/lifetime evidence from the unchanged pinned SDK.
const assert=require('node:assert/strict'),http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const bytes=Buffer.from([65,99,109,101,0,255]),records=[];
const server=http.createServer(async(request,response)=>{
  const chunks=[];for await(const chunk of request)chunks.push(chunk);
  const body=Buffer.concat(chunks);assert(body.length<=65536);
  const record={url:request.url,method:request.method,headers:request.headers,bodyBase64:body.toString('base64')};
  records.push(record);response.sendDate=false;
  if(request.url==='/fixture') {
    response.writeHead(200,{'Content-Type':'text/html','Connection':'close'});
    response.end('<!doctype html><title>Acme Request source</title>Acme');
  }else if(request.url==='/redirect-credentials') {
    response.writeHead(302,{Location:'http://Acme2:changed@127.0.0.1:'+server.address().port+'/after-redirect#Acme','Connection':'close'});
    response.end(JSON.stringify(record));
  }else if(request.url==='/auth-fallback'&&request.headers.authorization!=='Basic QWNtZUNvbmZpZ3VyZWQ6Y29ycmVjdA==') {
    response.writeHead(401,{'WWW-Authenticate':'Basic realm=Acme','Content-Type':'application/json','Connection':'close'});
    response.end(JSON.stringify(record));
  }else{
    response.writeHead(200,{'Content-Type':'application/json','Connection':'close'});
    response.end(JSON.stringify(record));
  }
});
async function source(context,url,name) {
  const page=await context.newPage();await page.goto(url+'/fixture');
  const pending=page.waitForRequest(request=>request.url()===url+'/source?case='+name);
  await page.evaluate(async({url,bytes})=>{
    const response=await fetch(url,{method:'POST',headers:{'X-Acme':'source','Content-Type':'application/Acme'},body:new Uint8Array(bytes)});
    if(response.status!==200)throw Error('Acme initial source status');
    await response.text();
  },{url:url+'/source?case='+name,bytes:[...bytes]});
  const request=await pending;
  assert.equal(request.method(),'POST');assert.deepEqual(request.postDataBuffer(),bytes);
  return {page,request};
}
async function fetch(client,request,options) {
  try {
    const response=await client.fetch(request,options);
    const value=await response.json();assert.equal(response.status(),200);
    await response.dispose();return {ok:true,value};
  }catch(error){return {ok:false,name:error.name,message:error.message};}
}
(async()=>{
  const report={cases:{},lifetime:{}};let browser,client,failure;
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const url='http://127.0.0.1:'+server.address().port;
  try {
    const chrome=process.argv[3]==='chrome';
    browser=await playwright[chrome?'chromium':process.argv[3]].launch({headless:process.argv[4]==='headless',
      ...(chrome?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    client=await playwright.request.newContext();
    const context=await browser.newContext();
    await context.addCookies([{name:'AcmeSource',value:'source-cookie',url}]);
    const captured=await source(context,url,'defaults');
    for(const [name,options] of [
      ['defaults',{}],['method',{method:'GET'}],['emptyMethod',{method:''}],
      ['headers',{headers:{'X-Acme':'override'}}],['emptyHeaders',{headers:{}}],
      ['latin1Header',{headers:{'X-Latin':String.fromCharCode(255)}}],
      ['body',{data:'Acme replacement'}],['emptyBody',{data:''}],
      ['params',{params:new URLSearchParams([['Acme','one'],['Acme','two']])}],
    ]) {
      const result=await fetch(client,captured.request,options);assert.equal(result.ok,true,JSON.stringify({name,result}));
      report.cases[name]=result;
    }
    const associated=await fetch(context.request,captured.request,{});
    assert.equal(associated.ok,true,JSON.stringify(associated));report.associated=associated;
    await client.addCookies([{name:'AcmeJar',value:'jar-cookie',url}]);
    report.initialHeaders={};
    for(const [name,headers] of [
      ['explicit',{Cookie:'AcmeExplicit=explicit-cookie',Host:'Acme-host.invalid'}],
      ['emptyCookie',{Cookie:''}],['jar',{}],
    ]) {
      const response=await client.get(url+'/initial-headers',{headers});
      assert.equal(response.status(),200);report.initialHeaders[name]=await response.json();await response.dispose();
    }
    const before=records.length;
    report.nonLatin1Header=await fetch(client,captured.request,{headers:{'X-Acme':'💩'}});
    assert.equal(report.nonLatin1Header.ok,false);assert.equal(records.length,before);
    report.urlInputs={};
    const credentials=new URL(url+'/url-input');credentials.username='Acme';credentials.password='known';
    for(const [name,input] of [
      ['fragment',url+'/url-input#Acme'],['credentials',credentials.href],
      ['outerWhitespace','\t '+url+'/url-input \r\n'],['spaces',url+'/Acme path?q=one two'],
    ]) {
      try {
        const response=await client.get(input);
        report.urlInputs[name]={ok:true,url:response.url(),value:await response.json()};
        await response.dispose();
      }catch(error){report.urlInputs[name]={ok:false,name:error.name,message:error.message};}
    }
    const explicit=await client.get(credentials.href,{headers:{Authorization:'Acme explicit'}});
    report.urlInputs.explicitAuthorization={ok:true,url:explicit.url(),value:await explicit.json()};await explicit.dispose();
    const redirected=new URL(credentials);redirected.pathname='/redirect-credentials';
    const changed=await client.get(redirected.href);
    report.urlInputs.redirectCredentials={ok:true,url:changed.url(),value:await changed.json()};await changed.dispose();
    const fallback=await playwright.request.newContext({httpCredentials:{username:'AcmeConfigured',password:'correct'}});
    try {
      const challenged=new URL(credentials);challenged.pathname='/auth-fallback';
      const response=await fallback.get(challenged.href);
      assert.equal(response.status(),200);report.urlInputs.challengeFallback={ok:true,url:response.url(),value:await response.json()};await response.dispose();
    }finally{await fallback.dispose();}
    await captured.page.close();
    report.lifetime.cachedAfterPageClose=await fetch(client,captured.request,{});
    await context.close();
    report.lifetime.cachedAfterContextClose=await fetch(client,captured.request,{});
    for(const close of ['page','context']) {
      const owner=await browser.newContext(),fresh=await source(owner,url,'unwarmed-'+close);
      if(close==='page')await fresh.page.close();else await owner.close();
      report.lifetime['unwarmedAfter'+(close==='page'?'Page':'Context')+'Close']=await fetch(client,fresh.request,{});
      if(close==='page')await owner.close();
    }
    report.source={method:captured.request.method(),bodyBase64:captured.request.postDataBuffer().toString('base64')};
    report.records=records;
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally {
    try{await client?.dispose();await browser?.close();}finally{await utils.gracefullyCloseAll();}
    server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
