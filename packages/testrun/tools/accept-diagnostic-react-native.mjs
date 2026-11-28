// Selected Expo or bare RN native Debug/Hermes acceptance on one new owned Android
// emulator or iOS simulator. Owns that target, Metro and a private manager.
import assert from 'node:assert/strict';
import {execFile,spawn} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';
import {appendFile,cp,link,mkdir,readFile,stat,writeFile} from 'node:fs/promises';
import {isAbsolute,join} from 'node:path';
import {connect,createServer} from 'node:net';
import {setTimeout as delay} from 'node:timers/promises';
import {promisify} from 'node:util';
import {cleanupDiagnosticAndroid} from './cleanup-diagnostic-android.mjs';
import {cleanupOwnedSimulator,cleanupReactNative,metroWitness,stopOwnedMetro} from './cleanup-diagnostic-react-native.mjs';
import {ownedReactNativeApp,reportFramework,verifyIosProject} from './diagnostic-react-native-app.mjs';

if(process.argv[2]==='--cleanup'){
  await cleanupReactNative(...process.argv.slice(3));
}else{
const ios=process.argv[2]==='--ios';
let selectedCLI,appRoot,tool,sdk,javaHome,runtime,appBundle,root,port;
if(ios){
  [selectedCLI,appRoot,tool,runtime,appBundle,root,port='8097']=process.argv.slice(3);
  assert.equal(process.platform,'darwin','iOS simulator acceptance requires macOS');
  for(const path of [selectedCLI,appRoot,tool,appBundle,root])assert.ok(path&&isAbsolute(path),'Select absolute CLI, prepared app root, pnpm, built simulator app and new evidence root');
  assert.match(runtime??'',/^com\.apple\.CoreSimulator\.SimRuntime\.iOS-[\d-]+$/,'Select an installed iOS runtime identifier');
}else{
  [selectedCLI,appRoot,tool,sdk,javaHome,root,port='8097']=process.argv.slice(2);
  for(const path of [selectedCLI,appRoot,tool,sdk,javaHome,root])assert.ok(path&&isAbsolute(path),'Select absolute CLI, prepared app root, pnpm, SDK, JDK and new evidence root');
}
assert.match(port,/^\d+$/);assert.ok(+port>1024&&+port<65536);
const {profile,project}=await ownedReactNativeApp(appRoot),bundle=profile.appId;
if(ios)await verifyIosProject(project,profile);
await mkdir(root,{mode:0o700});const cli=join(root,'testrun');await link(selectedCLI,cli);
// Metro's loopback family depends on its CLI; readiness selects the answering one.
const reportPath=join(root,'acceptance.json'),loopbacks={'127.0.0.1':`http://127.0.0.1:${port}`,'::1':`http://[::1]:${port}`};let endpoint;
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const env={...process.env,TESTRUN_DATA_ROOT:join(root,'state'),CI:'1',EXPO_NO_TELEMETRY:'1',EXPO_NO_DEV_MENU:'1'};
for(const key of ['TESTRUN_TASK_ID','CODEX_THREAD_ID'])delete env[key];
const report={schemaVersion:1,state:'running',scope:{platform:ios?'ios':'android',host:process.platform,arch:process.arch,node:process.version,app:profile.description,framework:reportFramework(profile,ios?'ios':'android'),workBudgetMs:1800000},identity:{appRoot,cliSha256:sha(await readFile(cli)),harnessSha256:sha(await readFile(new URL(import.meta.url)))},cases:[],collectors:[],cleanup:{},gaps:['Proxy/app probe metadata is not authenticated native process association','Logical buffers are not a heap census or GC reclamation; bridge/network controls are simulated','No causal capture overhead, loss guarantee or independent source-map symbolication']};
const execute=promisify(execFile),deadline=Date.now()+1800000;let logBytes=0,target,diagnosticTarget,metro,metroExit,forwarder,connected=false,cleaning=false,investigation;const forwarded=new Set();
async function record(value){const text=JSON.stringify(value)+'\n';logBytes+=Buffer.byteLength(text);assert.ok(logBytes<67108864,'Private operation-log bound');await appendFile(join(root,'operations.jsonl'),text,{mode:0o600});}
async function run(program,args,{timeout=45000,allowFailure=false,input}={}){
  if(!cleaning)assert.ok(Date.now()<deadline,'Acceptance work budget');
  const started=Date.now();
  let stdout='',stderr='',code=0,error;
  try{const child=execute(program,args,{cwd:root,env,timeout,maxBuffer:4194304});if(input!==undefined)child.child.stdin.end(input);({stdout,stderr}=await child);}
  catch(e){stdout=e.stdout??'';stderr=e.stderr??'';code=e.code??'unknown';error=e;}
  await record({program,args,stdout,stderr,code,durationMs:Date.now()-started});if(error&&!allowFailure)throw error;return {stdout,stderr,code};
}
async function call(args,options){return JSON.parse((await run(cli,['--json',...args],options)).stdout);}
async function act(action){return call(['act','--target',target,JSON.stringify(action)]);}
async function find(query){const args=query.identifier?['--identifier',query.identifier]:['--label',query.label];if(query.role)args.push('--role',query.role);const result=await call(['ui','find','--target',target,...args,'--allow-partial']);return result.data.matches;}
async function node(id){const nodes=await find({identifier:id});assert.equal(nodes.length,1,JSON.stringify(nodes));return nodes[0];}
async function tap(label){await act({action:'tap',element:{query:{role:'button',label:{text:label,mode:'exact'}}},allowPartial:true});}
async function buttonPoint(label){const nodes=(await find({label})).filter(n=>n.role==='button'&&n.label.toLowerCase()===label.toLowerCase());assert.equal(nodes.length,1,JSON.stringify(nodes));const point=nodes[0].center;assert.ok(point&&[point.x,point.y].every(v=>Number.isFinite(v)&&v>=0&&v<=1));return point;}
// Android reports a checked state; iOS exposes the switch value.
function checked(current){if(typeof current.states?.checked==='boolean')return current.states.checked;const value=String(current.value).toLowerCase();assert.ok(['0','1','on','off','true','false'].includes(value),JSON.stringify(current));return ['1','on','true'].includes(value);}
async function switchTo(id,expected){if(checked(await node(id))!==expected)await act({action:'tap',element:{query:{identifier:id}},allowPartial:true});assert.equal(checked(await node(id)),expected);}
// Readiness polls are read-only: a failed accessibility read is counted and polled
// again within the same deadline. Input is never repeated.
// iOS Fabric Text exposes its label but not its testID. There the harness selects
// the unique text node with the slot's exact expected text or fixture prefix.
const textPrefixes={'acme-probe':'Acme probe ','acme-native-bridge':'Acme native bridge '};
async function textNodes(id,expected){
  if(!ios)return find({identifier:id});
  const prefix=textPrefixes[id];assert.ok(prefix||expected,`No iOS text selector for ${id}`);
  return (await find({label:prefix??expected,role:'text'})).filter(n=>prefix?n.label.startsWith(prefix):n.label===expected);
}
async function poll(id,expected){try{return await textNodes(id,expected);}catch(error){let code;try{code=JSON.parse(error.stderr).error.code;}catch{}if(code!=='PLATFORM_FAILED')throw error;report.identity.readinessReadFailures=(report.identity.readinessReadFailures??0)+1;return [];}}
async function expectText(id,expected,timeoutMs=30000){const end=Date.now()+timeoutMs;let last;do{const nodes=await poll(id,expected);assert.ok(nodes.length<=1,JSON.stringify(nodes));last=nodes[0]?.label;if(last===expected)return last;await delay(200);}while(Date.now()<end);assert.equal(last,expected);}
async function probeEpoch(previous){const end=Date.now()+90000;let last;do{const nodes=await poll('acme-probe');assert.ok(nodes.length<=1);last=nodes[0]?.label;if(last&&/^Acme probe (?!disabled)/.test(last)&&last!==previous)return last;await delay(200);}while(Date.now()<end);throw Error(`Fresh fixture probe did not become visible: ${last}`);}
async function settle(id,analysis){const end=Date.now()+45000;do{const value=await call(analysis?['diagnostics','analysis-status',id,analysis]:['diagnostics','status',id]);if(value.settled)return value;await delay(100);}while(Date.now()<end);throw Error('Diagnostic settlement deadline');}
async function start(name,source){const value=await call(['diagnostics','start',investigation,'--submission-id',name,'--source',JSON.stringify(source),'--limits',JSON.stringify({durationMs:30000,maxBytes:8388608,maxEvents:4096})]);return value.collectors.at(-1).collectorId;}
async function activeWindow(name,source){const cid=await start(name,source),until=Date.now()+15000;do{const status=await call(['diagnostics','status',investigation]),collector=status.collectors.find(c=>c.collectorId===cid);assert.equal(collector.settled,false,'Capture must remain active before fixture input');if(collector.sourceReport?.stage===6&&collector.sourceReport.contextObserved){if(source.mode==='cpu')assert.equal(collector.sourceReport.profilerMayBeActive,true);return cid;}await delay(100);}while(Date.now()<until);throw Error('Runtime capture did not enter its sampling window before input');}
async function finish(name,cid){const status=await settle(investigation),collector=status.collectors.find(c=>c.collectorId===cid);report.collectors.push({name,...collector});assert.ok(['partial','complete'].includes(collector.phase),JSON.stringify(collector));assert.ok(collector.bytes>0);let bytes=Buffer.alloc(0),cursor;while(bytes.length<collector.bytes){const part=await call(['diagnostics','read',investigation,cid,'--limit','65536',...(cursor?['--cursor',cursor]:[])]);const chunk=Buffer.from(part.dataBase64,'base64');assert.ok(chunk.length);bytes=Buffer.concat([bytes,chunk]);cursor=part.nextCursor;assert.ok(cursor||bytes.length===collector.bytes);}assert.equal(sha(bytes),collector.sha256);await writeFile(join(root,`${name}.ndjson`),bytes,{mode:0o600});if(collector.sourceKind==='react_native'){assert.equal(collector.reason,'react_native_sampled_window',JSON.stringify(collector));if(!['discover','rediscover'].includes(name))assert.equal(collector.sourceReport.contextObserved,true);}return {cid,collector,packets:bytes.toString().trim().split('\n').map(line=>JSON.parse(line))};}
async function collect(name,source){return finish(name,await start(name,source));}
async function analyze(copy,filters,metadata){const args=['diagnostics','analyze',investigation,copy.cid,'--submission-id',`parse-${copy.cid}`];if(metadata)args.push('--metadata',JSON.stringify(metadata));const value=await call(args);assert.equal((await settle(investigation,value.analysisId)).phase,'complete');return call(['diagnostics','query',investigation,value.analysisId,'--filters',JSON.stringify(filters)]);}
function rn(mode,page,extra={}){return {kind:'react_native',endpoint,targetId:diagnosticTarget,appId:bundle,mode,...page,probe:true,allowDebuggerTakeover:true,sampleMs:10000,...extra};}
async function discover(name){const copy=await collect(name,{kind:'react_native',endpoint,targetId:diagnosticTarget,appId:bundle,mode:'discover'});const discovery=copy.packets.find(p=>p.kind==='discovery');assert.ok(discovery,JSON.stringify(copy.packets));const pages=discovery.data.pages;assert.equal(pages.length,1,JSON.stringify(pages));return {pageId:pages[0].id,logicalDeviceId:pages[0].logicalDeviceId};}
async function assertion(mode,value){const path=join(root,`${mode}.json`),definition='Observed Acme RN result control; logical fixture-owned typed-array bytes';await writeFile(path,JSON.stringify({schemaVersion:1,records:[{kind:'metric',name:'Acme retained buffers',value,unit:'bytes',definition,data:{mode,observedTextView:`retain: ${value}`}}]}),{flag:'wx',mode:0o600});const copy=await collect(mode,{kind:'import',path,format:'testrun-events-v1'});const result=await analyze(copy,{view:'assertions',assertions:{checks:[{id:'released',domain:'performance',name:'Acme retained buffers',predicate:{type:'metric',unit:'bytes',definition,max:0,aggregation:'max'}}]}},{observationComplete:true});assert.equal(result.outcome,mode==='defective'?'assertion_failed':'passed');report.cases.push({name:`same-condition-${mode}`,outcome:result.outcome});}

// Each device adapter owns target lifecycle, artifact identity and native snapshots.
// native() returns the single app process and its own-process log entries {pid,message}.
function android(){
  const avds=join(root,'avd'),apk=join(project,'android/app/build/outputs/apk/debug/app-debug.apk');
  const name=`Testrun-QA-Diagnostics-${randomUUID()}`,alias=`android:avd:${name}`,adb=join(sdk,'platform-tools/adb');
  Object.assign(env,{ANDROID_AVD_HOME:avds,ANDROID_HOME:sdk,ANDROID_SDK_ROOT:sdk,JAVA_HOME:javaHome,TESTRUN_ADB:adb,TESTRUN_EMULATOR:join(sdk,'emulator/emulator'),TESTRUN_AAPT2:join(sdk,'build-tools/36.0.0/aapt2')});
  const serial=()=>target.slice(8);
  return {
    breadcrumbs:'bounded app-owned logcat breadcrumbs',scopeGap:profile.scopeGap,
    async describe(){report.scope.image='system-images;android-36;google_apis_playstore;x86_64';report.identity.avd=name;report.identity.apkSha256=sha(await readFile(apk));report.identity.buildSha256=report.identity.apkSha256;},
    async start(){
      await mkdir(avds,{mode:0o700});
      await run(join(sdk,'cmdline-tools/latest/bin/avdmanager'),['create','avd','--name',name,'--package',report.scope.image,'--device','pixel_6'],{input:'no\n',timeout:60000});
      const config=join(avds,`${name}.avd/config.ini`),original=await readFile(config,'utf8');await writeFile(config,original.split('\n').filter(line=>!['hw.lcd.width','hw.lcd.height','hw.lcd.density','skin.name','skin.path'].some(key=>line.startsWith(key+'='))).join('\n')+'\nhw.lcd.width=720\nhw.lcd.height=1280\nhw.lcd.density=320\nskin.name=720x1280\nskin.path=_no_skin\n');
      connected=true;const connection=await run(cli,['--json','connect','--target',alias,'--boot'],{timeout:240000,allowFailure:true});
      if(connection.code!==0){
        assert.ok(['TIMEOUT','RECOVERING'].includes(JSON.parse(connection.stderr).error.code));
        const until=Date.now()+210000;let ready=false;
        while(Date.now()<until){const status=await call(['status','--target',alias]);if(status.state==='ready'){ready=true;break;}assert.equal(status.state,'recovering',JSON.stringify(status.health));await delay(500);}
        assert.ok(ready);
        const reused=await run(cli,['--json','connect','--target',alias,'--boot'],{allowFailure:true});
        if(reused.code!==0){
          // Ready-actor reuse reconciles the alias before its automatic screenshot.
          // A failed read does not revoke that connection; require fresh ready status
          // and later actual native controls instead of repeating connect or input.
          const error=JSON.parse(reused.stderr).error;
          assert.equal(error.code,'PLATFORM_FAILED');assert.equal(error.details?.exitCode,255);
          assert.equal((await call(['status','--target',alias])).state,'ready');
          report.identity.bootstrapObservationError=error;
        }
      }
      const status=await call(['status','--target',alias]);target=status.health.target.targetId;assert.match(target,/^android:emulator-\d+$/);report.identity.target=target;diagnosticTarget=target;
      assert.equal((await run(adb,['-s',serial(),'emu','avd','name'])).stdout.split('\n')[0].trim(),name);
      report.identity.guest={};for(const key of ['ro.build.fingerprint','ro.build.version.sdk','ro.product.cpu.abi'])report.identity.guest[key]=(await run(adb,['-s',serial(),'shell','getprop',key])).stdout.trim();
      assert.equal(report.identity.guest['ro.build.version.sdk'],'36');assert.equal(report.identity.guest['ro.product.cpu.abi'],'x86_64');
    },
    async install(){const installed=await call(['app','install','--target',target,apk]);assert.equal(installed.verified,true);assert.equal(installed.packageSha256,report.identity.apkSha256);report.identity.install=installed;},
    async logCursor(){report.identity.logSince=(await run(adb,['-s',serial(),'shell','date','+%s'])).stdout.trim()+'.000000';assert.match(report.identity.logSince,/^\d+\.000000$/);},
    async native(name){
      const copy=await collect(name,{kind:'android',targetId:target,appId:bundle,userId:0,mode:'snapshot',systemContext:false,includeReports:false,since:report.identity.logSince});
      const installed=copy.packets.find(p=>p.kind==='installed');assert.equal(installed.appId,bundle);
      const processes=copy.packets.find(p=>p.kind==='processes').data.processes;assert.equal(processes.length,1);assert.ok(processes[0].generation);
      const logs=copy.packets.filter(p=>p.kind==='logs').flatMap(p=>p.data.entries??[]).map(row=>row.entry).filter(entry=>entry.pid===processes[0].pid);
      return {...copy,process:processes[0],logs};
    },
    // Retain bounded direct PID/UID reads to separate collector selection from missing output.
    async missingBreadcrumb(process){for(const selector of ['--uid='+process.uid,'--pid='+process.pid])await run(adb,['-s',serial(),'shell','logcat','-d','-v','epoch,uid,usec','-b','main','-b','crash',selector,'-T',report.identity.logSince,'*:V'],{allowFailure:true});},
    async release(){await cleanupDiagnosticAndroid(root,sdk,javaHome);},
  };
}
function simulator(){
  const set=join(root,'simulators'),executable=join(appBundle,'AcmeDiagnostics');let uuid;
  env.TESTRUN_SIMULATOR_DEVICE_SET=set;for(const key of ['TESTRUN_XCRUN','TESTRUN_APPLE_PS','TESTRUN_CODESIGN','TESTRUN_ADB'])delete env[key];
  const sim=args=>run('/usr/bin/xcrun',['simctl','--set',set,...args],{timeout:120000});
  return {
    breadcrumbs:'bounded app-owned os_log breadcrumbs',scopeGap:profile.iosScopeGap,
    async describe(){
      report.scope.runtime=runtime;report.scope.deviceType='com.apple.CoreSimulator.SimDeviceType.iPhone-16';
      report.identity.appInfo=JSON.parse((await run('/usr/bin/plutil',['-convert','json','-o','-',join(appBundle,'Info.plist')])).stdout);
      assert.equal(report.identity.appInfo.CFBundleIdentifier,bundle);
      report.identity.appSha256=sha(await readFile(executable));report.identity.buildSha256=report.identity.appSha256;
      // Xcode Debug builds can place app code in a separate debug dylib beside the stub.
      const dylib=join(appBundle,'AcmeDiagnostics.debug.dylib');if(await stat(dylib).then(()=>true,e=>e.code==='ENOENT'?false:Promise.reject(e)))report.identity.appDebugDylibSha256=sha(await readFile(dylib));
    },
    async start(){
      await mkdir(set,{mode:0o700});
      const inventory=JSON.parse((await run('/usr/bin/xcrun',['simctl','list','runtimes','--json'])).stdout);
      const selected=inventory.runtimes.find(value=>value.identifier===runtime&&value.isAvailable);assert.ok(selected,'Selected runtime is already installed');report.identity.runtime=selected;
      connected=true; // Release inspects the private set even if create's reply is lost.
      uuid=(await sim(['create',`Testrun-QA-Diagnostics-${randomUUID()}`,report.scope.deviceType,runtime])).stdout.trim();assert.match(uuid,/^[A-Fa-f0-9-]{36}$/);report.identity.simulatorUUID=uuid;
      const listing=await call(['target','list','--kind','simulator']);const owned=listing.targets.find(value=>value.targetId.endsWith(':'+uuid));assert.ok(owned);assert.equal(owned.deviceSetPath,set);
      target=owned.targetId;report.identity.target=target;diagnosticTarget=`simulator:${uuid}`;
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
    async install(){
      report.identity.install=await call(['app','install','--target',target,appBundle],{timeout:120000});
      // The prebuilt RN core compiles in port 8081. Select the owned Metro explicitly in
      // the app's preferences on this private simulator; RN checks it before its default.
      const location=`127.0.0.1:${port}`;
      await sim(['spawn',uuid,'defaults','write',bundle,'RCT_jsLocation','-string',location]);
      assert.equal((await sim(['spawn',uuid,'defaults','read',bundle,'RCT_jsLocation'])).stdout.trim(),location);
      report.identity.packagerLocation=location;
    },
    async logCursor(){report.identity.logWindowStartedAt=new Date().toISOString();},
    async native(name){
      // The window counts back from collection, not from the action, which precedes it by
      // a few seconds. A Debug Expo app exceeded the unchanged output bound in 15 seconds.
      const copy=await collect(name,{kind:'ios',targetId:diagnosticTarget,appId:bundle,deviceSet:set,logSeconds:8,collectLogs:true,systemContext:false});
      const installed=copy.packets.find(p=>p.kind==='installed').data.identity;
      assert.equal(installed.appId,bundle);assert.ok(installed.executable.startsWith(set+'/'));assert.equal(sha(await readFile(installed.executable)),report.identity.appSha256);
      const processes=copy.packets.find(p=>p.kind==='processes').data.processes;assert.equal(processes.length,1,JSON.stringify(processes));assert.ok(processes[0].generation);
      const logs=(copy.packets.find(p=>p.kind==='logs')?.data.entries??[]).filter(entry=>entry.pid===processes[0].pid);
      return {...copy,process:processes[0],logs,installed};
    },
    // Retain one bounded direct read of the fixture subsystem for the observed PID.
    async missingBreadcrumb(process){await run('/usr/bin/xcrun',['simctl','--set',set,'spawn',uuid,'log','show','--style','ndjson','--last','60s','--predicate',`processID == ${process.pid} AND subsystem == "com.example.acme.diagnostics"`],{timeout:30000,allowFailure:true});},
    async release(){await cleanupOwnedSimulator(root);},
  };
}
const device=ios?simulator():android();
await device.describe();
report.scope.nativeFixtureBridge=device.breadcrumbs;report.gaps.unshift(device.scopeGap);
await writeFile(reportPath,JSON.stringify(report),{flag:'wx',mode:0o600});
try{
  // Refuse a port occupied on either loopback family, without adopting or stopping its listener.
  for(const host of Object.keys(loopbacks)){const reservation=createServer();await new Promise((accept,reject)=>{reservation.once('error',reject);reservation.listen(+port,host,accept);});await new Promise((accept,reject)=>reservation.close(e=>e?reject(e):accept()));}
  report.identity.dependencies=JSON.parse(await readFile(join(project,'package.json'),'utf8'));
  report.identity.lockSha256=sha(await readFile(join(project,'pnpm-lock.yaml')));
  if(ios)report.identity.podfileLockSha256=sha(await readFile(join(project,'ios/Podfile.lock')));
  await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  await device.start();await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  const owner=JSON.parse(await readFile(join(root,'state/owner.json'),'utf8')),executable=(await run('/bin/ps',['-p',String(owner.pid),'-o','comm='])).stdout.trim();report.identity.managerSha256=sha(await readFile(executable));assert.equal(report.identity.managerSha256,report.identity.cliSha256);
  const fixtures=new URL('../tests/fixtures/diagnostics/',import.meta.url);for(const file of ['AcmeDiagnostics.jsx','DiagnosticApp.jsx','runtime.js'])await cp(new URL(file,fixtures),join(project,'diagnostics',file));
  await writeFile(join(project,'diagnostics/acceptance.json'),JSON.stringify({enabled:true,metadata:{appId:bundle,targetId:diagnosticTarget,buildId:report.identity.buildSha256,buildMode:'development',clientMode:profile.clientMode,projectId:'acme-diagnostics'}}));
  // Run the owned Metro process group; retain bounded output and wait on exit.
  metro=spawn(tool,profile.metroArgs(port),{cwd:project,env,detached:true,stdio:['ignore','pipe','pipe']});
  metroExit=new Promise(resolve=>{metro.once('error',e=>resolve({error:String(e)}));metro.once('exit',(code,signal)=>resolve({code,signal}));});
  report.identity.metro={pid:metro.pid,witness:await metroWitness(metro.pid)};
  await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  let metroBytes=0;for(const stream of [metro.stdout,metro.stderr])stream.on('data',chunk=>{metroBytes+=chunk.length;if(metroBytes<=4194304)appendFile(join(root,'metro.log'),chunk,{mode:0o600}).catch(()=>{});});
  const until=Date.now()+90000;while(!endpoint&&Date.now()<until){for(const candidate of Object.values(loopbacks)){try{const response=await fetch(`${candidate}/status`,{signal:AbortSignal.timeout(2000)});if((await response.text()).includes('packager-status:running')){endpoint=candidate;break;}}catch{}}if(!endpoint)await delay(500);}assert.ok(endpoint,'Owned Metro readiness');report.identity.metroEndpoint=endpoint;
  if(endpoint===loopbacks['::1']){
    // Metro's inspector accepts DevTools origins for its 127.0.0.1 base URL, not [::1],
    // while this CLI listens only on ::1. Own an explicit loopback forwarder for it.
    forwarder=createServer(client=>{const upstream=connect({host:'::1',port:+port});const end=()=>{client.destroy();upstream.destroy();};for(const socket of [client,upstream])socket.on('error',end).on('close',end);forwarded.add(client).add(upstream);client.pipe(upstream).pipe(client);});
    await new Promise((accept,reject)=>{forwarder.once('error',reject);forwarder.listen(+port,'127.0.0.1',accept);});
    endpoint=loopbacks['127.0.0.1'];report.identity.metroForwarder={listen:endpoint,to:loopbacks['::1']};
    assert.match(await (await fetch(`${endpoint}/status`,{signal:AbortSignal.timeout(5000)})).text(),/packager-status:running/);
  }
  await device.install();
  const bundled=await fetch(`${endpoint}${profile.bundlePath(report.scope.platform)}`,{signal:AbortSignal.timeout(180000)});assert.equal(bundled.status,200,'Owned Metro bundle must compile');
  const bundleHash=createHash('sha256');let bundleBytes=0;for await(const chunk of bundled.body){bundleBytes+=chunk.length;assert.ok(bundleBytes<67108864,'Prewarmed bundle bound');bundleHash.update(chunk);}report.identity.prewarmedMetroBundleSha256=bundleHash.digest('hex');report.identity.prewarmedMetroBundleBytes=bundleBytes;
  await act({action:'app_launch',appId:bundle,...(ios?{readiness:{timeoutMs:30000}}:{})});await expectText('acme-result','Acme ready',90000);const firstEpoch=await probeEpoch(null);await expectText('acme-native-bridge','Acme native bridge available');await device.logCursor();await tap('Native breadcrumb');
  const opened=await call(['diagnostics','open','--submission-id','rn','--question',`Verify selected owned ${profile.framework} ${report.scope.platform} Hermes fixture`,'--binding',JSON.stringify({targetId:diagnosticTarget,appId:bundle,actionIds:['acme:exception','acme:retain']})]);investigation=opened.investigationId;
  report.identity.investigationId=investigation;await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  const baseline=await device.native('native-baseline');
  const nativeStartup=baseline.logs.some(entry=>entry.message.includes('Acme native bridge requested'));
  if(!nativeStartup)await device.missingBreadcrumb(baseline.process);
  assert.ok(nativeStartup,'Actual own-PID native bridge breadcrumb');report.cases.push({name:'installed-native-process',process:baseline.process});
  let page=await discover('discover');report.identity.inspector=page;
  const caughtPoint=await buttonPoint('exception'),logging=await activeWindow('caught-console',rn('logs',page));await act({action:'tap',point:caughtPoint});await expectText('acme-result','exception: Acme JavaScript fixture failure');const caught=await finish('caught-console',logging);assert.ok(JSON.stringify(caught.packets).includes('Acme caught exception: Acme JavaScript fixture failure'));
  const probe=caught.packets.find(p=>p.kind==='probe'&&p.data.probe?.metadata?.appId===bundle);assert.ok(probe);assert.equal(probe.data.probe.metadata.targetId,diagnosticTarget);assert.equal(`Acme probe ${probe.data.probe.metadata.runtimeGeneration}`,firstEpoch);
  const events=await analyze(caught,{limit:100});assert.ok(events.rows.length);const reactCommits=events.rows.filter(row=>row.record.kind==='metric'&&row.record.name==='Acme diagnostics').length;assert.ok(reactCommits,'Actual reported React Profiler commits');report.cases.push({name:'actual-caught-console-and-probe',rows:events.rows.length,reactCommits});
  await tap('retain');await expectText('acme-result','retain: 1048576');await assertion('defective',1048576);await tap('reset');await expectText('acme-result','reset: undefined');
  await switchTo('acme-corrected',true);await tap('retain');await expectText('acme-result','retain: 0');await assertion('corrected',0);await tap('exception');await expectText('acme-result','exception: "ready"');report.cases.push({name:'actual-retention-reset-correction'});
  await switchTo('acme-corrected',false);
  const workPoint=await buttonPoint('Profile workload'),cpu=await activeWindow('hermes-cpu',rn('cpu',page,{allowProfilerControl:true}));await act({action:'tap',point:workPoint});const hermes=await finish('hermes-cpu',cpu);assert.ok(hermes.packets.some(p=>p.kind==='probe'&&p.data.hermes===true));const stacks=await analyze(hermes,{kind:'stack',limit:100});assert.ok(stacks.rows.length,'Actual Hermes sample stacks');assert.equal(hermes.collector.sourceReport.profilerMayBeActive,false);assert.equal(hermes.collector.sourceReport.cleanup.profilerStopAcknowledged,true);report.cases.push({name:'actual-hermes-cpu',stackRows:stacks.rows.length});
  // Snapshot immediately after the action so a short native log window covers it.
  await device.logCursor();await tap('retain');const correlated=await device.native('native-trigger');await expectText('acme-result','retain: 1048576');
  const triggered=correlated.logs.some(e=>e.message.includes('Acme trigger retain'));if(!triggered)await device.missingBreadcrumb(correlated.process);assert.ok(triggered,'Actual own-PID native action breadcrumb');assert.equal(correlated.process.generation,baseline.process.generation);report.cases.push({name:'native-log-correlation',pid:correlated.process.pid});
  await tap('Reload JavaScript');const secondEpoch=await probeEpoch(firstEpoch);await expectText('acme-result','Acme ready');page=await discover('rediscover');const reloaded=await collect('after-reload',rn('logs',page,{sampleMs:1000}));assert.ok(reloaded.packets.some(p=>p.kind==='probe'&&p.data.probe?.metadata?.runtimeGeneration));report.cases.push({name:'explicit-reload-and-fresh-inspector',before:firstEpoch,after:secondEpoch});
  await switchTo('acme-uncaught',true);const uncaughtPoint=await buttonPoint('exception'),uncaughtId=await activeWindow('uncaught-console',rn('logs',page));await act({action:'tap',point:uncaughtPoint});const uncaught=await finish('uncaught-console',uncaughtId);assert.ok(JSON.stringify(uncaught.packets).includes('Acme uncaught exception requested'));assert.ok(JSON.stringify(uncaught.packets).includes('Acme JavaScript fixture failure'));const failures=await analyze(uncaught,{kind:'failure',limit:100});assert.ok(failures.rows.some(row=>JSON.stringify(row.record).includes('Acme JavaScript fixture failure')),'Actual fixture inspector error evidence');report.cases.push({name:'actual-unhandled-error',failureRows:failures.rows.length});
  report.state='passed';
}catch(error){report.state='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{
  cleaning=true;const errors=[];
  if(investigation)try{await call(['diagnostics','close',investigation]);await settle(investigation);await call(['diagnostics','remove',investigation]);report.cleanup.investigationRemoved=true;}catch(e){errors.push(String(e));}
  if(forwarder)try{const closed=new Promise(accept=>forwarder.close(accept));for(const socket of forwarded)socket.destroy();await closed;report.cleanup.forwarderClosed=true;}catch(e){errors.push(String(e));}
  if(metro)try{await stopOwnedMetro(report.identity.metro);await metroExit;report.cleanup.metroStopped=true;}catch(e){errors.push(String(e));}
  try{await writeFile(join(project,'diagnostics/acceptance.json'),'{"enabled":false}\n');report.cleanup.probeConfigDisabled=true;}catch(e){errors.push(String(e));}
  report.cleanup.errors=errors;await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  if(connected)try{await device.release();report.cleanup.recovery=JSON.parse(await readFile(reportPath,'utf8')).cleanup.recovery;}catch(e){errors.push(String(e));}
  if(errors.length){report.state='failed';process.exitCode=1;}report.cleanup.errors=errors;
  const recovery=report.cleanup.recovery;
  await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify({state:report.state,report:reportPath,cases:report.cases,cleanup:{investigationRemoved:report.cleanup.investigationRemoved,metroStopped:report.cleanup.metroStopped,probeConfigDisabled:report.cleanup.probeConfigDisabled,forwarderClosed:report.cleanup.forwarderClosed,manager:recovery?.manager,emulatorAbsentFromADB:recovery?.emulatorAbsentFromADB,remainingAVDFiles:recovery?.remainingAVDFiles,remainingSimulators:recovery?.remainingSimulators,errors},error:report.error?.slice(0,2000),errorTruncated:(report.error?.length??0)>2000}));
}
}
