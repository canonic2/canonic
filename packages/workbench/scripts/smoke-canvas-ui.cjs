/* Production acceptance: pages own artboards; ordinary states stay alternatives. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const server = require('../server/node/server');
const { Browser } = require('./chrome.cjs');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
async function main() {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-canvas-ui-'));
  // Run authored examples from an owned project copy. Rspack's cache is keyed
  // by project root, so testing the live checkout would contend with VS Code.
  const fixtureRepo = path.join(folder, 'project');
  fs.mkdirSync(fixtureRepo);
  const repo = path.resolve(__dirname, '../../..');
  fs.copyFileSync(path.join(repo, 'workbench.yaml'), path.join(fixtureRepo, 'workbench.yaml'));
  const copy = (from, to) => fs.cpSync(from, to, { recursive: true, filter: source => !['node_modules', '.canonic', '.git'].includes(path.basename(source)) && !path.basename(source).startsWith('.workbench-') });
  const fixtureDemo = path.join(fixtureRepo, 'demo');
  copy(path.join(repo, 'demo'), fixtureDemo);
  fs.symlinkSync(path.join(repo, 'demo/node_modules'), path.join(fixtureDemo, 'node_modules'), 'junction');
  const fixtureWorkbench = path.join(fixtureRepo, 'packages/workbench');
  fs.mkdirSync(fixtureWorkbench, { recursive: true });
  for (const area of ['design', 'ui', 'contracts']) copy(path.join(repo, 'packages/workbench', area), path.join(fixtureWorkbench, area));
  fs.copyFileSync(path.join(repo, 'packages/workbench/package.json'), path.join(fixtureWorkbench, 'package.json'));
  fs.symlinkSync(path.join(repo, 'packages/workbench/node_modules'), path.join(fixtureWorkbench, 'node_modules'), 'junction');
  fs.writeFileSync(path.join(folder, 'workbench.yaml'), `name: Acme
collections:
  - name: Pages
    items:
      - label: Form
        src: form.html
        states:
          - id: default
            label: Default
          - id: error
            label: Error
      - label: Other
        src: other.html
      - label: Comparison
        src: comparison.workbench.ts
`);
  fs.writeFileSync(path.join(folder, 'form.html'), '<html><body><input aria-label="Name"><p>Form</p></body></html>');
  fs.writeFileSync(path.join(folder, 'other.html'), '<html><body>Other page</body></html>');
  fs.writeFileSync(path.join(folder, 'comparison.workbench.ts'), `import {definePreview} from '@canonic2/workbench';export default definePreview({id:'comparison',adapter:'html',source:{entry:'./component.ts'},inputs:{text:'First'},states:{default:{},alternate:{inputs:{text:'Alternate'}}},artboards:{one:{label:'One',width:320,height:120},two:{label:'Two',width:480,height:160,inputs:{text:'Second'}}}});`);
  fs.writeFileSync(path.join(folder, 'component.ts'), `export default function(canvas,context){const field=document.createElement('input');field.value=context.inputs.text;canvas.append(field);}`);
  const browser = new Browser(); let running, demo, design, handoff;
  try {
    running = await server.start({ root: folder, onHandoff(text, payload) { handoff = { text, payload }; } });
    await browser.launch(); const target = await browser.createTarget(); await target.enable();
    const errors = []; target.on('Runtime.exceptionThrown', e => errors.push(e.exceptionDetails.exception?.description || e.exceptionDetails.text));
    async function until(expression) {
      for (let i = 0; i < 180; i++) { if (await target.evaluate(expression)) return; await wait(100); }
      throw Error('Timed out: ' + expression + '\n' + JSON.stringify(errors) + '\n' + await target.evaluate('JSON.stringify({hash:location.hash,controls:{hidden:document.getElementById("previewControls")?.hidden,expanded:document.getElementById("previewControls")?.getAttribute("aria-expanded"),panelHidden:document.querySelector(".wb-preview-controls")?.hidden,fields:document.querySelectorAll(".wb-preview-controls label input").length},blank:document.getElementById("blank")?.textContent,toast:document.getElementById("toast")?.textContent,state:document.getElementById("artboards")?.state})'));
    }
    const state = 'document.getElementById("artboards")?.state';
    const frames = 'document.querySelectorAll("wb-canvas > iframe:not([inert])")';
    await target.navigate(running.url);
    await until(`${state}?.artboards.length===0 && !document.getElementById('blank').hidden && document.getElementById('blank').textContent==='Pick a page or a component to see it here.'`);
    assert.equal(await target.evaluate("getComputedStyle(document.getElementById('artboards')).display"), 'none', 'the shell alone renders the empty message');
    await target.evaluate('location.hash="#form.html@mobile"');
    await until(`${state}?.artboards.length===1 && ${state}.artboards[0].view?.ready`);
    assert.equal(await target.evaluate("getComputedStyle(document.getElementById('artboards')).display"), 'block');
    assert.equal(await target.evaluate("document.getElementById('blank').hidden"), true);
    await target.evaluate('location.hash=""');
    await until(`${state}.artboards.length===0 && !document.getElementById('blank').hidden`);
    assert.equal(await target.evaluate("getComputedStyle(document.getElementById('artboards')).display"), 'none', 'clearing selection returns to one empty message');
    await target.evaluate('location.hash="#form.html@mobile"');
    await until(`${state}.artboards.length===1 && ${state}.artboards[0].view?.ready`);
    assert.equal(await target.evaluate('document.querySelectorAll(".wb-topbar").length'), 1);
    assert.equal(await target.evaluate('!!document.querySelector("#stateButton") && !!document.querySelector("#topbarOverflowMenu") && !!document.querySelector("#shapeTool") && !!document.querySelector("#zoomCenter")'), true);
    assert.equal(await target.evaluate('!!document.querySelector("#showStates,#add,#picker,.canvas-actions,.inspector")'), false);
    await target.evaluate('document.getElementById("stateMenu").querySelectorAll("button")[1].click()');
    await until('location.hash.includes(":error")');
    await until(`${state}.artboards.length===1 && ${state}.artboards[0].view?.ready && ${state}.artboards[0].target.state==='error'`);
    await target.evaluate('location.hash="#comparison.workbench.ts"');
    await until(`${state}.artboards.length===2 && ${state}.artboards.every(b=>b.view?.ready)`);
    assert.deepEqual(await target.evaluate(`${state}.artboards.map(b=>[b.target.artboard,b.target.state,b.size.width,b.size.height])`), [['one',null,320,120],['two',null,480,160]]);
    await target.evaluate('document.getElementById("stateMenu").querySelectorAll("button")[1].click()');
    await until(`${state}.artboards.length===2 && ${state}.artboards.every(b=>b.view?.ready && b.target.state==='alternate')`);
    assert.equal(await target.evaluate(`[...${frames}].every(f=>f.contentDocument.querySelector('iframe.is-active').contentDocument.querySelector('input').value==='Alternate')`), true);
    await target.evaluate('document.getElementById("stateMenu").querySelectorAll("button")[0].click()');
    await until(`${state}.artboards.length===2 && ${state}.artboards.every(b=>b.view?.ready && b.target.state===null)`);
    await target.evaluate(`${frames}[0].contentDocument.querySelector('iframe.is-active').contentDocument.querySelector('input').value='First only'`);
    assert.equal(await target.evaluate(`${frames}[1].contentDocument.querySelector('iframe.is-active').contentDocument.querySelector('input').value`), 'Second');
    await target.evaluate('document.getElementById("artboards").shadowRoot.querySelectorAll("section button")[1].click()');
    await target.evaluate('document.getElementById("sizeSwitcher").shadowRoot.querySelector("[data-key=laptop]").click()');
    await until(`${state}.artboards[1].size.width===1512`);
    assert.equal(await target.evaluate(`${state}.artboards[0].size.width`), 320);
    await target.evaluate('document.getElementById("artboards").shadowRoot.querySelectorAll("section button")[0].click()');
    await until(`${state}.selected===${state}.artboards[0].id`);
    assert.equal(await target.evaluate(`${frames}[0].contentDocument.querySelector('iframe.is-active').contentDocument.querySelector('input').value`), 'First only');
    await target.evaluate('document.querySelector("[data-tool=arrow]").click()');
    await until(`${state}.artboards[0].view.tool==='arrow'`);
    assert.equal(await target.evaluate(`${state}.artboards[1].view.tool`), 'pointer');
    const rail = await target.evaluate(`(()=>{const r=document.getElementById('artboards').shadowRoot.querySelector('section[data-selected=true] [data-edge=right]').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await target.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: rail.x, y: rail.y, button: 'left', buttons: 1, clickCount: 1 });
    await target.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: rail.x + 25, y: rail.y, button: 'left', buttons: 1 });
    await target.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: rail.x + 25, y: rail.y, button: 'left', buttons: 0 });
    await until(`${state}.artboards[0].size.width>320`);
    assert.equal(await target.evaluate(`${state}.artboards[1].size.width`), 1512);
    await target.evaluate('document.getElementById("handoff").click()');
    for (let i = 0; i < 60 && !handoff; i++) await wait(1000);
    assert.ok(handoff, 'the original handoff button captures the complete canvas');
    assert.equal(handoff.payload.artboards.length, 2);
    assert.deepEqual(handoff.payload.artboards.map(b=>b.artboard), ['one','two']);
    assert.ok(fs.existsSync(path.join(folder, handoff.payload.file)));
    await target.evaluate('document.getElementById("zoomValue").click();document.getElementById("zoomMenu").querySelectorAll("button")[1].click()');
    assert.equal(await target.evaluate('document.getElementById("zoomValue").textContent'), '100%');
    await target.evaluate('location.hash="#other.html@mobile"');
    await until(`${state}.artboards.length===1 && ${state}.artboards[0].target.src==='other.html' && ${state}.artboards[0].view?.ready`);
    assert.equal(await target.evaluate(`${frames}.length`), 1);
    demo = await server.start({ root: fixtureDemo, config: { dir: fixtureRepo, key: 'acme' } });
    await target.navigate(demo.url + '#previews/alert.workbench.ts~design');
    await until(`${state}?.artboards.length===5 && ${state}.artboards.every(b=>b.view?.ready && b.size.width===560 && b.size.height<150)`);
    assert.deepEqual(await target.evaluate(`${state}.artboards.map(b=>b.target.artboard)`), ['info','success','warning','danger','autopilot']);
    assert.equal(await target.evaluate(`${state}.artboards.every(b=>b.target.state===null)`), true);
    assert.equal(await target.evaluate('document.getElementById("stateControl").hidden'), true);
    assert.equal(await target.evaluate(`document.querySelectorAll('#pageList [data-src="previews/alert.workbench.ts"][data-state]').length`), 0);
    await until('!document.getElementById("previewControls").hidden');
    await target.evaluate('document.getElementById("previewControls").click()');
    await target.evaluate('document.getElementById("topbarOverflowButton").click()');
    assert.equal(await target.evaluate('document.getElementById("topbarOverflowMenu").textContent.includes("Configure pages")'), true);
    await target.evaluate('document.getElementById("configureSpace").click()');
    await until('document.getElementById("configDialog").open');
    await target.evaluate('document.getElementById("configClose").click()');
    await target.evaluate('document.getElementById("exportSpace").click()');
    await until('document.getElementById("exportDialog").shadowRoot.querySelector("dialog").open');
    await target.evaluate('document.getElementById("exportDialog").shadowRoot.querySelector("dialog").close()');
    await target.evaluate('document.getElementById("topbarOverflowButton").click()');
    await until('!document.querySelector(".wb-preview-controls").hidden && document.querySelector(".wb-preview-controls label input")');
    await target.evaluate(`const labels=[...document.querySelectorAll('.wb-preview-controls label')];const field=labels.find(l=>l.firstChild.textContent.toLowerCase()==='body').querySelector('input,textarea');field.value='Long content '.repeat(100);field.dispatchEvent(new Event('change'));`);
    await until(`${state}.artboards[0].size.height>150`);
    assert.equal(await target.evaluate(`${state}.artboards.slice(1).every(b=>b.size.height<150)`), true);
    assert.equal(await target.evaluate(`getComputedStyle(${frames}[0].contentDocument.querySelector('iframe.is-active').contentDocument.body).backgroundColor`), 'rgba(0, 0, 0, 0)');
    await target.evaluate(`document.querySelector('.wb-preview-controls>button').click()`);
    await until(`${state}.artboards.every(b=>b.size.height<150)`);
    await target.evaluate('document.getElementById("previewControls").click()');
    const shot = await target.send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync('/private/tmp/canonic-existing-chrome-artboards.png', Buffer.from(shot.data, 'base64'));
    await target.evaluate('location.hash="#previews/button.workbench.ts~design"');
    await until(`${state}.artboards.length===1 && ${state}.artboards[0].target.src==='previews/button.workbench.ts' && ${state}.artboards[0].view?.ready`);
    assert.equal(await target.evaluate('document.getElementById("stateControl").hidden'), false);
    await target.evaluate('document.getElementById("stateMenu").querySelectorAll("button")[2].click()');
    await until(`${state}.artboards.length===1 && ${state}.artboards[0].target.state==='gradient' && ${state}.artboards[0].view?.ready`);
    design = await server.start({ root: fixtureWorkbench, config: { dir: fixtureRepo, key: 'workbench' } });
    const config = await (await fetch(new URL('config', design.url))).json();
    const canvasPage = Object.keys(config.pages).find(src => src === 'design/canvas.md' || src === 'design/canvas.workbench.ts');
    assert.ok(canvasPage, 'the component design page is discovered');
    await target.navigate(design.url + '#' + canvasPage + '~docs');
    await until(`${state}?.artboards.length===1 && ${state}.artboards[0].view?.ready`);
    await until(`(()=>{const page=${frames}[0].contentDocument.querySelector('iframe.is-active')?.contentDocument;const examples=page?.querySelectorAll('[data-wb-example]');return examples?.length===3 && [...examples].every(e=>e.dataset.status==='ready')})()`);
    assert.deepEqual(errors, []);
    console.log('Page artboards passed: ordinary HTML and Button states remain single views; Feedback has five declared artboards with no variant states; independent sizes, controls, annotations, handoff and design examples work.');
  } finally { await browser.close(); await running?.close(); await demo?.close(); await design?.close(); fs.rmSync(folder, { recursive: true, force: true }); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
