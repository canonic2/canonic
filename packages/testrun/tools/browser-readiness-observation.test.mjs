import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source=await readFile(new URL('../tests/fixtures/browser-readiness-observation.js',import.meta.url),'utf8');
function fixture() {
  const button={hidden:true,disabled:true},output={},events={},frames=[];
  let changed,clock=0,focused=true;
  const document={visibilityState:'visible',hasFocus:()=>focused,
    querySelector(selector){assert.equal(selector,'#waiting');return button;},
    createElement(kind){assert.equal(kind,'input');return output;},
    body:{append(element){assert.equal(element,output);}},
    addEventListener(name,callback){events[name]=callback;}};
  const install=vm.runInNewContext(source,{document,
    window:{addEventListener(name,callback){events[name]=callback;}},
    performance:{now:()=>clock},requestAnimationFrame:callback=>frames.push(callback),
    MutationObserver:class {
      constructor(callback){changed=callback;}
      observe(element,options){assert.equal(element,button);assert.equal(JSON.stringify(options),JSON.stringify({attributes:true,attributeFilter:['hidden','disabled']}));}
    }});
  install('#waiting');
  return {button,output,document,events,changed:()=>changed(),
    frame(at){clock=at;frames.shift()(at);},read:()=>JSON.parse(output.value),
    focus(value){focused=value;},clock(value){clock=value;}};
}
test('fixture observations preserve button readiness and report actual frame and attribute changes',()=>{
  const f=fixture();
  assert.equal(f.output.hidden,true);assert.equal(f.output.readOnly,true);
  assert.deepEqual(f.button,{hidden:true,disabled:true});
  assert.equal(f.read().frames,0);assert.equal(f.read().lastFrameAt,null);
  f.frame(17);f.frame(34);
  assert.equal(f.read().frames,2);assert.equal(f.read().lastFrameAt,34);
  f.button.hidden=false;f.button.disabled=false;f.changed();
  assert.equal(f.read().changes,1);assert.equal(f.read().hidden,false);assert.equal(f.read().disabled,false);
  assert.deepEqual(f.button,{hidden:false,disabled:false});
  assert.ok(f.output.value.length<1024);
});
test('visibility and focus observations never invent frame progress while animation callbacks stop',()=>{
  const f=fixture();f.frame(17);
  f.clock(10000);f.document.visibilityState='hidden';f.focus(false);f.events.visibilitychange();
  const value=f.read();
  assert.equal(value.frames,1);assert.equal(value.lastFrameAt,17);
  assert.equal(value.sampledAt,10000);assert.equal(value.visibility,'hidden');assert.equal(value.focused,false);
  f.document.visibilityState='visible';f.focus(true);f.events.focus();
  assert.equal(f.read().frames,1);assert.equal(f.read().lastFrameAt,17);
});
