// Unchanged-SDK diagnostic for traced collection, source RunTo and native input.
const assert=require('node:assert/strict'),fs=require('node:fs');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const tracked=promise=>{promise.catch(()=>{});return promise;};
async function within(promise,label,ms=30000){let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Acme '+label+' deadline')),ms))]);}finally{clearTimeout(timer);}}
const internal=(owner,callback)=>owner._wrapApiCall(callback,{internal:true});
async function paused(debugger_){const end=Date.now()+10000;while(!debugger_.pausedDetails()){if(Date.now()>=end)throw Error('Acme automation pause unavailable');await delay(10);}}
(async()=>{
  const mode=process.argv[3],engine=process.argv[6]||'firefox',owned=process.argv[4]==='owned',report={engine,mode,owned,stage:'launch',completedClicks:0,calls:[]};
  if(owned){const adapter=require('node:vm').runInThisContext(fs.readFileSync(process.argv[5],'utf8'));adapter.installContextCleanup(playwright._connection.toImpl(playwright),require(process.argv[2]+'/lib/coreBundle.js').server.BrowserContext);}
  function stage(name){report.stage=name;fs.writeFileSync('source-input-'+mode+'-progress.json',JSON.stringify(report));}
  let browser,context,page,debugger_,failure;const calls=new Map();
  try{
    browser=await playwright[engine].launch({headless:mode==='headless',...(engine==='firefox'&&mode==='headed'?{firefoxUserPrefs:{'layout.css.devPixelsPerPx':'1.0','browser.uidensity':2}}:{})});
    const siblingContext=await browser.newContext(),sibling=await siblingContext.newPage();
    context=await browser.newContext({viewport:{width:1280,height:720},deviceScaleFactor:1});
    page=await context.newPage();page.setDefaultTimeout(30000);
    await page.setContent("<script>globalThis.acmeDebuggerClicks=0</script><button id=Acme-private-step onclick='acmeDebuggerClicks++'>Acme</button>");
    debugger_=context.debugger;
    const serverDebugger=debugger_._connection.toImpl(debugger_);
    debugger_.on('apicallsupdated',updates=>{for(const update of updates){const old=calls.get(update.id);const logs=[...(old?.logs||[]),...(update.newLogEntries||[])];calls.set(update.id,{...old,...update,logs:logs.slice(-8)});if(calls.size>32)calls.delete(calls.keys().next().value);}});
    await debugger_._enable();await context.tracing.start({snapshots:true});
    stage('boundary-first');await debugger_.requestPause();
    const first=tracked(page.locator('#Acme-private-step').click());await paused(debugger_);
    await internal(page,()=>page.locator('#Acme-private-step').count());
    await internal(page,()=>page.locator('#Acme-private-step').textContent());
    await internal(page,()=>page.locator('#Acme-private-step').boundingBox());
    await context.tracing.stop({path:'Acme-source-input-trace-'+engine+'-'+mode+'.zip'});
    await debugger_.next();await within(first,'first click');report.completedClicks++;
    stage('boundary-second');
    const second=tracked(page.locator('#Acme-private-step').click());await paused(debugger_);await debugger_.resume();await within(second,'second click');report.completedClicks++;
    assert.equal(await page.evaluate(()=>acmeDebuggerClicks),2);
    stage('source-first');await debugger_.requestPause();
    const sourceFirst=tracked(page.locator('#Acme-private-step').click());await paused(debugger_);
    const matchingLine=fs.readFileSync(__filename,'utf8').split('\n').findIndex(line=>line.trimStart().startsWith('const matching ='))+1;
    assert.ok(matchingLine>0);await debugger_.runTo({file:__filename,line:matchingLine});await within(sourceFirst,'first source click');report.completedClicks++;
    for(let index=0;index<3;index++){
      stage('source-nonmatching-'+index);
      await page.locator('#Acme-private-step').click();report.completedClicks++;
      assert.equal(debugger_.pausedDetails(),null);
    }
    assert.equal(await page.evaluate(()=>acmeDebuggerClicks),6);
    stage('source-matching');
    const matching = tracked(page.locator('#Acme-private-step').click());
    await paused(debugger_);await debugger_.resume();await within(matching,'matching click');report.completedClicks++;
    assert.equal(await page.evaluate(()=>acmeDebuggerClicks),7);
    stage('paused-close');await debugger_.requestPause();
    const pending=tracked(page.locator('#Acme-private-step').click());await paused(debugger_);await context.close();context=null;
    // The public SDK debugger is disposed with its context. Release the saved
    // server pause solely for fixture cleanup after the context is closed.
    await assert.rejects(debugger_.resume(),/Target page, context or browser has been closed/);
    report.closedContextResume='target-closed';assert.equal(await sibling.evaluate(()=>6*7),42);
    if(owned){assert.equal(serverDebugger.isPaused(),false);report.fixtureCleanup='owned-driver-context-close-release';}
    else {serverDebugger.resume();report.fixtureCleanup='server-resume-after-context-close';}
    await within(assert.rejects(pending),'closed caller settlement',10000);
    stage('complete');
  }catch(error){
    failure=error;report.failure={name:error.name,message:error.message,stage:report.stage};
    if(page){try{report.renderer=await within(internal(page,()=>page.evaluate(()=>{const element=document.querySelector('#Acme-private-step'),box=element?.getBoundingClientRect();return {count:globalThis.acmeDebuggerClicks,visibility:document.visibilityState,focused:document.hasFocus(),disabled:element?.disabled,box:box&&{x:box.x,y:box.y,width:box.width,height:box.height}};})),'failure renderer');}catch(error){report.renderer={error:error.message};}}
  }finally{
    report.calls=[...calls.values()];
    try{if(context){await context.close();if(debugger_?.pausedDetails())await debugger_.resume();}}finally{try{await browser?.close();}finally{await utils.gracefullyCloseAll();}}
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
