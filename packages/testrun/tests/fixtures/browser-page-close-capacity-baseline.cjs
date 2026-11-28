// Unchanged pinned SDK: acknowledged page closure preserves its context owners.
const http=require('node:http'),path=require('node:path'),assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const headless=process.argv[3]==='headless',cycles=65;
const settle=promise=>promise.then(value=>({ok:true,value}),error=>({ok:false,name:error.name,message:error.message}));
const within=async promise=>{let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Acme page-close reference deadline')),10000))]);}finally{clearTimeout(timer);}};
(async()=>{
  let wire=0;
  const server=http.createServer((request,response)=>{
    if(request.url==='/acme-sw.js'){response.writeHead(200,{'Content-Type':'application/javascript','Cache-Control':'no-store'});response.end("self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));");}
    else if(request.url==='/wire/held'){wire++;response.end('Acme unexpected wire');}
    else{response.writeHead(200,{'Content-Type':'text/html'});response.end('<!doctype html><title>Acme page lifetime</title>');}
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}`;
  const report={engine:'chrome',mode:headless?'headless':'headed',cycles,closed:[],wire:0};let browser,failure;
  try{
    browser=await playwright.chromium.launch({headless,executablePath:path.join(process.argv[4],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')});
    const context=await browser.newContext();context.setDefaultTimeout(10000);
    try{
      const sibling=await context.newPage();await sibling.goto(origin);
      assert.equal(await sibling.evaluate(async()=>{await navigator.serviceWorker.register('/acme-sw.js');await navigator.serviceWorker.ready;return true;}),true);
      const worker=context.serviceWorkers().find(worker=>worker.url()===origin+'/acme-sw.js');assert.ok(worker);
      let workerClosed=0;worker.on('close',()=>workerClosed++);
      for(let index=0;index<cycles;index++){
        const page=await context.newPage();let arrived,release,finished,late,pageClosed=0;
        const routed=new Promise(resolve=>arrived=resolve),barrier=new Promise(resolve=>release=resolve),done=new Promise(resolve=>finished=resolve);
        try{
          await page.route('**/wire/held',async route=>{arrived();await barrier;late=await settle(route.fulfill({body:'Acme late'}));finished();});
          await page.goto(origin);page.on('close',()=>pageClosed++);
          const pending=settle(page.evaluate(()=>fetch('/wire/held').then(response=>response.text())));
          await within(routed);assert.equal(pageClosed,0);await page.close();
          const result=await within(pending);release();await within(done);
          assert.equal(result.ok,false);assert.match(result.message,/Target page, context or browser has been closed/);
          assert.equal(late.ok,true);assert.equal(pageClosed,1);assert.equal(workerClosed,0);
          const siblingValue=await sibling.evaluate(()=>42),workerValue=await worker.evaluate(()=>42);
          assert.equal(siblingValue,42);assert.equal(workerValue,42);
          report.closed.push({index,pending:result,lateReply:late,pageCloseEvents:pageClosed,workerCloseEvents:workerClosed,sibling:siblingValue,worker:workerValue});
        }finally{release();await page.close();}
      }
      report.sibling=await sibling.evaluate(()=>42);report.worker=await worker.evaluate(()=>42);assert.equal(wire,0);
    }finally{await context.close();}
  }catch(error){failure=error;report.failure={name:error.name,message:error.message,stack:error.stack};}
  finally{try{await browser?.close();}finally{await utils.gracefullyCloseAll();}await new Promise(resolve=>server.close(resolve));report.wire=wire;report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));}
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
