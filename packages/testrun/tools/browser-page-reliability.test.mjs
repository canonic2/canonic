import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {test} from 'node:test';
import {runInNewContext} from 'node:vm';

async function helper(relativePath) {
  const source = await readFile(new URL(relativePath, import.meta.url), 'utf8');
  return runInNewContext(source, {TextEncoder});
}

test('an acknowledged click can remove its target while hit verification remains strict', async () => {
  const action = await helper('../src/browser/cdp/session/actions.js');
  const removed = {isConnected:false};
  const click = {kind:'click'};
  assert.equal((await action({}, removed, {stage:'prepare', action:click})).error, 'STALE_REFERENCE');
  const injected = {testrunInterceptor:{stop:()=>'done'}};
  assert.equal((await action(injected, removed, {stage:'verify', action:click})).done, true);
  assert.equal(injected.testrunInterceptor, null);
  const missed = {testrunInterceptor:{stop:()=>'overlay received input'}};
  assert.equal((await action(missed, removed, {stage:'verify', action:click})).error, 'HIT_TARGET_CHANGED');
  const check = {kind:'check',checked:true};
  assert.equal((await action({}, removed, {stage:'verify', action:check})).error, 'STALE_REFERENCE');
});

test('an oversized page snapshot retains usable locators and tells the caller how to narrow', async () => {
  const read = await helper('../src/browser/reading/dom.js');
  const button = {
    tagName:'BUTTON', children:[], innerText:'🚀'.repeat(140),
    matches:selector=>selector.startsWith('a[href]'),
    hasAttribute:()=>false, getAttribute:()=>null,
  };
  const root = {...button, matches:()=>false, children:Array(70).fill(button)};
  const frame = {
    connectionId:'b484e038-786e-43be-8620-dfeec5509ef2',
    id:'11ac538f-d748-47c1-94ad-960f32fb94fb',
    generation:'81e4b0fd-d6bd-4671-b353-38982e851bba',
  };
  let retained = 0;
  const options = {
    read:'snapshot', maxBytes:1024, frame,
    retain:(element, owner)=>({kind:'element',frame:owner,id:`00000000-0000-0000-0000-${String(++retained).padStart(12, '0')}`}),
  };
  const encoded = read(root, options);
  const result = JSON.parse(encoded);
  assert.ok(Buffer.byteLength(encoded) <= options.maxBytes);
  assert.equal(result.truncated, true);
  assert.equal(result.narrowingRequired, true);
  assert.match(result.hint, /locator.*smaller/);
  assert.ok(result.value.length > 0);
  assert.equal(result.value.length, retained);
  assert.deepEqual(result.value[0].locator.frame, frame);
});
