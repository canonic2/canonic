const fs = require('node:fs');
const path = require('node:path');
const delay = (ms) => new Promise((r) => setTimeout(r, ms));
async function edits(target, fixture, manifest) {
  const files = [
    manifest.first,
    manifest.first.replace('.md', '.examples.ts'),
    manifest.shape === 'Overview' ? 'assets/icons.js' : 'shared.ts',
  ];
  const originals = new Map(
    files.map((name) => [name, fs.readFileSync(path.join(fixture, name), 'utf8')]),
  );
  const results = {};
  const wait = async (expression) => {
    const deadline = Date.now() + 12000;
    while (Date.now() < deadline) {
      const value = await target.evaluate(expression);
      if (value) return value;
      await delay(10);
    }
    const observed = await target.evaluate(
      `(async()=>{const f=window.__labFind(${JSON.stringify(manifest.first)});if(!f)return null;const hub=f.w.parent;let serverRevision;try{serverRevision=await f.w.fetch(f.w.__workbenchDocs.revisionUrl,{cache:'no-store',signal:AbortSignal.timeout(2000)}).then(r=>r.json());}catch(error){serverRevision={error:String(error)};}return {ready:f.w.__workbenchReady,examples:[...f.doc.querySelectorAll('[data-wb-example]')].map(n=>({status:n.dataset.status,text:n.querySelector('[data-wb-example-stage]')?.textContent})),revision:f.w.__workbenchDocs.revision,serverRevision,pushState:hub.__wbPushSource?.readyState,documentVisibility:f.doc.visibilityState,frames:f.chain.map(n=>({visibility:getComputedStyle(n).visibility,display:getComputedStyle(n).display,width:n.getBoundingClientRect().width,height:n.getBoundingClientRect().height}))};})()`,
    );
    throw Error(
      'Edit did not reach requested content: ' +
        expression +
        ' observed=' +
        JSON.stringify(observed),
    );
  };
  const marker = 'const acmeFenceEdit = "Acme exact revision";';
  const current = `window.__labFind(${JSON.stringify(manifest.first)})`;
  const originalMarker = manifest.shape === 'Overview' ? 'Yearly plans' : 'Acme shared original';
  const originalReady = `(()=>{const f=${current};return f?.w.__workbenchReady&&[...f.doc.querySelectorAll('[data-wb-example-stage] *')].some(n=>n.textContent===${JSON.stringify(originalMarker)});})()`;
  try {
    await target.evaluate(`__labSelect(${JSON.stringify(manifest.first)},'edit-setup')`);
    let started = performance.now();
    fs.writeFileSync(
      path.join(fixture, files[0]),
      originals.get(files[0]) + `\n\n\`\`\`ts\n${marker}\n\`\`\`\n`,
    );
    await wait(
      `(()=>{const f=${current};return f?.w.__workbenchReady&&f.chain.every(n=>getComputedStyle(n).visibility==='visible')&&[...f.doc.querySelectorAll('code[data-wb-code]')].some(n=>n.textContent===${JSON.stringify(marker)});})()`,
    );
    const visibleMs = performance.now() - started;
    await target.evaluate(`${current}.w.wbDocsPage.whenHighlighted().then(()=>true)`);
    results.fence = { visibleMs, colouredMs: performance.now() - started, exact: true };
    fs.writeFileSync(path.join(fixture, files[0]), originals.get(files[0]));
    await wait(
      `(()=>{const f=${current};return f&&!f.doc.body.textContent.includes('Acme exact revision');})()`,
    );
    await wait(originalReady);
    const original = originals.get(files[1]);
    const modified =
      manifest.shape === 'Overview'
        ? original.replace('Yearly plans', 'Acme exact example edit')
        : original.replace(
            "'<div class=\"lab-example\">'+sentinel+'</div>'",
            '\'<div class="lab-example">Acme exact example edit</div>\'',
          );
    if (modified === original) throw Error('Example edit fixture does not match');
    started = performance.now();
    fs.writeFileSync(path.join(fixture, files[1]), modified);
    await wait(
      `(()=>{const f=${current};return f&&[...f.doc.querySelectorAll('[data-wb-example-stage] *')].some(n=>n.textContent==='Acme exact example edit');})()`,
    );
    results.example = { visibleMs: performance.now() - started, exact: true };
    fs.writeFileSync(path.join(fixture, files[1]), original);
    await wait(originalReady);
    const sharedOriginal = originals.get(files[2]);
    const sharedEdit =
      manifest.shape === 'Overview'
        ? sharedOriginal.replace(
            'export function hydrateIcons(root = document) {',
            `export function hydrateIcons(root = document) {\n  root.append(Object.assign(document.createElement('p'),{textContent:'Acme exact shared edit'}));`,
          )
        : sharedOriginal.replace('Acme shared original', 'Acme exact shared edit');
    started = performance.now();
    fs.writeFileSync(path.join(fixture, files[2]), sharedEdit);
    await wait(
      `(()=>{const f=${current};return f&&[...f.doc.querySelectorAll('p,div')].some(n=>n.textContent==='Acme exact shared edit');})()`,
    );
    results.shared = { visibleMs: performance.now() - started, exact: true };
    fs.writeFileSync(path.join(fixture, files[2]), sharedOriginal);
    await wait(originalReady);
    return results;
  } finally {
    for (const [name, body] of originals) fs.writeFileSync(path.join(fixture, name), body);
  }
}
async function faults(target, variant, manifest, url) {
  if (variant === 'D') return { diagnostic: true };
  const reports = [];
  const faultPage = manifest.expected[manifest.second]?.length ? manifest.second : manifest.first;
  for (const fault of ['failure', 'timeout']) {
    const held = [],
      interceptionErrors = [],
      injections = [];
    let failuresSent = 0;
    const off = target.on('Fetch.requestPaused', (e) => {
      held.push(e);
      if (fault === 'failure')
        injections.push(
          target.send('Fetch.failRequest', { requestId: e.requestId, errorReason: 'Failed' }).then(
            () => failuresSent++,
            (error) => interceptionErrors.push(String(error)),
          ),
        );
    });
    await target.send('Network.setCacheDisabled', { cacheDisabled: true });
    await target.send('Fetch.enable', {
      patterns: [
        {
          urlPattern:
            variant === '0' ? '*/_workbench/docs/highlight*' : '*/_workbench/ui/src/lab/worker.js',
          requestStage: 'Response',
        },
      ],
    });
    try {
      const destination = new URL(url);
      destination.searchParams.set('labFault', fault);
      destination.hash = faultPage + '~html';
      target.pageUrl = null;
      await target.navigate(destination.href);
      await target.evaluate(
        `(async()=>{const deadline=performance.now()+10000;while(!window.__labSelect||!window.wbItem?.(${JSON.stringify(faultPage)})){if(performance.now()>deadline)throw Error('Fault setup not ready');await new Promise(r=>setTimeout(r,10));}return true;})()`,
      );
      const result = await target.evaluate(
        `__labSelect(${JSON.stringify(faultPage)},${JSON.stringify('fault-' + fault)})`,
      );
      const plain = await target.evaluate(
        `__labFind(${JSON.stringify(faultPage)}).doc.querySelector('code[data-wb-code]').children.length===0`,
      );
      if (!held.length || !plain) throw Error('Fault did not leave exact plain code');
      await Promise.all(injections);
      if (fault === 'failure' && !failuresSent)
        throw Error('No active colouring request received the injected failure');
      if (interceptionErrors.some((error) => !/Invalid InterceptionId/.test(error)))
        throw Error('Unexpected interception failure: ' + interceptionErrors.join('; '));
      // Resume late work; it must not colour the aborted request's nodes.
      for (const request of held)
        await target
          .send('Fetch.continueRequest', { requestId: request.requestId })
          .catch(() => {});
      await delay(250);
      const latePlain = await target.evaluate(
        `__labFind(${JSON.stringify(faultPage)}).doc.querySelector('code[data-wb-code]').children.length===0`,
      );
      if (!latePlain) throw Error('Late response coloured timed-out code');
      reports.push({
        fault,
        plain,
        latePlain,
        completionMs: result.colouredMs,
        requests: held.length,
        failuresSent,
        cancelledInterceptions: interceptionErrors,
      });
    } finally {
      off();
      await target.send('Fetch.disable');
      await target.send('Network.setCacheDisabled', { cacheDisabled: false });
    }
  }
  return reports;
}
async function lifecycle(target, manifest) {
  return target.evaluate(
    `(async()=>{const {doc,w}=__labFind(${JSON.stringify(manifest.first)});const {createHighlighting}=await w.eval("import('/_workbench/ui/src/modules/docs/page/highlighting.js')");const url=w.__workbenchDocs.sourceUrl+${JSON.stringify(manifest.example)};const original=(await w.fetch(url+'&plain=1').then(r=>r.json())).text;const node=doc.createElement('code');node.textContent=original;doc.body.append(node);const highlighter=createHighlighting();highlighter.source(node,url);node.textContent='Acme newer source';await highlighter.settled();const stale=node.textContent==='Acme newer source'&&!node.children.length;node.textContent=original;highlighter.source(node,url);highlighter.dispose();await highlighter.settled();const disposed=node.textContent===original&&!node.children.length;node.remove();return {stale,disposed};})()`,
  );
}
async function copyAndIsolation(target, manifest) {
  const copy = await target.evaluate(
    `(async()=>{const {doc,w}=__labFind(${JSON.stringify(manifest.first)});const clipboard=w.navigator.clipboard;const descriptor=Object.getOwnPropertyDescriptor(clipboard,'writeText');let copied;try{Object.defineProperty(clipboard,'writeText',{configurable:true,value:async text=>{copied=text;}});doc.querySelector('[data-wb-copy-code]').click();const end=performance.now()+5000;while(copied===undefined&&performance.now()<end)await new Promise(r=>setTimeout(r,5));return copied;}finally{if(descriptor)Object.defineProperty(clipboard,'writeText',descriptor);else delete clipboard.writeText;}})()`,
  );
  const source = await target.evaluate(
    `__labFind(${JSON.stringify(manifest.first)}).w.fetch(__labFind(${JSON.stringify(manifest.first)}).w.__workbenchDocs.sourceUrl+${JSON.stringify(manifest.example)}+'&plain=1').then(r=>r.json()).then(r=>r.text)`,
  );
  if (copy !== source) throw Error('Copy output differs from source');
  await target.evaluate(
    `(()=>{const {doc}=__labFind(${JSON.stringify(manifest.first)});for(const button of doc.querySelectorAll('[data-wb-copy-code]')){button.classList.remove('is-done');button.title='Copy code';}return true;})()`,
  );
  await target.evaluate(
    `(()=>{const {doc,w}=__labFind(${JSON.stringify(manifest.first)});w.__labOwnedState='Acme first document';doc.body.style.setProperty('--lab-owned-style','first');return true;})()`,
  );
  await target.evaluate(`__labSelect(${JSON.stringify(manifest.second)},'isolation')`);
  const independent = await target.evaluate(
    `(()=>{const {doc,w}=__labFind(${JSON.stringify(manifest.second)});return !w.__labOwnedState&&!w.getComputedStyle(doc.body).getPropertyValue('--lab-owned-style');})()`,
  );
  if (!independent) throw Error('Document state/style escaped its owner');
  // Hold a never-visited destination's document. Changing two hashes within
  // one JS turn would only test coalescing, not an in-flight supersession.
  const held = [];
  const off = target.on('Fetch.requestPaused', (event) => held.push(event));
  await target.send('Fetch.enable', {
    patterns: [{ urlPattern: '*/supersede.md*', requestStage: 'Response' }],
  });
  try {
    await target.evaluate(`location.hash='supersede.md~html';true`);
    const end = Date.now() + 5000;
    while (!held.length && Date.now() < end) await delay(5);
    if (!held.length) throw Error('Supersession did not start its delayed document');
    const returned = await target.evaluate(
      `__labSelect(${JSON.stringify(manifest.first)},'supersession')`,
    );
    let released = 0;
    for (const request of held)
      try {
        await target.send('Fetch.continueRequest', { requestId: request.requestId });
        released++;
      } catch {
        /* Navigation already cancelled this request. */
      }
    await delay(100);
    const final = await target.evaluate(
      `(()=>{const f=__labFind(${JSON.stringify(manifest.first)});return location.hash.includes(${JSON.stringify(manifest.first)})&&f.chain.every(n=>getComputedStyle(n).visibility==='visible');})()`,
    );
    if (!final) throw Error('A stale destination replaced the superseding selection');
    return {
      copy: true,
      independent,
      supersession: final,
      inFlight: held.length,
      released,
      documentId: returned.documentId,
    };
  } finally {
    off();
    await target.send('Fetch.disable');
  }
}
async function exampleFailure(target, fixture, manifest) {
  const file = path.join(fixture, manifest.first.replace('.md', '.examples.ts'));
  const original = fs.readFileSync(file, 'utf8');
  const name = manifest.shape === 'Overview' ? 'tokensInUse' : 'basic';
  try {
    fs.writeFileSync(
      file,
      `export const ${name}=()=>{throw new Error('Acme expected example failure');};\n`,
    );
    const deadline = Date.now() + 10000;
    let found = false;
    while (Date.now() < deadline) {
      found = await target.evaluate(
        `(()=>{const f=__labFind(${JSON.stringify(manifest.first)});return !!f?.doc.querySelector('[data-wb-example][data-status="error"]')?.textContent.includes('Acme expected example failure');})()`,
      );
      if (found) break;
      await delay(10);
    }
    if (!found) throw Error('Example failure was not contained in its panel');
    await target.evaluate(`__labSelect(${JSON.stringify(manifest.second)},'failure-neighbour')`);
    return { contained: true, neighbour: true };
  } finally {
    fs.writeFileSync(file, original);
    await target.evaluate(`__labSelect(${JSON.stringify(manifest.first)},'failure-restore')`);
    await target.evaluate(
      `(async()=>{const end=performance.now()+10000;while(true){const f=__labFind(${JSON.stringify(manifest.first)});if(f?.w.__workbenchReady&&[...f.doc.querySelectorAll('[data-wb-example-stage] *')].some(n=>n.textContent===${JSON.stringify(manifest.shape === 'Overview' ? 'Yearly plans' : 'Acme shared original')}))return true;if(performance.now()>end)throw Error('Original example did not recover after failure injection');await new Promise(r=>setTimeout(r,10));}})()`,
    );
  }
}
module.exports = { edits, faults, lifecycle, copyAndIsolation, exampleFailure };
