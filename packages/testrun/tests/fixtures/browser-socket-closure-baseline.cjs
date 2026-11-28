// Unchanged SDK WebSocket routing for the 4.3 closure: default relay and mock,
// a lookbehind regex with an unmatched sibling, an unrouted worker socket and a
// 512 KiB relayed message. Arguments: playwright-core, engine, mode, browsers.
// Scenario page functions are shared with the installed checkpoint.
const http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]);
const {wsServer:WebSocketServer}=require(process.argv[2]+'/lib/utilsBundle.js');
const fixture=require('./browser-socket-closure-scenarios.json');
(async()=>{
  const httpServer=http.createServer((request,response)=>{
    const worker=request.url==='/socket-worker.js';
    response.writeHead(200,{'Content-Type':worker?'text/javascript':'text/html'});
    response.end(worker?fixture.worker:fixture.page);
  });
  const connections={};
  const sockets=new WebSocketServer({server:httpServer});
  sockets.on('connection',(socket,request)=>{
    const name=new URL(request.url,'http://127.0.0.1').pathname.slice(1);
    connections[name]=(connections[name]||0)+1;
    socket.on('message',(data,isBinary)=>socket.send(data,{binary:isBinary}));
    socket.send('Acme-open');
  });
  await new Promise(resolve=>httpServer.listen(0,'127.0.0.1',resolve));
  const port=httpServer.address().port,origin=`http://127.0.0.1:${port}`,base=`ws://127.0.0.1:${port}`;
  const engine=process.argv[3],headless=process.argv[4]==='headless';
  const report={cases:{}};let browser;
  try {
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    for(const scenario of fixture.scenarios){
      const context=await browser.newContext();const page=await context.newPage();
      try {
        await page.goto(origin);
        const matcher=scenario.regex?new RegExp(scenario.regex.source,scenario.regex.flags):`${base}/${scenario.routed}`;
        await page.routeWebSocket(matcher,ws=>{if(scenario.relay)ws.connectToServer();});
        const run=([source,argument])=>(0,eval)('('+source+')')(argument);
        report.cases[scenario.name]=JSON.parse(await page.evaluate(run,[scenario.page,{base,...scenario.argument}]));
      } finally { await context.close(); }
    }
  } finally { if(browser)await browser.close(); }
  report.connections=connections;
  // Every browser is closed; exit without waiting for idle loopback sockets.
  process.stdout.write(JSON.stringify(report),()=>process.exit(0));
})().catch(error=>{process.stderr.write(String(error.stack||error));process.exit(1);});
