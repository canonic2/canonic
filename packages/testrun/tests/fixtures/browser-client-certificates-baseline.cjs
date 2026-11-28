// Unchanged pinned SDK: standalone and browser-associated TLS client identities.
const fs=require('node:fs'),assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const input=JSON.parse(fs.readFileSync(process.argv[5],'utf8'));
const decode=entry=>Object.fromEntries(Object.entries(entry).map(([key,value])=>[key,['cert','key','pfx'].includes(key)?Buffer.from(value,'base64'):value]));
const certificates=input.certificates.map(decode);
(async()=>{
  let browser,failure;const report={engine:process.argv[3],mode:process.argv[4],cases:{}};
  async function observe(context,name,url,expected) {
    const response=await context.get(url),body=await response.text();
    assert.equal(response.status(),200);assert.equal(body,expected);
    report.cases[name]={status:response.status(),identity:body};
  }
  async function isolated(selected,work) {
    const context=await playwright.request.newContext({ignoreHTTPSErrors:true,clientCertificates:selected});
    try {await work(context);}finally{await context.dispose();}
  }
  try {
    await isolated(certificates,async context=>{
      await observe(context,'multiA',input.first,'A');
      await observe(context,'multiB',input.second,'B');
      await observe(context,'multiRedirect',input.first+'/redirect','B');
    });
    await isolated([certificates[0]],context=>observe(context,'singleRedirect',input.first+'/redirect','none'));
    await isolated([],context=>observe(context,'empty',input.first,'none'));
    for (const [name,entries] of Object.entries(input.formats||{})) {
      const selected=entries.map(decode);
      await isolated(selected,async context=>{
        await observe(context,name+'A',input.first,'A');
        await observe(context,name+'Redirect',input.first+'/redirect','B');
      });
      const invalid=selected.map(entry=>({...entry,passphrase:'incorrect'}));
      await assert.rejects(()=>isolated(invalid,context=>context.get(input.first)));
    }
    if(process.argv[3]!=='chrome') {
      browser=await playwright[process.argv[3]].launch({headless:process.argv[4]==='headless'});
      const context=await browser.newContext({ignoreHTTPSErrors:true,clientCertificates:certificates});
      try {
        await observe(context.request,'inheritedA',input.first,'A');
        await observe(context.request,'inheritedRedirect',input.first+'/redirect','B');
      }finally{await context.close();}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally {
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
