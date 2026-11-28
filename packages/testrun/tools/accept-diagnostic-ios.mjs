// Explicit Acme simulator acceptance. No user's device, default set or manager.
import assert from 'node:assert/strict';
import {execFile, spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {createHash, randomUUID} from 'node:crypto';
import {appendFile, link, mkdir, readFile, readdir, realpath, writeFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {agentWorkflow} from './diagnostic-agent-workflow.mjs';

const [selectedCLI, app, root, runtime, selectedCase='all']=process.argv.slice(2);
assert.equal(process.platform,'darwin');
for(const path of [selectedCLI,app,root]) assert.ok(path&&isAbsolute(path),'Select absolute CLI, built Acme app and new private evidence root');
assert.ok(/^com\.apple\.CoreSimulator\.SimRuntime\.iOS-[\d-]+$/.test(runtime??''),'Select an installed iOS runtime identifier');
assert.ok(['all','ui','trace','faults','functional','sample-probe'].includes(selectedCase));
await mkdir(root,{mode:0o700});
const reportPath=join(root,'acceptance.json'); await writeFile(reportPath,'{"state":"incomplete"}\n',{flag:'wx',mode:0o600});
const cli=join(root,'testrun'); await link(selectedCLI,cli);
const bundle='com.example.acme.diagnostics', set=join(root,'simulators'); await mkdir(set,{mode:0o700});
const sha=bytes=>createHash('sha256').update(bytes).digest('hex'), execute=promisify(execFile);
const environment={...process.env,TESTRUN_DATA_ROOT:join(root,'state'),TESTRUN_SIMULATOR_DEVICE_SET:set};
for(const key of ['TESTRUN_TASK_ID','CODEX_THREAD_ID','TESTRUN_XCRUN','TESTRUN_APPLE_PS','TESTRUN_CODESIGN','TESTRUN_ADB']) delete environment[key];
const report={schemaVersion:1,state:'running',scope:{host:'macOS',architecture:process.arch,node:process.version,runtime,deviceType:'com.apple.CoreSimulator.SimDeviceType.iPhone-16',configuration:'Debug simulator; no physical/release claim',selectedCase,workBudgetMs:900000},identity:{cliSha256:sha(await readFile(cli)),appSha256:sha(await readFile(join(app,'AcmeDiagnostics'))),harnessSha256:sha(await readFile(new URL(import.meta.url)))},cases:[],collectors:[],cleanup:{},gaps:['Simulator process and stack costs belong to its host process, not physical iOS','Logical buffer counts are not a heap census or proof of GC/RSS release','No global crash-folder discovery, automatic IPS transfer, watchdog/Jetsam, independent symbolication or causal overhead certification']};
let uuid,target,cleanup=false,connected=false,logBytes=0;
const investigations=[], consoles=new Set(), deadline=Date.now()+900000;
async function run(program,args,{timeout=40000,env=environment,allowFailure=false}={}) {
  if(!cleanup) assert.ok(Date.now()<deadline,'Acceptance work budget');
  const began=performance.now(); let stdout='',stderr='',exitCode=0,failure;
  try {({stdout,stderr}=await execute(program,args,{cwd:root,env,timeout,maxBuffer:2097152}));}
  catch(error){stdout=error.stdout??'';stderr=error.stderr??'';exitCode=error.code??'unknown';failure={code:error.code,killed:error.killed,signal:error.signal,message:error.message};if(!allowFailure){await record({program,args,exitCode,stdout,stderr,failure,wallMs:performance.now()-began});throw error;}}
  const wallMs=performance.now()-began;await record({program,args,exitCode,stdout,stderr,failure,wallMs});return {stdout,stderr,exitCode,wallMs,failure};
}
async function record(value){const line=JSON.stringify(value)+'\n';logBytes+=Buffer.byteLength(line);assert.ok(logBytes<=33554432,'Private operation log bound');await appendFile(join(root,'operations.jsonl'),line,{mode:0o600});}
async function call(args,options){const value=await run(cli,['--json',...args],options);return JSON.parse(value.stdout);}
async function sim(args,options){return run('/usr/bin/xcrun',['simctl','--set',set,...args],options);}
async function consoleApp(name){
  const args=['simctl','--set',set,'launch','--console-pty',uuid,bundle];
  const child=spawn('/usr/bin/xcrun',args,{cwd:root,env:environment,stdio:['pipe','pipe','pipe']});
  child.stdin.on('error',()=>{}); // No input; keep the console's input transport open until exit.
  let stdout=Buffer.alloc(0),stderr=Buffer.alloc(0),overflow=false,closed=false;
  for(const [stream,kind] of [[child.stdout,'stdout'],[child.stderr,'stderr']])stream.on('data',chunk=>{const current=kind==='stdout'?stdout:stderr;if(current.length+chunk.length>524288)overflow=true;const bounded=Buffer.concat([current,chunk.subarray(0,Math.max(0,524288-current.length))]);if(kind==='stdout')stdout=bounded;else stderr=bounded;});
  const exited=new Promise(resolve=>{child.once('error',error=>{closed=true;resolve({error:String(error)});});child.once('close',(code,signal)=>{closed=true;resolve({code,signal});});});
  const owner={child,exited,get closed(){return closed;},async finish(){let timer;try{const result=await Promise.race([exited,new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('Owned simctl console exit deadline')),10000);})]);await writeFile(join(root,`${name}.stdout`),stdout,{flag:'wx',mode:0o600});await writeFile(join(root,`${name}.stderr`),stderr,{flag:'wx',mode:0o600});assert.ok(!overflow,'Native console output bound');assert.ok(!result.error,JSON.stringify(result));consoles.delete(owner);return result;}finally{clearTimeout(timer);}}};
  consoles.add(owner);await record({program:'/usr/bin/xcrun',args,pid:child.pid,phase:'owned_console_started'});return owner;
}
async function act(action){return call(['act','--target',target,JSON.stringify(action)]);}
async function match(identifier){const value=await call(['ui','find','--target',target,'--identifier',identifier,'--allow-partial']);assert.ok(Array.isArray(value.data?.matches),JSON.stringify(value));assert.ok(value.data.matches.length<=1,`Unique Acme identifier: ${identifier}`);return value.data.matches[0];}
async function visible(identifier){
  for(let n=0;n<4;n++){
    const node=await match(identifier);if(node?.center)return node;
    const down=identifier.startsWith('acme-')||node?.frame?.y<0;
    await act({action:'swipe',from:{x:0.7,y:down?0.25:0.8},to:{x:0.7,y:down?0.8:0.25},durationMs:300});
  }
  throw Error(`Acme control not visible: ${identifier}`);
}
async function tap(identifier){await visible(identifier);return act({action:'tap',element:{query:{identifier}},allowPartial:true});}
async function switchTo(identifier,value){
  const node=await visible(identifier), observed=String(node.value).toLowerCase();
  assert.ok(['0','1','on','off','true','false'].includes(observed),JSON.stringify(node));
  if(['1','on','true'].includes(observed)!==value)await tap(identifier);
  const after=await match(identifier);assert.equal(['1','on','true'].includes(String(after.value).toLowerCase()),value);
}
async function state(expected){const node=await match('acme-result');assert.ok(node, 'Acme result accessible');const text=node.label;assert.equal(text,expected,JSON.stringify(node));return text;}
async function settle(id,analysis){const until=Date.now()+25000;while(Date.now()<until){const value=await call(analysis?['diagnostics','analysis-status',id,analysis]:['diagnostics','status',id]);if(value.settled)return value;await delay(50);}throw Error('Diagnostic settlement deadline');}
async function open(targetId,name){const value=await call(['diagnostics','open','--submission-id',name,'--question','Verify owned Acme iOS simulator','--binding',JSON.stringify({targetId,appId:bundle})]);investigations.push(value.investigationId);return value.investigationId;}
async function collect(id,name,source){
  const started=await call(['diagnostics','start',id,'--submission-id',name,'--source',JSON.stringify(source),'--limits',JSON.stringify({durationMs:20000,maxBytes:8388608,maxEvents:4096})]);
  const cid=started.collectors.at(-1).collectorId, settled=await settle(id), collector=settled.collectors.find(value=>value.collectorId===cid);
  const reason={ios:'apple_sampled_window',native_trace:'native_trace_window'}[source.kind];
  assert.equal(collector.phase,reason?'partial':'complete',JSON.stringify(collector));assert.equal(collector.reason,reason??'source_complete');assert.ok(collector.bytes>0&&collector.sha256?.length===64);
  report.collectors.push({name,id,cid,phase:collector.phase,reason:collector.reason,sha256:collector.sha256,bytes:collector.bytes,sourceReport:collector.sourceReport});
  return {cid,collector};
}
async function analyze(id,cid,filters,metadata){const args=['diagnostics','analyze',id,cid,'--submission-id',`parse-${cid}`];if(metadata)args.push('--metadata',JSON.stringify(metadata));const value=await call(args), status=await settle(id,value.analysisId);assert.equal(status.phase,'complete',JSON.stringify(status));return call(['diagnostics','query',id,value.analysisId,'--filters',JSON.stringify(filters)]);}
async function raw(id,cid,bytes){let text='',cursor;for(let offset=0;offset<bytes;){const part=await call(['diagnostics','read',id,cid,'--limit','65536',...(cursor?['--cursor',cursor]:[])]);const buffer=Buffer.from(part.dataBase64,'base64');assert.ok(buffer.length);offset+=buffer.length;text+=buffer.toString();cursor=part.nextCursor;assert.ok(cursor||offset===bytes);}return text;}
async function ios(id,name,logs=true){
  const copy=await collect(id,name,{kind:'ios',targetId:`simulator:${uuid}`,appId:bundle,deviceSet:set,logSeconds:10,collectLogs:logs,systemContext:false});
  const text=await raw(id,copy.cid,copy.collector.bytes), packets=text.trim().split('\n').map(line=>JSON.parse(line));
  const installed=packets.find(value=>value.kind==='installed').data.identity;
  assert.equal(installed.appId,bundle);assert.ok(installed.executable.startsWith(set+'/'));
  assert.equal(sha(await readFile(installed.executable)),report.identity.appSha256);
  const processes=packets.find(value=>value.kind==='processes').data.processes;assert.equal(processes.length,1,JSON.stringify(packets));assert.ok(processes[0].generation);
  const queried=await analyze(id,copy.cid,{kind:'process',limit:20});assert.ok(queried.rows.length>0);
  return {...copy,packets,process:processes[0],installed,text};
}
async function launch(){await act({action:'app_launch',appId:bundle,readiness:{element:{identifier:'acme-result'},timeoutMs:20000}});await state('Acme ready');}
async function sample(id,pid,name,generation){const copy=await collect(id,name,{kind:'native_trace',backend:'stack_sample',mode:'capture',targetId:'host:macos',appId:bundle,processId:pid,allowAttach:true,durationMs:1000});const rows=await analyze(id,copy.cid,{kind:'stack',limit:100});assert.ok(rows.rows.length>0);assert.equal(copy.collector.sourceReport.cleanup,'owned_host_files_removed');const identity=copy.collector.sourceReport.identity;assert.equal(identity.pid,pid);assert.equal(String(identity.kernelBirth),generation.split(':').at(-1));assert.deepEqual(identity.kernelBirth,identity.after.kernelBirth);assert.equal(identity.executable,identity.after.executable);const executable=await realpath(identity.executable);assert.ok(executable.startsWith((await realpath(set))+'/'));assert.equal(sha(await readFile(executable)),report.identity.appSha256);return {copy,rows};}
async function nativeProbe(diagnostic){
  const before=await ios(diagnostic,'before-probe',false), path=join(root,'sample.native');
  const began=performance.now(), observed=await run('/usr/bin/sample',[String(before.process.pid),'1.000','10','-file',path],{timeout:20000,allowFailure:true});
  const after=await ios(diagnostic,'after-probe',false);assert.equal(before.process.generation,after.process.generation);
  const bytes=await readFile(path).catch(error=>error.code==='ENOENT'?Buffer.alloc(0):Promise.reject(error));assert.ok(bytes.length<=4194304);
  report.cases.push({name:'diagnostic-only-native-sampler-probe',pid:before.process.pid,generation:before.process.generation,exitCode:observed.exitCode,failure:observed.failure,nativeWallMs:observed.wallMs,probeWithContinuityWallMs:performance.now()-began,bytes:bytes.length,sha256:sha(bytes),meaning:'One-second direct native producer with a separate 20-second diagnostic deadline; not a product capture pass'});
  if(observed.exitCode===0&&bytes.length){const native=await open('host:macos','probe-import'), copy=await collect(native,'actual-probe-import',{kind:'native_trace',backend:'stack_sample',mode:'import',targetId:'host:macos',appId:bundle,path});const rows=await analyze(native,copy.cid,{kind:'stack',limit:100});assert.ok(rows.rows.length);report.cases.push({name:'actual-producer-probe-import',rows:rows.rows.length});}
}
try {
  report.identity.appInfo=JSON.parse((await run('/usr/bin/plutil',['-convert','json','-o','-',join(app,'Info.plist')])).stdout);
  const inventory=JSON.parse((await run('/usr/bin/xcrun',['simctl','list','runtimes','--json'])).stdout);
  const selected=inventory.runtimes.find(value=>value.identifier===runtime&&value.isAvailable);assert.ok(selected,'Selected runtime is already installed');report.identity.runtime=selected;
  report.identity.capabilities=await call(['diagnostics','capabilities']);for(const kind of ['ios','native_trace'])assert.ok(report.identity.capabilities.sources.some(value=>value.kind===kind),`Selected CLI lacks ${kind}`);
  uuid=(await sim(['create',`Testrun-QA-Diagnostics-${randomUUID()}`,report.scope.deviceType,runtime])).stdout.trim();assert.match(uuid,/^[A-Fa-f0-9-]{36}$/);report.identity.simulatorUUID=uuid;
  const listing=await call(['target','list','--kind','simulator']);const selectedTarget=listing.targets.find(value=>value.targetId.endsWith(':'+uuid));assert.ok(selectedTarget);assert.equal(selectedTarget.deviceSetPath,set);target=selectedTarget.targetId;report.identity.target=selectedTarget;
  connected=true;const connection=await call(['connect','--target',target,'--boot'],{timeout:180000});assert.equal(connection.state,'ready',JSON.stringify(connection));
  await call(['app','install','--target',target,app]);await launch();
  if(selectedCase==='sample-probe'){
    const diagnostic=await open(`simulator:${uuid}`,'sample-probe');await nativeProbe(diagnostic);
    report.state='diagnostic_only';
  }else{
  await call(['ui','snapshot','--target',target,'--allow-partial']);
  await tap('Retain one MiB');const defective=Number(await state('1048576'));
  await tap('Native retained bytes');await state('1048576');await tap('Reset');await state('Acme reset');
  await switchTo('acme-corrected',true);await tap('Retain one MiB');const corrected=Number(await state('0'));await tap('Native retained bytes');await state('0');
  for(const [control,result] of [['Swift crash','ready'],['Objective-C exception','ready'],['C++ crash','ready'],['Main-thread hang (6 seconds)','hang requested'],['Background crash','background requested']]){await switchTo('acme-armed',true);await tap(control);await state(result);}
  report.cases.push({name:'actual-ui-retention-reset-corrected-native-controls',defective,corrected});
  await switchTo('acme-corrected',false);await tap('Retain one MiB');
  const diagnostic=await open(`simulator:${uuid}`,'ios');let baseline,logPacket,actionLog;
  for(let attempt=1;attempt<=3;attempt++){
    baseline=await ios(diagnostic,`installed-process-logs-${attempt}`);logPacket=baseline.packets.find(value=>value.kind==='logs');
    actionLog=logPacket.data.entries?.find(entry=>entry.pid===baseline.process.pid&&/Acme (action|result)/.test(entry.message));
    if(actionLog)break;if(attempt<3)await delay(500);
  }
  assert.equal(logPacket.data.status,'sampled',JSON.stringify(logPacket));assert.ok(actionLog,'Actual persisted Acme log from the observed app PID');
  report.cases.push({name:'real-simulator-installed-process-logs',pid:baseline.process.pid,generation:baseline.process.generation,logs:logPacket.data.entries.length});
  const measured={};let primary;
  for(const [mode,value] of [['defective',defective],['corrected',corrected]]){
    const path=join(root,`${mode}.json`), definition='Acme UIKit result label after Swift buffer retention; logical owned Data bytes, not a heap census';
    await writeFile(path,JSON.stringify({schemaVersion:1,records:[{kind:'metric',name:'Acme retained buffers',value,unit:'bytes',definition,data:{observedUILabel:String(value),mode}}]}),{flag:'wx',mode:0o600});
    const copy=await collect(diagnostic,mode,{kind:'import',path,format:'testrun-events-v1'}), result=await analyze(diagnostic,copy.cid,{view:'assertions',assertions:{checks:[{id:'released',domain:'performance',name:'Acme retained buffers',predicate:{type:'metric',unit:'bytes',definition,max:0,aggregation:'max'}}]}},{observationComplete:true});
    assert.equal(result.outcome,mode==='defective'?'assertion_failed':'passed');report.cases.push({name:`same-condition-${mode}`,outcome:result.outcome});measured[mode]=result.analysisId;
  }
  if(['all','trace'].includes(selectedCase)){
    const native=await open('host:macos','host-process'), ordinary=await sample(native,baseline.process.pid,'ordinary-sample',baseline.process.generation);
    await switchTo('acme-armed',true);await tap('Main-thread hang (6 seconds)');
    const hung=await sample(native,baseline.process.pid,'injected-hang-sample',baseline.process.generation);assert.match(JSON.stringify(hung.rows),/sleep|nanosleep/);
    await delay(6500);await tap('Reset');await state('Acme reset');
    report.cases.push({name:'native-host-stack-sample-hang-recovery',ordinaryRows:ordinary.rows.rows.length,hangRows:hung.rows.rows.length,meaning:'Exact simulator app host PID; producer frames, not physical-device CPU or causal overhead'});
  }
  await act({action:'app_terminate',appId:bundle});await launch();
  const relaunched=await ios(diagnostic,'relaunched',false);assert.notEqual(relaunched.process.generation,baseline.process.generation);report.cases.push({name:'relaunch-native-generation',before:baseline.process,after:relaunched.process});
  if(selectedCase==='functional')await nativeProbe(diagnostic);
  if(['all','faults','functional'].includes(selectedCase)){
    for(const [name,control] of [['swift','Swift crash'],['objc','Objective-C exception'],['cpp','C++ crash'],['background','Background crash']]){
      await act({action:'app_terminate',appId:bundle});
      const consolePath=join(root,`${name}.console`), console=await consoleApp(name);
      await call(['wait','--target',target,JSON.stringify([{kind:'exists',query:{identifier:'acme-result'}}]),'--timeout-ms','20000','--allow-partial']);
      await state('Acme ready');const before=await ios(diagnostic,`${name}-before`,false), pid=before.process.pid;assert.ok(Number.isSafeInteger(pid)&&pid>0);
      await switchTo('acme-armed',true);await tap(control);
      const until=Date.now()+15000;let absent=false;
      while(Date.now()<until){const observed=await run('/bin/ps',['-p',String(pid),'-o','pid=,lstart=,comm='],{allowFailure:true});if(observed.exitCode===1&&!observed.stdout.trim()){absent=true;break;}await delay(100);}assert.ok(absent,'Owned fatal-trigger process ended');
      const consoleExit=await console.finish(), output=Buffer.concat([await readFile(join(root,`${name}.stdout`)),await readFile(join(root,`${name}.stderr`))]);assert.ok(output.length>0&&output.length<=1048576);assert.ok(output.toString().includes(`Acme trigger ${control} corrected=false`),'Exact literal Acme native trigger breadcrumb');
      await writeFile(consolePath,output,{flag:'wx',mode:0o600});
      const path=join(root,`${name}-failure.json`);await writeFile(path,JSON.stringify({schemaVersion:1,records:[{kind:'failure',name:'Acme fatal-trigger process ended',data:{pid,generation:before.process.generation,control,observation:'actual simulator process absent after armed native trigger; signal/OS report not inferred'}}]}),{flag:'wx',mode:0o600});
      const copy=await collect(diagnostic,`${name}-failure`,{kind:'import',path,format:'testrun-events-v1'}), failures=await analyze(diagnostic,copy.cid,{kind:'failure',limit:10});assert.equal(failures.rows.length,1);primary??=failures.analysisId;
      await collect(diagnostic,`${name}-console`,{kind:'import',path:consolePath,format:'utf8'});report.cases.push({name:`actual-${name}-fatal-process-end`,pid,priorGeneration:before.process.generation,consoleExit,consoleSha256:sha(output),meaning:'Native producer PTY console plus process disappearance; no inferred signal or automatic IPS discovery'});
      // Explicit new launch after a known ended process, never replay the tap.
      await launch();
    }
  }
  if(selectedCase==='functional'){
    await switchTo('acme-armed',true);const began=performance.now();const action=await tap('Main-thread hang (6 seconds)');await delay(6500);await tap('Reset');await state('Acme reset');
    const recovered=await ios(diagnostic,'hang-recovered',false);report.cases.push({name:'injected-main-thread-sleep-and-responsive-reset',pid:recovered.process.pid,generation:recovered.process.generation,actionElapsedMs:action.elapsedMs,journeyWallMs:performance.now()-began,meaning:'Actual armed sleep control and responsive reset; native stack capture and causal latency remain separate'});
    report.gaps.push('Functional selection deliberately excludes a successful native_trace capture claim; the bounded direct tool probe is diagnostic-only');
  }
  if(primary){
    const agent=await agentWorkflow({cli,cwd:root,env:environment,record,root,investigation:diagnostic,primary,baseline:measured.defective,corrected:measured.corrected,platform:'ios',preset:'crash'});
    report.cases.push({name:'agent-workflow-over-mcp',...agent});
  }else report.gaps.push('This case selection has no fatal failure analysis; the MCP agent workflow did not run');
  report.state='passed';
  }
} catch(error){report.state='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{
  cleanup=true;const errors=[];
  if(connected){try{report.cleanup.disconnect=await call(['disconnect','--target',target]);assert.equal(report.cleanup.disconnect.closed,true);}catch(error){errors.push(String(error));}}
  for(const id of investigations){try{await call(['diagnostics','close',id]);await settle(id);await call(['diagnostics','remove',id]);}catch(error){errors.push(String(error));}}
  try{report.cleanup.manager=await call(['manager','stop']);}catch(error){errors.push(String(error));}
  // Inspect only this newly created private set even if create's reply was lost.
  // Discovering that result is safe; repeating create after uncertainty is not.
  try{const listing=JSON.parse((await sim(['list','devices','--json'])).stdout);const devices=Object.values(listing.devices).flat();assert.ok(devices.length<=1);for(const owned of devices){assert.ok(owned.name.startsWith('Testrun-QA-Diagnostics-'));if(uuid)assert.equal(owned.udid,uuid);if(owned.state!=='Shutdown')await sim(['shutdown',owned.udid]);await sim(['delete',owned.udid]);}const after=JSON.parse((await sim(['list','devices','--json'])).stdout);report.cleanup.remainingSimulators=Object.values(after.devices).flat().length;assert.equal(report.cleanup.remainingSimulators,0);}catch(error){errors.push(String(error));}
  // Only after owned target shutdown/deletion: console signals are forwarded
  // to its app, so never kill a console while app input is still in flight.
  for(const owner of consoles){try{if(!owner.closed)owner.child.kill('SIGTERM');await owner.finish();}catch(error){errors.push(String(error));}}
  report.cleanup.remainingConsoles=consoles.size;
  async function files(directory){let count=0;for(const entry of await readdir(directory,{withFileTypes:true}).catch(error=>error.code==='ENOENT'?[]:Promise.reject(error)))count+=entry.isDirectory()?await files(join(directory,entry.name)):1;return count;}
  report.cleanup.remainingDiagnosticFiles=await files(join(root,'state/diagnostics'));report.cleanup.errors=errors;
  if(errors.length||report.cleanup.remainingDiagnosticFiles||report.cleanup.remainingConsoles){report.state='failed';process.exitCode=1;}
  await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify({state:report.state,report:reportPath,cases:report.cases.map(value=>value.name),cleanup:{closed:report.cleanup.disconnect?.closed,manager:report.cleanup.manager,remainingSimulators:report.cleanup.remainingSimulators,remainingDiagnosticFiles:report.cleanup.remainingDiagnosticFiles,errors},error:report.error}));
}
