// Shared bounded DOM reader; page content and labels are untrusted data.
(element, options) => {
  const cap = Math.min(4096, Math.floor(options.maxBytes / 3));
  let truncated = false;
  const text = (value, limit = cap) => {
    const string = String(value ?? '');
    let end = Math.min(string.length, limit);
    if (end < string.length && string.charCodeAt(end - 1) >= 0xD800 && string.charCodeAt(end - 1) <= 0xDBFF) --end;
    if (end < string.length) truncated = true;
    return string.slice(0, end);
  };
  const secure = el => el.matches?.('input[type="password" i],input[autocomplete~="current-password" i],input[autocomplete~="new-password" i]');
  const label = el => text(el.labels?.[0]?.innerText || el.getAttribute('aria-label') || el.getAttribute('placeholder') || '');
  let value;
  if (options.read === 'styles') {
    const style = getComputedStyle(element);
    const names=options.properties?.length?options.properties:['display','visibility','opacity','position','color','background-color','font-family','font-size','font-weight','line-height','width','height','overflow','pointer-events','transform'];
    value = Object.fromEntries(names.map(name => [name,text(style.getPropertyValue(name))]));
  }
  if (options.read === 'readable') {
    const root = element.querySelector('article,main,[role=main]') || element;
    value = {title:text(document.title),url:text(location.href),text:text(root.innerText || root.textContent)};
  }
  if (options.read === 'text') value = text(element.innerText || element.textContent);
  if (options.read === 'state') {
    const box = element.getBoundingClientRect();
    value = {tag: element.tagName.toLowerCase(), role: element.getAttribute('role'), label: label(element),
      value: secure(element) ? null : ('value' in element ? text(element.value) : null),
      valueMasked: !!secure(element), checked: 'checked' in element ? element.checked : null,
      disabled: element.matches(':disabled,[aria-disabled="true"]'),
      required: element.matches(':required'), invalid: element.matches(':invalid,[aria-invalid="true"]'),
      focused: element === document.activeElement,
      box: {x: box.x, y: box.y, width: box.width, height: box.height}};
  }
  if (options.read === 'form') {
    const fields = [];
    const pending=[element]; let visited=0;
    while (pending.length && visited++ < 512) {
      const el=pending.pop();
      if (el.matches('input,select,textarea,button')) {
        if (fields.length === 64) { truncated = true; break; }
        fields.push({tag: el.tagName.toLowerCase(), type: el.type, label: label(el), name: text(el.name),
          value: secure(el) ? null : text(el.value), valueMasked: !!secure(el), checked: el.checked ?? null,
          required: el.required, disabled: el.disabled, invalid: el.matches(':invalid'),
          options: el.options ? Array.from(el.options).slice(0,32).map(o => ({label:text(o.label),value:text(o.value),selected:o.selected})) : null});
      }
      for (const root of [el,el.shadowRoot].filter(Boolean)) {
        for (let i=root.children.length-1;i>=0;i--) {
          if (pending.length===512) {truncated=true;break;}
          pending.push(root.children[i]);
        }
      }
    }
    if (pending.length) truncated=true;
    value = fields;
  }
  if (options.read === 'snapshot') {
    const hint='Pass a locator for a smaller region, dialog, or container and retry.';
    const encoder=new TextEncoder();
    const nodes=[];const pending=[element];let visited=0;
    let bytes=encoder.encode(JSON.stringify({value:[],truncated:true,narrowingRequired:true,hint})).length;
    while(pending.length && visited++<512) {
      const el=pending.pop();
      if(el.matches('a[href],button,input,select,textarea,iframe,[role],[tabindex],[contenteditable=true]')) {
        if(nodes.length===32){truncated=true;break;}
        const node={tag:text(el.tagName.toLowerCase(),64),role:el.hasAttribute('role')?text(el.getAttribute('role'),64):null,
          label:text(label(el),128),text:secure(el)?null:text(el.innerText||'',128),valueMasked:!!secure(el),
          locator:{kind:'element',frame:options.frame,id:'00000000-0000-0000-0000-000000000000'}};
        bytes+=encoder.encode(JSON.stringify(node)).length+(nodes.length?1:0);
        if(bytes>options.maxBytes){truncated=true;break;}
        node.locator=options.retain(el,options.frame);
        nodes.push(node);
      }
      for(const root of [el,el.shadowRoot].filter(Boolean))for(let i=root.children.length-1;i>=0;i--){
        if(pending.length===512){truncated=true;break;}pending.push(root.children[i]);
      }
    }
    if(pending.length)truncated=true;
    return JSON.stringify(truncated?{value:nodes,truncated,narrowingRequired:true,hint}:{value:nodes,truncated});
  }
  if (options.read === 'dom') {
    const nodes = []; let visited = 0;
    const visit = (el, depth) => {
      if (++visited > 128 || depth > 12) { truncated = true; return; }
      const children = [];
      const node = {tag:el.tagName.toLowerCase(),role:el.getAttribute('role'),label:label(el),
        text: el.children.length ? null : text(el.innerText || el.textContent),children};
      if (secure(el)) { node.text = null; node.valueMasked = true; }
      for (const root of [el,el.shadowRoot].filter(Boolean)) {
        for (let i=0;i<root.children.length;i++) {
          if (visited>=128) {truncated=true;break;}
          children.push(visit(root.children[i],depth+1));
        }
      }
      return node;
    };
    nodes.push(visit(element,0)); value = nodes;
  }
  if (options.read === 'messages') {
    const state = globalThis.__testrunMessages;
    value = {entries: state?.entries.slice(-32) || [], dropped:state?.dropped || 0, documentScoped:true};
  }
  let result = {value,truncated};
  if (new TextEncoder().encode(JSON.stringify(result)).length > options.maxBytes)
    result = {value:null,truncated:true,narrowingRequired:true,hint:'Pass a locator for a smaller subtree and retry.'};
  return JSON.stringify(result);
}
