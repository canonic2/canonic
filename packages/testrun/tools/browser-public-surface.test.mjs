import test from 'node:test';
import assert from 'node:assert/strict';
import { publicSurface } from './browser-public-surface.mjs';

test('public schemas preserve operation choices, defaults, enums and recursive references', () => {
  const choice = {
    properties: {
      operation: {const:'page_wait'},
      timeoutMs: {type:'integer',default:30000},
      state: {enum:['visible','hidden']},
      value: {$ref:'#/$defs/value'},
    },
    required:['operation'],
  };
  const rows = publicSurface([{
    name:'browser_send',
    inputSchema: {
      properties:{request:{oneOf:[choice]}},
      $defs:{value:{properties:{next:{$ref:'#/$defs/value'}}}},
    },
  }]);
  const timeout = rows.find(row => row.name.endsWith('.timeoutMs'));
  assert.equal(timeout.operationName, 'page_wait');
  assert.equal(timeout.schema.default, 30000);
  assert.equal(timeout.required, false);
  assert.deepEqual(rows.find(row => row.name.endsWith('.state')).schema.enum, ['visible','hidden']);
  assert.equal(rows.find(row => row.name.endsWith('.next')).schema.$ref, '#/$defs/value');
  assert.throws(() => publicSurface([]));
  assert.throws(() => publicSurface([{name:'browser_send',inputSchema:{}},{name:'browser_send',inputSchema:{}}]));
  const shared = publicSurface([
    {name:'browser_send',inputSchema:{}},
    {name:'act',inputSchema:{anyOf:[{properties:{deviceOnly:{type:'boolean'}}},{properties:{targetId:{description:'Explicit browser target'},page:{type:'object'}}}] }},
  ]);
  assert.ok(shared.some(row => row.name === 'act.page'));
  assert.ok(!shared.some(row => row.name.includes('deviceOnly')));
});

test('compacted request definitions preserve nested operation and parameter addresses', () => {
  const rows = publicSurface([{
    name:'browser_send',
    inputSchema:{
      properties:{request:{oneOf:[
        {properties:{operation:{const:'page_wait'},condition:{$ref:'#/$defs/pageCondition'}}},
        {properties:{operation:{const:'http_fetch'},body:{$ref:'#/$defs/httpRequestBody'}}},
      ]}},
      $defs:{
        pageCondition:{oneOf:[{properties:{kind:{const:'visible'}},required:['kind']}]},
        httpRequestBody:{anyOf:[{properties:{text:{type:'string'}},required:['text']}]},
      },
    },
  }]);
  const condition = rows.find(row => row.name === 'browser_send.request/oneOf/0.condition/oneOf/0.kind');
  const body = rows.find(row => row.name === 'browser_send.request/oneOf/1.body/anyOf/0.text');
  assert.equal(condition.operationName, 'page_wait');
  assert.equal(body.operationName, 'http_fetch');
  assert.equal(condition.required, true);
  assert.equal(body.required, true);
  assert.ok(!rows.some(row => row.name.includes('/$defs/')));
});
