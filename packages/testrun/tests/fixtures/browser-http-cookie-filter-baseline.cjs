// Unchanged pinned SDK methods over an owned jar; no external requests.
const assert=require('node:assert/strict'),path=require('node:path');
const data=require('./browser-cookie-filter-cases.json');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const fields=['name','value','domain','path','expires','httpOnly','secure','sameSite'];
function project(values){return values.map(c=>Object.fromEntries(fields.map(k=>[k,c[k]])));}
function criteria(options){return Object.fromEntries(Object.entries(options).map(([key,value])=>
  [key,typeof value==='string'?value:new RegExp(value.regex,value.flags)]));}
async function cases(client){
  const results=[];
  for(const test of data.cases){
    await client.clearCookies();await client.addCookies(data.cookies);
    let outcome='cleared';
    try{await client.clearCookies(criteria(test.options));}
    catch(error){if(!(error instanceof SyntaxError))throw error;outcome='invalid';}
    results.push({name:test.name,outcome,remaining:project(await client.cookies())});
  }
  await client.clearCookies();return results;
}
const cookie=(name,expires=-1)=>({name,value:'Acme',domain:'example.com',path:'/',expires});
async function ordering(){
  const report={};
  for(const variant of ['beforeRead','afterRead','afterNoMatchClear']){
    const client=await playwright.request.newContext();
    try{
      await client.addCookies([cookie('first',0),cookie('second')]);
      if(variant==='afterRead')await client.cookies();
      if(variant==='afterNoMatchClear')await client.clearCookies({name:'missing'});
      await client.addCookies([cookie('first')]);
      report[variant]=(await client.cookies()).map(c=>c.name);
    }finally{await client.dispose();}
  }
  const client=await playwright.request.newContext();
  try{
    const expiry=(Date.now()+50)/1000;
    await client.addCookies([cookie('first',expiry),cookie('second')]);
    const until=Date.now()+2000;
    while(Date.now()<=expiry*1000){assert(Date.now()<until);await new Promise(resolve=>setTimeout(resolve,1));}
    await client.addCookies([cookie('third')]);await client.clearCookies({name:'missing'});
    await client.addCookies([cookie('first')]);
    report.expiredMember=(await client.cookies()).map(c=>c.name);
  }finally{await client.dispose();}
  assert.deepEqual(report.beforeRead,['first','second']);
  assert.deepEqual(report.afterRead,['second','first']);
  assert.deepEqual(report.afterNoMatchClear,['second','first']);
  assert.deepEqual(report.expiredMember,['first','second','third']);
  return report;
}
(async()=>{
  const report={unicode:process.versions.unicode};let browser,failure;
  try{
    report.ordering=await ordering();
    const isolated=await playwright.request.newContext();
    try{report.isolated=await cases(isolated);}finally{await isolated.dispose();}
    const chrome=process.argv[3]==='chrome';
    browser=await playwright[chrome?'chromium':process.argv[3]].launch({headless:process.argv[4]==='headless',
      ...(chrome?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    const context=await browser.newContext();
    try{report.associated=await cases(context.request);}finally{await context.close();}
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
