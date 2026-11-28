import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import {runInNewContext} from 'node:vm';
const source=await readFile(new URL('../src/browser/reading/websocket-routing.js',import.meta.url),'utf8');
const one='00000000-0000-0000-0000-000000000001',two='00000000-0000-0000-0000-000000000002';
const plain=value=>JSON.parse(JSON.stringify(value));
function fixture(reply=()=>[]){
  const sent=[],events=[];let emit;
  const install=callback=>{emit=callback;return {Mock:class{},dispatch:command=>{
    sent.push(plain(command));
    if(command.type==='closePage')emit({id:command.id,type:'onClosePage',code:command.code,reason:command.reason,wasClean:command.wasClean});
  }};};
  const registry=runInNewContext(`(${source})`,{URL,TextEncoder,atob})(install,Mock=>Mock);
  const callback=event=>{events.push(plain(event));return reply(event);};
  registry.add('sdk','wss://example.com/acme',callback,true,1,true);
  const tick=async()=>{for(let n=0;n<32;n++)await Promise.resolve();};
  return {registry,sent,events,emit:event=>emit(event),tick,create:async(id='s')=>{emit({id,type:'onCreate',url:'wss://example.com/acme'});await tick();}};
}

test('SDK close-handler removal restores only that direction and preserves the other owner',async()=>{
  const page=fixture(event=>event.type==='onCreate'?[{type:'connect'}]:null);
  await page.create();page.registry.handlers('sdk','s',{pageClose:one,serverClose:two});
  page.registry.handlers('sdk','s',{pageClose:null});
  page.emit({id:'s',type:'onClosePage',code:3001,reason:'Acme page',wasClean:true});await page.tick();
  assert.equal(page.sent.at(-1).type,'closeServer');assert.equal(page.events.length,1);
  page.emit({id:'s',type:'onCloseServer',code:3001,reason:'Acme page',wasClean:true});await page.tick();
  assert.equal(page.events.at(-1).listenerId,two);assert.equal(page.sent.filter(command=>command.type==='closePage').length,0);
  page.registry.closeSocket('sdk','s','page',{code:3002,reason:'Acme final'});await page.tick();
  assert.equal(page.registry.active,0);assert.equal(page.registry.pending,0);
  const server=fixture(event=>event.type==='onCreate'?[{type:'connect'}]:null);
  await server.create();server.registry.handlers('sdk','s',{pageClose:one,serverClose:two});
  server.registry.handlers('sdk','s',{serverClose:null});
  server.emit({id:'s',type:'onCloseServer',code:3001,reason:'Acme server',wasClean:true});await server.tick();
  assert.equal(server.sent.at(-1).type,'closePage');
  assert.deepEqual(server.events.slice(1).map(event=>({type:event.type,listenerId:event.listenerId,code:event.code})),[{type:'onClosePage',listenerId:one,code:3001}]);
  assert.equal(server.registry.active,0);assert.equal(server.registry.pending,0);
});

test('SDK removal does not replay a held close through the newly restored default',async()=>{
  let release;const f=fixture(event=>event.type==='onCreate'?[{type:'connect'}]:new Promise(resolve=>release=resolve));
  await f.create();f.registry.handlers('sdk','s',{serverClose:one});
  f.emit({id:'s',type:'onCloseServer',code:3001,reason:'Acme held',wasClean:true});await f.tick();
  assert.equal(f.events.at(-1).listenerId,one);assert.equal(f.registry.pending,1);
  f.registry.handlers('sdk','s',{serverClose:null});
  f.emit({id:'s',type:'onCloseServer',code:3002,reason:'Acme duplicate',wasClean:true});await f.tick();
  assert.equal(f.events.length,2);assert.equal(f.sent.length,1);
  release(null);await f.tick();assert.equal(f.registry.pending,0);assert.equal(f.registry.active,1);
  assert.equal(f.sent.length,1,'A prior handler-owned event must not gain default propagation after removal');
  f.registry.closeSocket('sdk','s','page',{code:3003,reason:'Acme final'});await f.tick();
  assert.equal(f.registry.active,0);assert.equal(f.registry.pending,0);
});

test('SDK WebSocket defaults forward both directions and open a mock after its handler',async()=>{
  const f=fixture(event=>event.id==='mock'?null:[{type:'connect'}]);await f.create();
  f.emit({id:'s',type:'onMessageFromPage',data:{data:'Acme',isBase64:false}});
  f.emit({id:'s',type:'onMessageFromServer',data:{data:'ABH/',isBase64:true}});
  await f.tick();
  assert.deepEqual(f.sent.map(command=>command.type),['connect','sendToServer','sendToPage']);
  assert.equal(f.events.length,1);
  assert.deepEqual(f.sent[2].data,{data:'ABH/',isBase64:true});
  f.emit({id:'s',type:'onCloseServer',code:3001,reason:'Acme server',wasClean:true});await f.tick();
  assert.equal(f.sent.at(-1).type,'closePage');assert.equal(f.registry.active,0);assert.equal(f.registry.pending,0);
  await f.create('mock');assert.equal(f.sent.at(-1).type,'ensureOpened');
  const count=f.sent.length;f.emit({id:'mock',type:'onMessageFromPage',data:{data:'Acme discarded',isBase64:false}});await f.tick();
  assert.equal(f.sent.length,count);assert.equal(f.events.length,2);
  const manual=fixture(()=>[]);manual.registry.add('commands','wss://example.com/manual',event=>manual.events.push(plain(event))&&[],true,2);
  manual.emit({id:'legacy',type:'onCreate',url:'wss://example.com/manual'});await manual.tick();
  assert.equal(manual.sent.length,0,'Existing command mode has no implicit opening');
});

test('SDK WebSocket handler updates preserve arrival identities and remove forwarding interception',async()=>{
  const pending=[];const f=fixture(event=>event.type==='onCreate'?[{type:'connect'}]:new Promise(resolve=>pending.push(resolve)));
  await f.create();assert.equal(f.registry.handlers('sdk','s',{pageMessage:one,serverMessage:one}),'configured');
  f.emit({id:'s',type:'onMessageFromPage',data:{data:'Acme first',isBase64:false}});
  f.registry.handlers('sdk','s',{pageMessage:two,serverMessage:null});await f.tick();
  assert.equal(f.events.at(-1).listenerId,one,'Already arrived events retain the old handler');
  f.emit({id:'s',type:'onMessageFromPage',data:{data:'Acme second',isBase64:false}});await f.tick();
  assert.equal(f.events.at(-1).listenerId,two);
  assert.throws(()=>f.registry.handlers('sdk','s',{pageMessage:one,serverMessage:42}),/WEBSOCKET_HANDLERS_INVALID/);
  f.emit({id:'s',type:'onMessageFromPage',data:{data:'Acme atomic',isBase64:false}});await f.tick();
  assert.equal(f.events.at(-1).listenerId,two,'Malformed updates must not partially replace another handler');
  assert.equal(f.registry.handlers('foreign','s',{pageMessage:one}),'WEBSOCKET_SOCKET_EXPIRED');
  f.registry.handlers('sdk','s',{pageMessage:null});
  f.emit({id:'s',type:'onMessageFromPage',data:{data:'Acme restored',isBase64:false}});
  f.emit({id:'s',type:'onMessageFromServer',data:{data:'Acme server',isBase64:false}});await f.tick();
  assert.deepEqual(f.sent.map(command=>command.type),['connect','sendToServer','sendToPage']);
  assert.equal(f.sent[1].data.data,'Acme restored');
  for(const resolve of pending)resolve([]);await f.tick();assert.equal(f.registry.pending,0);
  assert.deepEqual(f.events.filter(event=>event.listenerId).map(event=>event.listenerId),[one,two,two]);
});

test('SDK WebSocket close handlers own propagation and receive the final acknowledgement',async()=>{
  const f=fixture(event=>event.type==='onCreate'?[{type:'connect'}]:null);
  await f.create();f.registry.handlers('sdk','s',{pageClose:one,serverClose:two});
  f.emit({id:'s',type:'onClosePage',code:3001,reason:'Acme page',wasClean:true});await f.tick();
  assert.deepEqual(f.sent.map(command=>command.type),['connect'],'Page close handler suppresses automatic server closure');
  assert.equal(f.registry.closeSocket('sdk','s','server',{code:3002,reason:'Acme server'}),'close-requested');
  f.emit({id:'s',type:'onCloseServer',code:3002,reason:'Acme server',wasClean:true});await f.tick();
  assert.deepEqual(f.sent.map(command=>command.type),['connect','closeServer'],'Server close handler suppresses automatic page closure');
  assert.equal(f.registry.closeSocket('sdk','s','page',{code:3003,reason:'Acme final'}),'close-requested');await f.tick();
  assert.deepEqual(f.events.filter(event=>event.listenerId).map(event=>[event.type,event.listenerId,event.code]),[['onClosePage',one,3001],['onCloseServer',two,3002],['onClosePage',one,3003]]);
  assert.deepEqual(f.sent.map(command=>command.type),['connect','closeServer','closePage']);
  assert.equal(f.registry.closeSocket('sdk','s','page',{}),'WEBSOCKET_SOCKET_EXPIRED');
  assert.equal(f.registry.active,0);assert.equal(f.registry.pending,0);
});

test('SDK WebSocket handler ownership rejects command-mode edits and mixed forwarding authorities',async()=>{
  const f=fixture(()=>[]);f.registry.add('legacy','wss://example.com/legacy',()=>[],true,2);
  f.emit({id:'legacy',type:'onCreate',url:'wss://example.com/legacy'});await f.tick();
  assert.equal(f.registry.handlers('legacy','legacy',{pageMessage:one}),'WEBSOCKET_SDK_MODE_REQUIRED');
  const invalid=fixture(()=>[{type:'connect'},{type:'setForwarding',from:'page',enabled:true}]);
  await invalid.create();
  assert.deepEqual(invalid.sent.map(command=>command.type),['closePage','closeServer'],'Whole reply validation prevents partial connect before conflicting authority');
  assert.equal(invalid.registry.active,0);assert.equal(invalid.registry.pending,0);
});

test('SDK WebSocket explicit page close propagates its acknowledgement without a close handler',async()=>{
  const f=fixture(()=>[{type:'connect'}]);await f.create();
  for(const options of [{code:1006},{reason:'é'.repeat(62)},{type:'connect'},null]){
    assert.throws(()=>f.registry.closeSocket('sdk','s','page',options),/WEBSOCKET_CLOSE_INVALID/);
    assert.deepEqual(f.sent.map(command=>command.type),['connect']);
  }
  assert.equal(f.registry.closeSocket('foreign','s','page',{}),'WEBSOCKET_SOCKET_EXPIRED');
  f.registry.closeSocket('sdk','s','page',{code:3005,reason:'Acme explicit'});await f.tick();
  assert.deepEqual(f.sent.map(command=>command.type),['connect','closePage','closeServer']);
  f.emit({id:'s',type:'onCloseServer',code:3005,reason:'Acme explicit',wasClean:true});await f.tick();
  assert.equal(f.registry.active,0);assert.equal(f.registry.pending,0);
});

test('SDK WebSocket automatic forwarding to a closed endpoint preserves the surviving endpoint',async()=>{
  for(const closed of ['server','page']) {
    const f=fixture(event=>event.type==='onCreate'?[{type:'connect'}]:null);await f.create();
    f.registry.handlers('sdk','s',{[closed==='server'?'serverClose':'pageClose']:one});
    f.emit({id:'s',type:closed==='server'?'onCloseServer':'onClosePage',code:3002,reason:'Acme half close',wasClean:true});await f.tick();
    const before=f.sent.length;
    f.emit({id:'s',type:closed==='server'?'onMessageFromPage':'onMessageFromServer',data:{data:'Acme discarded',isBase64:false}});await f.tick();
    assert.equal(f.sent.length,before,'A failed default forward must not close the surviving endpoint');
    assert.equal(f.registry.active,1);assert.equal(f.registry.pending,0);
    assert.equal(f.registry.send('sdk','s',closed==='server'?'page':'server',{data:'Acme survivor',isBase64:false}),'sent');
    assert.equal(f.sent.at(-1).type,closed==='server'?'sendToPage':'sendToServer');
    f.registry.remove('sdk');assert.equal(f.registry.active,0);
  }
});
