// Pinned SDK close defaults and handler removal over owned loopback peers.
const http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const {wsServer:WebSocketServer,ws:WebSocket}=require(process.argv[2]+'/lib/utilsBundle.js');
const wait=async predicate=>{const end=Date.now()+5000;while(!predicate()){if(Date.now()>=end)throw Error('Acme close checkpoint did not settle');await new Promise(resolve=>setTimeout(resolve,5));}};
(async()=>{
  const httpServer=http.createServer((request,response)=>{response.writeHead(200,{'Content-Type':'text/html'});response.end('<!doctype html><title>Acme close ownership</title>');});
  const peers=new Map(),closed=new Map(),wireErrors=new Map();const sockets=new WebSocketServer({server:httpServer});
  sockets.on('connection',(socket,request)=>{const name=request.url.slice(1);peers.set(name,socket);wireErrors.set(name,[]);socket.on('error',error=>wireErrors.get(name).push({code:error.code,message:error.message}));socket.on('close',(code,reason)=>closed.set(name,{code,reason:reason.toString()}));socket.send('Acme-open');});
  await new Promise(resolve=>httpServer.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${httpServer.address().port}`,engine=process.argv[3],headless=process.argv[4]==='headless';let browser;
  const cases=[];
  try{
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    for(const name of ['page-default','page-empty','page-empty-reason','page-reason','server-default','server-empty','server-empty-reason','server-reason','remove-page','remove-server','pending-server-removal']){
      const context=await browser.newContext(),page=await context.newPage();let route,server,release;const events=[];
      try{
        await page.goto(origin);
        // A transparent observer preserves native arguments, results and exceptions.
        await page.evaluate(()=>{const Native=WebSocket;globalThis.AcmeNativeCloses=[];globalThis.WebSocket=class extends Native{close(...args){const call={arguments:args.map(value=>value===undefined?{kind:'undefined'}:{kind:typeof value,value}),error:null};AcmeNativeCloses.push(call);try{return super.close(...args);}catch(error){call.error=error.name;throw error;}}};});
        const url=`ws://127.0.0.1:${httpServer.address().port}/${name}`;
        await page.routeWebSocket(url,ws=>{
          route=ws;server=ws.connectToServer();
          if(name==='remove-page'){
            route.onClose(()=>events.push({event:'old-page-close'}));route.onClose(null);
            server.onClose((code,reason)=>events.push({event:'server-close',code,reason}));
          }
          if(name==='remove-server'){
            server.onClose(()=>events.push({event:'old-server-close'}));server.onClose(null);
            route.onClose((code,reason)=>events.push({event:'page-close',code,reason}));
          }
          if(name==='pending-server-removal')server.onClose(async(code,reason)=>{events.push({event:'server-close',code,reason});await new Promise(resolve=>release=resolve);events.push({event:'resolved'});});
        });
        await page.evaluate(url=>new Promise((resolve,reject)=>{globalThis.AcmeCloseEvents=[];const socket=globalThis.AcmeCloseSocket=new WebSocket(url);socket.onmessage=event=>{AcmeCloseEvents.push({type:'text',data:event.data});if(event.data==='Acme-open')resolve();};socket.onclose=event=>AcmeCloseEvents.push({type:'close',code:event.code,reason:event.reason,wasClean:event.wasClean});socket.onerror=()=>reject(Error('Acme socket error'));}),url);
        let heldPage=null,afterRemoval=null;
        if(name==='remove-page'){
          await page.evaluate(()=>AcmeCloseSocket.close(3001,'Acme page'));await wait(()=>events.length===1&&closed.has(name));
          heldPage=await page.evaluate(()=>AcmeCloseSocket.readyState);await route.close({code:3002,reason:'Acme final'});
        }else if(name==='remove-server'){
          await server.close({code:3001,reason:'Acme server'});await wait(()=>events.length===1&&closed.has(name));
        }else if(name==='pending-server-removal'){
          await server.close({code:3001,reason:'Acme server'});await wait(()=>events.length===1&&closed.has(name));
          server.onClose(null);heldPage=await page.evaluate(()=>AcmeCloseSocket.readyState);
          route.send('Acme survivor');await page.waitForFunction(()=>AcmeCloseEvents.length===2);
          release();await wait(()=>events.length===2);afterRemoval=await page.evaluate(()=>AcmeCloseSocket.readyState);
          await route.close({code:3003,reason:'Acme final'});
        }else{
          const endpoint=name.startsWith('page-')?route:server;
          if(name.endsWith('-default'))await endpoint.close();
          else await endpoint.close(name.endsWith('-empty')?{}:{reason:name.endsWith('-empty-reason')?'':'Acme reason'});
          await page.waitForFunction(()=>AcmeNativeCloses.length>0);
          const errors=await page.evaluate(()=>AcmeNativeCloses.some(call=>call.error));
          if(!errors)await wait(()=>closed.has(name));
        }
        if(!name.startsWith('server-')||await page.evaluate(()=>AcmeNativeCloses.every(call=>!call.error)))await page.waitForFunction(()=>AcmeCloseSocket.readyState===WebSocket.CLOSED);
        cases.push({name,events,heldPage,afterRemoval,...await page.evaluate(()=>({pageReady:AcmeCloseSocket.readyState,page:AcmeCloseEvents,nativeCalls:AcmeNativeCloses})),serverReady:peers.get(name).readyState,serverClosed:closed.get(name)??null,wireErrors:structuredClone(wireErrors.get(name))});
      }finally{await context.close();}
    }
  }finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    await new Promise(resolve=>sockets.close(resolve));await new Promise(resolve=>httpServer.close(resolve));
  }
  process.stdout.write(JSON.stringify({cases,ownedProcesses:utils.gracefullyCloseSet.size}));
})().catch(error=>{console.error(error);process.exitCode=1;});
