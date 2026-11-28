// Public pinned SDK cookie administration; no SDK mutation or external requests.
const assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const fields=['name','value','domain','path','expires','httpOnly','secure','sameSite'];
function project(values){return values.map(c=>Object.fromEntries(fields.map(k=>[k,c[k]]))).sort((a,b)=>JSON.stringify(a).localeCompare(JSON.stringify(b)));}
const initial=[
  {name:'acme',value:'url',url:'http://127.0.0.1/acme/page',secure:true},
  {name:'scope',value:'secure',domain:'example.com',path:'/acme',secure:true},
  {name:'local',value:'secure',domain:'localhost',path:'/',secure:true},
  {name:'acme',value:'root',domain:'127.0.0.1',path:'/'},
];
async function exercise(client){
  await client.addCookies(initial);
  await client.addCookies([{name:'acme',value:'replacement',domain:'127.0.0.1',path:'/'}]);
  const result={all:project(await client.cookies()),string:project(await client.cookies('https://child.example.com/acme-extra')),
    union:project(await client.cookies(['http://127.0.0.1/acme/page','http://localhost/']))};
  assert.equal(result.all.length,4);assert.equal(result.string.length,1);assert.equal(result.union.length,3);
  await client.addCookies([{name:'acme',value:'deleted',domain:'127.0.0.1',path:'/',expires:0}]);
  result.expired=project(await client.cookies());assert.equal(result.expired.length,3);
  return result;
}
(async()=>{
  const report={};let browser,failure;
  try{
    const isolated=await playwright.request.newContext();
    try{report.isolated=await exercise(isolated);}finally{await isolated.dispose();}
    {
      const chrome=process.argv[3]==='chrome';
      browser=await playwright[chrome?'chromium':process.argv[3]].launch({headless:process.argv[4]==='headless',
        ...(chrome?{executablePath:require('node:path').join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
      const context=await browser.newContext();
      try{report.associated=await exercise(context.request);}finally{await context.close();}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
