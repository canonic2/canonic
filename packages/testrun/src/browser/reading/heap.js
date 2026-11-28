// Fixed privileged DevTools bridge. Only typed queries and owned capture bytes enter here.
async function heapBridge(key, action, id, argument) {
  const state = globalThis[Symbol.for(key)] ||= {items:new Map(), next:1};
  const fail = code => { throw Error(`canonic:${code}`); };
  const bounded = value => {
    let nodes=0,bytes=0,truncated=false,stopped=false;
    const omitted=Symbol(),encoder=new TextEncoder();
    const charge=size=>{if(bytes+size>65000){truncated=true;stopped=true;return false;}bytes+=size;return true;};
    const scalar=value=>charge(encoder.encode(JSON.stringify(value)??'null').length)?value:omitted;
    const nameLimit=action==='query'||action==='compare'?(argument?.maxNameLength??100):100;
    const depthLimit=action==='query'&&argument?.kind==='retaining_paths'?2*(argument.maxDepth??32)+8:12;
    const visit=(value,depth,key)=>{
      if(stopped)return omitted;
      if(++nodes>4096){truncated=true;stopped=true;return omitted;}
      if(depth>depthLimit){truncated=true;return scalar(null);}
      if(typeof value==='string'){
        const limit=key==='name'||key==='value'||key?.endsWith('Name')?nameLimit:1024;
        let text='',length=0;
        for(const point of value){if(length++===limit){truncated=true;break;}text+=point;}
        return scalar(text);
      }
      if(value===null||typeof value!=='object')return scalar(value);
      if(!charge(2))return omitted;
      if(Array.isArray(value)){
        if(value.length>100)truncated=true;
        const result=[];
        for(let index=0;index<Math.min(value.length,100)&&!stopped;index++){
          if(index&&!charge(1))break;
          const item=visit(value[index],depth+1,key);
          if(item===omitted||(stopped&&['items','classes','objects'].includes(key)))break;
          result.push(item);
        }
        return result;
      }
      const result={};let count=0;
      for(const [key,item] of Object.entries(value)){
        if(stopped)break;
        if(count++===100||key.length>65000){truncated=true;break;}
        if(!charge(encoder.encode(JSON.stringify(key)).length+1+(count>1?1:0)))break;
        const projected=visit(item,depth+1,key);if(projected===omitted)break;
        Object.defineProperty(result,key,{value:projected,enumerable:true});
      }
      return result;
    };
    const projected=visit(value,0);let data=projected===omitted?null:projected;
    if(data&&typeof data==='object'&&!Array.isArray(data)){
      if(Array.isArray(value.items)&&Number.isSafeInteger(value.startPosition)&&Number.isSafeInteger(value.totalLength)){
        const items=data.items??[],next=value.startPosition+items.length,more=next<value.totalLength;
        data={...data,items,startPosition:value.startPosition,endPosition:next,totalLength:value.totalLength,hasMore:more,nextOffset:more?next:null};
      }else if(Number.isSafeInteger(value.offset)&&Number.isSafeInteger(value.total)){
        const field=['items','classes','objects'].find(field=>Array.isArray(value[field]));
        if(field){const items=data[field]??[],next=value.offset+items.length,more=next<value.total;data={...data,[field]:items,offset:value.offset,total:value.total,hasMore:more,nextOffset:more?next:null};}
      }
    }
    if(encoder.encode(JSON.stringify(data)).length>65536)fail('HEAP_RESULT_BOUNDS');return {data,truncated};
  };
  if(action==='initialize'){
    if(state.items.size>=2)fail('HEAP_MODEL_CAPACITY');
    const module=await import('devtools://devtools/bundled/models/heap_snapshot/heap_snapshot.js');
    if(!module.HeapSnapshotProxy?.HeapSnapshotWorkerProxy)fail('HEAP_MODEL_UNAVAILABLE');
    const Common=await import('devtools://devtools/bundled/core/common/common.js');
    const entry={uid:state.next++,module,worker:null,loader:null,proxy:null,promise:null};
    let resolve,reject;entry.promise=new Promise((yes,no)=>{resolve=yes;reject=no;});entry.promise.catch(()=>{});
    entry.worker=new module.HeapSnapshotProxy.HeapSnapshotWorkerProxy((name)=>{if(name==='BrokenSnapshot')reject(Error('Invalid heap snapshot'));},Common.Console.Console.instance());
    entry.loader=entry.worker.createLoader(entry.uid,proxy=>{entry.proxy=proxy;proxy.setProfileUid(entry.uid);resolve(proxy);});state.items.set(id,entry);return true;
  }
  const entry=state.items.get(id);if(!entry)fail('UNKNOWN_HEAP_SNAPSHOT');
  if(action==='release'){entry.worker.dispose();state.items.delete(id);return true;}
  if(action==='append'){await entry.loader.write(argument);return true;}
  if(action==='finish'){
    let timer;
    try{await entry.loader.close();const proxy=await Promise.race([entry.promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('Heap parsing deadline')),30000);})]);await proxy.updateStaticData();entry.loader=null;return bounded({statistics:await proxy.getStatistics(),staticData:proxy.staticData});}
    catch(error){entry.worker.dispose();state.items.delete(id);throw error;}finally{clearTimeout(timer);}
  }
  const p=entry.proxy;if(!p||entry.loader)fail('HEAP_MODEL_PENDING');
  if(action==='compare'){
    const base=state.items.get(argument.base);if(!base?.proxy||base.loader)fail('UNKNOWN_HEAP_SNAPSHOT');
    const definitions=await p.interfaceDefinitions();const aggregates=await base.proxy.aggregatesForDiff(definitions);
    const diffs=Object.values(await p.calculateSnapshotDiff(base.uid,aggregates)).filter(item=>item.addedCount||item.removedCount).sort((a,b)=>b.sizeDelta-a.sizeDelta);
    const totals={addedCount:0,removedCount:0,countDelta:0,addedSize:0,removedSize:0,sizeDelta:0};
    for(const item of diffs)for(const key of Object.keys(totals))totals[key]+=item[key]??0;
    const summary=(item,classIndex)=>({classIndex,name:item.name,addedCount:item.addedCount,removedCount:item.removedCount,countDelta:item.countDelta,addedSize:item.addedSize,removedSize:item.removedSize,sizeDelta:item.sizeDelta});
    if(argument.classIndex!=null){
      const item=diffs[argument.classIndex];if(!item)fail('UNKNOWN_HEAP_DIFF_CLASS');
      const objects=[];
      for(const [ids,sizes,change] of [[item.addedIds,item.addedSelfSizes,'added'],[item.deletedIds,item.deletedSelfSizes,'deleted']]){
        for(let index=0;index<(ids?.length??0);index++)objects.push({id:ids[index],selfSize:sizes[index],change});
      }
      objects.sort((a,b)=>b.selfSize-a.selfSize);
      return bounded({classDiff:summary(item,argument.classIndex),total:objects.length,offset:argument.offset,hasMore:objects.length>argument.offset+argument.limit,addedObjectSnapshot:id,deletedObjectSnapshot:argument.base,objects:objects.slice(argument.offset,argument.offset+argument.limit)});
    }
    return bounded({total:diffs.length,totals,offset:argument.offset,hasMore:diffs.length>argument.offset+argument.limit,classes:diffs.slice(argument.offset,argument.offset+argument.limit).map((item,index)=>summary(item,argument.offset+index))});
  }
  const q=argument,offset=q.offset||0,limit=q.limit||50;
  const page=items=>({total:items.length,offset,hasMore:items.length>offset+limit,items:items.slice(offset,offset+limit)});
  if(q.kind==='summary')return bounded({statistics:await p.getStatistics(),staticData:p.staticData,nativeContexts:await p.getNativeContextSizes(),retainedByContext:await p.getRetainedByContextSummary()});
  if(q.kind==='contexts'){
    const analysis=await p.analyzeContexts(), ranked=[];
    for(const scope of analysis.scopes){
      if(q.scopeInfoNodeId!=null&&scope.scopeInfoNodeId!==q.scopeInfoNodeId)continue;
      for(const context of scope.contexts){
        const score=context.deadFieldsRetainedSizeSum;
        if(score<(q.retainedSize?.min??0)||score>(q.retainedSize?.max??Infinity))continue;
        ranked.push({scope,context});
      }
    }
    ranked.sort((a,b)=>b.context.deadFieldsRetainedSizeSum-a.context.deadFieldsRetainedSizeSum);
    const selected=new Map();
    for(const [index,{scope,context}] of ranked.slice(offset,offset+limit).entries()){
      if(!selected.has(scope))selected.set(scope,{scopeInfoNodeId:scope.scopeInfoNodeId,scriptNodeId:scope.scriptNodeId,...scope,contexts:[]});
      selected.get(scope).contexts.push({rank:offset+index,contextNodeId:context.contextNodeId,deadFieldsRetainedSizeSum:context.deadFieldsRetainedSizeSum,...context});
    }
    const result={totalContexts:ranked.length,offset,hasMore:ranked.length>offset+limit,scoreMetric:'deadFieldsRetainedSizeSum',scoreIsReclaimableBytes:false,scopes:[...selected.values()]};
    for(const [key,value] of Object.entries(analysis))if(!Object.prototype.hasOwnProperty.call(result,key))Object.defineProperty(result,key,{value,enumerable:true});
    const projection=bounded(result),data=projection.data;
    const present=new Set((data.scopes??[]).flatMap(scope=>(scope.contexts??[]).filter(context=>Number.isSafeInteger(context?.contextNodeId)&&Number.isSafeInteger(context.rank)).map(context=>context.rank)));
    let next=offset;while(present.has(next))next++;
    const scopes=(data.scopes??[]).map(scope=>({...scope,contexts:(scope.contexts??[]).filter(context=>context&&context.rank<next)})).filter(scope=>scope.contexts.length);
    return {...projection,data:{...data,scopes,hasMore:next<ranked.length,nextOffset:next<ranked.length?next:null}};
  }
  if(q.kind==='duplicate_strings')return bounded(page(await p.getDuplicateStrings()));
  const nodeFilter=async()=>{
    const filter=new entry.module.HeapSnapshotModel.NodeFilter();
    if(q.filterName==='attributedToSpecificNativeContext'){
      if(q.objectId==null)fail('HEAP_QUERY_BOUNDS');
      const node=await p.nodeIndexForId(q.objectId);if(node===undefined)fail('UNKNOWN_HEAP_NODE');
      filter.filterName=`nativeContext_${node}`;
    }else if(q.filterName)filter.filterName=q.filterName;
    return filter;
  };
  if(q.kind==='classes'){
    const aggregates=await p.aggregatesWithFilter(await nodeFilter());
    const rows=Object.entries(aggregates).map(([classKey,value])=>({classKey,name:value.name,count:value.count,selfSize:value.self,retainedSize:value.maxRet,distance:value.distance,type:value.type}));
    const result={objectCount:rows.reduce((count,item)=>count+item.count,0),totalSelfSize:rows.reduce((size,item)=>size+item.selfSize,0),...page(rows.sort((a,b)=>b.retainedSize-a.retainedSize))};
    entry.classIds ||= new Map();entry.classKeys ||= [];entry.classKeyBytes ||= 0;
    for(const item of result.items){
      let classId=entry.classIds.get(item.classKey);
      if(classId===undefined){
        if(item.classKey.length>65536)fail('HEAP_CLASS_SELECTOR_CAPACITY');
        const bytes=new TextEncoder().encode(item.classKey).length;
        if(bytes>65536||entry.classKeys.length>=100000||entry.classKeyBytes+bytes>8388608)fail('HEAP_CLASS_SELECTOR_CAPACITY');
        classId=entry.classKeys.push(item.classKey);entry.classIds.set(item.classKey,classId);entry.classKeyBytes+=bytes;
      }
      item.classId=classId;
    }
    return bounded(result);
  }
  let provider;
  if(q.kind==='class_nodes'){
    const classKey=entry.classKeys?.[q.classId-1];if(classKey===undefined)fail('UNKNOWN_HEAP_CLASS');
    provider=p.createNodesProviderForClass(classKey,await nodeFilter());
  }
  else if(q.kind==='objects')provider=p.queryObjects({className:q.className??undefined,propertyName:q.propertyName??undefined,nodeType:q.nodeType??undefined,minRetainedSize:q.retainedSize?.min??undefined,maxRetainedSize:q.retainedSize?.max??undefined,minSelfSize:q.selfSize?.min??undefined,maxSelfSize:q.selfSize?.max??undefined,isDetached:q.isDetached??undefined,sortBy:q.sortBy??undefined});
  else {
    const node=await p.nodeIndexForId(q.nodeId);if(node===undefined)fail('UNKNOWN_HEAP_NODE');
    if(q.kind==='object')return bounded(await p.getObjectInfo(node));
    if(q.kind==='dominators')return bounded(await p.getDominatorsOf(node));
    if(q.kind==='retaining_paths')return bounded(await p.getRetainingPaths(node,q.maxDepth??undefined,q.maxNodes??undefined,q.maxSiblings??undefined));
    if(q.kind==='edges')provider=p.createEdgesProvider(node,{sortBy:q.sortBy??'retainedSize',minRetainedSize:q.minRetainedSize??undefined,excludePrimitives:q.excludePrimitives??true});
    if(q.kind==='retainers')provider=p.createRetainingEdgesProvider(node);
  }
  if(!provider)fail('HEAP_QUERY_INVALID');
  try{return bounded(await provider.serializeItemsRange(offset,offset+limit));}finally{provider.dispose();}
}
