// Selected Flutter Debug acceptance on one new owned Android emulator or iOS simulator.
// Owns that target, a private manager, the endpoint file for the app's own VM Service
// and, on Android, its adb forward; nothing else.
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';
import {appendFile,link,mkdir,readFile,readdir,rm,stat,writeFile} from 'node:fs/promises';
import {isAbsolute,join} from 'node:path';
import {createServer} from 'node:net';
import {setTimeout as delay} from 'node:timers/promises';
import {promisify} from 'node:util';
import {cleanupDiagnosticAndroid} from './cleanup-diagnostic-android.mjs';
import {cleanupOwnedSimulator} from './cleanup-diagnostic-react-native.mjs';
import {flutterAppId,flutterIosAppId,ownedFlutterApp} from './diagnostic-flutter-app.mjs';

const execute=promisify(execFile),sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const childEnv=extra=>{const env={...process.env,...extra};for(const key of ['TESTRUN_TASK_ID','CODEX_THREAD_ID'])delete env[key];return env;};
// The service announcement each Flutter Debug engine writes to its own process log.
const announcement=/Dart VM [Ss]ervice is listening on http:\/\/127\.0\.0\.1:(\d+)\/([A-Za-z0-9_\-=+]+)\//g;

/** Release one retained run: investigation, endpoint files, Android forward, target and manager. */
export async function cleanupFlutter(root,sdk,javaHome){
  assert.ok(root&&isAbsolute(root),'Select the absolute retained run root');
  const path=join(root,'acceptance.json'),report=JSON.parse(await readFile(path,'utf8'));
  assert.equal(report.scope.framework,'flutter-'+report.scope.platform);assert.equal(sha(await readFile(join(root,'testrun'))),report.identity.cliSha256);
  const ios=report.scope.platform==='ios';
  if(!ios)for(const value of [sdk,javaHome])assert.ok(value&&isAbsolute(value),'Select the Android SDK and JDK used by the retained run');
  const env=ios?childEnv({TESTRUN_DATA_ROOT:join(root,'state'),TESTRUN_SIMULATOR_DEVICE_SET:join(root,'simulators')})
    :childEnv({TESTRUN_DATA_ROOT:join(root,'state'),ANDROID_AVD_HOME:join(root,'avd'),ANDROID_HOME:sdk,JAVA_HOME:javaHome,TESTRUN_ADB:join(sdk,'platform-tools/adb')});
  const call=async args=>JSON.parse((await execute(join(root,'testrun'),['--json',...args],{env,cwd:root,timeout:60000,maxBuffer:4194304})).stdout);
  const id=report.identity.investigationId;
  if(id&&!report.cleanup.investigationRemoved){
    assert.match(id,/^diag-[a-f0-9]{32}$/);await call(['diagnostics','close',id]);const until=Date.now()+45000;let settled=false;
    while(Date.now()<until){if((await call(['diagnostics','status',id])).settled){settled=true;break;}await delay(100);}
    assert.ok(settled,'Owned investigation settled before removal');await call(['diagnostics','remove',id]);report.cleanup.investigationRemoved=true;
  }
  const forward=report.identity.vmForward;
  if(!ios&&forward&&!report.cleanup.forwardRemoved){
    assert.match(report.identity.target??'',/^android:emulator-\d+$/);assert.match(String(forward.hostPort),/^\d+$/);
    // The forward also ends with its emulator; a refused removal only means it is gone.
    try{await execute(join(sdk,'platform-tools/adb'),['-s',report.identity.target.slice(8),'forward','--remove',`tcp:${forward.hostPort}`],{env,timeout:30000});}catch{}
    report.cleanup.forwardRemoved=true;
  }
  for(const name of report.identity.endpointFiles??[]){assert.match(name,/^vm-service-\d+\.uri$/);await rm(join(root,name),{force:true});}
  report.cleanup.endpointFilesRemoved=true;
  await writeFile(path,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  if(ios)await cleanupOwnedSimulator(root);else await cleanupDiagnosticAndroid(root,sdk,javaHome);
}

if(process.argv[2]==='--cleanup'){await cleanupFlutter(...process.argv.slice(3));}
else{
const measuring=process.argv[2]==='--measure',argv=process.argv.slice(measuring?3:2);
const ios=argv[0]==='--ios';
let selectedCLI,appRoot,sdk,javaHome,runtime,root,hostPort;
if(ios){
  [selectedCLI,appRoot,runtime,root]=argv.slice(1);
  assert.equal(process.platform,'darwin','iOS simulator acceptance requires macOS');
  for(const path of [selectedCLI,appRoot,root])assert.ok(path&&isAbsolute(path),'Select absolute CLI, prepared Flutter root and new evidence root');
  assert.match(runtime??'',/^com\.apple\.CoreSimulator\.SimRuntime\.iOS-[\d-]+$/,'Select an installed iOS runtime identifier');
}else{
  [selectedCLI,appRoot,sdk,javaHome,root,hostPort='8098']=argv;
  for(const path of [selectedCLI,appRoot,sdk,javaHome,root])assert.ok(path&&isAbsolute(path),'Select absolute CLI, prepared Flutter root, Android SDK, JDK and new evidence root');
  assert.match(hostPort,/^\d+$/);assert.ok(+hostPort>1024&&+hostPort<65536);
}
const {project}=await ownedFlutterApp(appRoot),bundle=ios?flutterIosAppId:flutterAppId;
await mkdir(root,{mode:0o700});const cli=join(root,'testrun');await link(selectedCLI,cli);
const reportPath=join(root,'acceptance.json');
const env=childEnv({TESTRUN_DATA_ROOT:join(root,'state'),CI:'1'});
const report={schemaVersion:1,state:'running',scope:{platform:ios?'ios':'android',framework:ios?'flutter-ios':'flutter-android',host:process.platform,arch:process.arch,node:process.version,app:'Flutter generated Debug (JIT) with fixture-owned breadcrumb channel',workBudgetMs:1800000},
  identity:{appRoot,cliSha256:sha(await readFile(cli)),harnessSha256:sha(await readFile(new URL(import.meta.url))),endpointFiles:[]},cases:[],collectors:[],cleanup:{},
  gaps:['Selected Flutter Debug only; physical, profile/release and other Flutter versions unverified','VM-reported PID is producer metadata, compared with the native snapshot but not authenticated','Dart heap/class reads cover the isolate group; no leak, GC or native-memory conclusion','Private evidence can contain the debug VM Service token; it expires with its process']};
const deadline=Date.now()+1800000;let logBytes=0,target,diagnosticTarget,investigation,connected=false,cleaning=false,endpointFile;
async function save(){await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});}
async function record(value){const text=JSON.stringify(value)+'\n';logBytes+=Buffer.byteLength(text);assert.ok(logBytes<67108864,'Private operation-log bound');await appendFile(join(root,'operations.jsonl'),text,{mode:0o600});}
async function run(program,args,{timeout=45000,allowFailure=false,input,quiet=false}={}){
  if(!cleaning)assert.ok(Date.now()<deadline,'Acceptance work budget');
  const started=Date.now();let stdout='',stderr='',code=0,error;
  try{const child=execute(program,args,{cwd:root,env,timeout,maxBuffer:8388608});if(input!==undefined)child.child.stdin.end(input);({stdout,stderr}=await child);}
  catch(e){stdout=e.stdout??'';stderr=e.stderr??'';code=e.code??'unknown';error=e;}
  // Quiet reads can carry the VM Service token; only their outcome is logged.
  await record(quiet?{program,args,code,stdoutBytes:stdout.length,durationMs:Date.now()-started}:{program,args,stdout,stderr,code,durationMs:Date.now()-started});
  if(error&&!allowFailure)throw error;return {stdout,stderr,code};
}
async function call(args,options){return JSON.parse((await run(cli,['--json',...args],options)).stdout);}
async function act(action){return call(['act','--target',target,JSON.stringify(action)]);}
// A launch whose every step completed but whose follow-up screenshot failed has a known
// outcome: record the observation error and continue with read-only readiness checks.
async function launch(){
  const action={action:'app_launch',appId:bundle,...(ios?{readiness:{timeoutMs:30000}}:{})},result=await run(cli,['--json','act','--target',target,JSON.stringify(action)],{allowFailure:true});
  if(result.code===0)return;
  const error=JSON.parse(result.stderr).error,steps=error.details?.result?.lastAction?.steps??[];
  assert.ok(error.code==='BATCH_FAILED'&&steps.length===1&&steps[0].ok&&steps[0].data?.state==='completed'&&steps[0].observationError,result.stderr);
  (report.identity.launchObservationErrors??=[]).push(steps[0].observationError);
}
async function find(query){const args=query.identifier?['--identifier',query.identifier]:['--label',query.label];const result=await call(['ui','find','--target',target,...args,'--allow-partial']);return result.data.matches;}
async function node(id){const nodes=await find({identifier:id});assert.equal(nodes.length,1,JSON.stringify(nodes));return nodes[0];}
// Resolve the button with a polling read first: a rebuilt Flutter view can briefly
// expose no semantics, and an element tap would then refuse without sending input.
async function tap(label){await act({action:'tap',point:await buttonPoint(label)});}
// A Flutter view can briefly expose no semantics nodes; this read-only lookup polls.
const usable=point=>point&&[point.x,point.y].every(v=>Number.isFinite(v)&&v>=0&&v<=1);
async function buttonPoint(label){const end=Date.now()+30000;let nodes;do{nodes=(await find({label})).filter(n=>n.role==='button'&&n.label.toLowerCase()===label.toLowerCase());if(nodes.length&&nodes.every(n=>usable(n.center)))break;(report.identity.emptyButtonReads??=0,report.identity.emptyButtonReads++);await delay(500);}while(Date.now()<end);assert.equal(nodes.length,1,JSON.stringify(nodes));const point=nodes[0].center;assert.ok(point&&[point.x,point.y].every(v=>Number.isFinite(v)&&v>=0&&v<=1));return point;}
// Android reports a checked state; iOS exposes the switch value.
function checked(current){if(typeof current.states?.checked==='boolean')return current.states.checked;const value=String(current.value).toLowerCase();assert.ok(['0','1','on','off','true','false'].includes(value),JSON.stringify(current));return ['1','on','true'].includes(value);}
async function settledNode(id){const end=Date.now()+30000;let nodes;do{nodes=await poll(id);if(nodes.length&&nodes.every(n=>usable(n.center)))break;await delay(500);}while(Date.now()<end);assert.equal(nodes.length,1,JSON.stringify(nodes));return nodes[0];}
async function switchTo(id,expected){const current=await settledNode(id);if(checked(current)!==expected)await act({action:'tap',point:current.center});const end=Date.now()+30000;let state;do{state=checked(await settledNode(id));if(state===expected)return;await delay(200);}while(Date.now()<end);assert.equal(state,expected);}
// Readiness polls are read-only: a failed or timed-out accessibility read is counted and
// polled again within the same deadline. Input is never repeated.
async function poll(id){try{return await find({identifier:id});}catch(error){let code;try{code=JSON.parse(error.stderr).error.code;}catch{}if(!['PLATFORM_FAILED','TIMEOUT','UI_UNAVAILABLE'].includes(code))throw error;(report.identity.readinessReadFailures??={})[code]=(report.identity.readinessReadFailures[code]??0)+1;return [];}}
// A freshly booted owned emulator can show "System UI isn't responding" over the app. While
// an expected node is absent, choose that system dialog's Wait once per appearance and record it.
async function systemDialog(){if(ios)return;const waits=await find({identifier:'android:id/aerr_wait'}).catch(()=>[]);if(waits.length!==1||!usable(waits[0].center))return;const title=(await find({identifier:'android:id/alertTitle'}).catch(()=>[]))[0]?.label;(report.identity.systemDialogs??=[]).push({title,observedAt:new Date().toISOString()});await act({action:'tap',point:waits[0].center});}
async function expectText(id,expected,timeoutMs=30000){const end=Date.now()+timeoutMs;let last;do{const nodes=await poll(id);assert.ok(nodes.length<=1,JSON.stringify(nodes));last=nodes[0]?.label;if(last===expected)return;if(!nodes.length)await systemDialog();await delay(200);}while(Date.now()<end);assert.equal(last,expected);}
async function settle(id,analysis){const end=Date.now()+45000;do{const value=await call(analysis?['diagnostics','analysis-status',id,analysis]:['diagnostics','status',id]);if(value.settled)return value;await delay(100);}while(Date.now()<end);throw Error('Diagnostic settlement deadline');}
async function start(label,source){const value=await call(['diagnostics','start',investigation,'--submission-id',label,'--source',JSON.stringify(source),'--limits',JSON.stringify({durationMs:30000,maxBytes:8388608,maxEvents:4096})]);return value.collectors.at(-1).collectorId;}
// Dart observation starts at collector stage 5, after before-snapshots and object reads.
async function activeWindow(label,source){
  const cid=await start(label,source),until=Date.now()+20000;
  do{
    const status=await call(['diagnostics','status',investigation]),collector=status.collectors.find(c=>c.collectorId===cid);
    // Retain an early-settled capture's packets before failing, so its fence is inspectable.
    if(collector.settled){if(collector.bytes>0)await finish(`${label}-early`,cid).catch(()=>{});throw Error(`Capture settled before fixture input: ${collector.reason}; ${JSON.stringify(collector.sourceReport?.sourceLoss)}`);}
    if(collector.sourceReport?.stage===5)return cid;await delay(100);
  }while(Date.now()<until);
  throw Error('Dart capture did not enter its observation window before input');
}
async function finish(label,cid){
  const status=await settle(investigation),collector=status.collectors.find(c=>c.collectorId===cid);report.collectors.push({label,...collector});
  assert.ok(['partial','complete'].includes(collector.phase),JSON.stringify(collector));assert.ok(collector.bytes>0);
  let bytes=Buffer.alloc(0),cursor;while(bytes.length<collector.bytes){const part=await call(['diagnostics','read',investigation,cid,'--limit','65536',...(cursor?['--cursor',cursor]:[])]);const chunk=Buffer.from(part.dataBase64,'base64');assert.ok(chunk.length);bytes=Buffer.concat([bytes,chunk]);cursor=part.nextCursor;assert.ok(cursor||bytes.length===collector.bytes);}
  assert.equal(sha(bytes),collector.sha256);await writeFile(join(root,`${label}.ndjson`),bytes,{mode:0o600});
  if(collector.sourceKind==='dart'){const summary=collector.sourceReport;assert.equal(summary.generationChanged,false,JSON.stringify(summary));assert.equal(summary.connectionLost,false,JSON.stringify(summary));if(summary.cleanup)assert.equal(summary.cleanup.socketClosed,true);}
  return {cid,collector,packets:bytes.toString().trim().split('\n').map(line=>JSON.parse(line))};
}
async function collect(label,source){return finish(label,await start(label,source));}
async function analyze(copy,filters,metadata){const args=['diagnostics','analyze',investigation,copy.cid,'--submission-id',`parse-${copy.cid}`];if(metadata)args.push('--metadata',JSON.stringify(metadata));const value=await call(args);assert.equal((await settle(investigation,value.analysisId)).phase,'complete');return call(['diagnostics','query',investigation,value.analysisId,'--filters',JSON.stringify(filters)]);}
async function assertion(mode,value){const path=join(root,`${mode}.json`),definition='Observed Acme Flutter result control; logical fixture-owned typed-data bytes';await writeFile(path,JSON.stringify({schemaVersion:1,records:[{kind:'metric',name:'Acme retained buffers',value,unit:'bytes',definition,data:{mode,observedTextView:`retain: ${value}`}}]}),{flag:'wx',mode:0o600});const copy=await collect(mode,{kind:'import',path,format:'testrun-events-v1'});const result=await analyze(copy,{view:'assertions',assertions:{checks:[{id:'released',domain:'performance',name:'Acme retained buffers',predicate:{type:'metric',unit:'bytes',definition,max:0,aggregation:'max'}}]}},{observationComplete:true});assert.equal(result.outcome,mode==='defective'?'assertion_failed':'passed');report.cases.push({name:`same-condition-${mode}`,outcome:result.outcome});}
// Write the app's own announced service as a private endpoint file; never record its token.
async function endpoint(uri){const file=`vm-service-${report.identity.endpointFiles.length+1}.uri`;endpointFile=join(root,file);report.identity.endpointFiles.push(file);await writeFile(endpointFile,uri+'\n',{flag:'wx',mode:0o600});await save();}
async function announced(read){const until=Date.now()+30000;let match;while(!match&&Date.now()<until){match=[...(await read()).matchAll(announcement)].at(-1);if(!match)await delay(500);}assert.ok(match,'App process VM Service announcement');return match;}

// Each device adapter owns target lifecycle, artifact identity, native snapshots and the
// route to the app's VM Service. native() returns the single app process and its own logs.
function android(){
  const avds=join(root,'avd'),adb=join(sdk,'platform-tools/adb'),apk=join(project,'build/app/outputs/flutter-apk/app-debug.apk');
  const name=`Testrun-QA-Diagnostics-${randomUUID()}`,alias=`android:avd:${name}`;let serial;
  Object.assign(env,{ANDROID_AVD_HOME:avds,ANDROID_HOME:sdk,ANDROID_SDK_ROOT:sdk,JAVA_HOME:javaHome,TESTRUN_ADB:adb,TESTRUN_EMULATOR:join(sdk,'emulator/emulator'),TESTRUN_AAPT2:join(sdk,'build-tools/36.0.0/aapt2')});
  // Testrun's Android target generation follows the emulator's adb state and transport ID.
  // Record only this emulator's line at each transition so a reconnect can be located.
  async function transport(at){if(!serial)return;const {stdout}=await run(adb,['devices','-l'],{quiet:true,allowFailure:true});const line=stdout.split('\n').find(l=>l.split(/\s+/)[0]===serial);(report.identity.transports??=[]).push({at,state:line?.split(/\s+/)[1]??'absent',transportId:line?.match(/transport_id:(\d+)/)?.[1]??null,observedAt:new Date().toISOString()});}
  return {
    transport,
    async describe(){report.scope.image='system-images;android-36;google_apis_playstore;x86_64';report.identity.avd=name;report.identity.buildSha256=sha(await readFile(apk));},
    async start(){
      const reservation=createServer();await new Promise((accept,reject)=>{reservation.once('error',reject);reservation.listen(+hostPort,'127.0.0.1',accept);});await new Promise((accept,reject)=>reservation.close(e=>e?reject(e):accept()));
      await mkdir(avds,{mode:0o700});
      await run(join(sdk,'cmdline-tools/latest/bin/avdmanager'),['create','avd','--name',name,'--package',report.scope.image,'--device','pixel_6'],{input:'no\n',timeout:60000});
      const config=join(avds,`${name}.avd/config.ini`),original=await readFile(config,'utf8');await writeFile(config,original.split('\n').filter(line=>!['hw.lcd.width','hw.lcd.height','hw.lcd.density','skin.name','skin.path'].some(key=>line.startsWith(key+'='))).join('\n')+'\nhw.lcd.width=720\nhw.lcd.height=1280\nhw.lcd.density=320\nskin.name=720x1280\nskin.path=_no_skin\n');
      connected=true;const connection=await run(cli,['--json','connect','--target',alias,'--boot'],{timeout:240000,allowFailure:true});
      if(connection.code!==0){
        assert.ok(['TIMEOUT','RECOVERING'].includes(JSON.parse(connection.stderr).error.code),connection.stderr);
        const until=Date.now()+210000;let ready=false;
        while(Date.now()<until){const status=await call(['status','--target',alias]);if(status.state==='ready'){ready=true;break;}assert.equal(status.state,'recovering',JSON.stringify(status.health));await delay(500);}
        assert.ok(ready,'Owned emulator readiness');report.identity.bootRecovery=JSON.parse(connection.stderr).error.code;
        // Reusing the ready actor binds the alias to its emulator serial before serial-addressed calls.
        const reused=await run(cli,['--json','connect','--target',alias,'--boot'],{allowFailure:true});
        if(reused.code!==0){
          // A failed automatic screenshot does not revoke that connection; require fresh ready status.
          const error=JSON.parse(reused.stderr).error;assert.ok(['PLATFORM_FAILED','CAPTURE_FAILED'].includes(error.code),reused.stderr);
          assert.equal((await call(['status','--target',alias])).state,'ready');report.identity.bootstrapObservationError=error;
        }
      }
      target=(await call(['status','--target',alias])).health.target.targetId;assert.match(target,/^android:emulator-\d+$/);serial=target.slice(8);diagnosticTarget=target;report.identity.target=target;await transport('connected');await save();
      // After Testrun reports ready, the emulator's guest adb can still reconnect while its own
      // post-boot steps run (marker: raised screen-off timeout). Wait for the marker and 15 s of
      // one unchanged device transport, then reconnect the target once if it changed.
      const began=Date.now();let stable=null,last=null,timeout,settled=false;
      while(Date.now()<began+240000){
        const {stdout}=await run(adb,['devices','-l'],{quiet:true,allowFailure:true}),line=stdout.split('\n').find(l=>l.split(/\s+/)[0]===serial);
        const now={state:line?.split(/\s+/)[1]??'absent',transportId:line?.match(/transport_id:(\d+)/)?.[1]??null};
        if(now.state!=='device'||now.transportId!==last?.transportId||now.state!==last?.state){stable=now.state==='device'?Date.now():null;(report.identity.transports??=[]).push({at:'settling',...now,observedAt:new Date().toISOString()});}
        last=now;
        if(now.state==='device'&&!settled){timeout=(await run(adb,['-s',serial,'shell','settings','get','system','screen_off_timeout'],{allowFailure:true,timeout:15000})).stdout.trim();settled=Number(timeout)>=1800000;}
        if(settled&&stable&&Date.now()-stable>=15000)break;
        await delay(2000);
      }
      const connectedTransport=report.identity.transports.find(t=>t.at==='connected')?.transportId,reconnected=last?.transportId!==connectedTransport;
      if(reconnected){await call(['connect','--target',alias,'--boot']);assert.equal((await call(['status','--target',alias])).health.target.targetId,diagnosticTarget,'Same emulator serial after its guest adb reconnect');await transport('reconnected');}
      report.identity.emulatorPostBoot={settled,screenOffTimeout:timeout,stableTransport:last,reconnected,waitedMs:Date.now()-began};await save();
      assert.equal((await run(adb,['-s',serial,'emu','avd','name'])).stdout.split('\n')[0].trim(),name);
      report.identity.guest={};for(const key of ['ro.build.fingerprint','ro.build.version.sdk','ro.product.cpu.abi'])report.identity.guest[key]=(await run(adb,['-s',serial,'shell','getprop',key])).stdout.trim();
      assert.equal(report.identity.guest['ro.build.version.sdk'],'36');assert.equal(report.identity.guest['ro.product.cpu.abi'],'x86_64');
    },
    async install(){const installed=await call(['app','install','--target',target,apk],{timeout:180000});assert.equal(installed.verified,true);assert.equal(installed.packageSha256,report.identity.buildSha256);report.identity.install=installed;},
    async logCursor(){report.identity.logSince=(await run(adb,['-s',serial,'shell','date','+%s'])).stdout.trim()+'.000000';assert.match(report.identity.logSince,/^\d+\.000000$/);},
    async native(label,retries=2){
      const copy=await collect(label,{kind:'android',targetId:target,appId:bundle,userId:0,mode:'snapshot',systemContext:false,includeReports:false,since:report.identity.logSince});
      // A read-only snapshot whose log read failed is retained and repeated at most twice; input never is.
      const failed=copy.packets.find(p=>p.kind==='logs'&&p.data.status==='read_failed');
      if(failed&&retries>0){(report.identity.logReadFailures??=[]).push({label,code:failed.data.code});await delay(5000);return this.native(`${label}-reread`,retries-1);}
      assert.equal(copy.packets.find(p=>p.kind==='installed').appId,bundle);
      const processes=copy.packets.find(p=>p.kind==='processes').data.processes;assert.equal(processes.length,1);assert.ok(processes[0].generation);
      const logs=copy.packets.filter(p=>p.kind==='logs').flatMap(p=>p.data.entries??[]).map(row=>row.entry).filter(entry=>entry.pid===processes[0].pid);
      return {...copy,process:processes[0],logs};
    },
    async missingBreadcrumb(process){await run(adb,['-s',serial,'shell','logcat','-d','-v','epoch,uid,usec','-b','main',`--pid=${process.pid}`,'-T',report.identity.logSince,'AcmeDiagnostics:V','*:S'],{allowFailure:true});},
    // Read this process's own announcement, forward its device port and write the endpoint.
    async vmService(pid){
      const [,devicePort,token]=await announced(async()=>(await run(adb,['-s',serial,'shell','logcat','-d','-v','brief',`--pid=${pid}`,'-T',report.identity.logSince],{quiet:true})).stdout);
      await transport('before-forward');await run(adb,['-s',serial,'forward',`tcp:${hostPort}`,`tcp:${devicePort}`]);await transport('after-forward');
      report.identity.vmForward={hostPort:+hostPort,devicePort:+devicePort,pid};report.cleanup.forwardRemoved=false;
      await endpoint(`ws://127.0.0.1:${hostPort}/${token}/ws`);
    },
    async release(){await cleanupFlutter(root,sdk,javaHome);},
  };
}
function simulator(){
  const set=join(root,'simulators'),appBundle=join(project,'build/ios/iphonesimulator/Runner.app'),executable=join(appBundle,'Runner');let uuid;
  env.TESTRUN_SIMULATOR_DEVICE_SET=set;for(const key of ['TESTRUN_XCRUN','TESTRUN_APPLE_PS','TESTRUN_CODESIGN','TESTRUN_ADB'])delete env[key];
  const sim=(args,options)=>run('/usr/bin/xcrun',['simctl','--set',set,...args],{timeout:120000,...options});
  return {
    async transport(){},
    async describe(){
      report.scope.runtime=runtime;report.scope.deviceType='com.apple.CoreSimulator.SimDeviceType.iPhone-16';
      report.identity.appInfo=JSON.parse((await run('/usr/bin/plutil',['-convert','json','-o','-',join(appBundle,'Info.plist')])).stdout);
      assert.equal(report.identity.appInfo.CFBundleIdentifier,bundle);
      report.identity.buildSha256=sha(await readFile(executable));
      // Xcode Debug builds can place app code in a separate debug dylib beside the stub.
      const dylib=join(appBundle,'Runner.debug.dylib');if(await stat(dylib).then(()=>true,e=>e.code==='ENOENT'?false:Promise.reject(e)))report.identity.appDebugDylibSha256=sha(await readFile(dylib));
    },
    async start(){
      await mkdir(set,{mode:0o700});
      const inventory=JSON.parse((await run('/usr/bin/xcrun',['simctl','list','runtimes','--json'])).stdout);
      const selected=inventory.runtimes.find(value=>value.identifier===runtime&&value.isAvailable);assert.ok(selected,'Selected runtime is already installed');report.identity.runtime=selected;
      connected=true; // Release inspects the private set even if create's reply is lost.
      uuid=(await sim(['create',`Testrun-QA-Diagnostics-${randomUUID()}`,report.scope.deviceType,runtime])).stdout.trim();assert.match(uuid,/^[A-Fa-f0-9-]{36}$/);report.identity.simulatorUUID=uuid;
      const listing=await call(['target','list','--kind','simulator']);const owned=listing.targets.find(value=>value.targetId.endsWith(':'+uuid));assert.ok(owned);assert.equal(owned.deviceSetPath,set);
      target=owned.targetId;report.identity.target=target;diagnosticTarget=`simulator:${uuid}`;await save();
      const connection=await run(cli,['--json','connect','--target',target,'--boot'],{timeout:240000,allowFailure:true});
      if(connection.code===0)assert.equal(JSON.parse(connection.stdout).state,'ready',connection.stdout);
      else{
        // A slow boot reports recovery; wait read-only for ready, then reuse that actor once.
        const error=JSON.parse(connection.stderr).error;assert.ok(['TIMEOUT','RECOVERING'].includes(error.code),connection.stderr);report.identity.bootRecovery=error.code;
        const until=Date.now()+210000;let ready=false;
        while(Date.now()<until){const status=await call(['status','--target',target]);if(status.state==='ready'){ready=true;break;}assert.equal(status.state,'recovering',JSON.stringify(status.health));await delay(500);}
        assert.ok(ready,'Owned simulator readiness');assert.equal((await call(['connect','--target',target,'--boot'])).state,'ready');
      }
    },
    async install(){report.identity.install=await call(['app','install','--target',target,appBundle],{timeout:120000});},
    async logCursor(){report.identity.logWindowStartedAt=new Date().toISOString();},
    async native(label){
      // The window counts back from collection; keep it short so Debug output stays bounded.
      const copy=await collect(label,{kind:'ios',targetId:diagnosticTarget,appId:bundle,deviceSet:set,logSeconds:8,collectLogs:true,systemContext:false});
      const installed=copy.packets.find(p=>p.kind==='installed').data.identity;
      assert.equal(installed.appId,bundle);assert.ok(installed.executable.startsWith(set+'/'));assert.equal(sha(await readFile(installed.executable)),report.identity.buildSha256);
      const processes=copy.packets.find(p=>p.kind==='processes').data.processes;assert.equal(processes.length,1,JSON.stringify(processes));assert.ok(processes[0].generation);
      const logs=(copy.packets.find(p=>p.kind==='logs')?.data.entries??[]).filter(entry=>entry.pid===processes[0].pid);
      return {...copy,process:processes[0],logs};
    },
    // Retain one bounded direct read of the fixture subsystem for the observed PID.
    async missingBreadcrumb(process){await sim(['spawn',uuid,'log','show','--style','ndjson','--last','60s','--predicate',`processID == ${process.pid} AND subsystem == "com.example.acme.diagnostics"`],{timeout:30000,allowFailure:true});},
    // A simulator app's service listens on host loopback: no forward, only the endpoint file.
    async vmService(pid){
      // NDJSON escapes "/" in strings, so match each record's decoded eventMessage.
      const messages=stdout=>stdout.split('\n').map(line=>{try{return JSON.parse(line).eventMessage??'';}catch{return '';}}).join('\n');
      const [,port,token]=await announced(async()=>messages((await sim(['spawn',uuid,'log','show','--style','ndjson','--last','5m','--predicate',`processID == ${pid} AND eventMessage CONTAINS "VM service is listening"`],{timeout:30000,quiet:true})).stdout));
      report.identity.vmService={port:+port,pid};await endpoint(`ws://127.0.0.1:${port}/${token}/ws`);
    },
    async release(){await cleanupFlutter(root);},
  };
}
function dart(mode,vm,extra={}){return {kind:'dart',endpointFile,targetId:diagnosticTarget,appId:bundle,runtimeKind:'flutter_native',mode,isolateId:vm.isolateId,vmStartTime:vm.vmStartTime,isolateStartTime:vm.isolateStartTime,buildMode:'debug',buildId:report.identity.buildSha256,sampleMs:5000,...extra};}
// Select the single non-system main isolate explicitly; never the first listed one.
async function discoverVm(label){
  const first=await collect(label,{kind:'dart',endpointFile,targetId:diagnosticTarget,appId:bundle,runtimeKind:'flutter_native',mode:'discover'});
  const vm=first.packets.find(p=>p.kind==='discovery').data.vm;const main=vm.isolates.filter(i=>i.name==='main'&&!i.isSystemIsolate);assert.equal(main.length,1,JSON.stringify(vm.isolates));
  const second=await collect(`${label}-isolate`,{kind:'dart',endpointFile,targetId:diagnosticTarget,appId:bundle,runtimeKind:'flutter_native',mode:'discover',isolateId:main[0].id});
  const isolate=second.packets.find(p=>p.kind==='discovery').data.isolate;assert.equal(isolate.id,main[0].id);
  return {isolateId:isolate.id,vmStartTime:vm.startTime,isolateStartTime:isolate.startTime,vmPid:vm.pid,version:vm.version,rootLibrary:isolate.rootLib?.uri};
}
const rowsText=rows=>rows.map(row=>JSON.stringify(row.record));
async function bytesUnder(path){let total=0;for(const entry of await readdir(path,{recursive:true,withFileTypes:true}))if(entry.isFile())total+=(await stat(join(entry.parentPath,entry.name))).size;return total;}
async function profileRounds(){const end=Date.now()+30000;do{const label=(await poll('acme-profile-rounds'))[0]?.label,match=label?.match(/^rounds: (\d+)$/);if(match)return +match[1];await delay(200);}while(Date.now()<end);throw Error('Fixture workload rounds');}
const median=values=>{const sorted=[...values].sort((a,b)=>a-b),mid=sorted.length>>1;return sorted.length%2?sorted[mid]:(sorted[mid-1]+sorted[mid])/2;};
// Opt-in measurement in one app process: the fixture's fixed-time workload with no
// capture, an active `observe` capture and an active `cpu` capture, in rotating order.
// Rounds are app throughput; CLI timings include each command's startup and IPC.
async function measure(vm,repetitions=5){
  const point=await buttonPoint('Profile workload'),state=join(root,'state'),samples=[],stateBefore=await bytesUnder(state);
  for(let i=0;i<repetitions;i++)for(const arm of ['none','observe','cpu'].map((_,k,all)=>all[(k+i)%3])){
    await tap('reset');await expectText('acme-result','Acme reset');
    const sample={arm,repetition:i};let cid;
    if(arm!=='none'){const started=Date.now();cid=await activeWindow(`measure-${arm}-${i}`,dart(arm,vm,{sampleMs:1000}));sample.admissionToWindowMs=Date.now()-started;}
    await act({action:'tap',point});await expectText('acme-result','profile: done');sample.rounds=await profileRounds();
    if(cid){
      let started=Date.now();await settle(investigation);sample.settleMs=Date.now()-started;
      started=Date.now();const copy=await finish(`measure-${arm}-${i}`,cid);sample.rawReadMs=Date.now()-started;sample.bytes=copy.collector.bytes;
      if(arm==='cpu'){started=Date.now();const value=await call(['diagnostics','analyze',investigation,cid,'--submission-id',`parse-${cid}`]);await settle(investigation,value.analysisId);sample.analysisMs=Date.now()-started;started=Date.now();await call(['diagnostics','query',investigation,value.analysisId,'--filters',JSON.stringify({kind:'stack',limit:20})]);sample.queryMs=Date.now()-started;}
    }
    samples.push(sample);await save();
  }
  let started=Date.now();await call(['diagnostics','status',investigation]);const statusMs=Date.now()-started;
  const stateAfter=await bytesUnder(state);
  const arms=Object.fromEntries(['none','observe','cpu'].map(arm=>{const rows=samples.filter(s=>s.arm===arm),rounds=rows.map(s=>s.rounds);return [arm,{rounds,medianRounds:median(rounds),minRounds:Math.min(...rounds),maxRounds:Math.max(...rounds)}];}));
  for(const arm of ['observe','cpu'])arms[arm].medianRelativeToNone=+(arms[arm].medianRounds/arms.none.medianRounds).toFixed(3);
  report.measurement={repetitions,samples,arms,statusMs,stateBytesBefore:stateBefore,stateBytesAfter:stateAfter,loadAverage:(await run('/usr/sbin/sysctl',['-n','vm.loadavg'])).stdout.trim()};await save();
}

const device=ios?simulator():android();
await device.describe();await save();
try{
  await device.start();await save();
  const owner=JSON.parse(await readFile(join(root,'state/owner.json'),'utf8')),managerExecutable=(await run('/bin/ps',['-p',String(owner.pid),'-o','comm='])).stdout.trim();report.identity.managerSha256=sha(await readFile(managerExecutable));assert.equal(report.identity.managerSha256,report.identity.cliSha256);
  await device.install();await device.logCursor();
  await launch();await expectText('acme-result','Acme ready',90000);await expectText('acme-native-bridge','Acme native bridge available',90000);
  const opened=await call(['diagnostics','open','--submission-id','flutter','--question',`Verify selected owned Flutter ${report.scope.platform} Debug fixture`,'--binding',JSON.stringify({targetId:diagnosticTarget,appId:bundle,actionIds:['acme:exception','acme:retain']})]);investigation=opened.investigationId;report.identity.investigationId=investigation;await save();
  const baseline=await device.native('native-baseline');const bridged=baseline.logs.some(e=>e.message.includes('Acme native bridge requested'));if(!bridged)await device.missingBreadcrumb(baseline.process);assert.ok(bridged,'Actual own-PID native bridge breadcrumb');
  await device.vmService(baseline.process.pid);let vm=await discoverVm('discover');await device.transport('after-discover');report.identity.vm=vm;await save();assert.equal(vm.vmPid,baseline.process.pid,'VM-reported PID matches the native app process');
  report.cases.push({name:'installed-native-process-and-vm',process:baseline.process,vmPid:vm.vmPid,vmVersion:vm.version});
  if(measuring)await measure(vm);else{
  // Caught Dart error: app-owned log record with the error object, observed through the VM Logging stream.
  const caughtPoint=await buttonPoint('exception'),caughtWindow=await activeWindow('caught',dart('observe',vm));await act({action:'tap',point:caughtPoint});await expectText('acme-result','exception: Bad state: Acme Dart fixture failure');
  const caught=await finish('caught',caughtWindow),caughtRows=await analyze(caught,{kind:'failure',limit:100});
  assert.ok(rowsText(caughtRows.rows).some(text=>text.includes('Acme caught exception')&&text.includes('Acme Dart fixture failure')),'Actual Dart Logging failure row');
  report.cases.push({name:'actual-caught-dart-error',failureRows:caughtRows.rows.length});
  await tap('retain');await expectText('acme-result','retain: 1048576');await assertion('defective',1048576);
  // Retained objects: class snapshot, then bounded instance/retaining-path reads for the fixture owner.
  const classes=await collect('classes',dart('memory',vm,{sampleMs:100})),profile=classes.packets.find(p=>p.kind==='snapshot'&&p.data.position==='after').data.allocations,members=profile.members??[];
  const faultsClass=members.filter(m=>m.class?.name==='AcmeFaults');assert.equal(faultsClass.length,1,'AcmeFaults allocation class');
  const objects=await collect('retention',dart('inspect',vm,{sampleMs:100,classIds:[faultsClass[0].class.id]})),retention=await analyze(objects,{kind:'retention',limit:20});
  // A long path's indexed row can exceed the 32 KiB row payload and stay raw-only; then
  // the retained raw witness must show the path through the fixture state's field.
  const throughState=elements=>elements.some((e,i)=>e.parentField==='faults'&&e.value?.class?.name==='_AcmeDiagnosticsState'&&elements[i-1]?.value?.class?.name==='AcmeFaults');
  const rawPaths=[],walk=value=>{if(Array.isArray(value))value.forEach(walk);else if(value&&typeof value==='object'){if(value.type==='RetainingPath'&&Array.isArray(value.elements))rawPaths.push(value.elements);Object.values(value).forEach(walk);}};walk(objects.packets);
  const indexed=rowsText(retention.rows).some(text=>text.includes('_AcmeDiagnosticsState')),rawOnly=retention.rows.some(row=>row.record.data?.observation?.availability==='raw_only');
  assert.ok(indexed||(rawOnly&&rawPaths.some(throughState)),'Observed retaining path through the fixture state');
  report.cases.push({name:'dart-class-and-retaining-path',classId:faultsClass[0].class.id,bytesCurrent:faultsClass[0].bytesCurrent,retentionRows:retention.rows.length,classesReported:profile.membersReported,classesKept:members.length,classesOmittedByLimit:profile.membersOmittedByLimit});
  await tap('reset');await expectText('acme-result','Acme reset');
  await switchTo('acme-corrected',true);await tap('retain');await expectText('acme-result','retain: 0');await assertion('corrected',0);await tap('exception');await expectText('acme-result','exception: ready');report.cases.push({name:'actual-retention-reset-correction'});
  await switchTo('acme-corrected',false);
  const workPoint=await buttonPoint('Profile workload'),cpuWindow=await activeWindow('dart-cpu',dart('cpu',vm,{sampleMs:1000}));await act({action:'tap',point:workPoint});
  const cpu=await finish('dart-cpu',cpuWindow),stacks=await analyze(cpu,{kind:'stack',limit:100});
  assert.ok(rowsText(stacks.rows).some(text=>text.includes('acmeProfileWork')),'Actual Dart CPU samples in the fixture workload');
  report.cases.push({name:'actual-dart-cpu',stackRows:stacks.rows.length});await expectText('acme-result','profile: done');
  await device.logCursor();await tap('retain');const correlated=await device.native('native-trigger');await expectText('acme-result','retain: 1048576');
  const triggered=correlated.logs.some(e=>e.message.includes('Acme trigger retain'));if(!triggered)await device.missingBreadcrumb(correlated.process);assert.ok(triggered,'Actual own-PID native action breadcrumb');assert.equal(correlated.process.generation,baseline.process.generation);
  report.cases.push({name:'native-log-correlation',pid:correlated.process.pid});
  await switchTo('acme-uncaught',true);const uncaughtPoint=await buttonPoint('exception'),uncaughtWindow=await activeWindow('uncaught',dart('observe',vm));await act({action:'tap',point:uncaughtPoint});
  const uncaught=await finish('uncaught',uncaughtWindow),failures=await analyze(uncaught,{kind:'failure',limit:100});
  assert.ok(rowsText(failures.rows).some(text=>text.includes('Acme unhandled error')&&text.includes('Acme Dart fixture failure')),'App-reported unhandled zone error');
  report.cases.push({name:'actual-unhandled-dart-error',failureRows:failures.rows.length});
  // Explicit relaunch: a new process, VM birth and service endpoint; nothing is reconnected implicitly.
  await device.logCursor();await act({action:'app_terminate',appId:bundle});await launch();await expectText('acme-result','Acme ready',90000);
  const relaunched=await device.native('native-relaunched');assert.notEqual(relaunched.process.generation,baseline.process.generation);
  await device.vmService(relaunched.process.pid);const next=await discoverVm('rediscover');assert.notEqual(next.vmStartTime,vm.vmStartTime);assert.equal(next.vmPid,relaunched.process.pid);
  report.cases.push({name:'explicit-relaunch-new-vm-generation',before:{pid:vm.vmPid,vmStartTime:vm.vmStartTime},after:{pid:next.vmPid,vmStartTime:next.vmStartTime}});vm=next;
  }
  report.state=measuring?'measured':'passed';
}catch(error){report.state='failed';report.error=String(error.stack??error);process.exitCode=1;cleaning=true;await device.transport('failure').catch(()=>{});}
finally{
  cleaning=true;await save();
  if(connected)try{await device.release();Object.assign(report.cleanup,JSON.parse(await readFile(reportPath,'utf8')).cleanup);}catch(e){report.cleanup.errors=[String(e)];report.state='failed';process.exitCode=1;}
  report.cleanup.errors??=[];await save();
  const recovery=report.cleanup.recovery;
  console.log(JSON.stringify({state:report.state,report:reportPath,cases:report.cases,measurement:report.measurement&&{arms:report.measurement.arms,statusMs:report.measurement.statusMs},cleanup:{investigationRemoved:report.cleanup.investigationRemoved,forwardRemoved:report.cleanup.forwardRemoved,endpointFilesRemoved:report.cleanup.endpointFilesRemoved,manager:recovery?.manager,emulatorAbsentFromADB:recovery?.emulatorAbsentFromADB,remainingAVDFiles:recovery?.remainingAVDFiles,remainingSimulators:recovery?.remainingSimulators,errors:report.cleanup.errors},error:report.error?.slice(0,2000)}));
}
}
