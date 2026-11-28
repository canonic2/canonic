// Controlled native tracing-start configuration comparison; bounded SDK settings, not full parity.
const fs=require('node:fs'),assert=require('node:assert/strict'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const root=process.argv[3],url=process.argv[4],executablePath=process.argv[5];
const source=fs.readFileSync(process.argv[6],'utf8');
const list=source.match(/const PERFORMANCE_CATEGORIES:.*?= &\[([\s\S]*?)\];/)[1];
const categories=[...list.matchAll(/"([^"]+)"/g)].map(item=>item[1]);
assert(categories.includes('disabled-by-default-devtools.v8-source-rundown-sources'));
(async()=>{
  const observations=[];let browser,failure;
  try {
    for(const variant of ['production','category-split','sdk-bounded','sdk-pinned-buffer']) {
      const traceConfig=variant==='production'?{recordMode:'recordUntilFull',traceBufferSizeInKb:16384,enableSampling:true,includedCategories:categories}:{traceBufferSizeInKb:16384,includedCategories:categories.filter(category=>!category.startsWith('-')),excludedCategories:categories.filter(category=>category.startsWith('-')).map(category=>category.slice(1))};
      if(variant==='category-split')Object.assign(traceConfig,{recordMode:'recordUntilFull',enableSampling:true});
      if(variant==='sdk-pinned-buffer')traceConfig.traceBufferSizeInKb=1200*1000;
      browser=await playwright.chromium.launch({executablePath,headless:true,args:['--use-mock-keychain'],env:{PATH:'/usr/bin:/bin',HOME:root}});
      const context=await browser.newContext(),page=await context.newPage();await page.goto(url);
      const cdp=await browser.newBrowserCDPSession(),pageCdp=await context.newCDPSession(page);
      const target=(await pageCdp.send('Target.getTargetInfo')).targetInfo.targetId;
      const frontend=await cdp.send('Target.openDevTools',{targetId:target,panelId:'timeline'});
      const sessionId=(await cdp.send('Target.attachToTarget',{targetId:frontend.targetId,flatten:false})).sessionId;
      let sequence=0;const pending=new Map();
      cdp.on('Target.receivedMessageFromTarget',event=>{
        if(event.sessionId!==sessionId)return;const message=JSON.parse(event.message),entry=pending.get(message.id);
        if(!entry)return;pending.delete(message.id);message.error?entry.reject(Error(JSON.stringify(message.error))):entry.resolve(message.result);
      });
      function call(method,params={}) {return new Promise((resolve,reject)=>{
        const id=++sequence,promise={resolve,reject};pending.set(id,promise);
        cdp.send('Target.sendMessageToTarget',{sessionId,message:JSON.stringify({id,method,params})}).catch(reject);
      });}
      await call('Runtime.enable');
      const ready=await call('Runtime.evaluate',{expression:"(async()=>{const SDK=await import('devtools://devtools/bundled/core/sdk/sdk.js');const until=Date.now()+15000;while(Date.now()<until){if(SDK.TargetManager.TargetManager.instance().primaryPageTarget()?.id()==="+JSON.stringify(target)+")return true;await new Promise(resolve=>setTimeout(resolve,25));}throw Error('Frontend not ready');})()",awaitPromise:true,returnByValue:true});
      assert.equal(ready.result.value,true,JSON.stringify(ready));
      // Match the production module probe before recording begins.
      const probe=await call('Runtime.evaluate',{expression:"(async()=>{for(const path of ['panels/lighthouse/lighthouse.js','models/trace/trace.js','models/heap_snapshot/heap_snapshot.js'])await import('devtools://devtools/bundled/'+path);return true;})()",awaitPromise:true,returnByValue:true});
      assert.equal(probe.result.value,true,JSON.stringify(probe));
      await page.goto('about:blank');
      const finished=new Promise(resolve=>cdp.once('Tracing.tracingComplete',resolve));
      const start={transferMode:'ReturnAsStream',traceConfig};
      if(variant==='production'||variant==='category-split')Object.assign(start,{streamFormat:'json',streamCompression:'none',screenshotMaxSize:500,screenshotMaxCount:60});
      await cdp.send('Tracing.start',start);
      await page.goto(url,{waitUntil:'load'});
      await cdp.send('Tracing.end');const ended=await finished;let bytes='';
      for(;;){const chunk=await cdp.send('IO.read',{handle:ended.stream,size:65536});assert(!chunk.base64Encoded);bytes+=chunk.data;if(chunk.eof)break;}
      await cdp.send('IO.close',{handle:ended.stream});
      fs.writeFileSync(path.join(root,'trace-start-'+variant+'.json'),bytes,{mode:0o600,flag:'wx'});
      const trace=JSON.parse(bytes),buffers=trace.metadata.trace_processor_stats.traced_buf;
      observations.push({variant,start,dataLossOccurred:ended.dataLossOccurred,bytes:Buffer.byteLength(bytes),revision:trace.metadata.revision,buffers});
      await browser.close();browser=null;
    }
  } catch(error){failure=error;}
  finally {
    try {await browser?.close();} finally {await utils.gracefullyCloseAll();}
    process.stdout.write(JSON.stringify({observations,ownedProcesses:utils.gracefullyCloseSet.size,failure:failure?.message}));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
