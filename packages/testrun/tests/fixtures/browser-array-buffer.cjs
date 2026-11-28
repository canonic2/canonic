// Unchanged pinned public SDK projection; internal injected ab tags are not a public byte ABI.
const assert=require('node:assert/strict'),path=require('node:path');
const [root,engine,mode]=process.argv.slice(2);
process.env.PLAYWRIGHT_BROWSERS_PATH=path.join(root,'browsers');
const type=require(path.join(root,'playwright-core-1.64.1/package'))[engine==='chrome'?'chromium':engine];
(async()=>{
 const options={headless:mode==='headless'};
 if(engine==='chrome')options.executablePath=path.join(root,'browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing');
 const browser=await type.launch(options);
 try{
  const page=await browser.newPage(),input=Uint8Array.from([0,255,128,65]).buffer;
  input.extra='Acme input';
  assert.deepEqual(await page.evaluate(value=>({tag:Object.prototype.toString.call(value),extra:value.extra}),input),{tag:'[object Object]',extra:'Acme input'});
  assert.deepEqual(await page.evaluate(()=>{globalThis.acmeBufferGetters=0;const b=new ArrayBuffer(4);b.extra=13;Object.defineProperty(b,'secret',{enumerable:true,get(){acmeBufferGetters++;throw Error('Acme getter');}});return b;}),{});
  assert.equal(await page.evaluate(()=>acmeBufferGetters),0);
  assert.deepEqual(await page.evaluate(()=>new ArrayBuffer(0)),{});
  assert.deepEqual(await page.evaluate(()=>{const b=new ArrayBuffer(2,{maxByteLength:8});if(!b.resizable)throw Error('Acme resizable baseline unavailable');return b;}),{});
  const graph=await page.evaluate(()=>{const b=new ArrayBuffer(2);return {a:b,b};});
  assert.deepEqual(graph,{a:{},b:{}});
  assert.notEqual(graph.a,graph.b);
  await page.setContent('<!doctype html><iframe></iframe>');
  assert.deepEqual(await page.evaluate(()=>new (document.querySelector('iframe').contentWindow.ArrayBuffer)(3)),{});
  const parent=await page.evaluateHandle(()=>({buffer:Uint8Array.from([0,255,128,65]).buffer}));
  const retained=await parent.getProperty('buffer');await parent.dispose();
  assert.deepEqual(await retained.jsonValue(),{});
  const bytes=await retained.evaluate(b=>new Uint8Array(b));
  assert.deepEqual(Array.from(bytes),[0,255,128,65]);bytes[0]=77;
  assert.deepEqual(await retained.evaluate(b=>Array.from(new Uint8Array(b))),[0,255,128,65]);
  const detach=()=>{const b=new ArrayBuffer(4);structuredClone(b,{transfer:[b]});return b;};
  assert.equal(await page.evaluate(detach),undefined);
  assert.equal(await page.evaluate(()=>{const b=new ArrayBuffer(4);structuredClone(b,{transfer:[b]});return {b};}),undefined);
  const detached=await page.evaluateHandle(detach);await assert.rejects(detached.jsonValue(),/detached/i);await detached.dispose();
  await page.exposeBinding('AcmeBufferBridge',(_source,value,reject)=>{
   assert.deepEqual(value,{});
   const result=new ArrayBuffer(2);result.extra=reject?'rejected':'resolved';
   if(reject)throw result;return result;
  });
  for(const reject of [false,true]){
   const result=await page.evaluate(async reject=>{const b=new ArrayBuffer(3);b.extra=99;try{return {rejected:false,value:await AcmeBufferBridge(b,reject)};}catch(value){return {rejected:true,value};}},reject);
   assert.deepEqual(result,{rejected:reject,value:{extra:reject?'rejected':'resolved'}});
  }
  await page.goto('data:text/html,<body>Acme new document</body>');await assert.rejects(retained.jsonValue());await retained.dispose();
  process.stdout.write(JSON.stringify({inputProjection:true,resultProjection:true,resizable:true,crossRealm:true,graphCopies:true,retained:true,detachedResult:true,detachedRead:true,bindingArgument:true,bindingResolve:true,bindingReject:true,navigationFence:true}));
 }finally{await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
