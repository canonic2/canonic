// Native-compatible certificate contexts measured against the pinned SDK.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const input=JSON.parse(fs.readFileSync(process.argv[6],'utf8'));
const decode=entry=>Object.fromEntries(Object.entries(entry).map(([key,value])=>[key,['cert','key','pfx'].includes(key)?Buffer.from(value,'base64'):value]));
// Payload-free phase markers on stderr attribute a slow or stalled reference step.
async function phase(name,action) {
  const write=state=>process.stderr.write(JSON.stringify({phase:name,state,at:Date.now()})+'\n');
  write('start');
  try {const value=await action();write('complete');return value;}
  catch(error){write('failed:'+error.name);throw error;}
}
(async()=>{
  const chrome=process.argv[3]==='chrome';let browser,failure;
  const report={cases:{}};
  try {
    browser=await phase('browser.launch',()=>playwright[chrome?'chromium':process.argv[3]].launch({headless:process.argv[4]==='headless',
      ...(chrome?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})}));
    for(const [name,certificates] of Object.entries(input.cases)) {
      const context=await phase(name+'.context.create',()=>browser.newContext({clientCertificates:certificates.map(decode),...(input.proxy?{proxy:input.proxy}:{})}));
      try {
        const page=await phase(name+'.page.create',()=>context.newPage());const row={};
        let response=await phase(name+'.plain.goto',()=>page.goto(input.plain+'/plain'));row.plainStatus=response.status();
        response=await phase(name+'.selected.goto',()=>page.goto(input.selected+'/selected'));row.selectedStatus=response.status();
        // This origin is deliberately untrusted; a 503 proxy response is a
        // refusal observation, never positive default-trust HTTPS acceptance.
        // Pinned WebKit rewrites localhost outside the configured origin key.
        // Its existing accepted localhost behavior is an anonymous page fetch.
        assert.equal(row.selectedStatus,process.argv[3]==='webkit'?200:503);
        row.selectedIdentity=row.selectedStatus===200?await page.locator('body').innerText():null;
        if(process.argv[3]==='webkit')assert.equal(row.selectedIdentity,'none');
        response=await phase(name+'.http.get',()=>context.request.get(input.selected+'/inherited',{ignoreHTTPSErrors:true}));
        row.httpStatus=response.status();row.httpIdentity=await phase(name+'.http.text',()=>response.text());await phase(name+'.http.dispose',()=>response.dispose());
        response=await phase(name+'.other.goto',()=>page.goto(input.other+'/other'));row.otherStatus=response.status();row.otherIdentity=await page.locator('body').innerText();
        const isolated=await phase(name+'.isolated.create',()=>browser.newContext());
        try {
          const isolatedPage=await phase(name+'.isolated.page',()=>isolated.newPage());
          try {await phase(name+'.isolated.goto',()=>isolatedPage.goto(input.other+'/isolated'));row.isolatedRejected=false;}
          catch {row.isolatedRejected=true;}
        } finally {await phase(name+'.isolated.close',()=>isolated.close());}
        report.cases[name]=row;
      } finally {await phase(name+'.context.close',()=>context.close());}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally {
    try{await phase('browser.close',()=>browser?.close());}finally{await phase('utils.gracefullyCloseAll',()=>utils.gracefullyCloseAll());}
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
  // The pinned SDK can keep Node's event loop alive after every owned process
  // has closed; the report above is complete, so leave explicitly.
  process.exit();
})().catch(error=>{console.error(error);process.exit(1);});
