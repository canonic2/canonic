// Exact owner-helper behavior through the unchanged pinned Chromium SDK.
const assert = require('node:assert/strict');
const http = require('node:http');
const {chromium} = require(process.argv[2]);
(async () => {
  const waiting=new Map();
  const gates=new Map();
  const server=http.createServer(async (request,response)=>{
    const url=new URL(request.url,'http://127.0.0.1');
    response.setHeader('Connection','close');
    response.setHeader('Cache-Control','no-store');
    if(url.pathname==='/ready') {
      waiting.get(url.searchParams.get('mode'))?.(Object.fromEntries(url.searchParams));
      response.writeHead(204);response.end();
    } else if(url.pathname==='/gate') {
      await gates.get(url.searchParams.get('mode'));
      response.writeHead(204);response.end();
    } else {response.setHeader('Content-Type','text/html');response.end('<!doctype html><button id=Acme>Acme</button>');}
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}`;
  let browser;
  let context;
  try {
    browser = await chromium.launch({headless:process.argv[3]==='headless',executablePath:process.argv[4]});
    context = await browser.newContext();
    const page = await context.newPage();
    await page.goto(origin);
    const handle = await page.$('#Acme');
    const install = async mode => {
      const ready=new Promise(resolve=>waiting.set(mode,resolve));
      let release;
      gates.set(mode,new Promise(resolve=>{release=resolve;}));
      await handle.evaluate((node,{mode,gateUrl,readyUrl}) => {
      globalThis.acmeOwnerReads=0;
      globalThis.acmeOwnerGesture=null;
      Object.defineProperty(node,'ownerDocument',{configurable:true,get(){
        globalThis.acmeOwnerReads++;
        globalThis.acmeOwnerGesture=navigator.userActivation.isActive;
        return {documentElement:mode==='reject'&&acmeOwnerReads>1 ? Promise.reject(new Error('Acme owner rejection')) : Promise.resolve(document.documentElement)};
      }});
      fetch(gateUrl).then(()=>{
        const before=navigator.userActivation.isActive;
        const popup=window.open('about:blank');if(popup)popup.close();
        fetch(readyUrl+'&before='+Number(before)+'&opened='+Number(!!popup)+'&active='+Number(navigator.userActivation.isActive));
      });
    },{mode,gateUrl:`${origin}/gate?mode=${mode}`,readyUrl:`${origin}/ready?mode=${mode}`});
      release();
      let timeout;
      try {
        const deadline=new Promise((_,reject)=>{timeout=setTimeout(()=>reject(new Error('Acme activation signal expired')),5000);});
        const observation=await Promise.race([ready,deadline]);
        assert.equal(observation.active,'0',JSON.stringify(observation));
      } finally {clearTimeout(timeout);}
    };
    await install('fulfill');
    assert.equal(await handle.ownerFrame(),page.mainFrame());
    const fulfilled=await page.evaluate(()=>({reads:acmeOwnerReads,gesture:acmeOwnerGesture}));
    assert.deepEqual(fulfilled,{reads:2,gesture:true});
    await install('reject');
    await assert.rejects(()=>handle.ownerFrame(),/Acme owner rejection/);
    const rejected=await page.evaluate(()=>({reads:acmeOwnerReads,gesture:acmeOwnerGesture}));
    assert.deepEqual(rejected,{reads:2,gesture:true});
    await handle.evaluate(node=>delete node.ownerDocument);
    await handle.dispose();
    process.stdout.write(JSON.stringify({fulfilled,rejected,activationConsumed:true}));
  } finally {
    try {if(context)await context.close();}
    finally {
      try {if(browser)await browser.close();}
      finally {await new Promise(resolve=>server.close(resolve));}
    }
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
