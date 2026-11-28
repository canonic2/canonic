(injected,options)=>{
  const key=Symbol.for('canonic.testrun.annotations');
  const previous=globalThis[key];
  const boxes=[];
  for(const marker of options.markers){const elements=injected.querySelectorAll(injected.parseSelector(marker.selector),document);if(elements.length!==1)throw Error(elements.length?'canonic:AMBIGUOUS_LOCATOR':'canonic:ELEMENT_NOT_FOUND');boxes.push({element:elements[0],label:marker.label});}
  const host=document.createElement('div');host.style.cssText='position:fixed!important;inset:0!important;pointer-events:none!important;z-index:2147483646!important;';
  const shadow=host.attachShadow({mode:'closed'});
  for(const entry of boxes){const box=document.createElement('div');box.style.cssText='position:absolute;outline:2px solid #e6007e;background:transparent;';const text=document.createElement('span');text.textContent=entry.label;text.style.cssText='position:absolute;top:0;left:0;background:#e6007e;color:white;font:12px sans-serif;';box.append(text);shadow.append(box);entry.box=box;}
  if(boxes.length)document.documentElement.append(host);
  if(previous){cancelAnimationFrame(previous.tick);previous.host.remove();}
  const state={host,id:options.id,tick:0};globalThis[key]=state;
  const update=()=>{if(globalThis[key]!==state)return;if(boxes.length&&!host.isConnected){delete globalThis[key];return;}for(const {element,box}of boxes){const rect=element.getBoundingClientRect();box.hidden=!element.isConnected||rect.width<=0||rect.height<=0;Object.assign(box.style,{left:`${rect.x}px`,top:`${rect.y}px`,width:`${rect.width}px`,height:`${rect.height}px`});}if(boxes.length)state.tick=requestAnimationFrame(update);};update();
  return {annotationId:options.id,count:boxes.length,documentScoped:true,redaction:false};
}
