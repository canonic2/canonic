// Observe bounded transient app messages from owned document creation onward.
(() => {
  if (globalThis.__testrunMessages) return;
  const state = {entries:[],dropped:0};
  Object.defineProperty(globalThis,'__testrunMessages',{value:state,configurable:true});
  const selector = '[role="alert"],[role="status"],[aria-live="polite"],[aria-live="assertive"]';
  let sequence = 0;
  state.reset = () => { state.entries.length=0; state.dropped=0; sequence=0; };
  const inspect = node => {
    if (node.nodeType !== 1) return;
    const matches=node.querySelectorAll(selector);
    state.dropped+=Math.max(0,matches.length-64);
    const candidates = [node,...Array.from(matches).slice(0,64)];
    for (const candidate of candidates) {
      if (!candidate.matches(selector)) continue;
      const text = (candidate.innerText || candidate.textContent || '').trim().slice(0,512);
      if (!text || state.entries.at(-1)?.text === text) continue;
      if (state.entries.length === 64) { state.entries.shift();state.dropped++; }
      state.entries.push({sequence:++sequence,text,role:candidate.getAttribute('role'),timestamp:performance.now()});
    }
  };
  const observer = new MutationObserver(records => {
    state.dropped+=Math.max(0,records.length-128);
    for (const record of records.slice(0,128)) {
      inspect(record.target.nodeType === 1 ? record.target : record.target.parentElement || document.documentElement);
      for (const node of record.addedNodes) inspect(node);
      for (const node of record.removedNodes) inspect(node);
    }
  });
  observer.observe(document,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:['role','aria-live']});
})();
