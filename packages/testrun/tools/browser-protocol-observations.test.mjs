import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source=await readFile(new URL('../tests/fixtures/browser-protocol-observations.cjs',import.meta.url),'utf8');
class Stream extends EventEmitter {
  writes=[];
  write(...args){this.writes.push(args);return false;}
}
function fixture(writeFailure=false,nativeOnly=false){
  const records=[],process=new EventEmitter();
  process.stdin=new Stream();process.stdout=new Stream();process.env={TESTRUN_CHILD_OBSERVATIONS:'/private/Acme/observations.jsonl'};
  if(nativeOnly)process.env.TESTRUN_NATIVE_PROTOCOL_ONLY='1';
  const child=new EventEmitter();child.pid=42;child.stdio=[null,null,null,new Stream(),new Stream()];
  const calls=[],cp={spawn(...args){calls.push(args);return child;}};
  vm.runInNewContext(source,{Buffer,performance:{now:()=>1},process,require(name){
    if(name==='node:child_process')return cp;
    if(name==='node:fs')return {writeFileSync(path,text,options){if(writeFailure)throw Error('Acme observation destination unavailable');assert.equal(path,process.env.TESTRUN_CHILD_OBSERVATIONS);assert.equal(options.mode,0o600);assert.equal(options.flag,'wx');records.push(...text.trim().split('\n').map(JSON.parse));}};
    throw Error(name);
  }});
  return {records,process,cp,child,calls,flush:()=>process.emit('exit',0)};
}
function frame(value){const body=Buffer.from(JSON.stringify(value)),head=Buffer.alloc(4);head.writeUInt32LE(body.length);return Buffer.concat([head,body]);}

test('protocol observer preserves passive reads, write arguments and backpressure',()=>{
  const f=fixture();
  assert.equal(f.process.stdin.listenerCount('data'),0);
  const callback=()=>{},bytes=frame({id:1,method:'goto',params:{url:'Acme-private'}});
  assert.equal(f.process.stdout.write(bytes,'utf8',callback),false);
  assert.equal(f.process.stdout.writes[0][0],bytes);
  assert.equal(f.process.stdout.writes[0][1],'utf8');
  assert.equal(f.process.stdout.writes[0][2],callback);
  assert.equal(f.cp.spawn('Acme-browser',['Acme-argument']),f.child);
  assert.deepEqual(f.calls,[['Acme-browser',['Acme-argument']]]);
  assert.equal(f.child.stdio[4].listenerCount('data'),0);
  assert.equal(f.records.length,0,'No observation I/O before natural exit');f.flush();
  assert.equal(JSON.stringify(f.records).includes('Acme-private'),false);
});

test('fragmented binding frames and nested browser messages retain identities without payloads',()=>{
  const f=fixture(),bytes=frame({id:'17',error:{error:{name:'TimeoutError',message:'Acme-private'}}});
  f.process.stdout.write(bytes.subarray(0,2));f.process.stdout.write(bytes.subarray(2,9));f.process.stdout.write(bytes.subarray(9));
  f.cp.spawn('Acme-browser',[]);
  const received=Buffer.from(JSON.stringify({method:'Playwright.pageProxyMessageReceived',params:{message:JSON.stringify({id:29,method:'Network.loadingFailed',params:{url:'Acme-private'}})}})+'\0');
  const delivered=[];f.child.stdio[4].on('data',bytes=>delivered.push(bytes));
  const first=received.subarray(0,11),second=received.subarray(11);
  f.child.stdio[4].emit('data',first);f.child.stdio[4].emit('data',second);
  assert.equal(delivered[0],first);assert.equal(delivered[1],second);
  f.flush();
  const binding=f.records.find(row=>row.lane==='driver-to-binding');
  assert.equal(binding.message.id,'17');assert.equal(binding.message.errorName,'TimeoutError');
  const browser=f.records.find(row=>row.lane==='browser-to-driver');
  assert.equal(browser.message.nested.id,29);assert.equal(browser.message.nested.method,'Network.loadingFailed');
  assert.equal(JSON.stringify(f.records).includes('Acme-private'),false);
});

test('observer bounds and malformed frames stop observation while forwarding original bytes',()=>{
  for(const bytes of [Buffer.from([255,255,255,127]),Buffer.from([1,0,0,0,123])]){
    const f=fixture();
    assert.equal(f.process.stdout.write(bytes),false);
    const next=frame({id:8,method:'close'});assert.equal(f.process.stdout.write(next),false);
    assert.equal(f.process.stdout.writes.length,2);
    assert.equal(f.process.stdout.writes[0][0],bytes);assert.equal(f.process.stdout.writes[1][0],next);
    f.flush();
    assert.equal(f.records.filter(row=>row.event.startsWith('observer-')).length,1);assert.match(f.records[0].event,/observer-(bounds|parse-error)/);
  }
});

test('observation file errors leave browser and binding I/O usable',()=>{
  const f=fixture(true),bytes=frame({id:1,method:'goto'});
  assert.equal(f.process.stdout.write(bytes),false);
  assert.equal(f.cp.spawn('Acme-browser',[]),f.child);
  const outgoing=Buffer.from('{"id":2,"method":"Page.navigate"}\0');
  assert.equal(f.child.stdio[3].write(outgoing),false);
  assert.equal(f.child.stdio[3].writes[0][0],outgoing);
  assert.doesNotThrow(()=>f.flush());
  assert.equal(f.records.length,0);
});

test('native-only observation admits the pinned injected-script frame without retaining source or reading report stdout',()=>{
  const f=fixture(false,true);f.cp.spawn('Acme-browser',[]);
  const source=Buffer.from(JSON.stringify({id:1,method:'Runtime.evaluate',params:{expression:'Acme-private-source'.repeat(20000)}})+'\0');
  assert.ok(source.length>262144);assert.equal(f.child.stdio[3].write(source),false);
  const mouse=Buffer.from(JSON.stringify({id:2,method:'Page.dispatchMouseEvent',params:{type:'mousedown',x:17,y:29}})+'\0');
  assert.equal(f.child.stdio[3].write(mouse),false);
  f.process.stdout.write('Acme ordinary JSON report');f.flush();
  assert.equal(f.records.some(row=>row.event.startsWith('observer-')),false);
  assert.equal(f.records.some(row=>row.lane==='driver-to-binding'),false);
  const native=f.records.find(row=>row.message?.method==='Page.dispatchMouseEvent');
  assert.equal(native.message.id,2);assert.equal(native.message.inputType,'mousedown');
  assert.equal(JSON.stringify(f.records).includes('Acme-private-source'),false);
  assert.equal(JSON.stringify(f.records).includes('"x":'),false);
});
test('native-only observation retains its explicit finite byte bound and forwards oversized bytes unchanged',()=>{
  const f=fixture(false,true);f.cp.spawn('Acme-browser',[]);
  const bytes=Buffer.alloc(2097153,65);
  assert.equal(f.child.stdio[3].write(bytes),false);
  assert.equal(f.child.stdio[3].writes[0][0],bytes);f.flush();
  const bound=f.records.find(row=>row.event==='observer-bounds');
  assert.equal(bound.maximum,2097152);assert.equal(bound.lane,'driver-to-browser');
});
