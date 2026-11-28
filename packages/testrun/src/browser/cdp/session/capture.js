// Owned temporary screenshot styles/masks. Rust always releases this transaction.
(injected, options) => {
  const key=Symbol.for('canonic.testrun.capture');
  const transactions=globalThis[key] ||= new Map();
  const release=state=>{if(!state)return;for(const node of state.nodes)node.remove();for(const animation of state.resume){try{animation.play();}catch{}}};
  if(options.cleanup){release(transactions.get(options.id));transactions.delete(options.id);return true;}
  if(transactions.size>=4)return {error:'CAPTURE_BUSY'};
  const nodes=[],resume=[];transactions.set(options.id,{nodes,resume});
  const one=locator=>{const elements=injected.querySelectorAll(injected.parseSelector(locator),document);if(elements.length!==1)throw Error(elements.length?'canonic:AMBIGUOUS_LOCATOR':'canonic:ELEMENT_NOT_FOUND');return elements[0];};
  const box=element=>{const r=element.getBoundingClientRect();return {x:r.x+scrollX,y:r.y+scrollY,width:r.width,height:r.height};};
  let clip=null;
  try {
    if(options.disableAnimations){const animations=document.getAnimations();if(animations.length>2048)throw Error('canonic:CAPTURE_BOUNDS');for(const animation of animations){if(animation.playState!=='running')continue;const timing=animation.effect?.getComputedTiming();if(!timing)continue;if(Number.isFinite(timing.endTime)){try{animation.finish();}catch{animation.cancel();}}else{resume.push(animation);animation.cancel();}}}
    if(options.element){const element=one(options.element);element.scrollIntoView({block:'center',inline:'center'});clip=box(element);if(clip.width<=0||clip.height<=0)throw Error('canonic:ELEMENT_NOT_VISIBLE');}
    const rectangles=[];
    for(const selector of options.masks){const elements=injected.querySelectorAll(injected.parseSelector(selector),document);if(!elements.length)throw Error('canonic:REDACTION_UNAVAILABLE');for(const element of elements){if(rectangles.length>=512)throw Error('canonic:REDACTION_UNAVAILABLE');rectangles.push(box(element));}}
    const style=document.createElement('style');style.textContent='*{caret-color:transparent!important}';document.documentElement.append(style);nodes.push(style);
    for(const r of rectangles){const mask=document.createElement('div');mask.style.cssText=`all:initial!important;position:absolute!important;left:${r.x}px!important;top:${r.y}px!important;width:${r.width}px!important;height:${r.height}px!important;background:#ff00ff!important;z-index:2147483647!important;pointer-events:none!important;`;document.documentElement.append(mask);nodes.push(mask);}
    return {clip,maskCount:rectangles.length};
  }catch(error){release({nodes,resume});transactions.delete(options.id);throw error;}
}
