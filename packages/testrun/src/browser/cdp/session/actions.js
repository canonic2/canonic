// Pinned helper semantics for element state, editing and event interception.
async (injected, element, options) => {
  const action = options.action;
  if (options.stage === 'verify') {
    const hit = injected.testrunInterceptor?.stop();
    injected.testrunInterceptor=null;
    if (hit && hit!=='done') return {error:'HIT_TARGET_CHANGED'};
    // A completed action can remove its own target. Only check/uncheck
    // requires a final observation of the same control's state.
    if (action.kind==='check') {
      if (!element?.isConnected) return {error:'STALE_REFERENCE'};
      if (injected.elementState(element,'checked',true).matches!==action.checked) return {error:'CHECK_STATE_UNCHANGED'};
    }
    return {done:true};
  }
  if (!element?.isConnected) return {error:'STALE_REFERENCE'};
  const editing = action.kind==='fill' || action.kind==='clear';
  const input = editing ? injected.retarget(element,'follow-label') : null;
  const dateInput = input?.nodeName === 'INPUT' && input.type === 'date';
  const dateValue = dateInput && action.kind==='fill' ? action.value.trim() : '';
  if (dateInput && dateValue) {
    const probe = input.ownerDocument.createElement('input');
    probe.type='date';
    probe.value=dateValue;
    if (probe.value!==dateValue) return {error:'INVALID_DATE_VALUE'};
  }
  if (options.stage === 'prepare') {
    const pointer = ['click','double_click','hover','check','drag'].includes(action.kind);
    const states = options.force ? [] : pointer ? ['visible','stable',...(action.kind==='hover'?[]:['enabled'])] : editing ? ['visible','enabled','editable'] : [];
    const missing = await injected.checkElementStates(element,states,true);
    if (missing) return {retry:true};
    if (action.kind==='check' && injected.elementState(element,'checked',true).matches===action.checked) return {done:true};
    if (pointer) {
      const rect = Array.from(element.getClientRects(),rect=>({left:Math.max(0,rect.left),right:Math.min(innerWidth,rect.right),top:Math.max(0,rect.top),bottom:Math.min(innerHeight,rect.bottom)})).find(rect=>rect.right>rect.left&&rect.bottom>rect.top);
      if (!rect) return {retry:true};
      const x=(rect.left+rect.right)/2;
      const y=(rect.top+rect.bottom)/2;
      if (!options.force) {
        const interceptor = injected.setupHitTargetInterceptor(element,action.kind==='hover'?'hover':action.kind==='drag'?'drag':'mouse',{x,y},false);
        if (typeof interceptor !== 'object') return {retry:true};
        injected.testrunInterceptor?.stop();
        injected.testrunInterceptor=interceptor;
      }
      return {pointer:true,x,y};
    }
    return {ready:true};
  }
  if (dateInput) {
    // A native date control takes a value directly, without focusing its picker.
    input.value=dateValue;
    input.dispatchEvent(new Event('input',{bubbles:true,composed:true}));
    input.dispatchEvent(new Event('change',{bubbles:true}));
    return {done:true};
  }
  if (editing) {
    const result = injected.fill(element, action.kind==='clear'?'':action.value);
    return {needsInput:result==='needsinput',done:result==='done'};
  }
  if (action.kind==='select') {
    const selected=injected.selectOptions(element,action.values.map(value=>({value})));
    if (selected==='error:optionsnotfound') return {error:'OPTION_NOT_FOUND'};
    return {selected};
  }
  if (action.kind==='dispatch') { injected.dispatchEvent(element,action.event,{}); return {done:true}; }
  if (action.kind==='blur') { element.blur(); return {done:true}; }
  const result=injected.focusNode(element,action.kind==='type');
  return result==='error:notconnected'?{error:'STALE_REFERENCE'}:{done:true};
}
