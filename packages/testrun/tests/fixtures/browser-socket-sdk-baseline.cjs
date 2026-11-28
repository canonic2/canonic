// Unchanged SDK wrapper semantics over owned HTTP/WebSocket loopback endpoints.
const http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const {wsServer:WebSocketServer,ws:WebSocket}=require(process.argv[2]+'/lib/utilsBundle.js');
const wait=async predicate=>{const end=Date.now()+5000;while(!predicate()){if(Date.now()>=end)throw Error('Acme socket checkpoint did not settle');await new Promise(resolve=>setTimeout(resolve,5));}};
(async()=>{
  const httpServer=http.createServer((request,response)=>{response.writeHead(200,{'Content-Type':'text/html'});response.end('<!doctype html><title>Acme socket SDK</title>');});
  const peers=new Map(),messages=new Map(),closed=new Map();
  const sockets=new WebSocketServer({server:httpServer});
  sockets.on('connection',(socket,request)=>{
    const name=request.url.slice(1);peers.set(name,socket);messages.set(name,[]);
    socket.on('message',(data,isBinary)=>{messages.get(name).push({isBinary,data:isBinary?[...data]:data.toString()});socket.send(data,{binary:isBinary});});
    socket.on('close',(code,reason)=>closed.set(name,{code,reason:reason.toString()}));
    socket.send('Acme-open');
  });
  await new Promise(resolve=>httpServer.listen(0,'127.0.0.1',resolve));
  const port=httpServer.address().port,origin=`http://127.0.0.1:${port}`;
  const engine=process.argv[3],headless=process.argv[4]==='headless';let browser;
  const report=[];
  try {
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    for(const name of ['default','replace','close','mock','close-default','server-only']){
      const context=await browser.newContext();const page=await context.newPage();const events=[];let route,server;
      const url=`ws://127.0.0.1:${port}/${name}`;
      try {
        await page.goto(origin);
        await page.routeWebSocket(url,ws=>{
          route=ws;if(name==='mock')return;
          server=ws.connectToServer();
          if(name==='replace'){
            ws.onMessage(()=>events.push('old-page-message'));
            ws.onMessage(message=>{events.push('page-message');server.send(message);});
            server.onMessage(()=>events.push('old-server-message'));
            server.onMessage(message=>{events.push('server-message');ws.send(message);});
          }
          if(name==='close'){
            ws.onClose(()=>events.push('old-page-close'));
            ws.onClose((code,reason)=>events.push({event:'page-close',code,reason}));
            server.onClose(()=>events.push('old-server-close'));
            server.onClose((code,reason)=>events.push({event:'server-close',code,reason}));
          }
          if(name==='server-only')server.onClose((code,reason)=>events.push({event:'server-close',code,reason}));
        });
        await page.evaluate(url=>new Promise((resolve,reject)=>{
          globalThis.acmeEvents=[];const socket=globalThis.acmeSocket=new WebSocket(url);socket.binaryType='arraybuffer';
          socket.onmessage=event=>acmeEvents.push(typeof event.data==='string'?{type:'text',data:event.data}:{type:'binary',data:[...new Uint8Array(event.data)]});
          socket.onclose=event=>acmeEvents.push({type:'close',code:event.code,reason:event.reason,wasClean:event.wasClean});
          socket.onerror=()=>reject(Error('Acme socket error'));socket.onopen=()=>resolve();
        }),url);
        if(name!=='mock')await page.waitForFunction(()=>acmeEvents.some(event=>event.type==='text'&&event.data==='Acme-open'));
        if(name==='server-only'){
          await server.close({code:3002,reason:'Acme server'});
          await wait(()=>events.some(event=>event.event==='server-close'));
          const readyAfterDiscardedSend=await page.evaluate(()=>{acmeSocket.send('Acme after server close');return acmeSocket.readyState;});
          route.send('Acme survivor');await page.waitForFunction(()=>acmeEvents.length===2);
          await route.close({code:3003,reason:'Acme final'});await page.waitForFunction(()=>acmeSocket.readyState===WebSocket.CLOSED);
          await wait(()=>closed.has(name));
          report.push({name,readyAfterDiscardedSend,events,page:await page.evaluate(()=>acmeEvents),server:messages.get(name),serverClosed:closed.get(name),connected:true});
        }else if(name==='close'){
          await page.evaluate(()=>acmeSocket.close(3001,'Acme page'));
          await wait(()=>events.some(event=>event.event==='page-close'));
          const heldServerOpen=peers.get(name).readyState===WebSocket.OPEN;
          await server.close({code:3002,reason:'Acme server'});
          await wait(()=>events.some(event=>event.event==='server-close'));
          const heldPageClosing=await page.evaluate(()=>acmeSocket.readyState===WebSocket.CLOSING);
          await route.close({code:3003,reason:'Acme final'});
          await page.waitForFunction(()=>acmeSocket.readyState===WebSocket.CLOSED);
          report.push({name,heldServerOpen,heldPageClosing,events,page:await page.evaluate(()=>acmeEvents),serverClosed:closed.get(name)});
        }else{
          if(name!=='mock'){
            await page.evaluate(()=>{acmeSocket.send('Acme-first');acmeSocket.send(new Uint8Array([0,17,255]));});
            await page.waitForFunction(()=>acmeEvents.length===3);
            if(name==='replace'){
              route.onMessage(null);server.onMessage(null);
              await page.evaluate(()=>acmeSocket.send('Acme-default-restored'));
              await page.waitForFunction(()=>acmeEvents.length===4);
            }
          }else await page.evaluate(()=>acmeSocket.send('Acme-discarded'));
          if(name==='close-default')await route.close({code:3004,reason:'Acme route'});
          else await page.evaluate(()=>acmeSocket.close(3001,'Acme page'));
          await page.waitForFunction(()=>acmeSocket.readyState===WebSocket.CLOSED);
          if(name!=='mock')await wait(()=>closed.has(name));
          report.push({name,events,page:await page.evaluate(()=>acmeEvents),server:messages.get(name)??[],serverClosed:closed.get(name)??null,connected:peers.has(name)});
        }
      }finally{await context.close();}
    }
  }finally{
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    await new Promise(resolve=>sockets.close(resolve));await new Promise(resolve=>httpServer.close(resolve));
  }
  process.stdout.write(JSON.stringify({cases:report,ownedProcesses:utils.gracefullyCloseSet.size}));
})().catch(error=>{console.error(error);process.exitCode=1;});
