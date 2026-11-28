// Unchanged pinned SDK worker header/credential inheritance on owned loopback.
const http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const engine=process.argv[3],headless=process.argv[4]==='headless';
const script="self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));";
const within=async promise=>{let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Acme worker defaults reference deadline')),10000))]);}finally{clearTimeout(timer);}};
(async()=>{
  const wire=[];
  const server=http.createServer((request,response)=>{
    const pathname=new URL(request.url,'http://localhost').pathname;
    if(pathname==='/acme-sw.js'){
      response.writeHead(200,{'Content-Type':'application/javascript','Cache-Control':'no-store'});response.end(script);return;
    }
    if(pathname.startsWith('/wire/')){
      const wanted=pathname==='/wire/auth-first'?'first':'second';
      const authorized=request.headers.authorization===`Basic ${Buffer.from('Acme:'+wanted).toString('base64')}`;
      const auth=pathname.startsWith('/wire/auth-');
      const row={path:pathname,context:request.headers['x-acme-context']??null,page:request.headers['x-acme-page']??null,authorizationPresent:!!request.headers.authorization,authorized};wire.push(row);
      if(auth&&!authorized){response.writeHead(401,{'WWW-Authenticate':`Basic realm="Acme ${pathname}"`,'Cache-Control':'no-store'});response.end('Acme unauthorized');return;}
      response.writeHead(200,{'Content-Type':'application/json','Cache-Control':'no-store'});response.end(JSON.stringify(row));return;
    }
    response.writeHead(200,{'Content-Type':'text/html','Cache-Control':'no-store'});response.end('<!doctype html><title>Acme worker defaults</title>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}`;
  const report={engine,mode:headless?'headless':'headed',wire,cases:[]};let browser,failure;
  try{
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    for(const name of ['headers','credentials']){
      const context=await browser.newContext(name==='headers'?{extraHTTPHeaders:{'X-Acme-Context':'initial'}}:{httpCredentials:{username:'Acme',password:'first',origin}});
      const row={name,stage:'start'};report.cases.push(row);
      try{
        const page=await context.newPage();await page.setExtraHTTPHeaders({'X-Acme-Page':'page-only'});await page.goto(origin);
        row.registration=await page.evaluate(async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;});
        row.workerCount=context.serviceWorkers().length;
        if(!row.workerCount){row.stage='no-sdk-worker-owner';continue;}
        const worker=context.serviceWorkers()[0];await page.close();
        const fetch=pathname=>within(worker.evaluate(async pathname=>{const response=await fetch(pathname);return {status:response.status,text:await response.text()};},pathname));
        if(name==='headers'){
          row.initial=await fetch('/wire/headers-initial');
          await context.setExtraHTTPHeaders({'X-Acme-Context':'changed'});
          row.changed=await fetch('/wire/headers-changed');
          await context.setExtraHTTPHeaders({});row.cleared=await fetch('/wire/headers-cleared');
        }else{
          row.initial=await fetch('/wire/auth-first');
          await context.setHTTPCredentials({username:'Acme',password:'second',origin});
          row.changed=await fetch('/wire/auth-second');
          await context.setHTTPCredentials(null);row.cleared=await fetch('/wire/auth-cleared');
        }
        row.stage='complete';
      }finally{await context.close();}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message,stack:error.stack};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    await new Promise(resolve=>server.close(resolve));report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
