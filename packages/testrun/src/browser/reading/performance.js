// Privileged, fixed native trace-model bridge; no application expression is evaluated.
async function performanceBridge(key,action,id,argument){
  const fail=code=>{throw Error(`canonic:${code}`);};
  const bounded=value=>{
    let nodes=0,truncated=false;const ancestors=new WeakSet();
    const visit=(value,depth)=>{
      if(++nodes>4096||depth>12){truncated=true;return null;}
      if(typeof value==='string'){if(value.length>1024){truncated=true;return Array.from(value).slice(0,1024).join('');}return value;}
      if(typeof value==='number'&&!Number.isFinite(value))return {specialNumber:String(value)};
      if(typeof value==='function'){truncated=true;return null;}
      if(value===null||typeof value!=='object')return value;
      if(ancestors.has(value)){truncated=true;return '[circular]';}ancestors.add(value);
      let result;
      if(value instanceof Map||value instanceof Set){result=[];let count=0;for(const item of value){if(count++===100){truncated=true;break;}result.push(visit(item,depth+1));}}
      else if(Array.isArray(value)){if(value.length>100)truncated=true;result=value.slice(0,100).map(item=>visit(item,depth+1));}
      else if(value instanceof URL)result=visit(String(value),depth+1);
      else{result={};let count=0;for(const [name,item]of Object.entries(value)){if(typeof item==='function')continue;if(count++===100){truncated=true;break;}Object.defineProperty(result,name,{value:visit(item,depth+1),enumerable:true});}}
      ancestors.delete(value);return result;
    };
    const data=visit(value,0);if(new TextEncoder().encode(JSON.stringify(data)).length>65536)fail('PERFORMANCE_RESULT_BOUNDS');return {data,truncated};
  };
  const redact=value=>{try{const url=new URL(String(value));if(!['http:','https:'].includes(url.protocol))return '[non-network URL]';url.username='';url.password='';url.search='';url.hash='';return url.href;}catch{return '[unavailable URL]';}};
  const symbol=Symbol.for(key);let entry=globalThis[symbol];
  if(action==='initialize'){
    if(entry)fail('PERFORMANCE_MODEL_CAPACITY');globalThis[symbol]={id,parts:[],bytes:0,model:null,trace:null,module:null};return true;
  }
  if(!entry||entry.id!==id)fail('UNKNOWN_PERFORMANCE_ANALYSIS');
  const dispose=()=>{entry.model?.deleteTraceByIndex(0);entry.model?.resetProcessor();entry.module?.Helpers.SyntheticEvents.SyntheticEventsManager.reset();entry.parts=[];entry.trace=null;delete globalThis[symbol];};
  if(action==='release'){dispose();return true;}
  if(action==='append'){
    entry.bytes+=new TextEncoder().encode(argument).byteLength;if(entry.bytes>33554432)fail('PERFORMANCE_TRACE_BOUNDS');entry.parts.push(argument);return true;
  }
  if(action==='finish'){
    try{
      const file=JSON.parse(entry.parts.join(''));if(!Array.isArray(file.traceEvents)||file.traceEvents.length>250000)fail('PERFORMANCE_TRACE_BOUNDS');entry.parts=[];
      const module=await import('devtools://devtools/bundled/models/trace/trace.js');entry.module=module;
      entry.model=module.TraceModel.Model.createWithAllHandlers();await entry.model.parse(file.traceEvents,{isCPUProfile:false,isFreshRecording:false});entry.trace=entry.model.parsedTrace();
      if(!entry.trace?.data)fail('PERFORMANCE_MODEL_FAILED');
    }catch(error){dispose();if(error.message==='No frames found in trace data')fail('PERFORMANCE_TRACE_METADATA_UNAVAILABLE');throw error;}
  }
  const trace=entry.trace;if(!trace)fail('PERFORMANCE_MODEL_PENDING');
  const sets=trace.insights??new Map();
  if(action==='finish'||argument.kind==='summary')return bounded({scope:'owned-browser-process',contextIsSelectionOnly:true,eventCount:trace.traceEvents.length,bounds:trace.data.Meta.traceBounds,mainFrameUrl:redact(trace.data.Meta.mainFrameURL),insightSets:[...sets].map(([insightSetId,set])=>({insightSetId,url:redact(set.url),frameId:set.frameId,bounds:set.bounds,navigationObserved:!!set.navigation,navigationTimestamp:set.navigation?.ts??null,insights:Object.keys(set.model),failedInsights:Object.keys(set.modelErrors)}))});
  const q=argument,offset=q.offset||0,limit=q.limit||50;
  if(q.kind==='timings'){
    const items=[];for(const [frameId,navigations]of trace.data.PageLoadMetrics.metricScoresByFrameId){for(const [navigation,metrics]of navigations){for(const [metricName,score]of metrics){items.push({frameId,navigationTimestamp:navigation.ts,navigationId:navigation.args?.data?.navigationId??null,metricName,value:score.timing,unit:metricName==='CLS'?'score':'microseconds',classification:score.classification,estimated:score.estimated??false});}}}
    return bounded({total:items.length,offset,hasMore:items.length>offset+limit,items:items.slice(offset,offset+limit),scope:'observed-trace-window'});
  }
  const set=sets.get(q.insightSetId);if(!set)fail('UNKNOWN_INSIGHT_SET');
  if(q.kind==='vitals'){
    const lcp=set.model.LCPBreakdown;const inp=set.model.INPBreakdown;const cls=set.model.CLSCulprits;
    const interaction=inp?.highPercentileInteractionEvent;
    const lcpValue=lcp?.lcpEvent&&Number.isFinite(lcp.lcpMs)?lcp.lcpMs:null;
    const clsValue=cls?entry.module.Insights.Common.getCLS(set).value:null;
    return bounded({insightSetId:q.insightSetId,frameId:set.frameId,bounds:set.bounds,scope:'observed-trace-window',fieldData:false,pageLifetime:false,LCP:{available:lcpValue!==null,value:lcpValue,unit:'milliseconds'},INP:{available:!!interaction,value:interaction?interaction.dur/1000:null,unit:'milliseconds',selection:'native-high-percentile-interaction'},CLS:{available:clsValue!==null,value:clsValue,unit:'score'},longestInteraction:inp?.longestInteractionEvent?{durationMs:inp.longestInteractionEvent.dur/1000}:null});
  }
  if(q.kind==='insights'){
    if(q.insight){if(!Object.hasOwn(set.model,q.insight))fail('UNKNOWN_PERFORMANCE_INSIGHT');const insight=set.model[q.insight];return bounded({insightSetId:q.insightSetId,insight,scope:'observed-trace-window'});}
    const items=Object.entries(set.model).map(([name,insight])=>({name,title:insight.title,state:insight.state,category:insight.category,metricSavings:insight.metricSavings,warnings:insight.warnings,wastedBytes:insight.wastedBytes}));
    return bounded({insightSetId:q.insightSetId,total:items.length,offset,hasMore:items.length>offset+limit,items:items.slice(offset,offset+limit),failedInsights:Object.keys(set.modelErrors)});
  }
  fail('PERFORMANCE_QUERY_INVALID');
}
