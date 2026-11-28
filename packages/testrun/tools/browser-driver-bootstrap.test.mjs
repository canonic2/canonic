import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {readFile} from 'node:fs/promises';
import {runInNewContext} from 'node:vm';
import test from 'node:test';

const source=await readFile(new URL('../src/browser/driver/bootstrap.js',import.meta.url),'utf8').catch(error=>{if(error.code==='ENOENT')return null;throw error;});
function observationClock() {
  const timers=[];
  return {timers,setInterval(callback,interval){const timer={callback,interval,closed:false,unrefs:0,unref(){this.unrefs++;}};timers.push(timer);return timer;},clearInterval(timer){timer.closed=true;},tick(){for(const timer of timers)if(!timer.closed)timer.callback();}};
}
const entryClock=observationClock();
const bootstrap=source?runInNewContext(source,{Buffer,setImmediate,setInterval:entryClock.setInterval,clearInterval:entryClock.clearInterval}):{installContextCleanup(){}};
class Context extends EventEmitter {
  static Events={Close:'close'};
  _closedStatus='open';
  releases=0;
  effects=0;
  paused=true;
  debugger(){return {isPaused:()=>this.paused,resume:()=>{this.releases++;this.paused=false;if(this._closedStatus!=='closed')this.effects++;}};}
}
function fixture(){
  let listener;const sdk={instrumentation:{addListener(value){listener=value;}}};
  bootstrap.installContextCleanup(sdk,Context);
  const before=context=>listener?.onBeforeCall({}, {attribution:{context}});
  return {before};
}
test('owned SDK cleanup waits for context closure before releasing its original pause',()=>{
  const f=fixture(),context=new Context();
  f.before(context);f.before(context);
  assert.equal(context.releases,0);
  context.emit(Context.Events.Close);
  assert.equal(context.releases,0,'An event without closed state cannot authorize input continuation');
  context._closedStatus='closing';context.emit(Context.Events.Close);
  assert.equal(context.releases,0);
  context._closedStatus='closed';context.emit(Context.Events.Close);
  assert.equal(context.releases,1,'The retained SDK pause needs a real release, not local object retirement');
  assert.equal(context.effects,0);
  context.emit(Context.Events.Close);assert.equal(context.releases,1,'Only one cleanup owner may release this pause');
});
test('unpaused closed contexts do not issue speculative resume or affect another context',()=>{
  const f=fixture(),closed=new Context(),sibling=new Context();
  closed.paused=false;f.before(closed);f.before(sibling);
  closed._closedStatus='closed';closed.emit(Context.Events.Close);
  assert.equal(closed.releases,0);assert.equal(sibling.paused,true);
  sibling._closedStatus='closed';sibling.emit(Context.Events.Close);
  assert.equal(sibling.releases,1);assert.equal(sibling.effects,0);
});
test('foreign contexts and missing attribution cannot acquire SDK cleanup ownership',()=>{
  const f=fixture(),foreign=new EventEmitter();foreign._closedStatus='closed';
  assert.doesNotThrow(()=>f.before(undefined));
  f.before(foreign);foreign.emit(Context.Events.Close);
  assert.equal(foreign.listenerCount(Context.Events.Close),0);
});
test('context-close release errors remain observable instead of becoming a cleanup receipt',()=>{
  const f=fixture(),context=new Context();
  context.debugger=()=>({isPaused:()=>true,resume(){throw Error('Acme closed-pause release failed');}});
  f.before(context);context._closedStatus='closed';
  assert.throws(()=>context.emit(Context.Events.Close),/Acme closed-pause release failed/);
});

function streams(maximum=256){
  const input=new EventEmitter(),output=new EventEmitter(),writes=[],messages=[],exits=[];
  output.write=chunk=>{writes.push(Buffer.from(chunk));return false;};
  const transport=bootstrap.createTransport(input,output,maximum,message=>messages.push(message),code=>exits.push(code));
  return {input,output,writes,messages,exits,transport};
}
function frame(value){
  const bytes=Buffer.from(JSON.stringify(value)),header=Buffer.alloc(4);header.writeUInt32LE(bytes.length);
  return Buffer.concat([header,bytes]);
}
const flush=()=>new Promise(setImmediate);
function childObservations(write) {
  let listener;const lines=[],clock=observationClock();
  const playwright={instrumentation:{addListener(value){listener=value;}}};
  bootstrap.installChildObservations(playwright,write??(line=>lines.push(line)),clock);
  const open=child=>listener.onBrowserOpen({options:{browserProcess:{process:child}}});
  const rows=()=>lines.map(line=>JSON.parse(line.slice('testrun:owned-child '.length)));
  return {open,rows,listener,clock};
}
function ownedChild(count=5) {
  const child=new EventEmitter();
  child.stdio=Array.from({length:count},(_,index)=>index===0?null:Object.assign(new EventEmitter(),{closed:false,destroyed:false,readableEnded:false,writableFinished:false}));
  return child;
}
test('owned child observations distinguish native exit, open streams and eventual close',()=>{
  const f=childObservations(),child=ownedChild();f.open(child);
  child.emit('exit',0,null);
  const before=JSON.parse(JSON.stringify(f.rows()));
  child.stdio[3].readableEnded=true;child.stdio[3].emit('end');
  child.stdio[3].closed=true;child.stdio[3].emit('close');
  child.emit('close',0,null);
  const rows=f.rows();
  assert.deepEqual(rows.map(row=>row.phase),['observing','native-exit','stream-end','stream-close','native-close']);
  assert.equal(rows[1].streams[3].closed,false);assert.equal(rows[4].streams[3].closed,true);
  assert.equal(before[1].streams[3].closed,false);
  assert.equal(rows[0].streams[0].present,false);assert.equal(rows[0].streams[0].closed,null);
});
test('late observation keeps known native exit separate from an observed exit event and samples only changes',()=>{
  const f=childObservations(),child=ownedChild();
  Object.assign(child,{exitCode:0,signalCode:null,_handle:null,_closesNeeded:5,_closesGot:3});
  f.open(child);f.clock.tick();
  assert.equal(f.rows().length,1);
  assert.deepEqual(f.rows()[0].nativeState,{exitStatusKnown:true,nativeHandlePresent:false,closesNeeded:5,closesGot:3});
  assert.equal(f.clock.timers[0].interval,1000);assert.equal(f.clock.timers[0].unrefs,1);
  child._closesGot=4;f.clock.tick();
  assert.deepEqual(f.rows().map(row=>row.phase),['observing','native-state']);
  assert.equal(f.rows()[1].nativeState.closesGot,4);
  child.emit('close',0,null);assert.equal(f.clock.timers[0].closed,true);
  child._closesGot=5;f.clock.tick();assert.equal(f.rows().length,3);
  assert(!f.rows().some(row=>row.phase==='native-exit'),'State must not invent a missing event');
});
test('native state observations omit codes and private accessors and preserve unknown bounded counters',()=>{
  const f=childObservations(),child=ownedChild();
  Object.defineProperty(child,'exitCode',{get(){throw Error('Acme exit accessor');}});
  Object.assign(child,{signalCode:'Acme private signal',_handle:'Acme private handle',_closesNeeded:Infinity,_closesGot:42});
  f.open(child);f.clock.tick();
  assert.deepEqual(f.rows()[0].nativeState,{exitStatusKnown:true,nativeHandlePresent:null,closesNeeded:null,closesGot:null});
  assert(!JSON.stringify(f.rows()).includes('Acme'));assert(!JSON.stringify(f.rows()).includes('42'));
});
test('owned child observation deduplicates owners and never performs cleanup mutations',()=>{
  const f=childObservations(),child=ownedChild();let mutations=0;
  child.kill=()=>mutations++;for(const stream of child.stdio.filter(Boolean))stream.destroy=()=>mutations++;
  f.open(child);f.open(child);assert.equal(f.rows().length,1);
  assert.equal(child.listenerCount('exit'),1);assert.equal(child.listenerCount('close'),1);
  child.emit('exit',null,'SIGKILL');child.emit('close',null,'SIGKILL');
  assert.equal(mutations,0);assert.deepEqual(f.rows().map(row=>row.owner),[1,1,1]);
});
test('owned child observations expose bounded capacity and missing sources without restricting browsers',()=>{
  const f=childObservations();
  f.listener.onBrowserOpen({});f.listener.onBrowserOpen({});
  const children=Array.from({length:6},()=>ownedChild());
  for(const child of children)assert.doesNotThrow(()=>f.open(child));
  assert.equal(f.rows().filter(row=>row.phase==='unavailable').length,1);
  assert.equal(f.rows().filter(row=>row.phase==='capacity').length,1);
  assert.deepEqual(f.rows().filter(row=>row.phase==='observing').map(row=>row.owner),[1,2,3,4]);
  assert.equal(children[4].listenerCount('exit'),0);assert.equal(children[5].listenerCount('exit'),0);
});
test('owned child observation never reads arguments, identity, environment or stream payloads',()=>{
  const f=childObservations(),child=ownedChild();
  for(const key of ['pid','spawnargs','spawnfile','env'])Object.defineProperty(child,key,{get(){throw Error('Acme private identity read');}});
  child.stdio[3].closed='Acme private value';
  Object.defineProperty(child.stdio[3],'fd',{get(){throw Error('Acme descriptor read');}});
  f.open(child);child.emit('exit',42,'Acme-private-signal');
  const rows=f.rows();assert.equal(rows[1].streams[3].closed,null);
  assert(!JSON.stringify(rows).includes('Acme'));assert(!JSON.stringify(rows).includes('42'));
});
test('failed observation writes do not throw through SDK child lifecycle callbacks',()=>{
  const f=childObservations(()=>{throw Error('Acme sink failure');}),child=ownedChild();
  assert.doesNotThrow(()=>f.open(child));
  assert.doesNotThrow(()=>child.emit('exit',0,null));
  assert.doesNotThrow(()=>child.stdio[3].emit('close'));
  assert.doesNotThrow(()=>child.emit('close',0,null));
});
test('owned child stream observations cover only the five declared descriptors',()=>{
  const f=childObservations(),child=ownedChild(8);f.open(child);
  assert.equal(f.rows()[0].streams.length,5);
  child.stdio[6].emit('close');assert.equal(f.rows().length,1);
  child.stdio[4].emit('finish');assert.deepEqual(f.rows()[1],{phase:'stream-finish',owner:1,index:4});
});
test('owned server entry installs child observations alongside closed-context cleanup',async()=>{
  let factory;const listeners=[],lines=[];
  const process=new EventEmitter();Object.assign(process,{stdin:new EventEmitter(),stdout:new EventEmitter(),stderr:{destroyed:false,write(value,callback){lines.push(value);callback();}}});
  const playwright={instrumentation:{addListener(listener){listeners.push(listener);}}};
  const server={DispatcherConnection:class{dispatch(){}},RootDispatcher:class{constructor(_connection,create){factory=create;}},PlaywrightDispatcher:class{},BrowserContext:Context,createPlaywright(){return playwright;}};
  bootstrap.run({server,utils:{gracefullyProcessExitDoNotHang(){throw Error('Unexpected exit');}}},process,256);
  await factory({}, {sdkLanguage:'javascript'});
  assert.equal(listeners.length,3);
  const observe=listeners.find(listener=>listener.onBrowserOpen),child=ownedChild();
  observe.onBrowserOpen({options:{browserProcess:{process:child}}});child.emit('exit',0,null);
  assert.deepEqual(lines.map(line=>JSON.parse(line.slice('testrun:owned-child '.length)).phase),['observing','native-exit']);
});
test('clock correction owns both SDK script paths once per context and preserves ordinary scripts',async()=>{
  let listener;
  bootstrap.installClockCorrections({instrumentation:{addListener(value){listener=value;}}},Context);
  const context=new Context(),calls=[];
  context.addInitScript=async(progress,source)=>{calls.push(['init',progress,source]);return 'retained-script';};
  context.clock={_evaluateInFrames:async source=>{calls.push(['live',source]);return 42;}};
  listener.onBeforeCall({}, {attribution:{context}});
  const wrapped=context.addInitScript;
  listener.onBeforeCall({}, {attribution:{context}});
  assert.equal(context.addInitScript,wrapped);
  assert.equal(await context.addInitScript('progress','globalThis.acme=42'),'retained-script');
  assert.equal(await context.clock._evaluateInFrames('globalThis.acme=42'),42);
  assert.deepEqual(calls,[['init','progress','globalThis.acme=42'],['live','globalThis.acme=42']]);
  await assert.rejects(async()=>context.addInitScript('progress','globalThis.__pwClock = (module.exports.inject())'),/CLOCK_BOOTSTRAP_UNSUPPORTED/);
  assert.equal(calls.length,2,'A changed clock boundary cannot dispatch');
  assert.doesNotThrow(()=>listener.onBeforeCall({}, {attribution:{context:{}}}));
});
test('driver transport preserves fragmented and batched frames with deferred ordered dispatch',async()=>{
  const f=streams(),one=frame({id:1,method:'Acme'}),two=frame({id:2,method:'second'});
  const bytes=Buffer.concat([one,two]);
  f.input.emit('data',bytes.subarray(0,2));f.input.emit('data',bytes.subarray(2,7));f.input.emit('data',bytes.subarray(7));
  assert.equal(f.messages.length,0);await flush();
  assert.deepEqual(JSON.parse(JSON.stringify(f.messages)),[{id:1,method:'Acme'},{id:2,method:'second'}]);
  f.transport.send({id:2,result:{text:'Acme café'}});
  const output=Buffer.concat(f.writes);assert.equal(output.readUInt32LE(),output.length-4);
  assert.deepEqual(JSON.parse(output.subarray(4)),{id:2,result:{text:'Acme café'}});
  f.input.emit('close');assert.deepEqual(f.exits,[0]);
});
test('invalid, excessive and truncated driver frames request graceful failure exactly once',async()=>{
  for(const bytes of [Buffer.from([0,0,0,0]),Buffer.from([1,1,0,0]),Buffer.from([1,0,0,0,123]),Buffer.from([1,0])]){
    const f=streams();f.input.emit('data',bytes);await flush();f.input.emit('close');
    f.input.emit('data',frame({id:99}));await flush();
    assert.deepEqual(f.exits,[1]);assert.equal(f.messages.length,0);
  }
});
test('EOF fences queued allocations and late writes without killing the SDK owner',async()=>{
  const f=streams();f.input.emit('data',frame({id:1,method:'launch'}));f.input.emit('close');await flush();
  assert.equal(f.messages.length,0);f.transport.send({id:1,result:{}});assert.equal(f.writes.length,0);
  assert.deepEqual(f.exits,[0]);
});
test('pipe write failures request owned graceful shutdown and retain no response success',()=>{
  const f=streams();f.output.write=()=>{throw Error('Acme closed pipe');};
  assert.doesNotThrow(()=>f.transport.send({id:1}));assert.deepEqual(f.exits,[1]);assert.equal(f.writes.length,0);
});
