import test from 'node:test';
import assert from 'node:assert/strict';
import { mimeTypes } from './browser-http-mime.mjs';

const section = (kind, variable, table) => `// node_modules/mime/dist/types/${kind}.js\nvar ${variable} = ${JSON.stringify(table,null,2)};\nObject.freeze(${variable});\n// next\n`;
test('MIME extraction preserves primary extensions and excludes starred aliases', () => {
  const bundle=section('standard','types2',{'application/json':['json'],'text/xml':['*xml']})+section('other','types',{'application/xml':['xml'],'text/x-acme':['acme']});
  assert.deepEqual(JSON.parse(mimeTypes(bundle)),{acme:'text/x-acme',json:'application/json',xml:'application/xml'});
});
test('MIME extraction refuses drift and executable or conflicting table data', () => {
  assert.throws(()=>mimeTypes(''),/Missing pinned/);
  const standard=section('standard','types2',{'application/json':['json']});
  assert.throws(()=>mimeTypes(standard+section('other','types',{'text/plain':['json']})),/Conflicting/);
  assert.throws(()=>mimeTypes(standard+section('other','types',{'text/plain':['a\r\nb']})),/Invalid/);
  assert.throws(()=>mimeTypes(standard+section('other','types',{'text/plain':['txt']}).replace('[\n    "txt"\n  ]','run()')),/JSON|Unexpected/);
});
