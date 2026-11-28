// Document-owned React inspection through the pinned DevTools renderer interfaces.
function openFramework(){
  const hook=window.__REACT_DEVTOOLS_GLOBAL_HOOK__;
  if(!hook?.rendererInterfaces?.get||typeof hook.sub!=='function')throw Error('React DevTools hook unavailable; prepare before application startup');
  let closed=false,profile=null,completedProfile=null,profileFailure=null;
  const ownershipKey=Symbol.for('testrun.framework.profileOwners'),token=Symbol('framework-registry');
  if(!hook[ownershipKey])Object.defineProperty(hook,ownershipKey,{value:new Map()});
  const owners=hook[ownershipKey];
  if(!(owners instanceof Map))throw Error('Framework profiling owner unavailable');
  const maximum=65536;
  const text=(value,max=1024)=>String(value??'').slice(0,max);
  const bounded=input=>{
    let count=0,bytes=0,truncated=false;const seen=new WeakSet();
    const visit=(value,depth)=>{
      bytes+=32;if(++count>2048||depth>12||bytes>45000){truncated=true;return '[truncated]';}
      if(typeof value==='string'){const result=text(value,2048);bytes+=new TextEncoder().encode(JSON.stringify(result)).length;if(result.length!==value.length)truncated=true;return result;}
      if(value===null||typeof value==='boolean'||typeof value==='number')return value;
      if(typeof value!=='object')return text(typeof value==='function'?'[function]':value);
      if(seen.has(value))return '[reference]';seen.add(value);
      if(Array.isArray(value)){const result=[];for(const item of value){if(result.length>=200||count>=2048||bytes>45000){truncated=true;break;}result.push(visit(item,depth+1));}return result;}
      const result=Object.create(null);let keys=0;
      for(const key of Object.keys(value)){if(++keys>100||count>=2048||bytes>45000){truncated=true;break;}const name=text(key,256);bytes+=new TextEncoder().encode(JSON.stringify(name)).length;const descriptor=Object.getOwnPropertyDescriptor(value,key);result[name]=descriptor&&'value'in descriptor?visit(descriptor.value,depth+1):'[accessor]';}
      return result;
    };
    return {data:visit(input,0),truncated};
  };
  const encode=value=>{const result=JSON.stringify(value);if(new TextEncoder().encode(result).length>maximum)throw Error('Framework result exceeds its bound');return result;};
  const renderers=()=>Array.from(hook.rendererInterfaces.keys()).slice(0,16).map(id=>{
    const renderer=hook.renderers?.get(id);
    return {rendererId:id,package:text(renderer?.rendererPackageName),version:text(renderer?.version),roots:hook.getFiberRoots(id).size};
  });
  const select=id=>{
    const choices=renderers();
    id??=choices.find(item=>item.roots&&!item.package.includes('react-server-dom'))?.rendererId??choices.find(item=>item.roots)?.rendererId;
    const ri=hook.rendererInterfaces.get(id);if(!ri)throw Error('React renderer unavailable');return {id,ri};
  };
  // Decode the pinned DevTools operations protocol rather than inventing fiber IDs.
  const decode=(ops,nodes,boundaries)=>{
    let i=2;if(!Array.isArray(ops)&&!ArrayBuffer.isView(ops))throw Error('Invalid React operations');
    const strings=[null],end=3+ops[2];i=3;
    if(end>ops.length||end>100000)throw Error('React operation table exceeds its bound');
    while(i<end){const length=ops[i++];if(length<0||i+length>end||length>8192)throw Error('Invalid React string table');let value='';for(let n=0;n<length;n++)value+=String.fromCodePoint(ops[i++]);strings.push(text(value));}
    let traversed=0;
    const rects=n=>n===-1?0:n*4;
    while(i<ops.length&&++traversed<=10000){
      const op=ops[i];let skip;
      if(op===1){const id=ops[i+1],type=ops[i+2];if(nodes.size<2000)nodes.set(id,{id,type,parent:type===11?0:ops[i+3],name:type===11?null:strings[ops[i+5]]??null,key:type===11?null:strings[ops[i+6]]??null});skip=type===11?7:8;}
      else if(op===2||op===9){skip=2+ops[i+1];for(let n=0;n<ops[i+1];n++)(op===2?nodes:boundaries).delete(ops[i+2+n]);}
      else if(op===3||op===10)skip=3+ops[i+2];
      else if(op===4||op===7)skip=3;
      else if(op===5)skip=4;
      else if(op===6)skip=1;
      else if(op===8){if(boundaries.size<2000)boundaries.set(ops[i+1],{id:ops[i+1],parent:ops[i+2],name:strings[ops[i+3]]??null,suspended:ops[i+4]===1,environments:[]});skip=6+rects(ops[i+5]);}
      else if(op===11)skip=3+rects(ops[i+2]);
      else if(op===12){let j=i+2;for(let n=0;n<ops[i+1];n++){if(j+5>ops.length)throw Error('Invalid suspense operations');const row=boundaries.get(ops[j]);const length=ops[j+4];if(length<0||length>1000)throw Error('Invalid suspense environments');if(row){row.suspended=ops[j+3]===1;row.environments=Array.from(ops.slice(j+5,j+5+length),id=>strings[id]??null);}j+=5+length;}skip=j-i;}
      else if(op===13)skip=2;
      else throw Error('Unsupported React operations protocol');
      if(!Number.isSafeInteger(skip)||skip<=0||i+skip>ops.length)throw Error('Invalid React operations');i+=skip;
    }
    return i<ops.length||nodes.size>=2000||boundaries.size>=2000;
  };
  const snapshot=async(id,ri)=>{
    const nodes=new Map(),boundaries=new Map();let truncated=false,total=0,failure=null;
    const unsubscribe=hook.sub('operations',ops=>{
      try{
        if(ops[0]!==id)return;
        if(total+ops.length>100000){truncated=true;return;}total+=ops.length;
        truncated=decode(ops,nodes,boundaries)||truncated;
      }catch(error){failure=error;}
    });
    try{ri.flushInitialOperations();await new Promise(resolve=>setTimeout(resolve,50));}finally{unsubscribe();}
    if(closed)throw Error('Framework registry closed');
    if(failure)throw failure;
    return {nodes:Array.from(nodes.values()),boundaries:Array.from(boundaries.values()),truncated};
  };
  const stop=()=>{
    if(!profile)return null;
    const current=profile;
    clearTimeout(current.timer);current.timer=null;
    try{
      if(current.phase!=='stopped'){
        current.ri.stopProfiling();current.phase='stopped';current.elapsedMs=performance.now()-current.started;
      }
      const report={rendererId:current.id,elapsedMs:current.elapsedMs,options:current.options,...bounded(current.ri.getProfilingData())};
      completedProfile=report;profile=null;profileFailure=null;if(owners.get(current.id)===token)owners.delete(current.id);
      return completedProfile;
    }catch(error){profileFailure=text(error?.message??error);throw error;}
  };
  return {
    describe(){if(closed)throw Error('Framework registry closed');return encode({framework:'React',renderers:renderers(),documentBound:true});},
    async query(query){
      if(closed)throw Error('Framework registry closed');
      if(query.kind==='renders_status')return encode({recording:profile?.phase==='start-uncertain'?null:profile!==null&&profile.phase!=='stopped',phase:profile?.phase??(completedProfile?'complete':'idle'),rendererId:profile?.id??completedProfile?.rendererId??null,elapsedMs:profile?(profile.elapsedMs??performance.now()-profile.started):null,options:profile?.options??completedProfile?.options??null,cleanupPending:profileFailure!==null,error:profileFailure});
      if(query.kind==='renders_stop'){
        if(!profile&&!completedProfile)throw Error('React profiling is not active');
        return encode({recording:false,source:'React-DevTools-renderer-profiler',...(profile?stop():completedProfile)});
      }
      const {id,ri}=select(query.rendererId);
      if(query.kind==='tree'||query.kind==='suspense'){
        const data=await snapshot(id,ri),rows=query.kind==='tree'?data.nodes:data.boundaries.filter(item=>item.parent!==0);
        const items=[];let next=query.offset,bytes=0,truncated=data.truncated;
        while(next<rows.length&&next<query.offset+query.limit){
          const row={...rows[next]};
          if(query.kind==='suspense'){
            let inspection=null;
            if(ri.hasElementWithId(row.id)){const result=ri.inspectElement(id,row.id,null,true);if(result?.type==='full-data')inspection=result.value;}
            const suspenders=inspection?.suspendedBy,known=Array.isArray(suspenders)?suspenders:Array.isArray(suspenders?.data)?suspenders.data:[];
            row.dynamic=row.suspended||known.length>0||Boolean(inspection?.unknownSuspenders);
            row.inspection=inspection?{suspendedBy:inspection.suspendedBy,unknownSuspenders:inspection.unknownSuspenders,owners:inspection.owners,stack:inspection.stack}:null;
            if(query.onlyDynamic&&!row.dynamic){next++;continue;}
          }
          const limited=bounded(row),size=new TextEncoder().encode(JSON.stringify(limited.data)).length;
          if(bytes+size>55000){truncated=true;break;}
          truncated=limited.truncated||truncated;items.push(limited.data);bytes+=size;next++;
        }
        return encode({rendererId:id,kind:query.kind,items,hasMore:next<rows.length,nextOffset:next,total:rows.length,truncated,snapshot:false,filterAppliedToPage:query.kind==='suspense'&&query.onlyDynamic});
      }
      if(query.kind==='inspect'){
        if(!ri.hasElementWithId(query.id))throw Error('React component unavailable in this document');
        const result=ri.inspectElement(id,query.id,null,true);
        if(result?.type!=='full-data')throw Error('React component inspection unavailable');
        return encode({rendererId:id,id:query.id,name:text(ri.getDisplayNameForElementID(query.id)),...bounded(result.value)});
      }
      if(query.kind==='renders_start'){
        if(profile)throw Error('React profiling already active');
        if(owners.has(id))throw Error('React renderer profiling belongs to another registry');
        if(typeof ri.startProfiling!=='function'||typeof ri.stopProfiling!=='function'||typeof ri.getProfilingData!=='function')throw Error('React renderer profiling unavailable');
        const options={recordChangeDescriptions:query.recordChangeDescriptions??true,recordTimeline:query.recordTimeline??false};
        completedProfile=null;profileFailure=null;profile={id,ri,started:performance.now(),timer:null,phase:'recording',options};owners.set(id,token);
        try{ri.startProfiling(options.recordChangeDescriptions,options.recordTimeline);}catch(error){profile.phase='start-uncertain';profileFailure=text(error?.message??error);throw error;}
        profile.timer=setTimeout(()=>{try{stop();}catch(error){profileFailure=text(error?.message??error);}},query.durationMs);
        return encode({rendererId:id,recording:true,options,source:'React-DevTools-renderer-profiler'});
      }
      throw Error('Unknown framework query');
    },
    close(){
      if(closed)return true;
      if(profile){
        const current=profile;clearTimeout(current.timer);current.timer=null;
        try{if(current.phase!=='stopped')current.ri.stopProfiling();}
        catch(error){profileFailure=text(error?.message??error);throw error;}
        if(owners.get(current.id)===token)owners.delete(current.id);
        profile=null;profileFailure=null;
      }
      closed=true;return true;
    },
  };
}
