import assert from 'node:assert/strict';
import { test } from 'node:test';
import { injectedSource,macEditingCommands,webSocketSource,storageSource } from './browser-cdp-helper.mjs';
import { readFile } from 'node:fs/promises';
import { runInNewContext } from 'node:vm';
import { createHash } from 'node:crypto';

const prefix = '// packages/playwright-core/src/generated/injectedScriptSource.ts\n';
const exportLine = 'module.exports = __toCommonJS(injectedScript_exports);';
const websocketExport='module.exports = __toCommonJS(webSocketMock_exports);';
test('native date fill sets a validated value without focusing the date picker', async () => {
  const source = await readFile(new URL('../src/browser/cdp/session/actions.js', import.meta.url), 'utf8');
  const action = runInNewContext(`(${source})`, { Event });
  const events = [];
  const input = {
    nodeName: 'INPUT', type: 'date', isConnected: true, value: '2026-01-01',
    ownerDocument: { createElement: () => ({
      set value(date) { this.valid = date === '2026-01-15' ? date : ''; },
      get value() { return this.valid; },
    }) },
    focus() { throw new Error('Date fill must not open the picker'); },
    dispatchEvent(event) { events.push(event.type); },
  };
  const injected = {
    retarget: () => input,
    checkElementStates: async () => undefined,
    fill() { throw new Error('Date fill must not type into the picker'); },
  };
  const fill = (stage, value) => action(injected, input, {stage, action: {kind:'fill', value}});
  assert.equal((await fill('prepare', '01/15/2026')).error, 'INVALID_DATE_VALUE');
  assert.equal(input.value, '2026-01-01');
  assert.deepEqual(events, []);
  assert.equal((await fill('prepare', '2026-01-15')).ready, true);
  assert.equal((await fill('commit', '2026-01-15')).done, true);
  assert.equal(input.value, '2026-01-15');
  assert.deepEqual(events, ['input', 'change']);
  assert.equal((await action(injected, input, {stage:'commit', action:{kind:'clear'}})).done, true);
  assert.equal(input.value, '');
});

test('storage helper extraction preserves literal codec bytes and refuses changed module boundaries', () => {
  const source="module.exports = __toCommonJS(storageScript_exports);\\nconst Acme = \\'private\\';\\n";
  const fixture=source=>bundle(source).replaceAll('injectedScriptSource','storageScriptSource');
  assert.equal(storageSource(fixture(source)),"module.exports = __toCommonJS(storageScript_exports);\nconst Acme = 'private';\n");
  assert.throws(()=>storageSource(''),/Missing pinned/);
  assert.throws(()=>storageSource(fixture(exportLine)),/Unexpected storage module/);
  assert.throws(()=>storageSource(fixture(source+'\\q')),/Unsupported storage/);
  assert.throws(()=>storageSource(fixture('module.exports = __toCommonJS(storageScript_exports);').replace("';","'; Acme();")),/suffix/);
});
test('pinned storage codec preserves selected fields and sorted OPFS bytes without probing omitted collectors', async () => {
  const source=await readFile(new URL('../src/browser/reading/storage.js',import.meta.url),'utf8');
  const manifest=JSON.parse(await readFile(new URL('./browser-provenance/cdp-storage.json',import.meta.url),'utf8'));
  assert.equal(createHash('sha256').update(source).digest('hex'),manifest.helperSha256);
  let databaseReads=0,fileReads=0;
  const file=bytes=>({kind:'file',getFile:async()=>({arrayBuffer:async()=>Uint8Array.from(bytes).buffer})});
  const directory=entries=>({kind:'directory',entries:async function*(){yield* entries;}});
  const root=directory([['z.bin',file([0,255,128])],['a',directory([['Acme.txt',file([65,99,109,101])]])]]);
  const localStorage={'Acme':'private','__pwCoverage.Acme':'excluded'};
  Object.defineProperty(localStorage,'getItem',{value:name=>localStorage[name]});
  const context={module:{exports:{}},localStorage,indexedDB:{databases:async()=>{databaseReads++;return [];}},navigator:{storage:{getDirectory:async()=>{fileReads++;return root;}}},btoa,atob};
  runInNewContext(source,context);
  const collect=options=>runInNewContext('new (module.exports.StorageScript())("chromium").collect(options)',{...context,options});
  const omitted=await collect({indexedDB:false,opfs:false});
  assert.equal(databaseReads,0);assert.equal(fileReads,0);
  // Compare the same plain JSON boundary that CDP returns to Rust.
  assert.deepEqual(JSON.parse(JSON.stringify(omitted)),{localStorage:[{name:'Acme',value:'private'}]});
  const selected=await collect({indexedDB:true,opfs:true});
  assert.equal(databaseReads,1);assert.equal(fileReads,1);
  assert.deepEqual(JSON.parse(JSON.stringify(selected)),{localStorage:[{name:'Acme',value:'private'}],indexedDB:[],opfs:[{path:'a',type:'directory'},{path:'a/Acme.txt',type:'file',base64:'QWNtZQ=='},{path:'z.bin',type:'file',base64:'AP+A'}]});
});
test('selector evaluation preserves first-match, strict, all-match and relative-root semantics', async () => {
  const source = await readFile(new URL('../src/browser/reading/selector-evaluation.js', import.meta.url), 'utf8');
  class AcmeElement { constructor() { this.children = [{name:'Acme first'}, {name:'Acme second'}]; } }
  const select = runInNewContext('(' + source + ')', {Element:AcmeElement});
  const root = new AcmeElement();
  const injected = {parseSelector: selector => selector, querySelectorAll: (selector, parent) => selector === '.missing' ? [] : parent.children};
  const options = {selector:'.acme', expression:'(element,arg)=>element.name+arg', isFunction:true, relative:true};
  assert.equal(await select(injected, root, options, '!'), 'Acme first!');
  await assert.rejects(select(injected, root, {...options,strict:true}, null), /AMBIGUOUS_LOCATOR/);
  await assert.rejects(select(injected, root, {...options,selector:'.missing'}, null), /ELEMENT_NOT_FOUND/);
  assert.equal(await select(injected, root, {...options,all:true,expression:'elements=>elements.length'}, null), 2);
  assert.equal(await select(injected, root, {...options,all:true,selector:'.missing',expression:'elements=>elements.length'}, null), 0);
  await assert.rejects(select(injected, {nodeType:1,children:root.children}, options, null), /INVALID_SCOPE/);
});
test('selector evaluation awaits functions, normalizes method forms and preserves expression mode', async () => {
  const source = await readFile(new URL('../src/browser/reading/selector-evaluation.js', import.meta.url), 'utf8');
  const context = {calls:0};
  const select = runInNewContext('(' + source + ')', context);
  const root = {nodeType:9,children:[{value:3}]};
  const injected = {parseSelector: selector=>selector, querySelectorAll: (_,parent)=>parent.children};
  const options = {selector:'.acme',expression:'async acme(element,arg) { return element.value + arg; }',isFunction:true};
  assert.equal(await select(injected,root,options,4),7);
  assert.equal(await select(injected,root,{...options,expression:'function (element) { return element.value; }'},null),3);
  assert.equal(await select(injected,root,{...options,expression:'17; 42',isFunction:false},null),42);
  assert.equal(await select(injected,root,{...options,expression:'element=>element.value',isFunction:null},null),3);
  await assert.rejects(select(injected,{...root,children:[]},{...options,expression:'()=>++calls'},null), /ELEMENT_NOT_FOUND/);
  assert.equal(context.calls,0);
});
test('selector attachment waits select strictly once and stop without calling user code on timeout or detached roots', async () => {
  const source = await readFile(new URL('../src/browser/reading/selector-evaluation.js', import.meta.url), 'utf8');
  class AcmeElement { constructor() { this.isConnected = true; } }
  let now = 0, polls = 0;
  const context = {Element:AcmeElement, performance:{now:()=>now}, setTimeout:resolve=>{now+=10;resolve();}, calls:0};
  const select = runInNewContext('(' + source + ')', context);
  const root = new AcmeElement(), node = {name:'Acme'};
  const injected = {parseSelector:s=>s, querySelectorAll:()=>++polls>=3?[node]:[]};
  const options = {selector:'.acme',expression:'element=>{calls++;return element;}',isFunction:true,relative:true,strict:true,waitForAttachment:true,waitTimeoutMs:50};
  assert.equal(await select(injected,root,options,null),node);
  assert.equal(context.calls,1);assert.equal(polls,3);
  const missing = {...injected,querySelectorAll:()=>[]};
  await assert.rejects(select(missing,root,options,null),/SELECTOR_WAIT_EXPIRED/);
  await assert.rejects(select({...injected,querySelectorAll:()=>[node,node]},root,options,null),/AMBIGUOUS_LOCATOR/);
  root.isConnected=false;
  await assert.rejects(select(injected,root,options,null),/SELECTOR_ROOT_DETACHED/);
  assert.equal(context.calls,1);
});
test('developer result projection annotates exact elements, preserves graphs and skips ordinary accessors',async()=>{
  const [source,reader]=await Promise.all([
    readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8'),
    readFile(new URL('../src/browser/reading/developer-result.js',import.meta.url),'utf8'),
  ]);
  const execute=body=>runInNewContext(`(()=>{class HTMLElement {constructor(){this.ownerDocument=document;this.isConnected=true;}}const document={};const codec=${source};const read=(${reader});const frame={id:'Acme frame'};let retained=0,getters=0;const references={retain(element,frame){retained++;return {kind:'element',frame,id:'Acme element'};}};${body}})()`,{TextEncoder,URL,btoa,atob});
  const result=execute("const node=new HTMLElement();const value={node,again:node,big:12n,get secret(){getters++;return 'Acme secret';}};value.self=value;const result=JSON.parse(read(value,frame,references,codec));return {result,retained,getters};");
  assert.equal(result.retained,1);assert.equal(result.getters,0);
  assert.equal(result.result.annotations.length,1);
  assert.deepEqual(Array.from(result.result.annotations[0].path),['node']);
  const entries=result.result.value.o;
  assert.equal(entries.find(entry=>entry.k==='big').v.bi,'12');
  assert.equal(entries.find(entry=>entry.k==='self').v.ref,result.result.value.id);
  assert.equal(entries.find(entry=>entry.k==='secret').v.o.find(entry=>entry.k==='kind').v.s,'accessor');
  assert.throws(()=>execute("const node=new HTMLElement();node.isConnected=false;return read(node,frame,references,codec);"),/DEVELOPER_TOOL_ELEMENT_SCOPE/);
  assert.throws(()=>execute("return read(Array.from({length:33},()=>new HTMLElement()),frame,references,codec);"),/VALUE_BOUNDS/);
  assert.throws(()=>execute("return read('x'.repeat(40000),frame,references,codec);"),/VALUE_BOUNDS/);
});
test('diagnostic argument reads preserve typed values and bound explicit property inspection',async()=>{
  const [source,reader]=await Promise.all([
    readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8'),
    readFile(new URL('../src/browser/reading/diagnostic-arguments.js',import.meta.url),'utf8'),
  ]);
  const execute=expression=>runInNewContext(`(()=>{const codec=${source};const read=(${reader})(codec);${expression}})()`,{TextEncoder,URL,btoa,atob});
  const typed=execute("return read({nested:{value:-0,big:12n}},{path:['nested'],properties:null});");
  assert.equal(typed.o.find(entry=>entry.k==='value').v.v,'-0');
  assert.equal(typed.o.find(entry=>entry.k==='big').v.bi,'12');
  const page=execute("return read({a:1,b:2,c:3},{path:[],properties:{offset:1,limit:1}});");
  assert.deepEqual(Array.from(page.o.find(entry=>entry.k==='names').v.a,item=>item.s),['b']);
  assert.equal(page.o.find(entry=>entry.k==='hasMore').v.b,true);
  assert.throws(()=>execute("return read('x'.repeat(40000),{path:[],properties:null});"),/VALUE_BOUNDS/);
  assert.equal(execute("let calls=0;const value={get nested(){calls++;return 17;}};read(value,{path:['nested'],properties:null});return calls;"),1);
});
function websocketBundle(source) {
  return '// packages/playwright-core/src/generated/webSocketMockSource.ts\nvar source13;\nvar init_webSocketMockSource = __esm({\n  "packages/playwright-core/src/generated/webSocketMockSource.ts"() {\n    "use strict";\n    source13 = `'+source+'`;\n  }\n});\n\n// next pinned module\n';
}
test('WebSocket source extraction preserves template syntax and escaped data without evaluating it',()=>{
  const encoded=websocketExport+'\\nfunction inject(globalThis) {}\\nconst value = \\`Acme \\${42}\\`;\\nconst name = "\\u0041\\x42";\\n';
  const expected=websocketExport+'\nfunction inject(globalThis) {}\nconst value = `Acme ${42}`;\nconst name = "AB";\n';
  assert.equal(webSocketSource(websocketBundle(encoded)),expected);
});
test('WebSocket extraction refuses dynamic interpolation and changed module boundaries',()=>{
  const source=websocketExport+'\\nfunction inject(globalThis) {}';
  assert.throws(()=>webSocketSource(''),/Missing pinned/);
  assert.throws(()=>webSocketSource(websocketBundle(source+'${untrusted}')),/Dynamic/);
  assert.throws(()=>webSocketSource(websocketBundle(source+'\\q')),/Unsupported/);
  assert.throws(()=>webSocketSource(websocketBundle(source).replace('`;','`;malicious();')),/suffix/);
  assert.throws(()=>webSocketSource(websocketBundle('function inject(globalThis) {}')),/Unexpected WebSocket module/);
  assert.throws(()=>webSocketSource(websocketBundle(source).replace('var source13;','var changed;')),/header/);
});
test('pinned WebSocket mock opens without a server, preserves binary offsets and releases closed IDs',async()=>{
  const source=await readFile(new URL('../src/browser/reading/websocket.js',import.meta.url),'utf8');
  const pin=JSON.parse(await readFile(new URL('./browser-provenance/cdp-websocket.json',import.meta.url),'utf8'));
  assert.equal(createHash('sha256').update(source).digest('hex'),pin.helperSha256);
  const events=[];let nativeConnections=0,serial=0;
  class FixtureCloseEvent extends Event {constructor(type,{code,reason,wasClean,...options}={}){super(type,options);this.code=code;this.reason=reason;this.wasClean=wasClean;}}
  const result=runInNewContext(`(()=>{${source}\nmodule.exports.inject()(globalThis);
    const socket=new WebSocket('/socket',['acme']);socket.binaryType='arraybuffer';const received=[];let closed=0;
    socket.addEventListener('message',event=>received.push(Array.from(new Uint8Array(event.data))));socket.addEventListener('close',()=>closed++);
    const id=socket._id;__pwWebSocketDispatch({id,type:'ensureOpened'});const opened=socket.readyState;
    socket.send(new Uint8Array([9,0,255,128,9]).subarray(1,4));
    __pwWebSocketDispatch({id,type:'sendToPage',data:{data:'AP+A',isBase64:true}});
    __pwWebSocketDispatch({id,type:'closePage',code:1000,reason:'Acme',wasClean:true});
    __pwWebSocketDispatch({id,type:'sendToPage',data:{data:'AQ==',isBase64:true}});
    return {opened,ready:socket.readyState,protocol:socket.protocol,closed,received};})()`,{
    module:{exports:{}},EventTarget,Event,MessageEvent,CloseEvent:FixtureCloseEvent,Blob,URL,Uint8Array,ArrayBuffer,DOMException,btoa,atob,
    window:{document:{baseURI:'https://example.com/acme/'}},crypto:{getRandomValues:bytes=>{bytes[0]=++serial;return bytes;}},
    WebSocket:class {constructor(){nativeConnections++;}},__pwWebSocketBinding:payload=>events.push(payload)
  });
  assert.equal(result.opened,1);assert.equal(result.ready,3);assert.equal(result.closed,1);assert.equal(result.protocol,'acme');
  assert.deepEqual(Array.from(result.received,bytes=>Array.from(bytes)),[[0,255,128]]);
  assert.equal(events[0].url,'wss://example.com/socket');assert.equal(events[0].protocols[0],'acme');
  assert.equal(events.find(event=>event.type==='onMessageFromPage').data.data,'AP+A');
  assert.equal(nativeConnections,0);
});
test('WebSocket protocol offers are validated before allocation and snapshotted before relay',async()=>{
  const [source,guard]=await Promise.all(['websocket.js','websocket-guard.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  let serial=0;const events=[],connections=[];
  class Native {constructor(url,protocols){this.readyState=0;this.protocol='Acme.v2';this.protocols=Array.from(protocols);connections.push(this);}}
  const result=runInNewContext(`(()=>{${source}\nmodule.exports.inject()(globalThis);
    globalThis.WebSocket=(${guard})(WebSocket,url=>url.startsWith('wss://example.com/selected'));
    const refused=[];
    for(const protocols of [[''],['Acme','Acme'],['Acme,other'],['Acme/other'],['Acme other'],['Acme\\n'],['東京'],Array(17).fill('Acme'),['x'.repeat(129)],[42]]){
      try{new WebSocket('/selected',protocols);throw Error('Invalid protocols accepted');}catch(error){refused.push(error.name);}
    }
    let fragment;try{new WebSocket('/selected#Acme');fragment='accepted';}catch(error){fragment=error.name;}
    const invalidAllocations=events.length;
    const offers=['Acme.v1','Acme.v2'];offers[Symbol.iterator]=()=>{throw Error('Protocol array iterator must not be executed');};
    const socket=new WebSocket('/selected',offers);offers[0]='Acme.changed';offers.push('Acme.extra');
    __pwWebSocketDispatch({id:socket._id,type:'connect'});socket._ws.readyState=1;socket._ws.onopen({});
    const allowed=new WebSocket('/selected',"!#$%&'*+-.^_\x60|~012Abc");
    __pwWebSocketDispatch({id:allowed._id,type:'ensureOpened'});
    const empty=new WebSocket('/selected');__pwWebSocketDispatch({id:empty._id,type:'ensureOpened'});
    return {refused,fragment,invalidAllocations,selected:socket.protocol,allowed:allowed.protocol,empty:empty.protocol};})()`,{
    module:{exports:{}},EventTarget,Event,MessageEvent,CloseEvent:class extends Event{},Blob,URL,Uint8Array,ArrayBuffer,DOMException,TextEncoder,btoa,atob,events,
    window:{document:{baseURI:'https://example.com/'}},crypto:{getRandomValues:bytes=>{bytes[0]=++serial;return bytes;}},WebSocket:Native,__pwWebSocketBinding:event=>events.push(event)
  });
  assert.deepEqual(Array.from(result.refused),Array(7).fill('SyntaxError').concat(Array(3).fill('RangeError')));
  assert.equal(result.fragment,'SyntaxError');assert.equal(result.invalidAllocations,0);assert.equal(result.selected,'Acme.v2');assert.equal(result.empty,'');
  assert.equal(result.allowed,"!#$%&'*+-.^_`|~012Abc");
  assert.deepEqual(Array.from(events[0].protocols),['Acme.v1','Acme.v2']);
  assert.deepEqual(connections[0].protocols,['Acme.v1','Acme.v2']);
});
test('WebSocket mock protocol-error closes release their owner and validate whole command batches',async()=>{
  const routing=await readFile(new URL('../src/browser/reading/websocket-routing.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{
    let emit,reply;const sent=[];
    const registry=(${routing})(callback=>{emit=callback;return {Mock:class{},dispatch:command=>sent.push(command)};},Mock=>Mock);
    const tick=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
    registry.add('owner','wss://example.com/socket',()=>reply,true);
    const closed=[];
    for(const code of [1000,1001,1002,1003,1007,1008,1009,1010,1011,1012,1013,1014,3000,4999]){
      reply=[{type:'closePage',code,reason:'Acme protocol close'},{type:'connect'}];
      const offset=sent.length;emit({id:String(code),type:'onCreate',url:'wss://example.com/socket'});await tick();
      closed.push({code:sent[offset].code,count:sent.length-offset,active:registry.active});
    }
    const invalid=[];
    for(const [type,code] of [['closePage',1004],['closePage',1005],['closePage',1006],['closePage',1015],['closePage',999],['closePage',5000],['closePage',1002.5],['closeServer',1002]]){
      reply=[{type:'connect'},{type,code}];const offset=sent.length;
      emit({id:type+code,type:'onCreate',url:'wss://example.com/socket'});await tick();
      invalid.push(sent.slice(offset));
    }
    return {closed,invalid,active:registry.active,pending:registry.pending};
  })()`,{URL,TextEncoder,atob});
  for(const [index,code] of [1000,1001,1002,1003,1007,1008,1009,1010,1011,1012,1013,1014,3000,4999].entries()){
    assert.equal(result.closed[index].code,code);assert.equal(result.closed[index].count,1);assert.equal(result.closed[index].active,0);
  }
  for(const commands of result.invalid){
    assert.deepEqual(Array.from(commands,command=>command.type),['closePage','closeServer']);
    assert.ok(Array.from(commands,command=>command.code).every(code=>code===3001));
  }
  assert.equal(result.active,0);assert.equal(result.pending,0);
});
test('WebSocket typed matchers retain query matching, Page precedence and private nonmatches',async()=>{
  const routing=await readFile(new URL('../src/browser/reading/websocket-routing.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{
    let emit;const calls=[],sent=[];
    const registry=(${routing})(callback=>{emit=callback;return {Mock:class{},dispatch:command=>sent.push(command)};},(Mock,matches)=>class {constructor(url){this.selected=matches(new URL(url).href);}});
    const tick=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
    const reply=owner=>event=>{calls.push(owner);return [{type:'closePage',code:1002,reason:owner}];};
    registry.add('context',{regexSource:'^http://example[.]com/acme/[^/]*[?]token=[^/]*$'},reply('context'),false);
    registry.add('page',{regexSource:'^http://example[.]com/acme/one[?]token=page$'},reply('page'),true);
    const selected=new registry.WebSocket('ws://example.com/acme/one?token=page').selected;
    const excluded=new registry.WebSocket('ws://example.com/acme/oneXtoken=page').selected;
    for(const [id,url] of [['one','ws://example.com/acme/one?token=page'],['two','ws://example.com/acme/two?token=context'],['none','ws://example.com/other?token=page']]){emit({id,type:'onCreate',url});await tick();}
    registry.remove('page');emit({id:'fallback',type:'onCreate',url:'ws://example.com/acme/one?token=page'});await tick();
    registry.remove('context');
    return {selected,excluded,calls,sent,active:registry.active,pending:registry.pending};
  })()`,{URL,TextEncoder,atob});
  assert.equal(result.selected,true);assert.equal(result.excluded,false);
  assert.deepEqual(Array.from(result.calls),['page','context','context']);
  assert.deepEqual(Array.from(result.sent,command=>command.type),['closePage','closePage','passthrough','closePage']);
  assert.equal(result.active,0);assert.equal(result.pending,0);
});
test('WebSocket compiled regex tables execute without native RegExp and validate byte and state bounds',async()=>{
  const [source,routing,guard]=await Promise.all(['websocket-regex.js','websocket-routing.js','websocket-guard.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  const classes=btoa(String.fromCharCode(...Array(128).fill(0)));
  const result=await runInNewContext(`(async()=>{
    const compile=${source};let constructions=0,allocations=0,emit;const calls=[];
    // This one-state machine accepts at end-of-input only; no URL byte accepts.
    const program={width:1,classes,transitions:'AAA=',acceptance:'Ag=='};
    const matches=compile(program);
    const ascii=matches('wss://example.com/Acme'),http=matches('https://example.com/Acme'),unicode=matches('ws://example.com/東京');
    const matchedAtStart=compile({...program,acceptance:'AQ=='});
    const unicodeAfterMatch=matchedAtStart('ws://example.com/東京');
    let oversized;try{matches('ws://example.com/'+'a'.repeat(8192));}catch(error){oversized=error.message;}
    const invalid=[];for(const changed of [{classes:'AA=='},{width:129},{transitions:'AQA='},{acceptance:'BA=='}]){
      try{compile({...program,...changed});invalid.push('accepted');}catch(error){invalid.push(error.message);}
    }
    const registry=(${routing})(callback=>{emit=callback;return {Mock:class{constructor(url){allocations++;this.url=url;}},dispatch:()=>{}};},${guard},compile);
    registry.add('broad',{regexSource:'.*'},()=>{throw Error('Wrong sibling');},false,1);
    globalThis.RegExp=class {constructor(){constructions++;throw Error('Native backtracking must not run');}};
    registry.add('regex',{automaton:program},event=>{calls.push(event.url);return [{type:'closePage',code:1002}];},false,2);
    const selected=new registry.WebSocket('ws://example.com/Acme').url;
    let conversions=0;
    const snapshot=new registry.WebSocket({toString(){conversions++;return 'ws://example.com/snapshot';}}).url;
    const beforeOversized=allocations;
    let boundedBeforeAllocation;try{new registry.WebSocket('ws://example.com/'+'a'.repeat(8192));}catch(error){boundedBeforeAllocation=error.message;}
    emit({id:'one',type:'onCreate',url:'ws://example.com/Acme'});
    for(let i=0;i<8;i++)await Promise.resolve();
    return {ascii,http,unicode,unicodeAfterMatch,oversized,invalid,selected,snapshot,conversions,beforeOversized,allocations,boundedBeforeAllocation,calls,constructions,active:registry.active};
  })()`,{classes,atob,btoa,TextEncoder,URL,Blob,ArrayBuffer,DOMException,window:{document:{baseURI:'http://example.com/'}}});
  assert.equal(result.ascii,true);assert.equal(result.http,false);assert.equal(result.unicode,false);assert.equal(result.unicodeAfterMatch,false);
  assert.equal(result.oversized,'WEBSOCKET_URL_BOUNDS');
  assert.deepEqual(Array.from(result.invalid),Array(4).fill('WEBSOCKET_REGEX_INVALID'));
  assert.equal(result.selected,'ws://example.com/Acme');assert.deepEqual(Array.from(result.calls),['ws://example.com/Acme']);
  assert.equal(result.snapshot,'ws://example.com/snapshot');assert.equal(result.conversions,1);
  assert.equal(result.boundedBeforeAllocation,'WEBSOCKET_URL_BOUNDS');assert.equal(result.allocations,result.beforeOversized);
  assert.equal(result.constructions,0);assert.equal(result.active,0);
});
test('WebSocket native-regex policies use the installation RegExp with fresh lastIndex and URL bounds',async()=>{
  const routing=await readFile(new URL('../src/browser/reading/websocket-routing.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(()=>{
    const registry=(${routing})(()=>({Mock:class{},dispatch:()=>{}}),(Mock,matches)=>class {constructor(url){this.selected=matches(url);}},()=>{throw Error('No DFA');});
    registry.add('lookbehind',{nativeRegex:{source:'(?<=/)Acme(?=\\\\?)',flags:'gi'}},()=>[],false,1);
    // Replacing the page's RegExp after installation does not change matching.
    globalThis.RegExp=class {constructor(){throw Error('Replaced RegExp');}};
    const first=new registry.WebSocket('wss://example.com/acme?token=1').selected;
    const repeated=new registry.WebSocket('wss://example.com/acme?token=1').selected;
    const missing=new registry.WebSocket('wss://example.com/acme').selected;
    const http=new registry.WebSocket('https://example.com/acme?token=1').selected;
    let oversized;try{new registry.WebSocket('wss://example.com/'+'a'.repeat(8192));}catch(error){oversized=error.message;}
    return {first,repeated,missing,http,oversized};
  })()`,{URL,TextEncoder});
  assert.equal(result.first,true);assert.equal(result.repeated,true,'g/y lastIndex never carries over');
  assert.equal(result.missing,false);assert.equal(result.http,false);
  assert.equal(result.oversized,'WEBSOCKET_URL_BOUNDS');
});
test('WebSocket URLPattern policies match each parsed URL component against its own program',async()=>{
  const [source,routing]=await Promise.all(['websocket-regex.js','websocket-routing.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  const classes=btoa(String.fromCharCode(...Array(128).fill(0)));
  const result=await runInNewContext(`(async()=>{
    const compile=${source};
    // One state: 'Ag==' accepts any whole component, 'AA==' accepts none.
    const accept={width:1,classes,transitions:'AAA=',acceptance:'Ag=='},reject={...accept,acceptance:'AA=='};
    const prefixless=compile(accept,true)('example.com'),whole=compile(accept)('example.com');
    const registry=(${routing})(()=>({Mock:class{},dispatch:()=>{}}),(Mock,matches)=>class {constructor(url){this.selected=matches(url);}},compile);
    registry.add('host',{urlPattern:[null,null,null,accept,null,null,null,null]},()=>[],false,1);
    const selected=new registry.WebSocket('ws://example.com/acme').selected;
    const http=new registry.WebSocket('http://example.com/acme').selected;
    registry.remove('host');
    // A rejecting pathname program excludes the socket although the hostname accepts.
    registry.add('path',{urlPattern:[null,null,null,accept,null,reject,null,null]},()=>[],false,2);
    const excluded=new registry.WebSocket('ws://example.com/acme').selected;
    return {prefixless,whole,selected,http,excluded};
  })()`,{classes,atob,btoa,TextEncoder,URL});
  assert.equal(result.prefixless,true,'component programs skip the WS(S) prefix check');
  assert.equal(result.whole,false);
  assert.equal(result.selected,true);assert.equal(result.http,false);assert.equal(result.excluded,false);
});

test('WebSocket sibling policies retain registration precedence across installation order and removal',async()=>{
  const routing=await readFile(new URL('../src/browser/reading/websocket-routing.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{
    let emit;const calls=[],sent=[];
    const registry=(${routing})(callback=>{emit=callback;return {Mock:class{},dispatch:command=>sent.push(command)};},Mock=>Mock);
    const tick=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
    const reply=owner=>event=>{calls.push([owner,event.id]);return [{type:'ensureOpened'}];};
    const matcher={regexSource:'^https?://example[.]com/socket$'};
    // Future-document init scripts may arrive in any order. Host registration
    // ordinals, rather than their installation order, select the newest sibling.
    registry.add('new-page',matcher,reply('new-page'),true,20);
    registry.add('old-page',matcher,reply('old-page'),true,10);
    registry.add('new-context',matcher,reply('new-context'),false,40);
    registry.add('old-context',matcher,reply('old-context'),false,30);
    emit({id:'first',type:'onCreate',url:'ws://example.com/socket'});await tick();
    registry.add('newest-page',matcher,reply('newest-page'),true,50);
    emit({id:'second',type:'onCreate',url:'ws://example.com/socket'});await tick();
    emit({id:'first',type:'onMessageFromPage',data:{data:'Acme',isBase64:false}});await tick();
    registry.remove('newest-page');
    const afterRemoval=registry.active;
    const originalSend=registry.send('new-page','first','page',{data:'still owned',isBase64:false});
    const wrongSend=registry.send('old-page','first','page',{data:'wrong owner',isBase64:false});
    emit({id:'third',type:'onCreate',url:'ws://example.com/socket'});await tick();
    registry.remove('new-page');
    emit({id:'fourth',type:'onCreate',url:'ws://example.com/socket'});await tick();
    registry.remove('old-page');
    emit({id:'fifth',type:'onCreate',url:'ws://example.com/socket'});await tick();
    registry.remove('new-context');
    emit({id:'sixth',type:'onCreate',url:'ws://example.com/socket'});await tick();
    registry.remove('old-context');
    emit({id:'unmatched',type:'onCreate',url:'ws://example.com/socket'});await tick();
    return {calls,sent,afterRemoval,originalSend,wrongSend,active:registry.active,pending:registry.pending};
  })()`,{URL,TextEncoder,atob});
  assert.deepEqual(Array.from(result.calls,pair=>Array.from(pair)),[['new-page','first'],['newest-page','second'],['new-page','first'],['new-page','third'],['old-page','fourth'],['new-context','fifth'],['old-context','sixth']]);
  assert.equal(result.afterRemoval,1);
  assert.equal(result.originalSend,'sent');assert.equal(result.wrongSend,'WEBSOCKET_SOCKET_EXPIRED');
  assert.deepEqual(Array.from(result.sent.filter(command=>command.type==='closePage'),command=>command.id),['second','first','third','fourth','fifth','sixth']);
  assert.equal(result.sent.at(-1).type,'passthrough');
  assert.equal(result.active,0);assert.equal(result.pending,0);
});
test('WebSocket guard bounds selected messages and connecting queues while preserving passthrough',async()=>{
  const [source,guard]=await Promise.all(['websocket.js','websocket-guard.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  let serial=0;const events=[],connections=[];
  class Native {
    constructor(){this.readyState=0;this.sent=[];connections.push(this);}
    send(data){this.sent.push(data);}
    close(code,reason){assert.ok(code===1000||code>=3000&&code<=4999);this.closed={code,reason};this.readyState=3;}
  }
  const realm={module:{exports:{}},EventTarget,Event,MessageEvent,CloseEvent:class extends Event{},Blob,URL,Uint8Array,ArrayBuffer,DOMException,TextEncoder,btoa,atob,
    window:{document:{baseURI:'https://example.com/'}},crypto:{getRandomValues:bytes=>{bytes[0]=++serial;return bytes;}},WebSocket:Native,__pwWebSocketBinding:payload=>events.push(payload)};
  const result=runInNewContext(`(()=>{${source}\nmodule.exports.inject()(globalThis);
    globalThis.WebSocket=(${guard})(WebSocket,url=>url==='wss://example.com/selected');
    const selected=new WebSocket('/selected');__pwWebSocketDispatch({id:selected._id,type:'ensureOpened'});
    const hidden=new ArrayBuffer(524289);Object.defineProperty(hidden,'byteLength',{value:1});
    const blob=new class extends Blob{get size(){return 1;}}(['x'.repeat(524289)]);
    const refused=[];for(const data of ['x'.repeat(524289),'é'.repeat(262145),new Uint8Array(524289),new Blob(['x'.repeat(524289)]),hidden,blob,{byteLength:1},'\\u0001'.repeat(131072)]){
      try{selected.send(data);}catch(error){refused.push(error.message);}
    }
    selected.send(new Uint8Array([9,0,255,9]).subarray(1,3));
    __pwWebSocketDispatch({id:selected._id,type:'connect'});
    for(let i=0;i<32;i++)selected._apiSendToServer('Acme');
    try{selected._apiSendToServer('overflow');}catch(error){refused.push(error.message);}
    const server=selected._ws;server.readyState=1;server.onopen({});selected._apiSendToServer('after-open');
    server.bufferedAmount=2097152;try{selected._apiSendToServer('overflow');}catch(error){refused.push(error.message);}server.bufferedAmount=0;
    server.onmessage({data:'x'.repeat(524289)});
    const ordinary=new WebSocket('/ordinary');__pwWebSocketDispatch({id:ordinary._id,type:'passthrough'});
    ordinary._ws.readyState=1;ordinary._ws.onopen({});ordinary.send('x'.repeat(524289));
    return {refused,buffered:selected._wsBufferedMessages.length,closed:selected.readyState};})()`,realm);
  assert.deepEqual(Array.from(result.refused),Array(8).fill('WEBSOCKET_MESSAGE_BOUNDS').concat('WEBSOCKET_QUEUE_BOUNDS','WEBSOCKET_QUEUE_BOUNDS'));
  assert.equal(result.buffered,0);assert.equal(result.closed,3);
  assert.equal(connections[0].sent.length,33);assert.equal(connections[0].closed.code,3009);
  assert.equal(connections[1].sent[0].length,524289);
  assert.equal(events.find(event=>event.type==='onMessageFromPage').data.data,'AP8=');
  assert.equal(events.some(event=>event.type==='onMessageFromServer'),false);
});
test('WebSocket routing preserves page precedence, private binary callbacks and independent release',async()=>{
  const [source,guard,routing]=await Promise.all(['websocket.js','websocket-guard.js','websocket-routing.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  let serial=0;const calls=[];
  class Close extends Event {constructor(type,options){super(type);Object.assign(this,{code:options.code,reason:options.reason});}}
  const result=await runInNewContext(`(async()=>{
    const install=emit=>{globalThis.__pwWebSocketBinding=emit;(()=>{const module={exports:{}};${source}\nmodule.exports.inject()(globalThis);})();return {Mock:WebSocket,dispatch:__pwWebSocketDispatch};};
    const registry=(${routing})(install,${guard});globalThis.WebSocket=registry.WebSocket;
    registry.add('context','wss://example.com/socket',async event=>{calls.push(['context',event]);return [{type:'ensureOpened'}];},false);
    registry.add('page','wss://example.com/socket',async event=>{calls.push(['page',event]);return event.type==='onCreate'?[{type:'ensureOpened'}]:event.type==='onMessageFromPage'?[{type:'sendToPage',data:event.data}]:[];},true);
    const socket=new WebSocket('/socket');socket.binaryType='arraybuffer';let received;
    socket.onmessage=event=>{received=Array.from(new Uint8Array(event.data));};
    for(let i=0;i<8;i++)await Promise.resolve();
    socket.send(new Uint8Array([9,0,255,9]).subarray(1,3));for(let i=0;i<8;i++)await Promise.resolve();
    const echoed=received;
    const wrongOwner=registry.send('context',socket._id,'page',{data:'AQI=',isBase64:true});
    const unsolicited=registry.send('page',socket._id,'page',{data:'AQI=',isBase64:true});
    const pushed=received;let oversized=false;
    try{registry.send('page',socket._id,'page',{data:'x'.repeat(524289),isBase64:false});}catch{oversized=true;}
    registry.remove('page');const closed=socket.readyState;
    const expired=registry.send('page',socket._id,'page',{data:'AQI=',isBase64:true});
    const sibling=new WebSocket('/socket');for(let i=0;i<8;i++)await Promise.resolve();
    const independent=sibling.readyState;registry.remove('context');for(let i=0;i<8;i++)await Promise.resolve();
    return {received:echoed,pushed,unsolicited,wrongOwner,expired,oversized,closed,independent,active:registry.active,pending:registry.pending};})()`,{
    calls,EventTarget,Event,MessageEvent,CloseEvent:Close,Blob,URL,Uint8Array,ArrayBuffer,DOMException,TextEncoder,btoa,atob,
    window:{document:{baseURI:'https://example.com/'}},crypto:{getRandomValues:bytes=>{bytes[0]=++serial;return bytes;}},WebSocket:class {constructor(){throw Error('Unexpected real connection');}}
  });
  assert.deepEqual(Array.from(result.received),[0,255]);assert.equal(result.closed,3);assert.equal(result.independent,1);
  assert.deepEqual(Array.from(result.pushed),[1,2]);assert.equal(result.unsolicited,'sent');assert.equal(result.wrongOwner,'WEBSOCKET_SOCKET_EXPIRED');assert.equal(result.expired,'WEBSOCKET_SOCKET_EXPIRED');assert.equal(result.oversized,true);
  assert.equal(result.active,0);assert.equal(result.pending,0);
  assert.equal(calls[0][0],'page');assert.equal(calls.find(([_,event])=>event.type==='onMessageFromPage')[1].data.data,'AP8=');
  assert.equal(calls.filter(([owner,event])=>owner==='context'&&event.type==='onCreate').length,1);
});
test('WebSocket routing validates complete replies and closes rejected or expired callbacks',async()=>{
  const [source,guard,routing]=await Promise.all(['websocket.js','websocket-guard.js','websocket-routing.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  let serial=0;
  const result=await runInNewContext(`(async()=>{
    const install=emit=>{globalThis.__pwWebSocketBinding=emit;(()=>{const module={exports:{}};${source}\nmodule.exports.inject()(globalThis);})();return {Mock:WebSocket,dispatch:__pwWebSocketDispatch};};
    const registry=(${routing})(install,${guard});globalThis.WebSocket=registry.WebSocket;
    registry.add('invalid','wss://example.com/invalid',()=>[{type:'ensureOpened'},{type:'passthrough'}],true);
    registry.add('expired','wss://example.com/expired',()=>Promise.reject(Error('Callback expired')),true);
    const invalid=new WebSocket('/invalid'),expired=new WebSocket('/expired');let opened=0;invalid.onopen=()=>opened++;
    for(let i=0;i<12;i++)await Promise.resolve();
    return {opened,invalid:invalid.readyState,expired:expired.readyState,active:registry.active,pending:registry.pending};})()`,{
    EventTarget,Event,MessageEvent,CloseEvent:class extends Event{},Blob,URL,Uint8Array,ArrayBuffer,DOMException,TextEncoder,btoa,atob,
    window:{document:{baseURI:'https://example.com/'}},crypto:{getRandomValues:bytes=>{bytes[0]=++serial;return bytes;}},WebSocket:class {constructor(){throw Error('Unexpected real connection');}}
  });
  assert.equal(result.opened,0);assert.equal(result.invalid,3);assert.equal(result.expired,3);assert.equal(result.active,0);assert.equal(result.pending,0);
});
test('WebSocket forwarding selects each direction, preserves message bounds and validates whole replies',async()=>{
  const routing=await readFile(new URL('../src/browser/reading/websocket-routing.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{
    let emit;const calls=[],sent=[];
    const registry=(${routing})(callback=>{emit=callback;return {Mock:class {},dispatch:command=>sent.push(command)};},Mock=>Mock);
    registry.add('owner','wss://example.com/socket',event=>{
      calls.push(event.type);
      if(event.type==='onCreate')return [{type:'setForwarding',from:'page',enabled:true}];
      if(event.type==='onMessageFromServer')return [{type:'setForwarding',from:'page',enabled:false},{type:'setForwarding',from:'server',enabled:true}];
      return [];
    },true);
    const tick=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
    emit({id:'one',type:'onCreate',url:'wss://example.com/socket'});await tick();
    emit({id:'one',type:'onMessageFromPage',data:{data:'AP8=',isBase64:true}});await tick();
    emit({id:'one',type:'onMessageFromServer',data:{data:'Acme response',isBase64:false}});await tick();
    emit({id:'one',type:'onMessageFromServer',data:{data:'Acme forwarded',isBase64:false}});await tick();
    emit({id:'one',type:'onMessageFromPage',data:{data:'Acme manual',isBase64:false}});await tick();
    const before=sent.length;
    emit({id:'one',type:'onMessageFromServer',data:{data:'é'.repeat(262145),isBase64:false}});await tick();
    const afterBounds=sent.slice(before),activeAfterBounds=registry.active;
    registry.add('invalid','wss://example.com/invalid',()=>[{type:'sendToPage',data:{data:'must not dispatch',isBase64:false}},{type:'setForwarding',from:'other',enabled:true}],true);
    emit({id:'invalid',type:'onCreate',url:'wss://example.com/invalid'});await tick();
    return {calls,sent,afterBounds,activeAfterBounds,active:registry.active,pending:registry.pending};
  })()`,{URL,TextEncoder,atob});
  assert.deepEqual(Array.from(result.calls),['onCreate','onMessageFromServer','onMessageFromPage']);
  assert.deepEqual(JSON.parse(JSON.stringify(result.sent.slice(0,2))),[
    {id:'one',type:'sendToServer',data:{data:'AP8=',isBase64:true}},
    {id:'one',type:'sendToPage',data:{data:'Acme forwarded',isBase64:false}},
  ]);
  assert.deepEqual(Array.from(result.afterBounds,command=>command.type),['closePage','closeServer']);
  assert.equal(result.sent.some(command=>command.data?.data==='must not dispatch'),false);
  assert.equal(result.activeAfterBounds,0);assert.equal(result.active,0);assert.equal(result.pending,0);
});
test('WebSocket close forwarding completes both sides without duplicate callbacks or leaked owners',async()=>{
  const [source,guard,routing]=await Promise.all(['websocket.js','websocket-guard.js','websocket-routing.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  for(const mode of ['page','server','abnormal','mock','manual','one-way']) {
    let serial=0;const connections=[];
    class Native {
      constructor(){this.readyState=0;this.closes=[];connections.push(this);}
      close(code,reason){this.closes.push({code,reason});this.readyState=2;}
    }
    class Close extends Event {constructor(type,{code,reason,wasClean}){super(type);Object.assign(this,{code,reason,wasClean});}}
    const result=await runInNewContext(`(async()=>{
      const calls=[],closed=[];
      const tick=async()=>{for(let i=0;i<12;i++)await Promise.resolve();};
      const install=emit=>{globalThis.__pwWebSocketBinding=emit;(()=>{const module={exports:{}};${source}\nmodule.exports.inject()(globalThis);})();return {Mock:WebSocket,dispatch:__pwWebSocketDispatch};};
      const registry=(${routing})(install,${guard});globalThis.WebSocket=registry.WebSocket;
      registry.add('owner','wss://example.com/socket',event=>{
        calls.push(event.type);
        if(event.type==='onCreate')return [
          {type:'setCloseForwarding',from:'page',enabled:mode!=='manual'},
          {type:'setCloseForwarding',from:'server',enabled:mode!=='manual'&&mode!=='one-way'},
          {type:mode==='mock'?'ensureOpened':'connect'}
        ];
        if(event.type==='onClosePage')return [{type:'closeServer',code:event.code,reason:event.reason}];
        if(event.type==='onCloseServer')return [{type:'closePage',code:event.code,reason:event.reason}];
        return [];
      },true);
      const socket=new WebSocket('/socket');socket.onclose=event=>closed.push({code:event.code,reason:event.reason,wasClean:event.wasClean});
      await tick();
      const server=socket._ws;
      if(server){server.readyState=1;server.onopen({});}
      let closing;
      if(mode==='server'||mode==='abnormal'){
        server.readyState=3;
        server.onclose({code:mode==='server'?1001:1006,reason:'Acme server close',wasClean:mode!=='abnormal'});
      }else{
        socket.close(3007,'Acme page close');closing=socket.readyState;await tick();
        if(server){server.readyState=3;server.onclose({code:3007,reason:'Acme page close',wasClean:true});}
      }
      await tick();
      const expired=registry.send('owner',socket._id,'page',{data:'Acme',isBase64:false});
      return {calls,closed,closing,ready:socket.readyState,active:registry.active,pending:registry.pending,expired};
    })()`,{
      mode,EventTarget,Event,MessageEvent,CloseEvent:Close,Blob,URL,Uint8Array,ArrayBuffer,DOMException,TextEncoder,btoa,atob,
      window:{document:{baseURI:'https://example.com/'}},crypto:{getRandomValues:bytes=>{bytes[0]=++serial;return bytes;}},WebSocket:Native
    });
    assert.equal(result.ready,3,mode);assert.equal(result.active,0,mode);assert.equal(result.pending,0,mode);
    assert.equal(result.expired,'WEBSOCKET_SOCKET_EXPIRED',mode);assert.equal(result.closed.length,1,mode);
    assert.equal(result.closed[0].code,mode==='server'?1001:mode==='abnormal'?1006:3007,mode);
    assert.equal(result.closed[0].wasClean,mode!=='abnormal',mode);
    assert.equal(result.closed[0].reason,mode==='server'||mode==='abnormal'?'Acme server close':'Acme page close',mode);
    assert.deepEqual(Array.from(result.calls),mode==='manual'?['onCreate','onClosePage','onCloseServer']:mode==='one-way'?['onCreate','onCloseServer']:['onCreate'],mode);
    if(connections.length)assert.equal(connections[0].closes.length,mode==='server'||mode==='abnormal'?0:1,mode);
    if(mode==='page')assert.equal(result.closing,2);
  }
});
test('WebSocket close forwarding can be disabled and invalid settings do not dispatch earlier commands',async()=>{
  const routing=await readFile(new URL('../src/browser/reading/websocket-routing.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{
    let emit;const calls=[],sent=[];
    const registry=(${routing})(callback=>{emit=callback;return {Mock:class {},dispatch:command=>sent.push(command)};},Mock=>Mock);
    const tick=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
    registry.add('owner','wss://example.com/socket',event=>{
      calls.push(event.type);
      if(event.type==='onCreate')return [{type:'setCloseForwarding',from:'server',enabled:true}];
      if(event.type==='onMessageFromPage')return [{type:'setCloseForwarding',from:'server',enabled:false}];
      return [{type:'closePage',code:3008,reason:'Acme manual close'}];
    },true);
    emit({id:'one',type:'onCreate',url:'wss://example.com/socket'});await tick();
    emit({id:'one',type:'onMessageFromPage',data:{data:'Acme',isBase64:false}});await tick();
    emit({id:'one',type:'onCloseServer',code:3007,reason:'Acme',wasClean:true});await tick();
    registry.add('invalid','wss://example.com/invalid',()=>[{type:'connect'},{type:'setCloseForwarding',from:'server',enabled:'true'}],true);
    emit({id:'invalid',type:'onCreate',url:'wss://example.com/invalid'});await tick();
    return {calls,sent,active:registry.active,pending:registry.pending};
  })()`,{URL,TextEncoder,atob});
  assert.deepEqual(Array.from(result.calls),['onCreate','onMessageFromPage','onCloseServer']);
  assert.equal(result.sent[0].type,'closePage');assert.equal(result.sent[0].code,3008);
  assert.equal(result.sent.some(command=>command.type==='connect'),false);
  assert.equal(result.active,0);assert.equal(result.pending,0);
});
async function socketOwnershipRealm(body) {
  const [source,guard,routing]=await Promise.all(['websocket.js','websocket-guard.js','websocket-routing.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  let serial=0;
  class Close extends Event {constructor(type,{code=0,reason='',wasClean=false}){super(type);Object.assign(this,{code,reason,wasClean});}}
  return runInNewContext(`(async()=>{
    const calls=[],connections=[],closed=[],messages=[];
    class Native {
      constructor(){this.readyState=0;this.closes=[];this.sent=[];connections.push(this);}
      send(data){if(this.readyState!==1)throw Error('Native socket is not open');this.sent.push(data);}
      close(...args){const [code,reason]=args;if(args.length&&code===undefined)throw new DOMException('Undefined close code converts to zero','InvalidAccessError');this.closes.push({code,reason,argumentCount:args.length});this.readyState=2;}
    }
    globalThis.WebSocket=Native;
    const tick=async()=>{for(let i=0;i<16;i++)await Promise.resolve();};
    const install=emit=>{globalThis.__pwWebSocketBinding=emit;(()=>{const module={exports:{}};${source}\nmodule.exports.inject()(globalThis);})();return {Mock:WebSocket,dispatch:__pwWebSocketDispatch};};
    const registry=(${routing})(install,${guard});globalThis.WebSocket=registry.WebSocket;
    ${body}
  })()`,{
    EventTarget,Event,MessageEvent,CloseEvent:Close,Blob,URL,Uint8Array,ArrayBuffer,DOMException,TextEncoder,btoa,atob,
    window:{document:{baseURI:'https://example.com/'}},crypto:{getRandomValues:bytes=>{bytes[0]=++serial;return bytes;}}
  });
}
test('WebSocket close commands preserve omitted codes through mock events and native handshakes',async()=>{
  for(const mode of ['page-omitted','page-explicit','server-omitted','server-explicit','server-reason']) {
    const result=await socketOwnershipRealm(`
      const mode=${JSON.stringify(mode)},serverSide=mode.startsWith('server'),explicit=mode.endsWith('explicit'),reasonOnly=mode==='server-reason';
      registry.add('owner','wss://example.com/socket',event=>{
        calls.push(event.type);
        if(event.type==='onCreate')return serverSide?[{type:'setCloseForwarding',from:'server',enabled:true},{type:'connect'}]:[{type:'ensureOpened'}];
        if(event.type==='onMessageFromPage')return [{type:serverSide?'closeServer':'closePage',...(explicit?{code:1000}:{}),...(reasonOnly?{reason:'Acme reason only'}:{})}];
        return [];
      },true);
      const socket=new WebSocket('/socket');socket.onclose=event=>closed.push(event.code);await tick();
      const server=socket._ws;if(server){server.readyState=1;server.onopen({});}
      socket.send('Acme close');await tick();
      const nativeCodeOmitted=server?server.closes[0].code===undefined:null;
      if(server){server.readyState=3;server.onclose({code:explicit||reasonOnly?1000:1005,reason:reasonOnly?'Acme reason only':'',wasClean:true});await tick();}
      return {closed,nativeCodeOmitted,nativeCloses:server?.closes.length??0,nativeCall:server?.closes[0],ready:socket.readyState,active:registry.active,pending:registry.pending};
    `);
    assert.deepEqual(Array.from(result.closed),[mode.endsWith('explicit')||mode==='server-reason'?1000:mode.startsWith('server')?1005:0],mode);
    assert.equal(result.nativeCloses,mode.startsWith('server')?1:0,mode);
    if(mode.startsWith('server'))assert.equal(result.nativeCodeOmitted,mode.endsWith('omitted'),mode);
    if(mode==='server-omitted')assert.equal(result.nativeCall.argumentCount,0);
    if(mode==='server-reason'){assert.equal(result.nativeCall.code,1000);assert.equal(result.nativeCall.reason,'Acme reason only');}
    assert.equal(result.ready,3,mode);assert.equal(result.active,0,mode);assert.equal(result.pending,0,mode);
  }
});
test('WebSocket server close keeps the page owner and refuses sends before and after acknowledgement',async()=>{
  const result=await socketOwnershipRealm(`
    registry.add('owner','wss://example.com/socket',event=>{
      calls.push(event.type);
      if(event.type==='onCreate')return [{type:'connect'}];
      if(event.type==='onMessageFromPage')return [{type:'closeServer',code:3007},{type:'closeServer',code:3007}];
      return [];
    },true);
    const socket=new WebSocket('/socket');socket.onmessage=event=>messages.push(event.data);socket.onclose=event=>closed.push(event.code);await tick();
    const server=socket._ws;server.readyState=1;server.onopen({});socket.send('Acme close');await tick();
    const before={active:registry.active,ready:socket.readyState,send:registry.send('owner',socket._id,'server',{data:'Acme forbidden',isBase64:false})};
    const pageSend=registry.send('owner',socket._id,'page',{data:'Acme page survives',isBase64:false});
    server.readyState=3;server.onclose({code:3007,reason:'Acme',wasClean:true});await tick();
    const after={active:registry.active,ready:socket.readyState,send:registry.send('owner',socket._id,'server',{data:'Acme forbidden',isBase64:false})};
    registry.remove('owner');await tick();
    return {before,after,pageSend,messages,closed,calls,nativeCloses:server.closes.length,nativeSends:server.sent.length,active:registry.active,pending:registry.pending};
  `);
  for(const phase of [result.before,result.after]){assert.equal(phase.active,1);assert.equal(phase.ready,1);assert.equal(phase.send,'WEBSOCKET_SOCKET_CLOSED');}
  assert.equal(result.pageSend,'sent');assert.deepEqual(Array.from(result.messages),['Acme page survives']);
  assert.equal(result.nativeCloses,1);assert.equal(result.nativeSends,0);assert.deepEqual(Array.from(result.closed),[3001]);
  assert.deepEqual(Array.from(result.calls),['onCreate','onMessageFromPage','onCloseServer']);
  assert.equal(result.active,0);assert.equal(result.pending,0);
});
test('WebSocket connection ownership is single-use and invalid reply state cannot publish earlier writes',async()=>{
  for(const mode of ['duplicate-batch','after-close','before-connect']) {
    const result=await socketOwnershipRealm(`
      const mode=${JSON.stringify(mode)};
      registry.add('owner','wss://example.com/socket',event=>{
        calls.push(event.type);
        if(event.type==='onCreate')return mode==='duplicate-batch'?[{type:'sendToPage',data:{data:'Acme leaked',isBase64:false}},{type:'connect'},{type:'connect'}]:[{type:mode==='before-connect'?'ensureOpened':'connect'}];
        if(event.type==='onCloseServer')return [{type:'sendToPage',data:{data:'Acme leaked',isBase64:false}},{type:'connect'}];
        return [];
      },true);
      const socket=new WebSocket('/socket');socket.onmessage=event=>messages.push(event.data);socket.onclose=event=>closed.push(event.code);await tick();
      let missing;
      if(mode==='after-close'){const server=socket._ws;server.readyState=1;server.onopen({});server.readyState=3;server.onclose({code:3007,reason:'Acme',wasClean:true});await tick();}
      if(mode==='before-connect'){missing=registry.send('owner',socket._id,'server',{data:'Acme forbidden',isBase64:false});registry.remove('owner');await tick();}
      return {connections:connections.length,messages,closed,missing,active:registry.active,pending:registry.pending};
    `);
    assert.equal(result.connections,mode==='after-close'?1:0,mode);assert.deepEqual(Array.from(result.messages),[],mode);
    assert.deepEqual(Array.from(result.closed),[3001],mode);assert.equal(result.active,0,mode);assert.equal(result.pending,0,mode);
    if(mode==='before-connect')assert.equal(result.missing,'WEBSOCKET_SERVER_NOT_CONNECTED');
  }
});
test('React source pin and framework snapshots preserve renderer IDs, pagination and subscription cleanup',async()=>{
  const [source,hook,pin]=await Promise.all(['framework.js','react-hook.js','react-hook-source.json'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  assert.equal(createHash('sha256').update(hook).digest('hex'),JSON.parse(pin).sha256);
  let listener=null,unsubscribed=0;
  const strings=[...Array.from('Acme',c=>c.codePointAt(0))];
  const ops=[2,1,5,4,...strings,1,1,11,0,0,0,0,1,7,5,1,0,1,0,0];
  const ri={flushInitialOperations:()=>listener(ops)};
  const window={__REACT_DEVTOOLS_GLOBAL_HOOK__:{rendererInterfaces:new Map([[1,{}],[2,ri]]),renderers:new Map([[1,{rendererPackageName:'react-server-dom'}],[2,{rendererPackageName:'react-dom',version:'18.3.1'}]]),getFiberRoots:id=>new Set([id]),sub:(_event,callback)=>{listener=callback;return()=>{unsubscribed++;listener=null;};}}};
  const registry=runInNewContext(`(${source})()`,{window,TextEncoder,setTimeout,clearTimeout,performance});
  const result=JSON.parse(await registry.query({kind:'tree',offset:1,limit:1}));
  assert.equal(result.rendererId,2);assert.equal(result.items[0].id,7);assert.equal(result.items[0].name,'Acme');assert.equal(result.items[0].parent,1);assert.equal(unsubscribed,1);
  ri.flushInitialOperations=()=>listener([2,1,0,999]);
  await assert.rejects(()=>registry.query({kind:'tree',offset:0,limit:1}),/Unsupported React operations/);assert.equal(unsubscribed,2);
  registry.close();await assert.rejects(()=>registry.query({kind:'tree',offset:0,limit:1}),/closed/);
});
test('framework profiling expires, retains its report and stops on release without redoing collection',async()=>{
  const source=await readFile(new URL('../src/browser/reading/framework.js',import.meta.url),'utf8');
  let started=0,stopped=0;const ri={startProfiling:()=>started++,stopProfiling:()=>stopped++,getProfilingData:()=>({dataForRoots:[{rootID:17,commitData:[{duration:2}]}]})};
  const window={__REACT_DEVTOOLS_GLOBAL_HOOK__:{rendererInterfaces:new Map([[1,ri]]),renderers:new Map([[1,{rendererPackageName:'react-dom'}]]),getFiberRoots:()=>new Set([1]),sub:()=>()=>{}}};
  const open=runInNewContext(`(${source})`,{window,TextEncoder,setTimeout,clearTimeout,performance}),registry=open(),other=open();
  await registry.query({kind:'renders_start',durationMs:5});
  await assert.rejects(()=>other.query({kind:'renders_start',durationMs:5}),/another registry/);
  await new Promise(resolve=>setTimeout(resolve,15));
  assert.equal(JSON.parse(await registry.query({kind:'renders_status'})).recording,false);
  const first=JSON.parse(await registry.query({kind:'renders_stop'}));assert.deepEqual(JSON.parse(await registry.query({kind:'renders_stop'})),first);assert.equal(stopped,1);assert.equal(started,1);
  await registry.query({kind:'renders_start',durationMs:60000});registry.close();assert.equal(stopped,2);
  other.close();
});
test('framework stop failure retains profiling ownership for cleanup retry',async()=>{
  const source=await readFile(new URL('../src/browser/reading/framework.js',import.meta.url),'utf8');
  let stops=0,starts=0;const ri={startProfiling:()=>starts++,stopProfiling:()=>{if(++stops===1)throw Error('Acme stop failed');},getProfilingData:()=>({dataForRoots:[]})};
  const window={__REACT_DEVTOOLS_GLOBAL_HOOK__:{rendererInterfaces:new Map([[1,ri]]),renderers:new Map([[1,{rendererPackageName:'react-dom'}]]),getFiberRoots:()=>new Set([1]),sub:()=>()=>{}}};
  const open=runInNewContext(`(${source})`,{window,TextEncoder,setTimeout,clearTimeout,performance}),registry=open(),other=open();
  await registry.query({kind:'renders_start',durationMs:5});await new Promise(resolve=>setTimeout(resolve,15));
  const status=JSON.parse(await registry.query({kind:'renders_status'}));assert.equal(status.recording,true);assert.equal(status.cleanupPending,true);
  await assert.rejects(()=>other.query({kind:'renders_start',durationMs:5}),/another registry/);
  registry.close();await other.query({kind:'renders_start',durationMs:5});other.close();assert.equal(starts,2);assert.equal(stops,3);
});
async function frameworkProfileFixture(ri){
  const source=await readFile(new URL('../src/browser/reading/framework.js',import.meta.url),'utf8');
  const timers=new Map();let sequence=0,now=0;
  const window={__REACT_DEVTOOLS_GLOBAL_HOOK__:{rendererInterfaces:new Map([[1,ri]]),renderers:new Map([[1,{rendererPackageName:'react-dom'}]]),getFiberRoots:()=>new Set([1]),sub:()=>()=>{}}};
  const open=runInNewContext(`(${source})`,{window,TextEncoder,setTimeout:callback=>{const id=++sequence;timers.set(id,callback);return id;},clearTimeout:id=>timers.delete(id),performance:{now:()=>++now}});
  return {open,timers};
}
test('framework profile flags preserve defaults, all boolean combinations and immutable report options',async()=>{
  const calls=[];let stops=0,reads=0;
  const {open,timers}=await frameworkProfileFixture({startProfiling:(...args)=>calls.push(args),stopProfiling:()=>stops++,getProfilingData:()=>{reads++;return {dataForRoots:[]};}});
  const registry=open();
  for(const flags of [{},{recordChangeDescriptions:false,recordTimeline:false},{recordChangeDescriptions:true,recordTimeline:true},{recordChangeDescriptions:false,recordTimeline:true},{recordChangeDescriptions:true,recordTimeline:false}]){
    const expected={recordChangeDescriptions:flags.recordChangeDescriptions??true,recordTimeline:flags.recordTimeline??false};
    const query={kind:'renders_start',durationMs:30000,...flags};
    const start=JSON.parse(await registry.query(query));assert.deepEqual(start.options,expected);
    query.recordChangeDescriptions=!expected.recordChangeDescriptions;query.recordTimeline=!expected.recordTimeline;
    assert.deepEqual(JSON.parse(await registry.query({kind:'renders_status'})).options,expected);
    assert.deepEqual(calls.at(-1),[expected.recordChangeDescriptions,expected.recordTimeline]);
    const stopped=JSON.parse(await registry.query({kind:'renders_stop'}));assert.deepEqual(stopped.options,expected);
    assert.deepEqual(JSON.parse(await registry.query({kind:'renders_stop'})),stopped);
  }
  registry.close();assert.equal(stops,5);assert.equal(reads,5);assert.equal(timers.size,0);
});
test('framework profile read failure retains stopped ownership and retries data without another stop',async()=>{
  let stops=0,reads=0;
  const {open,timers}=await frameworkProfileFixture({startProfiling:()=>{},stopProfiling:()=>stops++,getProfilingData:()=>{if(++reads===1)throw Error('Acme data unavailable');return {dataForRoots:[]};}});
  const registry=open(),other=open();await registry.query({kind:'renders_start',durationMs:30000});
  await assert.rejects(()=>registry.query({kind:'renders_stop'}),/Acme data unavailable/);
  const status=JSON.parse(await registry.query({kind:'renders_status'}));assert.equal(status.recording,false);assert.equal(status.phase,'stopped');assert.equal(status.cleanupPending,true);assert.equal(timers.size,0);
  await assert.rejects(()=>other.query({kind:'renders_start',durationMs:30000}),/another registry/);
  const stopped=JSON.parse(await registry.query({kind:'renders_stop'}));assert.equal(stops,1);assert.equal(reads,2);assert.equal(stopped.elapsedMs,status.elapsedMs);
  assert.deepEqual(JSON.parse(await registry.query({kind:'renders_stop'})),stopped);assert.equal(reads,2);
  await other.query({kind:'renders_start',durationMs:30000});other.close();registry.close();assert.equal(stops,2);assert.equal(reads,2);
});
test('framework release discards an unread stopped profile without recollecting or stopping again',async()=>{
  let stops=0,reads=0;
  const {open,timers}=await frameworkProfileFixture({startProfiling:()=>{},stopProfiling:()=>stops++,getProfilingData:()=>{reads++;throw Error('Acme permanent read failure');}});
  const registry=open(),other=open();await registry.query({kind:'renders_start',durationMs:30000});
  await assert.rejects(()=>registry.query({kind:'renders_stop'}),/permanent read failure/);
  assert.equal(registry.close(),true);assert.equal(registry.close(),true);assert.equal(stops,1);assert.equal(reads,1);
  await other.query({kind:'renders_start',durationMs:30000});other.close();assert.equal(stops,2);assert.equal(reads,1);assert.equal(timers.size,0);
});
test('framework start and stop failures retain their owner until acknowledged release',async()=>{
  let starts=0,stops=0,reads=0;
  const {open,timers}=await frameworkProfileFixture({startProfiling:()=>{if(++starts===1)throw Error('Acme uncertain start');},stopProfiling:()=>{if(++stops===1)throw Error('Acme stop failed');},getProfilingData:()=>{reads++;return {};}});
  const registry=open(),other=open();await assert.rejects(()=>registry.query({kind:'renders_start',durationMs:30000}),/uncertain start/);
  const status=JSON.parse(await registry.query({kind:'renders_status'}));assert.equal(status.phase,'start-uncertain');assert.equal(status.recording,null);assert.equal(status.cleanupPending,true);assert.equal(timers.size,0);
  await assert.rejects(()=>other.query({kind:'renders_start',durationMs:30000}),/another registry/);
  assert.throws(()=>registry.close(),/stop failed/);await assert.rejects(()=>other.query({kind:'renders_start',durationMs:30000}),/another registry/);
  registry.close();await other.query({kind:'renders_start',durationMs:30000});other.close();assert.equal(starts,2);assert.equal(stops,3);assert.equal(reads,0);assert.equal(timers.size,0);
});
test('framework profiling refuses a renderer without stop support before calling start',async()=>{
  let starts=0;const ri={startProfiling:()=>starts++,getProfilingData:()=>({})};const {open}=await frameworkProfileFixture(ri);
  const registry=open();await assert.rejects(()=>registry.query({kind:'renders_start',durationMs:30000}),/profiling unavailable/);assert.equal(starts,0);
  ri.stopProfiling=()=>{};const other=open();await other.query({kind:'renders_start',durationMs:30000});other.close();registry.close();assert.equal(starts,1);
});
test('framework Suspense filtering uses suspended/blocker evidence and preserves bounded page cursors',async()=>{
  const source=await readFile(new URL('../src/browser/reading/framework.js',import.meta.url),'utf8');let listener;
  const ops=[1,1,5,4,...Array.from('Acme',c=>c.codePointAt(0)),8,11,1,1,0,-1,8,12,1,1,0,-1];
  const ri={flushInitialOperations:()=>listener(ops),hasElementWithId:()=>true,inspectElement:(_renderer,id)=>({type:'full-data',value:{unknownSuspenders:id===11?2:0,owners:[]}})};
  const window={__REACT_DEVTOOLS_GLOBAL_HOOK__:{rendererInterfaces:new Map([[1,ri]]),renderers:new Map([[1,{rendererPackageName:'react-dom'}]]),getFiberRoots:()=>new Set([1]),sub:(_event,callback)=>{listener=callback;return()=>{listener=null;};}}};
  const registry=runInNewContext(`(${source})()`,{window,TextEncoder,setTimeout,clearTimeout,performance});
  const first=JSON.parse(await registry.query({kind:'suspense',onlyDynamic:true,offset:0,limit:1}));assert.equal(first.items[0].id,11);assert.equal(first.items[0].dynamic,true);assert.equal(first.items[0].inspection.unknownSuspenders,2);assert.equal(first.nextOffset,1);assert.equal(first.hasMore,true);
  const second=JSON.parse(await registry.query({kind:'suspense',onlyDynamic:true,offset:first.nextOffset,limit:1}));assert.equal(second.items.length,0);assert.equal(second.nextOffset,2);assert.equal(second.hasMore,false);registry.close();
});
test('framework inspection bounds cyclic values and avoids application getters',async()=>{
  const source=await readFile(new URL('../src/browser/reading/framework.js',import.meta.url),'utf8');
  const value={props:{data:{secret:'x'.repeat(8000)}}};value.props.self=value;Object.defineProperty(value.props,'getter',{enumerable:true,get(){throw Error('Getter executed');}});
  const ri={hasElementWithId:id=>id===17,inspectElement:()=>({type:'full-data',value}),getDisplayNameForElementID:()=> 'Acme'};
  const window={__REACT_DEVTOOLS_GLOBAL_HOOK__:{rendererInterfaces:new Map([[1,ri]]),renderers:new Map([[1,{rendererPackageName:'react-dom'}]]),getFiberRoots:()=>new Set([1]),sub:()=>()=>{}}};
  const registry=runInNewContext(`(${source})()`,{window,TextEncoder,setTimeout,clearTimeout,performance});
  const report=JSON.parse(await registry.query({kind:'inspect',id:17}));assert.equal(report.truncated,true);assert.equal(report.data.props.getter,'[accessor]');assert.equal(report.data.props.self,'[reference]');registry.close();
});
test('bundled developer validator matches its provenance and validates local references without external retrieval',async()=>{
  const source=await readFile(new URL('../src/browser/reading/developer-validator.js',import.meta.url),'utf8');
  const pin=JSON.parse(await readFile(new URL('../src/browser/reading/developer-validator-source.json',import.meta.url),'utf8'));
  assert.equal(createHash('sha256').update(source).digest('hex'),pin.outputSha256);
  const Validator=runInNewContext(source,{console});const validator=new Validator();
  const validate=validator.compile({type:'object',definitions:{amount:{type:'integer',minimum:1}},properties:{amount:{$ref:'#/definitions/amount'}},required:['amount'],additionalProperties:false});
  assert.equal(validate({amount:2}),true);assert.equal(validate({amount:'2'}),false);assert.throws(()=>validator.compile({$ref:'https://example.com/private-schema'}),/resolve reference/);
});
test('developer discovery captures the registered callback and schema, validates before invocation and releases its closures',async()=>{
  const [source,validator]=await Promise.all(['developer-tools.js','developer-validator.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  let calls=0,late;const tool={name:'Acme count',description:'Acme counter',inputSchema:{type:'object',properties:{amount:{type:'integer',minimum:1}},required:['amount'],additionalProperties:false},execute:params=>{calls++;return {amount:params.amount+1};}};
  const window={dispatchEvent:event=>{assert.equal(event.type,'devtoolstooldiscovery');late=event.respondWith;event.respondWith({name:'Acme',tools:[tool]});}};
  const registry=await runInNewContext(`(${source})(${validator})`,{window,CustomEvent:class{constructor(type){this.type=type;}},TextEncoder,setTimeout,console});
  const metadata=JSON.parse(registry.describe()),id=metadata.groups[0].tools[0].toolId;
  tool.execute=()=>{throw Error('Replacement executed');};tool.inputSchema.properties.amount.type='string';
  assert.equal(JSON.parse(await registry.validate(id,{amount:'wrong'})).valid,false);assert.equal(calls,0);
  assert.equal(JSON.parse(await registry.validate(id,{amount:2})).valid,true);assert.deepEqual(registry.invoke(id,{amount:2}),{amount:3});assert.equal(calls,1);
  late({name:'late',tools:[tool]});assert.equal(JSON.parse(registry.describe()).groups.length,1);
  registry.close();assert.throws(()=>registry.invoke(id,{amount:2}),/closed/);assert.equal(calls,1);
});
test('developer tools validate element handle placeholders and refuse detached or foreign elements before invocation',async()=>{
  const [source,validator]=await Promise.all(['developer-tools.js','developer-validator.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  const document={},HTMLElement=class{};let calls=0;
  const tool={name:'Acme element',description:'Inspect an element',inputSchema:{type:'object',properties:{element:{'x-mcp-type':'HTMLElement'}},required:['element'],additionalProperties:false},execute:({element})=>{calls++;return element.id;}};
  const window={dispatchEvent:event=>event.respondWith({name:'Acme',tools:[tool]})};
  const registry=await runInNewContext(`(${source})(${validator})`,{window,document,HTMLElement,CustomEvent:class{},TextEncoder,setTimeout,console});
  const metadata=JSON.parse(registry.describe()).groups[0].tools[0],id=metadata.toolId;
  assert.deepEqual(metadata.elementParameters,['element']);
  assert.equal(JSON.parse(await registry.validate(id,{element:{elementHandle:{connectionId:'Acme',id:'element',generation:'document'}}})).valid,true);
  assert.equal(JSON.parse(await registry.validate(id,{element:{elementHandle:{id:'element'}}})).valid,false);
  const element=Object.assign(new HTMLElement(),{ownerDocument:document,isConnected:true,id:'Acme'});
  assert.equal(registry.invoke(id,{element}),'Acme');assert.equal(calls,1);
  element.isConnected=false;assert.throws(()=>registry.invoke(id,{element}),/detached/);
  element.isConnected=true;element.ownerDocument={};assert.throws(()=>registry.invoke(id,{element}),/another document/);assert.equal(calls,1);
  registry.close();
});
test('developer tools resolve nested elements through local references, arrays and successful schema branches',async()=>{
  const [source,validator]=await Promise.all(['developer-tools.js','developer-validator.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  const document={},HTMLElement=class{};let calls=0;
  const elementSchema={'x-mcp-type':'HTMLElement'};
  const tool={name:'Acme nested',description:'Nested DOM inputs',inputSchema:{type:'object',definitions:{node:elementSchema,tree:{type:'object',properties:{node:{$ref:'#/definitions/node'},child:{$ref:'#/definitions/tree'}},additionalProperties:false}},properties:{box:{type:'object',properties:{nodes:{type:'array',items:{$ref:'#/definitions/node'}}},required:['nodes'],additionalProperties:false},choice:{oneOf:[{type:'object',properties:{kind:{const:'dom'},node:elementSchema},required:['kind','node'],additionalProperties:false},{type:'object',properties:{kind:{const:'plain'},node:{type:'object'}},required:['kind','node'],additionalProperties:false}]},tree:{$ref:'#/definitions/tree'}},required:['box','choice'],additionalProperties:false},execute:params=>{calls++;return params;}};
  const window={dispatchEvent:event=>event.respondWith({name:'Acme',tools:[tool]})};
  const registry=await runInNewContext(`(${source})(${validator})`,{window,document,HTMLElement,CustomEvent:class{},TextEncoder,setTimeout,console});
  const id=JSON.parse(registry.describe()).groups[0].tools[0].toolId;
  const placeholder={elementHandle:{connectionId:'Acme',id:'element',generation:'document'}};
  const params={box:{nodes:[placeholder,placeholder]},choice:{kind:'plain',node:placeholder},tree:{child:{node:placeholder}}};
  const checked=JSON.parse(await registry.validate(id,params));
  assert.equal(checked.valid,true,JSON.stringify(checked));
  assert.deepEqual(checked.elementPaths,[['box','nodes','0'],['box','nodes','1'],['tree','child','node']]);
  assert.equal(calls,0);
  const element=Object.assign(new HTMLElement(),{ownerDocument:document,isConnected:true});
  const resolved={box:{nodes:[element,element]},choice:params.choice,tree:{child:{node:element}}};
  assert.equal(registry.invoke(id,resolved,checked.elementPaths).choice.node,placeholder);assert.equal(calls,1);
  element.isConnected=false;assert.throws(()=>registry.invoke(id,resolved,checked.elementPaths),/detached/);assert.equal(calls,1);
  assert.equal(JSON.parse(await registry.validate(id,{box:{nodes:Array(33).fill(placeholder)},choice:params.choice})).valid,false);
  assert.equal(JSON.parse(await registry.validate(id,{box:{nodes:[{}]},choice:params.choice})).valid,false);
  registry.close();
});
test('developer element paths respect nested schema resource IDs and escaped property names',async()=>{
  const [source,validator]=await Promise.all(['developer-tools.js','developer-validator.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  const tool={name:'Acme resources',description:'Scoped references',inputSchema:{type:'object',definitions:{node:{type:'string'}},properties:{container:{$id:'https://example.com/acme-schema',type:'object',definitions:{node:{'x-mcp-type':'HTMLElement'}},properties:{'a/b~%':{$ref:'#/definitions/node'}},required:['a/b~%'],additionalProperties:false}},required:['container'],additionalProperties:false},execute:()=>true};
  tool.inputSchema.properties.container.$ref='#/definitions/base';
  tool.inputSchema.properties.container.definitions.base={type:'object'};
  const registry=await runInNewContext(`(${source})(${validator})`,{window:{dispatchEvent:event=>event.respondWith({name:'Acme',tools:[tool]})},CustomEvent:class{},TextEncoder,setTimeout,console});
  const id=JSON.parse(registry.describe()).groups[0].tools[0].toolId;
  const checked=JSON.parse(await registry.validate(id,{container:{'a/b~%':{elementHandle:{connectionId:'Acme',id:'element',generation:'document'}}}}));
  assert.equal(checked.valid,true,JSON.stringify(checked));assert.deepEqual(checked.elementPaths,[['container','a/b~%']]);registry.close();
});
test('developer discovery bounds definitions and accepts microtask registration without late mutation',async()=>{
  const [source,validator]=await Promise.all(['developer-tools.js','developer-validator.js'].map(name=>readFile(new URL(`../src/browser/reading/${name}`,import.meta.url),'utf8')));
  const window={dispatchEvent:event=>{Promise.resolve().then(()=>event.respondWith({name:'Acme',tools:Array.from({length:40},(_,index)=>({name:`Acme ${index}`,description:'Acme',inputSchema:{type:'object'},execute:()=>index}))}));}};
  const registry=await runInNewContext(`(${source})(${validator})`,{window,CustomEvent:class{},TextEncoder,setTimeout,console});
  const metadata=JSON.parse(registry.describe());assert.equal(metadata.truncated,true);assert.equal(metadata.groups[0].tools.length,32);registry.close();
});
test('native CSS bridge preserves cascade decisions, inherited declarations and wrappers with pagination',async()=>{
  const source=(await readFile(new URL('../src/browser/reading/css.js',import.meta.url),'utf8')).replace("await import('devtools://devtools/bundled/core/sdk/sdk.js')",'sdk');
  const styles=[false,true].map((inherited,index)=>({type:'Regular',inherited,parentRule:{selectorText:()=>index?'body':'#Acme',resourceURL:()=> 'https://example.com/style.css?private=yes',lineNumberInSource:()=>4,origin:'regular',layers:[{text:'Acme'}],media:[{text:'screen'}]},leadingProperties:()=>[{name:'color',value:'red',state:index?'Overloaded':'Active'},{name:'--Acme',value:'value',state:'Active'}]}));
  const inheritedProperties=styles[1].leadingProperties();inheritedProperties[1].value='x'.repeat(5000);styles[1].leadingProperties=()=>inheritedProperties;
  const matched={nodeStyles:()=>styles,isInherited:style=>style.inherited,propertyState:property=>property.state};
  const dom={pushNodesByBackendIdsToFrontend:async ids=>{assert.deepEqual([...ids],[17]);return new Map([[17,{enclosingElementOrSelf:()=>({id:9})}]]);}};
  const css={getMatchedStyles:async id=>{assert.equal(id,9);return matched;}};
  const sdk={TargetManager:{TargetManager:{instance:()=>({targets:()=>[{id:()=> 'foreign',model:()=>{throw Error('Foreign target accessed');}},{id:()=> 'Acme',model:type=>type==='dom'?dom:css}]})}},DOMModel:{DOMModel:'dom'},CSSModel:{CSSModel:'css'}};
  const result=await runInNewContext(`(${source})('Acme',17,1,1)`,{sdk,URL,TextEncoder});
  assert.equal(result.total,2);assert.equal(result.hasMore,false);assert.equal(result.items[0].inherited,true);assert.equal(result.items[0].declarations[0].state,'Overloaded');assert.equal(result.items[0].sourceUrl,'https://example.com/style.css');assert.equal(result.items[0].wrappers[0].kind,'layer');assert.equal(result.items[0].line,5);assert.equal(result.truncated,true);assert.equal(result.items[0].declarations[1].value.length,2048);
});
test('native CSS bridge refuses foreign targets',async()=>{
  const source=(await readFile(new URL('../src/browser/reading/css.js',import.meta.url),'utf8')).replace("await import('devtools://devtools/bundled/core/sdk/sdk.js')",'sdk');
  const sdk={TargetManager:{TargetManager:{instance:()=>({targets:()=>[]})}}};
  await assert.rejects(runInNewContext(`(${source})('Acme',17,0,10)`,{sdk,URL,TextEncoder}),/CSS_TARGET_UNAVAILABLE/);
});
test('native trace vitals preserve high-percentile interaction selection and unavailable observations',async()=>{
  const source=await readFile(new URL('../src/browser/reading/performance.js',import.meta.url),'utf8');
  for(const observed of [true,false]){
    const result=await runInNewContext(`(async()=>{const model=observed?{LCPBreakdown:{lcpEvent:{},lcpMs:42},INPBreakdown:{longestInteractionEvent:{dur:500000},highPercentileInteractionEvent:{dur:100000}},CLSCulprits:{}}:{};globalThis[Symbol.for('Acme')]={id:'analysis',trace:{insights:new Map([['NAV',{frameId:'Acme',bounds:{min:0,max:1},model}]])},module:{Insights:{Common:{getCLS:()=>({value:0.125})}}}};return (${source})('Acme','query','analysis',{kind:'vitals',insightSetId:'NAV'});})()`,{observed,TextEncoder,URL});
    assert.equal(result.data.INP.available,observed);assert.equal(result.data.INP.value,observed?100:null);assert.equal(result.data.LCP.value,observed?42:null);assert.equal(result.data.CLS.value,observed?0.125:null);assert.equal(result.data.fieldData,false);assert.equal(result.data.pageLifetime,false);
    if(observed)assert.equal(result.data.longestInteraction.durationMs,500);
  }
});
test('native trace insight details bound cyclic maps without executing lazy overlays and release all model owners',async()=>{
  const source=await readFile(new URL('../src/browser/reading/performance.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{const calls=[];const event={name:'Acme'};event.self=event;globalThis[Symbol.for('Acme')]={id:'analysis',parts:[],trace:{insights:new Map([['NAV',{model:{Acme:{relatedEvents:new Map([[event,['label']]]),createOverlays:()=>{throw Error('Overlay executed');}}}}]])},model:{deleteTraceByIndex:id=>calls.push(id),resetProcessor:()=>calls.push('reset')},module:{Helpers:{SyntheticEvents:{SyntheticEventsManager:{reset:()=>calls.push('synthetic-reset')}}}}};const bridge=${source};const detail=await bridge('Acme','query','analysis',{kind:'insights',insightSetId:'NAV',insight:'Acme'});let unknown=false;try{await bridge('Acme','query','analysis',{kind:'insights',insightSetId:'NAV',insight:'constructor'});}catch(error){unknown=error.message.endsWith('UNKNOWN_PERFORMANCE_INSIGHT');}await bridge('Acme','release','analysis',null);return {detail,unknown,calls,released:!globalThis[Symbol.for('Acme')]};})()`,{TextEncoder,URL});
  assert.equal(result.detail.truncated,true);assert.equal(result.detail.data.insight.relatedEvents[0][0].self,'[circular]');assert.equal(result.unknown,true);assert.equal(result.released,true);assert.deepEqual(Array.from(result.calls),[0,'reset','synthetic-reset']);
});
test('native trace load summaries disclose redacted navigation metadata and insight errors by name',async()=>{
  const source=await readFile(new URL('../src/browser/reading/performance.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={id:'analysis',trace:{traceEvents:[{}],data:{Meta:{traceBounds:{min:0,max:1},mainFrameURL:'https://Acme:private@example.com/path?secret=yes#private'}},insights:new Map([['NAV',{url:new URL('https://Acme:private@example.com/path?secret=yes'),frameId:'Acme',bounds:{min:0,max:1},model:{Acme:{}},modelErrors:{AcmeFailure:new Error('Acme private')}}]])}};return (${source})('Acme','query','analysis',{kind:'summary'});})()`,{TextEncoder,URL});
  assert.equal(result.data.mainFrameUrl,'https://example.com/path');assert.equal(result.data.insightSets[0].url,'https://example.com/path');assert.deepEqual(Array.from(result.data.insightSets[0].failedInsights),['AcmeFailure']);assert.doesNotMatch(JSON.stringify(result),/private|secret/);
});
test('heap detailed differences preserve added and deleted snapshot identities and summary indices',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  const diffs={Acme:{name:'Acme',addedCount:2,removedCount:1,sizeDelta:20,addedIds:[17,19],addedSelfSizes:[10,30],deletedIds:[11],deletedSelfSizes:[20]},Other:{name:'Other',addedCount:1,removedCount:0,sizeDelta:40}};
  const current={loader:null,proxy:{interfaceDefinitions:async()=>({}),calculateSnapshotDiff:async(uid)=>{assert.equal(uid,7);return diffs;}}},base={uid:7,loader:null,proxy:{aggregatesForDiff:async()=>({})}};
  const invoke=argument=>runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['current',current],['base',base]])};return (${source})('Acme','compare','current',argument);})()`,{current,base,argument,TextEncoder});
  const summary=(await invoke({base:'base',offset:1,limit:1})).data;assert.equal(summary.classes[0].classIndex,1);assert.equal(summary.classes[0].name,'Acme');
  assert.equal(summary.totals.addedCount,3);assert.equal(summary.totals.removedCount,1);assert.equal(summary.totals.sizeDelta,60);
  assert.equal((await invoke({base:'base',offset:1,limit:1,maxNameLength:2})).data.classes[0].name,'Ac');
  const detail=(await invoke({base:'base',classIndex:1,offset:1,limit:1})).data;
  assert.equal(detail.objects[0].id,11);assert.equal(detail.objects[0].change,'deleted');assert.equal(detail.total,3);assert.equal(detail.hasMore,true);assert.equal(detail.addedObjectSnapshot,'current');assert.equal(detail.deletedObjectSnapshot,'base');
  await assert.rejects(invoke({base:'base',classIndex:2,offset:0,limit:1}),/UNKNOWN_HEAP_DIFF_CLASS/);
});
test('heap retention filters use native node indices for exact context attribution',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  const filters=[],proxy={nodeIndexForId:async id=>id===17?9:undefined,aggregatesWithFilter:async filter=>{filters.push(filter.filterName);return {Acme:{name:'Acme',count:1,self:10,maxRet:20}};},createNodesProviderForClass:(key,filter)=>{assert.equal(key,'Acme');filters.push(filter.filterName);return {serializeItemsRange:async()=>({items:[]}),dispose:()=>filters.push('disposed')};}};
  const entry={proxy,loader:null,module:{HeapSnapshotModel:{NodeFilter:class{}}}};
  const context={entry,TextEncoder};
  const invoke=query=>runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',entry]])};return (${source})('Acme','query','snapshot',query);})()`,{...context,query});
  for(const filterName of ['objectsRetainedByDetachedDomNodes','objectsRetainedByConsole','objectsRetainedByEventHandlers','objectsRetainedByContexts','sharedNativeContext','noNativeContext']){
    await invoke({kind:'classes',filterName});assert.equal(filters.at(-1),filterName);
  }
  await invoke({kind:'classes',filterName:'attributedToSpecificNativeContext',objectId:17});assert.equal(filters.at(-1),'nativeContext_9');
  await invoke({kind:'class_nodes',classId:1,filterName:'attributedToSpecificNativeContext',objectId:17});assert.deepEqual(filters.slice(-2),['nativeContext_9','disposed']);
  await assert.rejects(invoke({kind:'classes',filterName:'attributedToSpecificNativeContext',objectId:18}),/UNKNOWN_HEAP_NODE/);
  await assert.rejects(invoke({kind:'classes',filterName:'attributedToSpecificNativeContext'}),/HEAP_QUERY_BOUNDS/);
});
test('heap class selectors preserve full keys and dispose instance providers',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  const key='Acme'.repeat(400),calls=[];
  const proxy={aggregatesWithFilter:async()=>({[key]:{name:'Acme',count:2,self:20,maxRet:30}}),createNodesProviderForClass:(actual)=>{assert.equal(actual,key);return {serializeItemsRange:async(start,end)=>{calls.push([start,end]);return {items:[{id:17}]};},dispose:()=>calls.push('disposed')};}};
  const entry={proxy,loader:null,module:{HeapSnapshotModel:{NodeFilter:class{}}}};
  const result=await runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',entry]])};const bridge=${source};const first=await bridge('Acme','query','snapshot',{kind:'classes'}),again=await bridge('Acme','query','snapshot',{kind:'classes'});const nodes=await bridge('Acme','query','snapshot',{kind:'class_nodes',classId:first.data.items[0].classId,offset:2,limit:3});let unknown=false;try{await bridge('Acme','query','snapshot',{kind:'class_nodes',classId:2});}catch(error){unknown=/UNKNOWN_HEAP_CLASS/.test(error.message);}return {first,again,nodes,unknown};})()`,{entry,TextEncoder});
  assert.equal(result.first.data.items[0].classKey.length,1024);assert.equal(result.first.data.items[0].classId,1);assert.equal(result.again.data.items[0].classId,1);assert.equal(result.nodes.data.items[0].id,17);assert.equal(result.unknown,true);assert.deepEqual(calls,[[2,5],'disposed']);assert.equal(entry.classKeys.length,1);
});
test('heap contexts filter and rank before pagination while preserving scope identities',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  const analysis={scriptsWithoutScopes:[{scriptNodeId:9}],scopes:[{scopeInfoNodeId:1,contexts:[{contextNodeId:11,deadFieldsRetainedSizeSum:10},{contextNodeId:12,deadFieldsRetainedSizeSum:30}]},{scopeInfoNodeId:2,contexts:[{contextNodeId:21,deadFieldsRetainedSizeSum:20}]}]};
  const invoke=query=>runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',{proxy:{analyzeContexts:async()=>analysis},loader:null}]])};return (${source})('Acme','query','snapshot',query);})()`,{analysis,query,TextEncoder});
  const ranked=(await invoke({kind:'contexts',retainedSize:{min:15,max:30},offset:1,limit:1})).data;
  assert.equal(ranked.totalContexts,2);assert.equal(ranked.scopes[0].scopeInfoNodeId,2);assert.equal(ranked.scopes[0].contexts[0].contextNodeId,21);assert.equal(ranked.hasMore,false);assert.equal(ranked.scoreIsReclaimableBytes,false);assert.equal(ranked.scriptsWithoutScopes[0].scriptNodeId,9);
  const selected=(await invoke({kind:'contexts',scopeInfoNodeId:1,limit:1})).data;
  assert.equal(selected.totalContexts,2);assert.equal(selected.scopes[0].contexts[0].contextNodeId,12);assert.equal(selected.hasMore,true);assert.equal(analysis.scopes[0].contexts.length,2);
});
test('heap query providers receive exact pagination and are disposed after success or failure',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  for(const failure of [false,true]){
    const calls=[];const provider={serializeItemsRange:async(start,end)=>{calls.push([start,end]);if(failure)throw Error('Acme provider failure');return {items:[{id:17,name:'Acme'}],totalLength:1};},dispose:()=>calls.push('disposed')};
    const proxy={queryObjects:options=>{assert.equal(options.className,'Acme');assert.equal(options.propertyName,'child');assert.equal(options.nodeType,'object');assert.equal(options.minRetainedSize,10);assert.equal(options.maxRetainedSize,20);assert.equal(options.minSelfSize,1);assert.equal(options.maxSelfSize,9);assert.equal(options.sortBy,'id');return provider;}};
    const pending=runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',{proxy,loader:null}]])};return (${source})('Acme','query','snapshot',{kind:'objects',className:'Acme',propertyName:'child',nodeType:'object',retainedSize:{min:10,max:20},selfSize:{min:1,max:9},sortBy:'id',offset:10,limit:10});})()`,{proxy,TextEncoder});
    if(failure)await assert.rejects(pending,/Acme provider failure/);else assert.equal((await pending).data.items[0].id,17);
    assert.deepEqual(calls,[[10,20],'disposed']);
  }
});
test('heap edge options and aggregate totals preserve native ordering before pagination',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  const calls=[],proxy={nodeIndexForId:async()=>3,createEdgesProvider:(node,options)=>{calls.push([node,options]);return {serializeItemsRange:async()=>({items:[]}),dispose:()=>calls.push('disposed')};},aggregatesWithFilter:async()=>({Acme:{name:'Acme',count:2,self:100,maxRet:200},Other:{name:'Other',count:3,self:10,maxRet:300}})};
  const entry={proxy,loader:null,module:{HeapSnapshotModel:{NodeFilter:class{}}}};
  const invoke=query=>runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',entry]])};return (${source})('Acme','query','snapshot',query);})()`,{entry,query,TextEncoder});
  await invoke({kind:'edges',nodeId:17,sortBy:'name',minRetainedSize:12,excludePrimitives:false});
  assert.equal(calls[0][0],3);assert.equal(calls[0][1].sortBy,'name');assert.equal(calls[0][1].minRetainedSize,12);assert.equal(calls[0][1].excludePrimitives,false);assert.equal(calls[1],'disposed');
  await invoke({kind:'edges',nodeId:17});assert.equal(calls[2][1].sortBy,'retainedSize');assert.equal(calls[2][1].excludePrimitives,true);
  const aggregates=(await invoke({kind:'classes',limit:1})).data;assert.equal(aggregates.items[0].name,'Other');assert.equal(aggregates.objectCount,5);assert.equal(aggregates.totalSelfSize,110);assert.equal(aggregates.total,2);assert.equal(aggregates.hasMore,true);
});
test('heap byte-limited provider pages keep complete rows and resume without skipping',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');let disposed=0;
  const proxy={queryObjects:()=>({serializeItemsRange:async(start,end)=>({startPosition:start,endPosition:end,totalLength:200,items:Array.from({length:end-start},(_,index)=>({id:start+index+17,name:'\0'.repeat(1024),type:'object'}))}),dispose:()=>disposed++})};
  const invoke=offset=>runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',{proxy,loader:null}]])};return (${source})('Acme','query','snapshot',{kind:'objects',offset,limit:100,maxNameLength:1024});})()`,{proxy,offset,TextEncoder});
  const first=await invoke(10),second=await invoke(first.data.nextOffset);
  assert.equal(first.truncated,true);assert.equal(first.data.endPosition,10+first.data.items.length);assert.equal(first.data.nextOffset,first.data.endPosition);assert.equal(first.data.hasMore,true);
  assert.equal(second.data.items[0].id,first.data.items.at(-1).id+1);assert.ok(Array.from(first.data.items).every(item=>item.type==='object'&&item.name.length===1024));assert.equal(disposed,2);
  for(const result of [first,second])assert.ok(Buffer.byteLength(JSON.stringify(result))<=65536);
});
test('heap grouped context pages resume at the first missing global rank after byte truncation',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  const field={name:'\0'.repeat(1024),valueName:'\0'.repeat(1024),valueNodeId:17,retainedSize:1};
  const analysis={scopes:[{scopeInfoNodeId:1,contexts:[{contextNodeId:101,deadFieldsRetainedSizeSum:300,deadFields:Array(4).fill(field)},{contextNodeId:103,deadFieldsRetainedSizeSum:100,deadFields:Array(2).fill(field)}]},{scopeInfoNodeId:2,contexts:[{contextNodeId:102,deadFieldsRetainedSizeSum:200,deadFields:[]}]}],scriptsWithoutScopes:[]};
  const invoke=offset=>runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',{proxy:{analyzeContexts:async()=>analysis},loader:null}]])};return (${source})('Acme','query','snapshot',{kind:'contexts',offset,limit:100,maxNameLength:1024});})()`,{analysis,offset,TextEncoder});
  const first=await invoke(0),second=await invoke(first.data.nextOffset);
  assert.equal(first.truncated,true);assert.equal(first.data.nextOffset,1);assert.equal(first.data.hasMore,true);assert.equal(first.data.scoreIsReclaimableBytes,false);assert.equal(second.data.nextOffset,null);assert.equal(second.data.hasMore,false);
  const ids=[first,second].flatMap(result=>Array.from(result.data.scopes).flatMap(scope=>Array.from(scope.contexts).map(context=>context.contextNodeId)));
  assert.deepEqual(ids,[101,102,103]);for(const result of [first,second])assert.ok(Buffer.byteLength(JSON.stringify(result))<=65536);
});
test('heap query results bound strings and bytes and reject unknown object identities',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  const invoke=(proxy,maxNameLength)=>runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',{proxy,loader:null}]])};return (${source})('Acme','query','snapshot',{kind:'object',nodeId:17,maxNameLength});})()`,{proxy,maxNameLength,TextEncoder});
  const proxy={nodeIndexForId:async()=>1,getObjectInfo:async()=>({name:'x'.repeat(2000)})};
  const bounded=await invoke(proxy);assert.equal(bounded.truncated,true);assert.equal(bounded.data.name.length,100);assert.equal((await invoke(proxy,1024)).data.name.length,1024);
  await assert.rejects(invoke({nodeIndexForId:async()=>undefined}),/UNKNOWN_HEAP_NODE/);
  for(const text of ['x'.repeat(1024),'\0'.repeat(1024),'😀'.repeat(1024)]){
    const prefix=await invoke({nodeIndexForId:async()=>1,getObjectInfo:async()=>Array(100).fill(text)});
    assert.equal(prefix.truncated,true);assert.ok(prefix.data.length>0&&prefix.data.length<100);assert.ok(Buffer.byteLength(JSON.stringify(prefix))<=65536);assert.ok(Array.from(prefix.data).every(item=>item===text));
  }
});
test('heap retaining searches forward exact limits and name projection bounds Unicode work',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');
  const calls=[];
  let chain={nodeName:'Acme leaf',children:[]};for(let i=0;i<8;i++)chain={nodeName:'Acme',children:[chain]};
  const proxy={nodeIndexForId:async id=>{assert.equal(id,17);return 3;},getRetainingPaths:async(...args)=>{calls.push(args);return [chain];},getObjectInfo:async()=>({name:'😀'.repeat(100000),type:'object'})};
  const paths=await runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',{proxy,loader:null}]])};return (${source})('Acme','query','snapshot',{kind:'retaining_paths',nodeId:17,maxDepth:10,maxNodes:23,maxSiblings:2,maxNameLength:10});})()`,{proxy,TextEncoder});
  assert.deepEqual(calls,[[3,10,23,2]]);let leaf=paths.data[0];for(let i=0;i<8;i++)leaf=leaf.children[0];assert.equal(leaf.nodeName,'Acme leaf');assert.equal(paths.truncated,false);
  await runInNewContext(`(async()=>{globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',{proxy,loader:null}]])};return (${source})('Acme','query','snapshot',{kind:'retaining_paths',nodeId:17});})()`,{proxy,TextEncoder});
  assert.deepEqual(calls[1],[3,undefined,undefined,undefined]);
  const short=await runInNewContext(`(async()=>{let inspected=0;const iterate=String.prototype[Symbol.iterator];String.prototype[Symbol.iterator]=function*(){const counted=String(this).startsWith('😀');for(const point of iterate.call(this)){if(counted&&++inspected>3)throw Error('Unbounded string traversal');yield point;}};globalThis[Symbol.for('Acme')]={items:new Map([['snapshot',{proxy,loader:null}]])};const result=await (${source})('Acme','query','snapshot',{kind:'object',nodeId:17,maxNameLength:2});return {result,inspected};})()`,{proxy,TextEncoder});
  assert.equal(short.result.data.name,'😀😀');assert.equal(short.result.data.type,'object');assert.equal(short.result.truncated,true);assert.equal(short.inspected,3);
});
test('releasing an owned heap model terminates its worker without deleting snapshot evidence',async()=>{
  const source=await readFile(new URL('../src/browser/reading/heap.js',import.meta.url),'utf8');let disposed=0;
  const result=await runInNewContext(`(async()=>{const owner={items:new Map([['snapshot',{worker}]])};globalThis[Symbol.for('Acme')]=owner;const released=await (${source})('Acme','release','snapshot',null);return {released,count:owner.items.size};})()`,{worker:{dispose:()=>disposed++},TextEncoder});
  assert.equal(result.released,true);assert.equal(result.count,0);assert.equal(disposed,1);
});
test('registered binding bridge preserves argument values, resolves once and closes without deleting application replacements',async()=>{
  const codec=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const bridge=await readFile(new URL('../src/browser/reading/binding.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{const codec=${codec};const install=${bridge};const sent=[];globalThis.acmeRaw=value=>sent.push(JSON.parse(value));install('acmeRaw','acme',codec);const pending=acme('Acme private',41);const owner=globalThis[Symbol.for('acmeRaw')];const first=owner.settle(sent[0].ticket,{n:42},false);const value=await pending;const duplicate=owner.settle(sent[0].ticket,{n:99},false);const replacement=()=>17;globalThis.acme=replacement;owner.close();return {value,first,duplicate,replacement:acme===replacement,args:sent[0].args};})()`,{TextEncoder,URL,btoa,atob});
  assert.equal(result.value,42);assert.equal(result.first,true);assert.equal(result.duplicate,false);assert.equal(result.replacement,true);assert.equal(result.args[0].s,'Acme private');assert.equal(result.args[1].n,41);
});
test('registered binding bridge bounds outstanding calls and bytes and rejects outstanding promises on close',async()=>{
  const codec=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const bridge=await readFile(new URL('../src/browser/reading/binding.js',import.meta.url),'utf8');
  const result=await runInNewContext(`(async()=>{const codec=${codec};const install=${bridge};let sent=0;globalThis.acmeRaw=()=>sent++;install('acmeRaw','acme',codec);const oversized=await acme('"'.repeat(600000)).catch(error=>error.message);const pending=Array.from({length:64},()=>acme(1).catch(error=>error.message));const capacity=await acme(2).catch(error=>error.message);globalThis[Symbol.for('acmeRaw')].close();return {sent,oversized,capacity,closed:await Promise.all(pending),removed:typeof acme};})()`,{TextEncoder,URL,btoa,atob});
  // The value codec and the payload check share the 1 MiB bound; either refuses before sending.
  assert.equal(result.sent,64);assert.match(result.oversized,/bounds/i);assert.match(result.capacity,/capacity/);assert.equal(result.removed,'undefined');assert.ok(Array.from(result.closed).every(message=>message.includes('closed')));
});
test('registered binding bridge preserves typed success and typed thrown values once',async()=>{
  const codec=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const bridge=await readFile(new URL('../src/browser/reading/binding.js',import.meta.url),'utf8');
  const cases=JSON.parse(await readFile(new URL('../tests/fixtures/browser-binding-values.json',import.meta.url),'utf8'));
  for(const mode of [false,'value'])for(const item of cases){
    const result=await runInNewContext(`(async()=>{const codec=${codec};let sent;globalThis.acmeRaw=value=>sent=JSON.parse(value);(${bridge})('acmeRaw','acme',codec);const make=(${item.make}),check=(${item.check});const pending=acme(make()).then(value=>({outcome:'resolved',matches:check(value)}),value=>({outcome:'rejected',matches:check(value)}));const owner=globalThis[Symbol.for('acmeRaw')];const encoded=codec.encodeBinding(make());const settled=owner.settle(sent.ticket,encoded,${JSON.stringify(mode)});const duplicate=owner.settle(sent.ticket,encoded,${JSON.stringify(mode)});return {genericStack:codec.encode(make()).e?.s,result:await pending,settled,duplicate,argument:check(codec.decode(sent.args[0],[]))};})()`,{TextEncoder,URL,btoa,atob});
    assert.equal(result.result.outcome,mode===false?'resolved':'rejected',item.name);
    assert.equal(result.result.matches,true,item.name);
    assert.equal(result.argument,true,item.name);assert.equal(result.settled,true);assert.equal(result.duplicate,false);
    if(item.name==='error')assert.equal(result.genericStack,'Acme private binding stack');
  }
});
test('native ArrayBuffer projection matches public SDK copies without reading own properties',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const foreign=runInNewContext('Uint8Array.from([0,255,128,65]).buffer');
  const execute=body=>runInNewContext(`(()=>{const codec=${source};${body}})()`,{TextEncoder,URL,btoa,atob,structuredClone,foreign});
  const result=JSON.parse(execute("let getters=0;const buffer=Uint8Array.from([0,255,128,65]).buffer;Object.defineProperty(buffer,'secret',{enumerable:true,get(){getters++;throw Error('Acme');}});return JSON.stringify({wire:codec.encode(buffer),graph:codec.encode({a:buffer,b:buffer}),foreign:codec.encode(foreign),empty:codec.encode(new ArrayBuffer(0)),getters,bytes:Array.from(new Uint8Array(buffer))});"));
  assert.deepEqual(result,{wire:{id:1,o:[]},graph:{id:1,o:[{k:'a',v:{id:2,o:[]}},{k:'b',v:{id:3,o:[]}}]},foreign:{id:1,o:[]},empty:{id:1,o:[]},getters:0,bytes:[0,255,128,65]});
  assert.throws(()=>execute('return codec.encode(new Uint8Array(524289));'),/VALUE_BOUNDS/);
});
test('native detached ArrayBuffers preserve generic-result versus strict-read semantics',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const execute=body=>runInNewContext(`(()=>{const codec=${source};const buffer=new ArrayBuffer(4);structuredClone(buffer,{transfer:[buffer]});${body}})()`,{TextEncoder,URL,btoa,atob,structuredClone});
  assert.deepEqual(JSON.parse(execute('return JSON.stringify(codec.encodeResult(buffer));')),{v:'undefined'});
  assert.deepEqual(JSON.parse(execute('return JSON.stringify(codec.encodeResult({buffer}));')),{v:'undefined'});
  assert.throws(()=>execute('return codec.encode(buffer);'),/detached/i);
  assert.throws(()=>execute('return codec.encodeBinding(buffer);'),/detached/i);
  assert.throws(()=>execute('return codec.encodeResult(new Uint8Array(524289));'),/VALUE_BOUNDS/);
  assert.throws(()=>execute('return codec.encodeResult(Number.MAX_VALUE);'),/UNSAFE_INTEGER/);
});
test('native generic evaluation normalizes function and method sources without invoking caller code',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const context={calls:0,TextEncoder,URL,btoa,atob};
  for(const expression of [
    'acme(value) { calls++; return value; }',
    'async acme(value) { calls++; return value; }',
    'function acme(value) { calls++; return value; }',
    'async function acme(value) { calls++; return value; }',
    'value => { calls++; return value; }',
  ]) {
    const before=context.calls;
    const fn=runInNewContext(`(0,eval)((${source}).normalize(${JSON.stringify(expression)},true))`,context);
    assert.equal(context.calls,before);
    assert.equal(await fn(undefined),undefined);
    assert.equal(context.calls,before+1);
    assert.equal(await fn(null),null);
    assert.equal(context.calls,before+2);
  }
  const before=context.calls;
  assert.throws(()=>runInNewContext(`(${source}).normalize('acme( {',true)`,context),/INVALID_EVALUATION_FUNCTION/);
  assert.equal(context.calls,before);
  const fn=runInNewContext(`(0,eval)((${source}).normalize('function acme(){calls++;}',false))`,context);
  assert.equal(typeof fn,'function');assert.equal(context.calls,before);
  assert.equal(runInNewContext(`(0,eval)((${source}).normalize('calls++; 42',false))`,context),42);
  assert.equal(context.calls,before+1);
});
test('native evaluation bridge preserves cycles, special values and typed-array offsets',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const encoded=runInNewContext(`(()=>{const codec=${source};const value={number:-0,big:12345678901234567890n,date:new Date('2026-10-28T00:00:00.000Z'),array:new Uint16Array([9,10,11]).subarray(1)};value.self=value;return codec.encode(value);})()`,{TextEncoder,URL,btoa,atob});
  assert.equal(encoded.o.find(entry=>entry.k==='self').v.ref,encoded.id);
  assert.equal(encoded.o.find(entry=>entry.k==='number').v.v,'-0');
  assert.equal(encoded.o.find(entry=>entry.k==='big').v.bi,'12345678901234567890');
  assert.equal(encoded.o.find(entry=>entry.k==='array').v.ta.b,Buffer.from(new Uint16Array([10,11]).buffer).toString('base64'));
});
test('native evaluation bridge resolves graph and handle arguments without prototype mutation',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const result=runInNewContext(`(()=>{const codec=${source};const value=codec.decode({id:1,o:[{k:'self',v:{ref:1}},{k:'handle',v:{h:0}},{k:'__proto__',v:{s:'Acme'}}]},[{value:42}]);return [value.self===value,value.handle,value.__proto__,Object.getPrototypeOf(value)===Object.prototype];})()`,{TextEncoder,URL,btoa,atob});
  assert.deepEqual(Array.from(result),[true,42,'Acme',true]);
});
test('native evaluation bridge refuses bounded graph/string overflows',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  for(const expression of ["'x'.repeat(1048576)",'Array(10001).fill(1)','9007199254740992']) {
    assert.throws(()=>runInNewContext(`(${source}).encode(${expression})`,{TextEncoder,URL,btoa,atob}),/VALUE_CONVERSION_REQUIRED|VALUE_BOUNDS|UNSAFE_INTEGER/);
  }
});
test('native public object projection keeps enumerable data and shared collection identity',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const result=runInNewContext(`(()=>{const codec=${source};const map=Object.assign(new Map([['Acme',42]]),{own:13});const input={prototype:Object.assign(Object.create({inherited:1}),{own:42}),map,set:Object.assign(new Set([42]),{own:13}),alias:map};const value=codec.decode(codec.encode(input),[]);return JSON.stringify({value,same:value.map===value.alias});})()`,{TextEncoder,URL,btoa,atob});
  assert.deepEqual(JSON.parse(result),{value:{prototype:{own:42},map:{own:13},set:{own:13},alias:{own:13}},same:true});
  const foreign=runInNewContext("new Date('2026-11-09T00:00:00.000Z')");
  assert.throws(()=>runInNewContext(`(${source}).encode(foreign)`,{TextEncoder,URL,btoa,atob,foreign}),/VALUE_CONVERSION_REQUIRED/);
  for(const kind of ['Node','Window'])assert.throws(()=>runInNewContext(`class ${kind} {}; (${source}).encode(new ${kind}())`,{TextEncoder,URL,btoa,atob}),/VALUE_CONVERSION_REQUIRED/);
});
test('native ordinary function and symbol results project to undefined without registering callbacks',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const result=JSON.parse(runInNewContext(`(()=>{const codec=${source};const value=codec.decode(codec.encode({fn:function Acme(){},symbol:Symbol('Acme')}),[]);return JSON.stringify({fn:codec.encode(function Acme(){}),symbol:codec.encode(Symbol('Acme')),keys:Object.keys(value),undefinedValues:Object.values(value).every(v=>v===undefined)});})()`,{TextEncoder,URL,btoa,atob}));
  assert.deepEqual(result,{fn:{v:'undefined'},symbol:{v:'undefined'},keys:['fn','symbol'],undefinedValues:true});
});
test('native public property projection skips throwing getters and preserves pinned toJSON results',async()=>{
  const source=await readFile(new URL('../src/browser/reading/evaluation.js',import.meta.url),'utf8');
  const execute=body=>runInNewContext(`(()=>{const codec=${source};${body}})()`,{TextEncoder,URL,btoa,atob});
  const result=JSON.parse(execute("let reads=0;const getter={get refused(){reads++;throw Error('Acme')},own:42};const enumerable={toJSON(){throw Error('Acme')},own:42};const hidden=Object.defineProperty({},'toJSON',{value(){return {own:42}}});const throwing=Object.defineProperty({},'toJSON',{value(){throw Error('Acme')}});const self=Object.defineProperty({},'toJSON',{value(){return this}});const value=codec.decode(codec.encode({getter,enumerable,hidden,throwing,self}),[]);return JSON.stringify({value,keys:Object.keys(value),selfUndefined:value.self===undefined,reads,rootSelf:codec.encode(self)});"));
  assert.deepEqual(result,{value:{getter:{own:42},enumerable:{toJSON:{},own:42},hidden:{own:42},throwing:{}},keys:['getter','enumerable','hidden','throwing','self'],selfUndefined:true,reads:1,rootSelf:{v:'undefined'}});
  assert.throws(()=>execute("return codec.encode(Object.assign(new Map(),{own:'x'.repeat(1048576)}));"),/VALUE_BOUNDS/);
  assert.throws(()=>execute("return codec.encode({get own(){return Number.MAX_VALUE}});"),/UNSAFE_INTEGER/);
  assert.throws(()=>execute("return codec.encode(Array.from({length:5000},()=>({toJSON(){}})));"),/VALUE_BOUNDS/);
});
test('Mac editing maps are extracted as JSON data without executing source',()=>{
  const source='// packages/playwright-core/src/server/macEditingCommands.ts\n    macEditingCommands = {\n      "Meta+KeyA":"selectAll:"\n    };\n// next section\n';
  assert.equal(JSON.parse(macEditingCommands(source))['Meta+KeyA'],'selectAll:');
  assert.throws(()=>macEditingCommands(source.replace('"selectAll:"','malicious()')));
});
const bundle = literal => `${prefix}var init = __esm({\n  init() {\n    source5 = '${literal}';\n  }\n});\n// next section\n`;

test('clipboard origin and secure-context guards run before touching the system clipboard',async()=>{
  const source=await readFile(new URL('../src/browser/reading/clipboard.js',import.meta.url),'utf8');
  let touched=0;
  const clipboard={readText(){++touched;throw Error('must not read')},writeText(){++touched;throw Error('must not write')}};
  for(const action of [{kind:'read'},{kind:'write',text:'Acme'}]) {
    const wrong=runInNewContext(`(${source})`,{location:{origin:'https://example.com'},isSecureContext:true,navigator:{clipboard}});
    assert.equal((await wrong({origin:'https://other.example',action})).error,'CLIPBOARD_ORIGIN_MISMATCH');
    const insecure=runInNewContext(`(${source})`,{location:{origin:'http://example.com'},isSecureContext:false,navigator:{clipboard}});
    assert.equal((await insecure({origin:'http://example.com',action})).error,'CLIPBOARD_UNAVAILABLE');
  }
  assert.equal(touched,0);
});

test('clipboard reads and writes preserve bounded text and report permission refusal',async()=>{
  const source=await readFile(new URL('../src/browser/reading/clipboard.js',import.meta.url),'utf8');
  let value='Acme 🙂';
  const clipboard={async readText(){return value},async writeText(text){value=text}};
  const run=runInNewContext(`(${source})`,{location:{origin:'https://example.com'},isSecureContext:true,navigator:{clipboard},TextEncoder});
  assert.equal((await run({origin:'https://example.com',action:{kind:'read'}})).text,value);
  assert.equal((await run({origin:'https://example.com',action:{kind:'write',text:'Acme changed'}})).written,true);
  assert.equal(value,'Acme changed');
  value='x'.repeat(16385);
  assert.equal((await run({origin:'https://example.com',action:{kind:'read'}})).error,'CLIPBOARD_BOUNDS');
  clipboard.readText=async()=>{throw Error('denied')};
  assert.equal((await run({origin:'https://example.com',action:{kind:'read'}})).error,'CLIPBOARD_PERMISSION_REQUIRED');
});

test('element registry keeps identity through shadow scopes and refuses detached or evicted nodes', async () => {
  const source=await readFile(new URL('../src/browser/reading/references.js',import.meta.url),'utf8');
  let next=0;
  const registry=runInNewContext(source,{crypto:{getRandomValues:bytes=>{new DataView(bytes.buffer).setUint32(0,++next);return bytes;}}});
  const document={}; const host={parentNode:document}; const shadow={host};
  const element={isConnected:true,parentNode:shadow};
  const frame={id:'frame',generation:'document'};
  const locator=registry.retain(element,frame);const token=`frame.document.${locator.id}`;
  assert.equal(registry.query(document,token),element);
  assert.equal(registry.queryAll({},token).length,0);
  element.isConnected=false;
  assert.throws(()=>registry.query(document,token),/STALE_REFERENCE/);
  element.isConnected=true;
  for(let i=0;i<512;i++)registry.retain({isConnected:true,parentNode:document},frame);
  assert.throws(()=>registry.query(document,token),/STALE_REFERENCE/);
});

test('browser helper extraction decodes literal bytes without executing upstream code', () => {
  const source = `${exportLine}\\nconst name=\\'Acme\\';\\nconst path=\\'C:\\\\Acme\\';\\n`;
  assert.equal(injectedSource(bundle(source)), `${exportLine}\nconst name='Acme';\nconst path='C:\\Acme';\n`);
});

test('browser helper extraction refuses changed declarations, escapes and trailing execution', () => {
  assert.throws(() => injectedSource(''), /Missing pinned/);
  assert.throws(() => injectedSource(bundle('not the module')), /Unexpected injected module/);
  assert.throws(() => injectedSource(bundle(exportLine + '\\x41')), /Unsupported injected/);
  assert.throws(() => injectedSource(bundle(exportLine).replace("';", "'; malicious();")), /Unexpected injected source suffix/);
});

test('bounded ARIA output retains valid Unicode at every truncation boundary', async () => {
  const source=await readFile(new URL('../src/browser/reading/truncate.js',import.meta.url),'utf8');
  const bound=runInNewContext(`(${source})`);
  const text='Acme 🙂 東京';
  for(let bytes=4;bytes<=64;bytes+=4) {
    const result=bound(text,bytes);
    assert.equal(JSON.parse(JSON.stringify(result)).value,result.value);
    assert.equal(result.value,Buffer.from(result.value).toString('utf8'));
    assert.ok(text.startsWith(result.value));
    assert.equal(result.truncated,result.value.length<text.length);
  }
  assert.equal(bound(text,24).value,'Acme ');
  assert.equal(bound(text,28).value,'Acme 🙂');
});

test('DOM reads retain complete Unicode code points at text boundaries', async () => {
  const source=await readFile(new URL('../src/browser/reading/dom.js',import.meta.url),'utf8');
  const read=runInNewContext(`(${source})`,{TextEncoder});
  const element={innerText:'a'.repeat(84)+'🙂tail'};
  const result=JSON.parse(read(element,{read:'text',maxBytes:256}));
  assert.equal(result.value,'a'.repeat(84));
  assert.equal(result.truncated,true);
  assert.equal(result.value,Buffer.from(result.value).toString('utf8'));
});

test('direct readable content bounds Unicode metadata as well as article text',async()=>{
  const source=await readFile(new URL('../src/browser/reading/content.js',import.meta.url),'utf8');
  const bound=runInNewContext(`(${await readFile(new URL('../src/browser/reading/truncate.js',import.meta.url),'utf8')})`);
  const content={querySelector:selector=>selector==='title'?{textContent:'東'.repeat(200)}:null,querySelectorAll:()=>[]};
  const document={createElement:()=>({content}),createTreeWalker:()=>({nextNode:()=>null})};
  const read=runInNewContext(`(${source})`,{document,NodeFilter:{SHOW_TEXT:4},TextEncoder});
  const result=read('<main>Acme</main>',{url:'https://example.com/'+'a'.repeat(100),maxBytes:256},bound);
  assert.ok(Buffer.byteLength(JSON.stringify(result))<=256);
  assert.equal(result.truncated,true);
});

test('missing elements satisfy detached and hidden conditions without calling element expectations', async () => {
  const source=await readFile(new URL('../src/browser/reading/query.js',import.meta.url),'utf8');
  const query=runInNewContext(`(${source})`,{document:{}});
  const injected={parseSelector:value=>value,querySelectorAll:()=>[],expect(){throw Error('Missing element reached expectation');}};
  for(const [kind,satisfied] of [['detached',true],['hidden',true],['attached',false],['visible',false],['text',false]]) {
    const result=await query(injected,()=>{}, {selector:'css=#missing',condition:{kind}},()=>{});
    assert.equal(result.satisfied,satisfied);
  }
});

test('actionability waits refuse missing, ambiguous, hidden and covered controls before dispatch',async()=>{
  const source=await readFile(new URL('../src/browser/reading/query.js',import.meta.url),'utf8');
  const query=runInNewContext(`(${source})`,{document:{},innerWidth:100,innerHeight:100});
  const element={scrollIntoView(){},getClientRects(){return [{left:10,top:10,right:30,bottom:30,width:20,height:20}]}};
  let elements=[element],missing=null,hit='done';
  const injected={parseSelector:x=>x,querySelectorAll:()=>elements,async checkElementStates(){return missing},expectHitTarget(){return hit}};
  const options={selector:'css=button',actionability:{kind:'click'}};
  const run=()=>query(injected,()=>{},options,()=>{});
  assert.equal((await run()).satisfied,true);
  hit={hitTargetDescription:'overlay'};assert.equal((await run()).satisfied,false);
  hit='done';missing={missingState:'disabled'};assert.equal((await run()).satisfied,false);
  missing=null;options.frameVisible=false;assert.equal((await run()).satisfied,false);
  options.frameVisible=true;elements=[];assert.equal((await run()).satisfied,false);
  elements=[element,element];assert.equal((await run()).error,'AMBIGUOUS_LOCATOR');
});

test('wrapped inline actions hit a visible fragment and still refuse an overlay',async()=>{
  const querySource=await readFile(new URL('../src/browser/reading/query.js',import.meta.url),'utf8');
  const actionSource=await readFile(new URL('../src/browser/cdp/session/actions.js',import.meta.url),'utf8');
  const globals={document:{},innerWidth:100,innerHeight:100};
  const query=runInNewContext(`(${querySource})`,globals);
  const action=runInNewContext(`(${actionSource})`,globals);
  let rects=[{left:80,top:10,right:110,bottom:20},{left:0,top:20,right:20,bottom:30}];
  const element={isConnected:true,scrollIntoView(){},getClientRects:()=>rects};
  let covered=false,points=[];
  const hit=point=>{points.push([point.x,point.y]);return covered?{hitTargetDescription:'overlay'}:'done'};
  const injected={parseSelector:x=>x,querySelectorAll:()=>[element],async checkElementStates(){return null},expectHitTarget:hit,
    setupHitTargetInterceptor(selected,kind,point){assert.equal(selected,element);return hit(point)==='done'?{stop:()=> 'done'}:'overlay'}};
  const run=()=>query(injected,()=>{},{selector:'css=a',actionability:{kind:'click'}},()=>{});
  const prepare=()=>action(injected,element,{stage:'prepare',action:{kind:'click'}});
  assert.equal((await run()).satisfied,true);
  const prepared=await prepare();assert.equal(prepared.pointer,true);assert.equal(prepared.x,90);assert.equal(prepared.y,15);
  assert.deepEqual(points,[[90,15],[90,15]]);
  covered=true;assert.equal((await run()).satisfied,false);assert.equal((await prepare()).retry,true);
  covered=false;rects=[{left:110,top:10,right:120,bottom:20}];
  assert.equal((await run()).satisfied,false);assert.equal((await prepare()).retry,true);
});

test('checked conditions pass the pinned helper its explicit checked expectation', async () => {
  const source=await readFile(new URL('../src/browser/reading/query.js',import.meta.url),'utf8');
  const query=runInNewContext(`(${source})`,{document:{}});
  const element={};
  const injected={parseSelector:value=>value,querySelectorAll:()=>[element],expect(selected,options){
    assert.equal(selected,element);
    assert.equal(options.expression,'to.be.checked');
    assert.equal(options.expectedValue.checked,true);
    return {matches:true};
  }};
  const result=await query(injected,()=>{}, {selector:'css=input',condition:{kind:'checked'}},()=>{});
  assert.equal(result.satisfied,true);
});

test('URL conditions observe the document even before its body exists', async () => {
  const source=await readFile(new URL('../src/browser/reading/query.js',import.meta.url),'utf8');
  const query=runInNewContext(`(${source})`,{document:{location:{href:'https://example.com/'}}});
  const injected={parseSelector:value=>value,querySelectorAll:()=>[],expect(){throw Error('URL reached element expectation');}};
  for (const [value,satisfied] of [['https://example.com/',true],['https://example.com/other',false]]) {
    const result=await query(injected,()=>{}, {selector:'css=body',condition:{kind:'url',value}},()=>{});
    assert.equal(result.satisfied,satisfied);
  }
});
