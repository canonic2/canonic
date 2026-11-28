// Pinned SDK HAR capture ownership and coexistence, on owned loopback only.
const http=require('node:http'),path=require('node:path'),fs=require('node:fs');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const engine=process.argv[3],headless=process.argv[4]==='headless';
const script="self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));";
const within=async promise=>{let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Acme worker capture reference deadline')),10000))]);}finally{clearTimeout(timer);}};
(async()=>{
  const wire=[];
  const server=http.createServer((request,response)=>{
    const pathname=new URL(request.url,'http://localhost').pathname;
    if(pathname==='/acme-sw.js'){response.writeHead(200,{'Content-Type':'application/javascript','Cache-Control':'no-store'});response.end(script);return;}
    if(pathname.startsWith('/wire/')){
      const authorized=request.headers.authorization===`Basic ${Buffer.from('Acme:first').toString('base64')}`;
      wire.push({path:pathname,context:request.headers['x-acme-context']??null,authorizationPresent:!!request.headers.authorization,authorized});
      if(pathname==='/wire/auth-first'&&!authorized){response.writeHead(401,{'WWW-Authenticate':'Basic realm="Acme capture"','Cache-Control':'no-store'});response.end('Acme unauthorized');return;}
      response.writeHead(200,{'Content-Type':'text/plain','Cache-Control':'no-store'});response.end(pathname==='/wire/auth-first'?'Acme authenticated':'Acme wire '+pathname);return;
    }
    response.writeHead(200,{'Content-Type':'text/html','Cache-Control':'no-store'});response.end('<!doctype html><title>Acme worker capture</title>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}`;
  const report={engine,mode:headless?'headless':'headed',wire,cases:[]};let browser,failure;
  try{
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    for(const name of ['future-full','existing-minimal','page-excluded','coexistence']){
      const file=path.join(process.cwd(),`worker-capture-${engine}-${headless?'headless':'headed'}-${name}.har`);
      const options={extraHTTPHeaders:{'X-Acme-Context':'Acme capture'},...(name==='coexistence'?{httpCredentials:{username:'Acme',password:'first',origin}}:{})};
      if(name==='future-full')options.recordHar={path:file,mode:'full',content:'embed',urlFilter:'**/wire/*'};
      const context=await browser.newContext(options),row={name,stage:'start',file};report.cases.push(row);
      try{
        const page=await context.newPage();await page.goto(origin);
        row.registration=await page.evaluate(async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;});
        row.workerCount=context.serviceWorkers().length;
        if(!row.workerCount){row.stage='no-sdk-worker-owner';continue;}
        const worker=context.serviceWorkers()[0];
        if(name==='page-excluded')await page.routeFromHAR(file,{update:true,updateMode:'full',updateContent:'embed',url:'**/wire/*'});
        else {
          await page.close();
          if(name!=='future-full')await context.routeFromHAR(file,{update:true,updateMode:name==='existing-minimal'?'minimal':'full',updateContent:'embed',url:'**/wire/*'});
        }
        const fetch=pathname=>within(worker.evaluate(pathname=>fetch(pathname).then(response=>response.text()),pathname));
        if(name==='coexistence'){
          const replay=path.join(process.cwd(),`worker-replay-${engine}-${headless?'headless':'headed'}.har`);
          fs.writeFileSync(replay,JSON.stringify({log:{version:'1.2',entries:[{request:{url:origin+'/wire/replay-hit',method:'GET',headers:[]},response:{status:200,headers:[{name:'Content-Type',value:'text/plain'}],content:{text:'Acme archive'}}}]}}),{flag:'wx',mode:0o600});
          await context.routeFromHAR(replay,{url:'**/wire/replay*',notFound:'fallback'});
          row.replayed=await fetch('/wire/replay-hit');row.authenticated=await fetch('/wire/auth-first');
        }else{row.fetched=await fetch('/wire/captured');}
        await context.close();
        const archive=JSON.parse(fs.readFileSync(file,'utf8'));
        row.entries=archive.log.entries.map(entry=>({path:new URL(entry.request.url).pathname,pageref:entry.pageref??null,workerRef:entry._serviceWorkerRef??null,status:entry.response.status,contextHeader:entry.request.headers.find(header=>header.name.toLowerCase()==='x-acme-context')?.value??null,body:entry.response.content.text??null,encoding:entry.response.content.encoding??null}));
        row.pageCount=archive.log.pages?.length??null;row.stage='complete';
      }finally{await context.close();}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message,stack:error.stack};}
  finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    await new Promise(resolve=>server.close(resolve));report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
