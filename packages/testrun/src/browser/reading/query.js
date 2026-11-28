// Shared pinned locator/condition semantics; Rust owns document routing and bounds.
async (injected, read, options, bound, geometry) => {
  let elements;
  try { elements = injected.querySelectorAll(injected.parseSelector(options.selector), document); }
  catch (error) {
    if (error.message === 'canonic:STALE_REFERENCE') {
      if(options.read==='frame')throw new Error('canonic:STALE_REFERENCE');
      return {error:'STALE_REFERENCE'};
    }
    if (options.read === 'frame') throw new Error('canonic:INVALID_SELECTOR');
    return {error:'INVALID_SELECTOR'};
  }
  if (options.read === 'frame' && elements.length !== 1)
    throw new Error(elements.length ? 'canonic:AMBIGUOUS_LOCATOR':'canonic:ELEMENT_NOT_FOUND');
  if(options.actionability) {
    if(elements.length>1)return {error:'AMBIGUOUS_LOCATOR'};
    if(!elements.length||options.frameVisible===false)return {satisfied:false};
    const element=elements[0];
    const pointer=['click','double_click','hover','check','drag'].includes(options.actionability.kind);
    const states=pointer?['visible','stable',...(options.actionability.kind==='hover'?[]:['enabled'])]:['fill','clear'].includes(options.actionability.kind)?['visible','enabled','editable']:[];
    if(pointer)element.scrollIntoView({block:'center',inline:'center'});
    if(await injected.checkElementStates(element,states,true))return {satisfied:false};
    if(pointer){
      // An inline element can wrap: the union's center may hit another control.
      const box=Array.from(element.getClientRects(),rect=>({left:Math.max(0,rect.left),right:Math.min(innerWidth,rect.right),top:Math.max(0,rect.top),bottom:Math.min(innerHeight,rect.bottom)})).find(rect=>rect.right>rect.left&&rect.bottom>rect.top);
      if(!box||injected.expectHitTarget({x:(box.left+box.right)/2,y:(box.top+box.bottom)/2},element)!=='done')return {satisfied:false};
    }
    return {satisfied:true};
  }
  if (options.condition) {
    const condition = options.condition;
    if (condition.kind === 'url') return {satisfied:document.location.href===condition.value};
    // The host applies the bounded URL matcher and load state; the page reports
    // its URL and how far its document's lifecycle events have finished.
    if (condition.kind === 'url_match') {
      const timing = performance.getEntriesByType('navigation')[0];
      const loaded = timing ? timing.loadEventEnd > 0 : document.readyState === 'complete';
      const parsed = timing ? timing.domContentLoadedEventEnd > 0 : document.readyState !== 'loading';
      return {href:document.location.href, loadState:loaded ? 'load' : parsed ? 'domcontentloaded' : 'commit'};
    }
    if (!elements.length && !['count','url'].includes(condition.kind))
      return {satisfied:['detached','hidden'].includes(condition.kind),matchCount:0};
    if (elements.length > 1 && condition.kind !== 'count') return {error:'AMBIGUOUS_LOCATOR'};
    const names = {attached:'to.be.attached',detached:'to.be.detached',visible:'to.be.visible',hidden:'to.be.hidden',enabled:'to.be.enabled',disabled:'to.be.disabled',editable:'to.be.editable',checked:'to.be.checked',text:'to.have.text',value:'to.have.value',count:'to.have.count',url:'to.have.url'};
    const expected = {expression:names[condition.kind],isNot:false};
    if (condition.kind === 'checked') expected.expectedValue={checked:true};
    if (condition.kind === 'count') expected.expectedNumber=condition.value;
    if (['text','value','url'].includes(condition.kind)) expected.expectedText=[{string:condition.value,matchSubstring:!!condition.contains,normalizeWhiteSpace:condition.kind==='text'}];
    const result = await injected.expect(elements[0], expected, elements, options.frameVisible !== false);
    return {satisfied:result.matches,matchCount:elements.length};
  }
  if (options.read === 'count') return {value:elements.length};
  if (elements.length > 1) return {error:'AMBIGUOUS_LOCATOR', count:elements.length};
  if (!elements.length) return {error:'ELEMENT_NOT_FOUND'};
  const element = elements[0];
  if (options.read === 'frame') return element;
  let result;
  if (options.read === 'aria') {
    const text = injected.testrunAriaSnapshot(element);
    result = bound(text,options.maxBytes);
  } else if (options.read === 'geometry') {
    result = geometry();
  } else result = JSON.parse(read(element, {...options,retain:(element,frame)=>injected._engines.get('testrun').retain(element,frame)}));
  if (new TextEncoder().encode(JSON.stringify(result)).length > options.maxBytes)
    return {value:null,truncated:true,narrowingRequired:true};
  return result;
}
