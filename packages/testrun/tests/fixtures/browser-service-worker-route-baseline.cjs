// Unchanged pinned SDK service-worker routing; owned browsers and loopback only.
const http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const engine=process.argv[3],headless=process.argv[4]==='headless';
const script=`self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));
self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));
self.addEventListener('fetch',event=>{const url=new URL(event.request.url);if(url.pathname.startsWith('/controlled/'))event.respondWith(fetch('/wire/'+url.pathname.slice('/controlled/'.length)));});`;
const facts=request=>{
  let frame=null,frameError=null;
  try{frame=request.frame().url();}catch(error){frameError={name:error.name,message:error.message};}
  return {path:new URL(request.url()).pathname,worker:request.serviceWorker()!==null,frame,frameError};
};
const settle=promise=>promise.then(value=>({ok:true,value}),error=>({ok:false,name:error.name,message:error.message}));
const within=async promise=>{let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Acme worker reference deadline')),10000))]);}finally{clearTimeout(timer);}};
(async()=>{
  const wire=[];
  const server=http.createServer((request,response)=>{
    const pathname=new URL(request.url,'http://localhost').pathname;
    if(pathname==='/acme-sw.js'){
      response.writeHead(200,{'Content-Type':'application/javascript','Cache-Control':'no-store'});response.end(script);
    }else if(pathname.startsWith('/wire/')){
      wire.push(pathname);response.writeHead(200,{'Content-Type':'text/plain','Cache-Control':'no-store'});response.end('Acme wire '+pathname);
    }else{
      response.writeHead(200,{'Content-Type':'text/html','Cache-Control':'no-store'});response.end('<!doctype html><title>Acme worker routes</title>');
    }
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}`;
  const report={engine,mode:headless?'headless':'headed',cases:[],wire};let browser,failure;
  try{
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    for(const name of ['ownership-removal','context-close']){
      const context=await browser.newContext(),page=await context.newPage();context.setDefaultTimeout(10000);
      const row={name,stage:'start',events:[],pageEvents:[],handlerResults:[]};report.cases.push(row);
      let heldResolve,release,finished;const arrived=new Promise(resolve=>heldResolve=resolve),barrier=new Promise(resolve=>release=resolve),handlerFinished=new Promise(resolve=>finished=resolve);
      const handler=label=>async route=>{
        const request=route.request();row.events.push({handler:label,...facts(request),sameWorker:context.serviceWorkers().includes(request.serviceWorker())});
        if(new URL(request.url()).pathname==='/wire/held'){heldResolve();await barrier;}
        const result=await settle(route.fulfill({contentType:'text/plain',body:'Acme '+label}));row.handlerResults.push(result);
        if(new URL(request.url()).pathname==='/wire/held')finished();
      };
      const first=handler('first'),second=handler('second');
      try{
        await context.route('**/wire/*',first);
        for(const kind of ['wire','controlled'])await page.route(`**/${kind}/*`,async route=>{row.pageEvents.push({kind,...facts(route.request())});await route.fulfill({body:'Acme page '+kind});});
        await page.goto(origin);
        row.registration=await page.evaluate(async()=>{
          if(!navigator.serviceWorker)return {available:false};
          try{
            const registering=async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;if(!navigator.serviceWorker.controller)await new Promise(resolve=>navigator.serviceWorker.addEventListener('controllerchange',resolve,{once:true}));return {available:true,controlled:true};};
            return await Promise.race([registering(),new Promise(resolve=>setTimeout(()=>resolve({available:true,deadline:true}),8000))]);
          }catch(error){return {available:true,name:error.name,message:error.message};}
        });
        row.stage='registered';row.workerCount=context.serviceWorkers().length;
        if(!row.registration.controlled){row.stage='registration-not-controlled';continue;}
        row.first=await within(page.evaluate(()=>fetch('/controlled/first').then(response=>response.text())));
        const worker=context.serviceWorkers()[0];
        if(!worker){row.stage='no-sdk-worker-owner';continue;}
        row.workerUrl=new URL(worker.url()).pathname;let closed=0;worker.on('close',()=>closed++);
        await page.close();row.stage='page-closed';
        row.afterPageClose=await within(worker.evaluate(()=>fetch('/wire/after-page-close').then(response=>response.text())));
        if(name==='ownership-removal'){
          await context.route('**/wire/*',second);
          row.replaced=await within(worker.evaluate(()=>fetch('/wire/replaced').then(response=>response.text())));
          await context.unroute('**/wire/*',second);
          row.restored=await within(worker.evaluate(()=>fetch('/wire/restored').then(response=>response.text())));
          const pending=settle(worker.evaluate(()=>fetch('/wire/held').then(response=>response.text())));
          await within(arrived);
          let removed=false;const removing=context.unrouteAll({behavior:'wait'}).then(()=>removed=true);
          await new Promise(resolve=>setImmediate(()=>setImmediate(resolve)));row.removalWaited=!removed;
          release();row.held=await within(pending);await removing;
          row.afterRemoval=await within(worker.evaluate(()=>fetch('/wire/after-removal').then(response=>response.text())));
          row.workerClosedBeforeContext=closed;
          await context.close();row.workerClosedAfterContext=closed;
        }else{
          const pending=settle(worker.evaluate(()=>fetch('/wire/held').then(response=>response.text())));
          await within(arrived);await context.close();row.workerClosedAfterContext=closed;
          row.pending=await pending;release();
          // The callback retains its original route object after target teardown.
          await within(handlerFinished);
        }
        row.stage='complete';
      }finally{release();await context.close();}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message,stack:error.stack};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    await new Promise(resolve=>server.close(resolve));
    report.ownedProcesses=utils.gracefullyCloseSet.size;
    process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
