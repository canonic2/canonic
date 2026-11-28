// The unchanged pinned request client owns connection, redirect and reuse facts.
const fs=require('node:fs'),assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const input=JSON.parse(fs.readFileSync(process.argv[5],'utf8'));
(async()=>{
  const report={engine:process.argv[3],mode:process.argv[4],cases:{}};
  let browser,failure;
  async function capture(context,url) {
    const response=await context.get(url),timing=response.timing();
    assert.equal(response.status(),200);
    assert(timing.responseStart>=0&&timing.responseEnd>=timing.responseStart);
    const details={url:response.url(),status:response.status(),
      securityDetails:await response.securityDetails(),serverAddr:await response.serverAddr(),timing};
    await response.dispose();return details;
  }
  async function cases(context) {
    const result={};
    result.http=await capture(context,input.http+'/echo');
    result.tls=await capture(context,input.tls+'/first');
    result.reused=await capture(context,input.tls+'/second');
    result.redirect=await capture(context,input.tls+'/redirect');
    assert.equal(result.http.securityDetails,null);
    assert.equal(result.redirect.securityDetails,null);
    assert.equal(result.redirect.url,input.http+'/echo');
    assert.deepEqual(result.tls.securityDetails,result.reused.securityDetails);
    assert.equal(result.reused.timing.connectStart,-1);
    assert.equal(result.reused.timing.secureConnectionStart,-1);
    return result;
  }
  try {
    const isolated=await playwright.request.newContext({ignoreHTTPSErrors:true});
    try{report.cases.isolated=await cases(isolated);}finally{await isolated.dispose();}
    if(process.argv[3]!=='chrome') {
      browser=await playwright[process.argv[3]].launch({headless:process.argv[4]==='headless'});
      const context=await browser.newContext({ignoreHTTPSErrors:true});
      try{report.cases.associated=await cases(context.request);}finally{await context.close();}
    }
    const proxy=await playwright.request.newContext({proxy:{server:input.proxy}});
    try{
      const response=await proxy.get(input.refusalUrl);
      assert.equal(response.status(),403);
      report.refusal={status:response.status(),serverAddr:await response.serverAddr(),
        securityDetails:await response.securityDetails(),timing:response.timing()};
      await response.dispose();
    }finally{await proxy.dispose();}
    const tunnel=await playwright.request.newContext({ignoreHTTPSErrors:true,proxy:{server:input.tunnel}});
    try{report.tunnel={http:await capture(tunnel,input.http+'/echo'),tls:await capture(tunnel,input.tls+'/tunnel')};}
    finally{await tunnel.dispose();}
    const socks=await playwright.request.newContext({ignoreHTTPSErrors:true,proxy:{server:input.socks}});
    try{report.socks=await capture(socks,input.socksUrl);}finally{await socks.dispose();}
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    report.ownedProcesses=utils.gracefullyCloseSet.size;
    process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
