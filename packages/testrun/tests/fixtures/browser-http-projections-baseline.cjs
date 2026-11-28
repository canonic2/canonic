// Pinned APIResponse header objects and ECMAScript JSON values over owned loopback HTTP.
const assert=require('node:assert/strict'),http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const unicode=unit=>'\\'+'u'+unit.toString(16).padStart(4,'0');
const text=value=>Buffer.from(value,'utf8');
const cases=[
  ['primitives',text('{"Acme":[null,true,false,1,1.5,"text"]}')],
  ['integer-rounding',text('[9007199254740993,9007199254740995,18446744073709551615,-9007199254740993]')],
  ['overflow-underflow',text('[1e400,-1e400,1e-400,-1e-400,-0,0.0]')],
  ['surrogates',text('["'+unicode(0xd800)+'","'+unicode(0xdc00)+'","'+unicode(0xd83d)+unicode(0xdca9)+'","💩"]')],
  ['surrogate-keys',text('{"'+unicode(0xd800)+'":1,"'+unicode(0xdc00)+'":2,"'+unicode(0xd83d)+unicode(0xdca9)+'":3,"💩":4}')],
  ['key-order',text('{"b":1,"2":2,"1":1,"00":0,"4294967294":4,"4294967295":5,"a":2,"b":3,"__proto__":{"Acme":true}}')],
  ['utf8-invalid-string',Buffer.from([34,0xe0,0x80,0x80,0xed,0xa0,0x80,0xf0,0x9f,34])],
  ['utf8-invalid-key',Buffer.from([123,34,0xff,34,58,49,125])],
  ['whitespace',text(' \r\n\t [1] \n')],
  ['bom',Buffer.concat([Buffer.from([0xef,0xbb,0xbf]),text('{}')])],
  ['literal-infinity',text('Infinity')],
  ['literal-nan',text('NaN')],
  ['leading-zero',text('01')],
  ['missing-fraction',text('1.')],
  ['missing-exponent',text('1e+')],
  ['trailing-comma',text('[1,]')],
  ['raw-control',Buffer.from([34,10,34])],
  ['trailing-data',text('{} true')],
];
function string(value) {
  const units=Array.from({length:value.length},(_,i)=>value.charCodeAt(i));
  return {kind:'string',units};
}
function value(input) {
  const nodes=[];let root;
  const pending=[{input,assign:id=>{root=id;}}];
  while(pending.length) {
    const item=pending.pop(),input=item.input,id=nodes.length;
    nodes.push(null);item.assign(id);
    if(input===null) {nodes[id]={kind:'null'};continue;}
    if(typeof input==='boolean') {nodes[id]={kind:'boolean',value:input};continue;}
    if(typeof input==='number') {
      const number=Object.is(input,-0)?'negative_zero':input===Infinity?'positive_infinity':input===-Infinity?'negative_infinity':input;
      nodes[id]={kind:'number',value:number};continue;
    }
    if(typeof input==='string') {nodes[id]=string(input);continue;}
    if(Array.isArray(input)) {
      const items=new Array(input.length);nodes[id]={kind:'array',items};
      for(let i=input.length-1;i>=0;i--) pending.push({input:input[i],assign:child=>{items[i]=child;}});
      continue;
    }
    const keys=Object.keys(input),entries=keys.map(key=>({key:string(key).units,value:null}));
    nodes[id]={kind:'object',entries};
    for(let i=keys.length-1;i>=0;i--) pending.push({input:input[keys[i]],assign:child=>{entries[i].value=child;}});
  }
  return {root,nodes};
}
let requests=0;
const server=http.createServer((request,response)=>{
  const index=Number(request.url.slice(1));assert(Number.isInteger(index)&&index>=0&&index<cases.length);
  requests++;response.sendDate=false;
  response.writeHead(200,'Acme',[
    'X-Acme','one','x-acme','two','Set-Cookie','AcmeA=one; Path=/','set-cookie','AcmeB=two; Path=/',
    '__proto__','Acme-proto','Constructor','Acme-ctor','2','two','1','one','X-Latin',String.fromCharCode(255),
    'Content-Length',String(cases[index][1].length),'Content-Type','application/json','Connection','close'
  ]);response.end(cases[index][1]);
});
async function exercise(client,url) {
  const rows=[];
  for(let i=0;i<cases.length;i++) {
    const response=await client.get(url+'/'+i),headers=response.headers(),headersArray=response.headersArray();
    assert.equal(headers['x-acme'],'one, two');
    assert.equal(headers['set-cookie'],'AcmeA=one; Path=/\nAcmeB=two; Path=/');
    assert.equal(Object.hasOwn(headers,'__proto__'),false);assert.equal(headers.constructor,'Acme-ctor');
    const bytes=await response.body(),decoded=await response.text();
    assert.deepEqual(bytes,cases[i][1]);assert.equal(decoded,bytes.toString('utf8'));
    let json;
    try {json={ok:true,value:value(await response.json())};}
    catch(error) {json={ok:false,name:error.name};assert.equal(error.name,'SyntaxError');}
    rows.push({name:cases[i][0],headers,headerKeys:Object.keys(headers),headersArray,text:decoded,json});
    await response.dispose();
  }
  return rows;
}
(async()=>{
  let browser,failure;const report={};
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const url='http://127.0.0.1:'+server.address().port;
  try {
    const isolated=await playwright.request.newContext();
    try {report.isolated=await exercise(isolated,url);}finally{await isolated.dispose();}
    const chrome=process.argv[3]==='chrome';
    browser=await playwright[chrome?'chromium':process.argv[3]].launch({
      headless:process.argv[4]==='headless',
      ...(chrome?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})
    });
    const context=await browser.newContext();
    try {report.associated=await exercise(context.request,url);}finally{await context.close();}
    assert.equal(requests,cases.length*2);report.requests=requests;
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally {
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
