// Unchanged pinned HAR routing with context-owned service-worker requests.
const http=require('node:http'),path=require('node:path'),fs=require('node:fs');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const engine=process.argv[3],headless=process.argv[4]==='headless';
const script="self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));";
const settle=promise=>promise.then(value=>({ok:true,value}),error=>({ok:false,name:error.name,message:error.message}));
const within=async promise=>{let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Acme worker HAR reference deadline')),10000))]);}finally{clearTimeout(timer);}};
(async()=>{
  const wire=[];
  const server=http.createServer((request,response)=>{
    const pathname=new URL(request.url,'http://localhost').pathname;
    if(pathname==='/acme-sw.js'){
      response.writeHead(200,{'Content-Type':'application/javascript','Cache-Control':'no-store'});response.end(script);
    }else if(pathname.startsWith('/wire/')){
      wire.push(pathname);response.writeHead(200,{'Content-Type':'text/plain','Cache-Control':'no-store'});response.end('Acme wire '+pathname);
    }else{
      response.writeHead(200,{'Content-Type':'text/html','Cache-Control':'no-store'});response.end('<!doctype html><title>Acme worker HAR</title>');
    }
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}`;
  const file=path.join(process.cwd(),`worker-reference-${engine}-${headless?'headless':'headed'}.har`);
  fs.writeFileSync(file,JSON.stringify({log:{version:'1.2',entries:[{
    request:{url:origin+'/wire/hit',method:'GET',headers:[]},
    response:{status:200,headers:[{name:'Content-Type',value:'text/plain'}],content:{text:'Acme archive'}}
  }]}}),{flag:'wx',mode:0o600});
  const report={engine,mode:headless?'headless':'headed',wire,cases:[]};let browser,failure;
  try{
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    for(const name of ['automatic','ordered']){
      const context=await browser.newContext(),page=await context.newPage();context.setDefaultTimeout(10000);
      const row={name,stage:'start',events:[],pageEvents:[]};report.cases.push(row);
      const fetch=pathname=>within(settle(context.serviceWorkers()[0].evaluate(pathname=>fetch(pathname).then(response=>response.text()),pathname)));
      try{
        if(name==='automatic')await context.routeFromHAR(file,{url:'**/wire/*',notFound:'abort'});
        await page.route('**/wire/*',async route=>{row.pageEvents.push(new URL(route.request().url()).pathname);await route.fulfill({body:'Acme page policy'});});
        await page.goto(origin);
        row.registration=await page.evaluate(async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;});
        row.workerCount=context.serviceWorkers().length;
        if(!row.workerCount){row.stage='no-sdk-worker-owner';continue;}
        await page.close();
        if(name==='automatic'){
          row.hit=await fetch('/wire/hit');row.miss=await fetch('/wire/miss');
          row.filtered=await fetch('/wireless');
          await context.unrouteAll();
          await context.routeFromHAR(file,{url:'**/wire/*',notFound:'fallback'});
          row.fallback=await fetch('/wire/fallback');row.hitAgain=await fetch('/wire/hit');
          await context.unrouteAll();row.afterRemoval=await fetch('/wire/removed');
        }else{
          await context.route('**/wire/*',async route=>{row.events.push({handler:'oldest',path:new URL(route.request().url()).pathname,worker:route.request().serviceWorker()===context.serviceWorkers()[0]});await route.fulfill({body:'Acme oldest'});});
          await context.routeFromHAR(file,{url:'**/wire/*',notFound:'fallback'});
          await context.route('**/wire/*',async route=>{row.events.push({handler:'newest',path:new URL(route.request().url()).pathname,worker:route.request().serviceWorker()===context.serviceWorkers()[0]});await route.fallback();});
          row.hit=await fetch('/wire/hit');row.miss=await fetch('/wire/miss');
          await context.unrouteAll();row.afterRemoval=await fetch('/wire/removed');
        }
        row.stage='complete';
      }finally{await context.close();}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message,stack:error.stack};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    await new Promise(resolve=>server.close(resolve));
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
