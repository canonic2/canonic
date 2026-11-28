// Fixed privileged bridge to Chrome's native cascade model.
async function cssBridge(targetId,backendNodeId,offset,limit){
  const SDK=await import('devtools://devtools/bundled/core/sdk/sdk.js');
  const target=SDK.TargetManager.TargetManager.instance().targets().find(target=>target.id()===targetId);
  if(!target)throw Error('canonic:CSS_TARGET_UNAVAILABLE');
  const dom=target.model(SDK.DOMModel.DOMModel),css=target.model(SDK.CSSModel.CSSModel);
  if(!dom||!css)throw Error('canonic:CSS_MODEL_UNAVAILABLE');
  const nodes=await dom.pushNodesByBackendIdsToFrontend(new Set([backendNodeId]));
  const node=nodes?.get(backendNodeId)?.enclosingElementOrSelf();
  if(!node)throw Error('canonic:CSS_NODE_UNAVAILABLE');
  const matched=await css.getMatchedStyles(node.id);
  if(!matched)throw Error('canonic:CSS_MODEL_UNAVAILABLE');
  let truncated=false;
  const text=value=>{value=String(value??'');if(value.length>2048){truncated=true;return Array.from(value).slice(0,2048).join('');}return value;};
  const url=value=>{if(!value)return null;try{const u=new URL(value);if(!['http:','https:'].includes(u.protocol))return '[non-network URL]';u.username='';u.password='';u.search='';u.hash='';return text(u.href);}catch{return '[unavailable URL]';}};
  const styles=matched.nodeStyles();
  const items=[];
  for(const style of styles.slice(offset,offset+limit)){
    const rule=style.parentRule;
    const wrappers=[];let wrapperBytes=0;
    for(const [kind,values]of [['layer',rule?.layers],['media',rule?.media],['container',rule?.containerQueries],['scope',rule?.scopes],['supports',rule?.supports]]){
      if(values?.length>32)truncated=true;
      for(const value of (values??[]).slice(0,32)){
        const wrapper={kind,text:text(value.text)};wrapperBytes+=new TextEncoder().encode(JSON.stringify(wrapper)).length;
        if(wrapperBytes>4096){truncated=true;break;}wrappers.push(wrapper);
      }
    }
    const properties=style.leadingProperties();
    if(properties.length>100)truncated=true;
    const declarations=[];let declarationBytes=0;
    for(const property of properties.slice(0,100)){
      const declaration={name:text(property.name),value:text(property.value),important:!!property.important,disabled:!!property.disabled,parsed:property.parsedOk!==false,state:matched.propertyState(property),implicit:!!property.implicit};
      declarationBytes+=new TextEncoder().encode(JSON.stringify(declaration)).length;
      if(declarationBytes>16384){truncated=true;break;}declarations.push(declaration);
    }
    const item={type:style.type,inherited:matched.isInherited(style),selector:rule?.selectorText?text(rule.selectorText()):null,sourceUrl:url(rule?.resourceURL?.()),line:rule?.lineNumberInSource?rule.lineNumberInSource(0)+1:null,origin:rule?.origin??null,wrappers,declarations};
    if(new TextEncoder().encode(JSON.stringify({...result(),items:[...items,item]})).length>60000){truncated=true;break;}
    items.push(item);
  }
  function result(){return {total:styles.length,offset,nextOffset:offset+items.length,hasMore:styles.length>offset+items.length,items,truncated,snapshot:false,order:'native-cascade-precedence',source:'bundled-DevTools-CSSMatchedStyles'};}
  if(!items.length&&styles.length>offset)throw Error('canonic:CSS_RESULT_BOUNDS');
  return result();
}
