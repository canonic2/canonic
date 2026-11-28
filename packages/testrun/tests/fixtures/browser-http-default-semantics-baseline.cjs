// Compare global headers and disabled request-local timeouts without changing the SDK.
const fs=require('node:fs'),assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const input=JSON.parse(fs.readFileSync(process.argv[5],'utf8'));
(async()=>{
  const report={},headers={'X-Acme':'global',Authorization:'Acme global'};
  let browser,failure;
  async function record(context,url,options) {
    const response=await context.get(url,options);
    assert.equal(response.status(),200);
    const result=await response.json();await response.dispose();return result;
  }
  async function global(context) {
    const result={first:await record(context,input.first+'/echo'),
      second:await record(context,input.second+'/echo'),
      override:await record(context,input.second+'/echo',{headers:{'x-acme':'request'}}),
      redirect:await record(context,input.first+'/cross')};
    assert.equal(result.first.headers['x-acme'],'global');
    assert.equal(result.second.headers['x-acme'],'global');
    assert.equal(result.second.headers.authorization,'Acme global');
    assert.equal(result.override.headers['x-acme'],'request');
    assert.equal(result.redirect.headers.authorization,undefined);
    return result;
  }
  try {
    for(const based of [false,true]) {
      const context=await playwright.request.newContext({extraHTTPHeaders:headers,...(based?{baseURL:input.first}:{})});
      try{report[based?'based':'isolated']=await global(context);}finally{await context.dispose();}
    }
    if(process.argv[3]!=='chrome') {
      browser=await playwright[process.argv[3]].launch({headless:process.argv[4]==='headless'});
      const context=await browser.newContext({extraHTTPHeaders:headers});
      try{
        report.associated=await global(context.request);
        await context.setExtraHTTPHeaders({'X-Acme':'changed'});
        report.changed=await record(context.request,input.second+'/echo');
        await context.setExtraHTTPHeaders({});
        report.reset=await record(context.request,input.first+'/echo');
      }finally{await context.close();}
    } else {
      for(const [name,extraHTTPHeaders,url] of [['changed',{'X-Acme':'changed'},input.second+'/echo'],['reset',{},input.first+'/echo']]) {
        const context=await playwright.request.newContext({extraHTTPHeaders});
        try{report[name]=await record(context,url);}finally{await context.dispose();}
      }
    }
    const zero=await playwright.request.newContext({timeout:0});
    try{
      report.contextZero=await record(zero,input.first+'/delay/40');
      try{await zero.get(input.first+'/delay/250',{timeout:100});throw new Error('Finite override unexpectedly completed');}
      catch(error){if(!/Timeout|timed out/i.test(error.message))throw error;report.finiteOverride=true;}
    }finally{await zero.dispose();}
    const finite=await playwright.request.newContext({timeout:1});
    try{report.requestZero=await record(finite,input.first+'/delay/40',{timeout:0});}
    finally{await finite.dispose();}
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    report.ownedProcesses=utils.gracefullyCloseSet.size;
    process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
