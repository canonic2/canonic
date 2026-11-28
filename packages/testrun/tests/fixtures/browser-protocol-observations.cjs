// Test preload only: observe already-emitted bytes without starting stream reads.
const cp=require('node:child_process'),fs=require('node:fs');
const destination=process.env.TESTRUN_CHILD_OBSERVATIONS,started=performance.now();
const nativeOnly=process.env.TESTRUN_NATIVE_PROTOCOL_ONLY==='1';
const maximum=nativeOnly?2097152:262144;
let records=0;const lines=[];
function record(event,fields={}) {
  if(++records>2048)return;
  lines.push(JSON.stringify({event,elapsedMs:performance.now()-started,...fields}));
}
function summary(message,depth=0) {
  if(!message||typeof message!=='object'||depth>2)return null;
  let nested=message.params?.message;
  if(typeof nested==='string'){try{nested=JSON.parse(nested);}catch{nested=null;}}
  return {id:typeof message.id==='number'||typeof message.id==='string'&&/^-?\d{1,20}$/.test(message.id)?message.id:undefined,method:typeof message.method==='string'?message.method:undefined,
    inputType:['mousemove','mousedown','mouseup'].includes(message.params?.type)?message.params.type:undefined,
    hasError:!!message.error,errorName:typeof message.error?.error?.name==='string'?message.error.error.name:undefined,
    nested:nested?summary(nested,depth+1):undefined};
}
function decoder(lane,delimited,ownerPid) {
  let buffered=Buffer.alloc(0),disabled=false;
  return chunk=>{
    if(disabled)return;
    try {
      const incoming=Buffer.isBuffer(chunk)?chunk:Buffer.from(chunk);
      if(buffered.length+incoming.length>maximum){disabled=true;record('observer-bounds',{lane,maximum});return;}
      buffered=Buffer.concat([buffered,incoming]);
      for(;;){
        let length,offset;
        if(delimited){length=buffered.indexOf(0);offset=0;if(length<0)return;}
        else {if(buffered.length<4)return;length=buffered.readUInt32LE();offset=4;if(length>maximum){disabled=true;record('observer-bounds',{lane,maximum});return;}if(buffered.length<length+4)return;}
        const message=JSON.parse(buffered.subarray(offset,offset+length).toString());
        buffered=buffered.subarray(offset+length+(delimited?1:0));
        record('protocol',{lane,ownerPid,message:summary(message)});
      }
    }catch{disabled=true;record('observer-parse-error',{lane});}
  };
}
function input(stream,observe) {
  const emit=stream.emit;
  stream.emit=function(event,...args){if(event==='data')observe(args[0]);return Reflect.apply(emit,this,[event,...args]);};
}
function output(stream,observe) {
  const write=stream.write;
  stream.write=function(...args){observe(args[0]);return Reflect.apply(write,this,args);};
}
if(!nativeOnly){input(process.stdin,decoder('binding-to-driver',false));output(process.stdout,decoder('driver-to-binding',false));}
const spawn=cp.spawn;
cp.spawn=function(...args){
  const child=Reflect.apply(spawn,this,args);
  record('spawn',{pid:child.pid});
  if(child.stdio[3])output(child.stdio[3],decoder('driver-to-browser',true,child.pid));
  if(child.stdio[4])input(child.stdio[4],decoder('browser-to-driver',true,child.pid));
  child.once('exit',(code,signal)=>record('exit',{pid:child.pid,code,signal}));
  child.once('close',(code,signal)=>record('close',{pid:child.pid,code,signal}));
  return child;
};
process.once('exit',code=>{
  record('driver-exit',{code});
  try {fs.writeFileSync(destination,lines.join('\n')+'\n',{mode:0o600,flag:'wx'});}
  catch { /* Observation failure must not interrupt the observed process. */ }
});
