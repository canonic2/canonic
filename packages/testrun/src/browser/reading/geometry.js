// Document-local viewport generation catches resize/scroll changes that return to the same size.
() => {
  const key=Symbol.for('canonic.testrun.geometry.v1');
  let state=globalThis[key];
  if(!state || state.document!==document || state.root!==document.documentElement){
    state?.abort?.abort();const abort=new AbortController();const signal=abort.signal;
    state={document,root:document.documentElement,epoch:1,abort};
    const changed=()=>{state.epoch=state.epoch===Number.MAX_SAFE_INTEGER?1:state.epoch+1;};
    for(const event of ['scroll','resize','pointerlockchange','fullscreenchange'])addEventListener(event,changed,{capture:true,passive:true,signal});
    if(visualViewport)for(const event of ['scroll','resize'])visualViewport.addEventListener(event,changed,{passive:true,signal});
    Object.defineProperty(globalThis,key,{value:state,configurable:true});
  }
  return {url:location.href,viewport:{width:innerWidth,height:innerHeight},dpr:devicePixelRatio,pointerLocked:!!document.pointerLockElement,scroll:{x:scrollX,y:scrollY},document:{width:Math.max(document.documentElement.scrollWidth,innerWidth),height:Math.max(document.documentElement.scrollHeight,innerHeight)},viewportEpoch:state.epoch,visualViewport:visualViewport?{x:visualViewport.offsetLeft,y:visualViewport.offsetTop,scale:visualViewport.scale}:null};
}
