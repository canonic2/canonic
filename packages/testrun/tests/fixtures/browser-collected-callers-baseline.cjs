// Collected ordinary/nonmatching RunTo callers through the unchanged pinned SDK.
const http=require('node:http'),assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(predicate,label){const end=Date.now()+5000;for(;;){const result=predicate();if(result)return result;if(Date.now()>=end)throw Error('Acme '+label+' unavailable');await delay(10);}}
async function site(){
  let release;const ready=new Promise(resolve=>release=resolve);
  const server=http.createServer(async(request,response)=>{
    if(request.url==='/ready'){await ready;response.writeHead(200,{'Connection':'close'});response.end('Acme ready');return;}
    response.writeHead(200,{'Content-Type':'text/html','Connection':'close'});
    response.end('<!doctype html><link rel=icon href="data:,"><button id=first>Acme first</button><button id=waiting hidden disabled>Acme waiting</button><input id=count value=0 readonly><script>document.querySelector("#waiting").onclick=()=>document.querySelector("#count").value=String(Number(document.querySelector("#count").value)+1);(async()=>{await fetch("/ready");const button=document.querySelector("#waiting");button.hidden=false;button.disabled=false;})()</script>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  return {url:`http://127.0.0.1:${server.address().port}`,release,close:()=>new Promise(resolve=>server.close(resolve))};
}
(async()=>{
  let browser,failure;const report={engine:process.argv[3],mode:process.argv[4],cases:[]};
  try{
    browser=await playwright[process.argv[3]].launch({headless:process.argv[4]==='headless'});
    for(const runTo of [false,true]){
      const context=await browser.newContext(),web=await site();const row={name:runTo?'nonmatching-run-to':'ordinary'};report.cases.push(row);
      try{
        const page=await context.newPage();await page.goto(web.url);
        const debugger_=context.debugger,calls=new Map();
        debugger_.on('apicallsupdated',updates=>{for(const call of updates)calls.set(call.id,{...calls.get(call.id),...call});});
        await debugger_._enable();
        if(runTo){
          await debugger_.requestPause();const first=page.locator('#first').click();first.catch(()=>{});
          await until(()=>debugger_.pausedDetails(),'first pause');
          await debugger_.runTo({file:'Acme-future-caller.rs',line:30,column:3});await first;
        }
        const previous=new Set(calls.keys());let settled=false;
        const pending=page.locator('#waiting').click();pending.then(()=>settled=true,()=>settled=true);pending.catch(()=>{});
        const waiting=await until(()=>[...calls.values()].find(call=>!previous.has(call.id)&&call.status==='running'&&/click/i.test(call.title)),'collected caller');
        row.waiting={title:waiting.title,status:waiting.status};
        assert.equal(debugger_.pausedDetails(),null);assert.equal(settled,false);
        row.beforeReady=Number(await page.locator('#count').inputValue());assert.equal(row.beforeReady,0);
        web.release();await pending;assert.equal(debugger_.pausedDetails(),null);
        row.afterReady=Number(await page.locator('#count').inputValue());assert.equal(row.afterReady,1);
        await until(()=>calls.get(waiting.id)?.status==='success','terminal caller');row.terminal=calls.get(waiting.id).status;
      }finally{web.release();await context.close();await web.close();}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message,stack:error.stack};}
  finally{try{await browser?.close();}finally{await utils.gracefullyCloseAll();}report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));}
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
