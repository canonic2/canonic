// Public SDK observations, including the second protocol serialization boundary.
const path=require('node:path'),assert=require('node:assert/strict');
const [root,engine,mode]=process.argv.slice(2);
process.env.PLAYWRIGHT_BROWSERS_PATH=path.join(root,'browsers');
const type=require(path.join(root,'playwright-core-1.64.1/package'))[engine==='chrome'?'chromium':engine];
const snapshot=value=>({type:typeof value,value:value===undefined?'Acme undefined':value});
(async()=>{
 const options={headless:mode==='headless'};
 if(engine==='chrome')options.executablePath=path.join(root,'browsers/chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing');
 const browser=await type.launch(options);
 try{
  const page=await browser.newPage(),report={};
  const cases={
   prototype:()=>Object.assign(Object.create({inherited:13}),{own:42}),
   class:()=>new(class Acme{constructor(){this.own=42}get inherited(){throw Error('Acme prototype getter')}}),
   map:()=>new Map([['Acme',42]]),
   mapOwn:()=>Object.assign(new Map([['Acme',42]]),{own:13}),
   set:()=>Object.assign(new Set([42]),{own:13}),
   function:()=>function Acme(){},
   symbol:()=>Symbol('Acme'),
   nested:()=>({fn:function Acme(){},symbol:Symbol('Acme'),own:42}),
   getter:()=>({get refused(){throw Error('Acme getter')},own:42}),
   enumerableJson:()=>({toJSON(){throw Error('Acme toJSON')},own:42}),
   hiddenJson:()=>Object.defineProperty({},'toJSON',{value(){return {own:42}}}),
   throwingJson:()=>Object.defineProperty({},'toJSON',{value(){throw Error('Acme toJSON')}}),
   selfJson:()=>Object.defineProperty({},'toJSON',{value(){return this}}),
  };
  for(const [name,expression] of Object.entries(cases)){
   try{report[name]={evaluate:snapshot(await page.evaluate(expression))};}catch(error){report[name]={evaluateError:error.message};}
   const handle=await page.evaluateHandle(expression);
   try{report[name].read=snapshot(await handle.jsonValue());}catch(error){report[name].readError=error.message;}
   await handle.dispose();
  }
  const graph=await page.evaluate(()=>{const m=new Map([['Acme',42]]);m.own=13;return {a:m,b:m}});
  report.mapIdentity={value:graph,same:graph.a===graph.b};
  const expected={prototype:{own:42},class:{own:42},map:{},mapOwn:{own:13},set:{own:13},function:undefined,symbol:undefined,nested:{fn:undefined,symbol:undefined,own:42},getter:{own:42},enumerableJson:{toJSON:{},own:42},hiddenJson:{own:42},throwingJson:{},selfJson:undefined};
  for(const [name,value] of Object.entries(expected))for(const route of ['evaluate','read']){
   assert.equal(report[name][route].type,typeof value,`${name}/${route}`);
   assert.deepEqual(report[name][route].value,value===undefined?'Acme undefined':value,`${name}/${route}`);
  }
  assert.equal(graph.a,graph.b);assert.deepEqual(graph,{a:{own:13},b:{own:13}});
  report.nestedKeys=Object.keys(report.nested.evaluate.value);
  assert.deepEqual(report.nestedKeys,['fn','symbol','own']);
  await page.exposeBinding('AcmeObjectBridge',(_source,value)=>{report.binding=snapshot(value);return {accepted:true};});
  assert.deepEqual(await page.evaluate(async()=>AcmeObjectBridge({map:Object.assign(new Map([['Acme',42]]),{own:13}),fn:function Acme(){},symbol:Symbol('Acme'),get refused(){throw Error('Acme getter')},own:42})),{accepted:true});
  assert.deepEqual(report.binding.value,{map:{own:13},fn:undefined,symbol:undefined,own:42});
  report.bindingKeys=Object.keys(report.binding.value);
  assert.deepEqual(report.bindingKeys,['map','fn','symbol','own']);
  process.stdout.write(JSON.stringify(report));
 }finally{await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
