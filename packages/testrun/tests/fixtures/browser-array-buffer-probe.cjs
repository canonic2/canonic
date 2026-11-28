// Diagnostic projection only; records actual public SDK behavior without an ABI assumption.
const path=require('node:path');
const [root,engine,mode]=process.argv.slice(2);
process.env.PLAYWRIGHT_BROWSERS_PATH=path.join(root,'browsers');
const type=require(path.join(root,'playwright-core-1.64.1/package'))[engine==='chrome'?'chromium':engine];
const snapshot=value=>({tag:Object.prototype.toString.call(value),keys:value&&typeof value==='object'?Object.keys(value):[],bytes:value instanceof ArrayBuffer?Array.from(new Uint8Array(value)):null});
(async()=>{
 const options={headless:mode==='headless'};
 if(engine==='chrome')options.executablePath=path.join(root,'browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing');
 const browser=await type.launch(options);
 try{
  const page=await browser.newPage(),input=Uint8Array.from([0,255,128,65]).buffer,report={};
  report.input=await page.evaluate(value=>({tag:Object.prototype.toString.call(value),keys:Object.keys(value),arrayBuffer:value instanceof ArrayBuffer}),input);
  report.output=snapshot(await page.evaluate(()=>Uint8Array.from([0,255,128,65]).buffer));
  const handle=await page.evaluateHandle(()=>Uint8Array.from([0,255,128,65]).buffer);
  report.read=snapshot(await handle.jsonValue());
  report.detached=await page.evaluate(()=>{const b=new ArrayBuffer(4);structuredClone(b,{transfer:[b]});return b;});
  const detached=await page.evaluateHandle(()=>{const b=new ArrayBuffer(4);structuredClone(b,{transfer:[b]});return b;});
  try{report.detachedRead={succeeded:true,value:snapshot(await detached.jsonValue())};}catch(error){report.detachedRead={succeeded:false,name:error.name,message:error.message};}
  await page.exposeBinding('AcmeProbe',(_source,value,reject)=>{
   report.bindingArgument=snapshot(value);
   const result=Uint8Array.from([9,8,7]).buffer;
   if(reject)throw result;
   return result;
  });
  for(const reject of [false,true]){
   report[reject?'bindingReject':'bindingResolve']=await page.evaluate(async reject=>{
    const input=Uint8Array.from([0,255,128,65]).buffer;
    try{const v=await AcmeProbe(input,reject);return{rejected:false,tag:Object.prototype.toString.call(v),keys:Object.keys(v),bytes:v instanceof ArrayBuffer?Array.from(new Uint8Array(v)):null};}
    catch(v){return{rejected:true,tag:Object.prototype.toString.call(v),keys:Object.keys(v),bytes:v instanceof ArrayBuffer?Array.from(new Uint8Array(v)):null};}
   },reject);
  }
  await handle.dispose();await detached.dispose();
  process.stdout.write(JSON.stringify(report));
 }finally{await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
