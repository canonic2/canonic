// Explicit owned Android fixture acceptance; no physical device or default AVD.
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createHash,randomUUID} from 'node:crypto';
import {appendFile,link,mkdir,readFile,readdir,realpath,writeFile} from 'node:fs/promises';
import {isAbsolute,join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

if(process.argv[2]==='--cleanup'){
  const {cleanupDiagnosticAndroid}=await import('./cleanup-diagnostic-android.mjs');
  await cleanupDiagnosticAndroid(...process.argv.slice(3));
}else{
const {agentWorkflow}=await import('./diagnostic-agent-workflow.mjs');
const [selectedCLI,apk,sdk,root,javaHome,simpleperf]=process.argv.slice(2);
for(const path of [selectedCLI,apk,sdk,root,javaHome])assert.ok(path&&isAbsolute(path),'Select absolute CLI/APK/SDK/new root/JDK');
if(simpleperf)assert.ok(isAbsolute(simpleperf));
await mkdir(root,{mode:0o700});
const reportPath=join(root,'acceptance.json');await writeFile(reportPath,'{"state":"incomplete"}\n',{flag:'wx',mode:0o600});
const cli=join(root,'testrun');await link(selectedCLI,cli);
const avds=join(root,'avd');await mkdir(avds,{mode:0o700});
const bundle='com.example.acme.diagnostics', image='system-images;android-36;google_apis_playstore;x86_64';
const name=`Testrun-QA-Diagnostics-${randomUUID()}`, avdTarget=`android:avd:${name}`;
const adb=join(sdk,'platform-tools/adb'), execute=promisify(execFile), sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const environment={...process.env,TESTRUN_DATA_ROOT:join(root,'state'),ANDROID_AVD_HOME:avds,ANDROID_HOME:sdk,ANDROID_SDK_ROOT:sdk,JAVA_HOME:javaHome,TESTRUN_ADB:adb,TESTRUN_EMULATOR:join(sdk,'emulator/emulator'),TESTRUN_AAPT2:join(sdk,'build-tools/36.0.0/aapt2')};
for(const key of ['TESTRUN_TASK_ID','CODEX_THREAD_ID','TESTRUN_XCRUN','TESTRUN_APPLE_PS','TESTRUN_CODESIGN'])delete environment[key];
const report={schemaVersion:1,state:'running',scope:{host:process.platform,architecture:process.arch,node:process.version,image,device:'pixel_6; owned 720x1280 read-only AVD',app:'unsigned-for-distribution Debug APK; user 0',workBudgetMs:900000},identity:{cliSha256:sha(await readFile(cli)),apkSha256:sha(await readFile(apk)),harnessSha256:sha(await readFile(new URL(import.meta.url))),avd:name},cases:[],collectors:[],gaps:['Selected emulator/Debug only; physical/OEM/release not certified','Logical buffers are not heap ownership, GC or RSS reclamation','No certified ANR/watchdog, native tombstone/unwinding, complete delivery or causal capture overhead'],cleanup:{}};
let target,connected=false,cleanup=false,logBytes=0;const investigations=[],deadline=Date.now()+900000;
async function run(program,args,{timeout=40000,allowFailure=false,input}={}){
  if(!cleanup)assert.ok(Date.now()<deadline,'Acceptance work budget');
  const began=performance.now();let stdout='',stderr='',exitCode=0,failure;
  try{const child=execute(program,args,{cwd:root,env:environment,timeout,maxBuffer:2097152});if(input!==undefined)child.child.stdin.end(input);({stdout,stderr}=await child);}
  catch(error){stdout=error.stdout??'';stderr=error.stderr??'';exitCode=error.code??'unknown';failure={code:error.code,killed:error.killed,signal:error.signal};if(!allowFailure){await record({program,args,stdout,stderr,exitCode,failure,wallMs:performance.now()-began});throw error;}}
  const value={program,args,stdout,stderr,exitCode,failure,wallMs:performance.now()-began};await record(value);return value;
}
async function record(value){const text=JSON.stringify(value)+'\n';logBytes+=Buffer.byteLength(text);assert.ok(logBytes<=33554432,'Private operation log bound');await appendFile(join(root,'operations.jsonl'),text,{mode:0o600});}
async function call(args,options){return JSON.parse((await run(cli,['--json',...args],options)).stdout);}
async function runtimeWitness(label){
  const owner=JSON.parse(await readFile(join(root,'state/owner.json'),'utf8'));assert.ok(Number.isSafeInteger(owner.pid)&&owner.pid>0);
  const executable=(await run('/bin/ps',['-p',String(owner.pid),'-o','comm='])).stdout.trim();assert.ok(isAbsolute(executable));
  const witness={label,pid:owner.pid,build:owner.build,protocol:owner.protocol,executable,sha256:sha(await readFile(executable)),states:[]};
  const base=await realpath(join(root,'state'));
  for(const version of (await readdir(base)).filter(name=>/^v\d+-/.test(name)).slice(0,8)){
    for(const entry of (await readdir(join(base,version))).filter(name=>name.startsWith('t-')).slice(0,32)){
      const selected=await readFile(join(base,version,entry,'selected.json'),'utf8').then(JSON.parse).catch(error=>error.code==='ENOENT'?null:Promise.reject(error));
      if(selected?.target!==avdTarget)continue;
      const directory=await realpath(selected.directory);assert.ok(directory.startsWith(base+'/'));
      const state=JSON.parse(await readFile(join(directory,'state.json'),'utf8'));witness.states.push({state:state.state,targetId:state.targetId,directory});
    }
  }
  (report.identity.runtimeWitnesses??=[]).push(witness);
  assert.equal(witness.sha256,report.identity.cliSha256,'Running manager executable must match the selected CLI');
}
async function connectOwned(){
  const started=Date.now(),result=await run(cli,['--json','connect','--target',avdTarget,'--boot'],{timeout:240000,allowFailure:true});
  await runtimeWitness('first-connect-reply');
  if(result.exitCode===0)return JSON.parse(result.stdout);
  const error=JSON.parse(result.stderr).error;
  assert.ok(['RECOVERING','TIMEOUT'].includes(error?.code),JSON.stringify(error));
  // A cold boot can outlast one connect reply. Read the same owner's status;
  // never create/start a second AVD or repeat app input while it is pending.
  // Native identity/readiness limits are 60s/240s from launch. The outer
  // bound also permits the final bounded doctor/cleanup calls to settle.
  while(Date.now()-started<300000){
    const status=await call(['status','--target',avdTarget]);
    if(status.state==='ready'){await runtimeWitness('ready-before-reuse');return call(['connect','--target',avdTarget,'--boot'],{timeout:40000});}
    assert.equal(status.state,'recovering',JSON.stringify(status));
    await delay(500);
  }
  throw Error('Owned emulator startup did not settle within its native lifecycle budget');
}
async function act(action){return call(['act','--target',target,JSON.stringify(action)]);}
const identifier=name=>`${bundle}:id/${name}`;
async function find(query){const value=await call(['ui','find','--target',target,...(query.identifier?['--identifier',query.identifier]:['--label',query.label]),'--allow-partial']);assert.ok(Array.isArray(value.data?.matches));assert.ok(value.data.matches.length<=1,JSON.stringify(value.data));return value.data.matches[0];}
async function visible(query){for(let n=0;n<5;n++){const node=await find(query);if(node?.center)return node;const top=Boolean(query.identifier);await act({action:'swipe',from:{x:0.65,y:top?0.25:0.8},to:{x:0.65,y:top?0.8:0.25},durationMs:250});}throw Error(`Acme control absent ${JSON.stringify(query)}`);}
async function tap(label){await visible({label});return act({action:'tap',element:{query:{label:{text:label,mode:'exact'}}},allowPartial:true});}
async function switchTo(name,checked){const query={identifier:identifier(name)},node=await visible(query);assert.equal(typeof node.states?.checked,'boolean',JSON.stringify(node));if(node.states.checked!==checked)await act({action:'tap',element:{query},allowPartial:true});assert.equal((await find(query)).states.checked,checked);}
async function state(expected){const node=await visible({identifier:identifier('acme_result')});assert.equal(node.label,expected,JSON.stringify(node));return node.label;}
async function launch(){await act({action:'app_launch',appId:bundle,readiness:{element:{identifier:identifier('acme_result')},timeoutMs:20000}});await state('Acme ready');}
async function settle(id,analysis){const until=Date.now()+45000;while(Date.now()<until){const value=await call(analysis?['diagnostics','analysis-status',id,analysis]:['diagnostics','status',id]);if(value.settled)return value;await delay(100);}throw Error('Diagnostic settlement deadline');}
async function open(){const value=await call(['diagnostics','open','--submission-id','android','--question','Verify owned Acme Android emulator','--binding',JSON.stringify({targetId:target,appId:bundle})]);investigations.push(value.investigationId);return value.investigationId;}
async function collect(id,name,source){
  const started=await call(['diagnostics','start',id,'--submission-id',name,'--source',JSON.stringify(source),'--limits',JSON.stringify({durationMs:30000,maxBytes:8388608,maxEvents:4096})]);
  const cid=started.collectors.at(-1).collectorId,status=await settle(id),collector=status.collectors.find(value=>value.collectorId===cid);
  const reason={android:'android_sampled_window',android_state:'android_sampled_window',native_trace:'native_trace_window'}[source.kind];
  assert.equal(collector.phase,reason?'partial':'complete',JSON.stringify(collector));assert.equal(collector.reason,reason??'source_complete');assert.ok(collector.bytes>0&&collector.sha256?.length===64);
  report.collectors.push({name,id,cid,phase:collector.phase,reason:collector.reason,bytes:collector.bytes,sha256:collector.sha256,sourceReport:collector.sourceReport});return {cid,collector};
}
async function analyze(id,cid,filters,metadata){const args=['diagnostics','analyze',id,cid,'--submission-id',`parse-${cid}`];if(metadata)args.push('--metadata',JSON.stringify(metadata));const analysis=await call(args),status=await settle(id,analysis.analysisId);assert.equal(status.phase,'complete',JSON.stringify(status));return call(['diagnostics','query',id,analysis.analysisId,'--filters',JSON.stringify(filters)]);}
async function packets(id,copy){let text='',cursor;for(let offset=0;offset<copy.collector.bytes;){const part=await call(['diagnostics','read',id,copy.cid,'--limit','65536',...(cursor?['--cursor',cursor]:[])]);const bytes=Buffer.from(part.dataBase64,'base64');assert.ok(bytes.length);offset+=bytes.length;text+=bytes.toString();cursor=part.nextCursor;assert.ok(cursor||offset===copy.collector.bytes);}return text.trim().split('\n').map(line=>JSON.parse(line));}
async function snapshot(id,name){const copy=await collect(id,name,{kind:'android',targetId:target,appId:bundle,userId:0,mode:'snapshot',systemContext:true,includeReports:false});const rows=await packets(id,copy);const installed=rows.find(row=>row.kind==='installed');assert.equal(installed.appId,bundle);assert.equal(installed.data.identity.versionCode,'1');assert.ok(installed.bootId&&installed.transportId&&installed.uid>=10000);return {...copy,packets:rows,processes:rows.find(row=>row.kind==='processes').data.processes,uid:installed.uid,boot:installed.bootId};}
function logs(copy){return copy.packets.filter(row=>row.kind==='logs').flatMap(row=>row.data.entries??[]).map(row=>row.entry);}
async function sameAssertion(id,mode,value){const path=join(root,`${mode}.json`),definition='Acme Android result TextView after Java buffer retention; logical owned array bytes, not a heap census';await writeFile(path,JSON.stringify({schemaVersion:1,records:[{kind:'metric',name:'Acme retained buffers',value,unit:'bytes',definition,data:{mode,observedTextView:String(value)}}]}),{flag:'wx',mode:0o600});const copy=await collect(id,mode,{kind:'import',path,format:'testrun-events-v1'}),result=await analyze(id,copy.cid,{view:'assertions',assertions:{checks:[{id:'released',domain:'performance',name:'Acme retained buffers',predicate:{type:'metric',unit:'bytes',definition,max:0,aggregation:'max'}}]}},{observationComplete:true});assert.equal(result.outcome,mode==='defective'?'assertion_failed':'passed');report.cases.push({name:`same-condition-${mode}`,outcome:result.outcome});return result.analysisId;}
try{
  report.identity.sdkImage=await readFile(join(sdk,'system-images/android-36/google_apis_playstore/x86_64/source.properties'),'utf8');
  report.identity.adbVersion=(await run(adb,['version'])).stdout;report.identity.emulatorVersion=(await run(environment.TESTRUN_EMULATOR,['-version'])).stdout;
  await run(join(sdk,'cmdline-tools/latest/bin/avdmanager'),['create','avd','--name',name,'--package',image,'--device','pixel_6'],{timeout:60000,input:'no\n'});
  const config=join(avds,`${name}.avd/config.ini`),original=await readFile(config,'utf8');
  await writeFile(config,original.split('\n').filter(line=>!['hw.lcd.width','hw.lcd.height','hw.lcd.density','skin.name','skin.path'].some(key=>line.startsWith(key+'='))).join('\n')+'\nhw.lcd.width=720\nhw.lcd.height=1280\nhw.lcd.density=320\nskin.name=720x1280\nskin.path=_no_skin\n');
  connected=true;const connection=await connectOwned();assert.equal(connection.state,'ready',JSON.stringify(connection));
  report.identity.connectionTarget=connection.targetId;
  // QA can retain the pooled AVD alias. Native diagnostics must use the
  // resolved serial exposed by that same owner's ready status.
  const ready=await call(['status','--target',avdTarget]);assert.equal(ready.state,'ready',JSON.stringify(ready));
  target=ready.health?.target?.targetId;assert.match(target,/^android:emulator-\d+$/);report.identity.target=target;
  const serial=target.slice(8),avdName=await run(adb,['-s',serial,'emu','avd','name']);assert.equal(avdName.stdout.split('\n')[0].trim(),name);
  report.identity.guest={};for(const property of ['ro.build.fingerprint','ro.build.version.release','ro.build.version.sdk','ro.product.cpu.abi'])report.identity.guest[property]=(await run(adb,['-s',serial,'shell','getprop',property])).stdout.trim();
  assert.equal(report.identity.guest['ro.build.version.sdk'],'36');assert.equal(report.identity.guest['ro.product.cpu.abi'],'x86_64');
  const install=await call(['app','install','--target',target,apk]);assert.equal(install.verified,true);assert.equal(install.buildVerified,true);assert.equal(install.packageSha256,report.identity.apkSha256);report.identity.install=install;
  await launch();
  await tap('Java exception (caught)');await state('java.lang.IllegalStateException: Acme Java fixture failure');
  await tap('Kotlin exception (caught)');await state('java.lang.IllegalStateException: Acme Kotlin fixture failure');
  await tap('Retain one MiB');const defective=Number(await state('1048576'));await tap('Reset');await state('Acme reset');
  await switchTo('acme_corrected',true);await tap('Retain one MiB');const corrected=Number(await state('0'));
  for(const [control,result] of [['Java crash','ready'],['Kotlin crash','ready'],['Main-thread hang (6 seconds)','hang requested'],['Background crash','background requested'],['Kill this app','kill control ready']]){await switchTo('acme_armed',true);await tap(control);await state(result);}
  report.cases.push({name:'actual-ui-caught-retention-reset-corrected-native-controls',defective,corrected});
  await switchTo('acme_corrected',false);await tap('Retain one MiB');
  const id=await open(),baseline=await snapshot(id,'baseline');assert.equal(baseline.processes.length,1);const process=baseline.processes[0];assert.ok(process.generation&&process.startTicks);assert.ok(logs(baseline).some(entry=>entry.pid===process.pid&&entry.message.includes('Acme trigger Retain one MiB corrected=false')));
  const processRows=await analyze(id,baseline.cid,{kind:'process',limit:20});assert.ok(processRows.rows.length);report.cases.push({name:'real-installed-process-logs',pid:process.pid,generation:process.generation,logs:logs(baseline).length,uid:baseline.uid});
  const measured={defective:await sameAssertion(id,'defective',defective),corrected:await sameAssertion(id,'corrected',corrected)};let primary;
  const resources=await collect(id,'resources',{kind:'android_state',targetId:target,appId:bundle,userId:0,groups:['process','memory','frames'],mode:'snapshot',label:'owned-acme'});const metrics=await analyze(id,resources.cid,{kind:'metric',limit:100});assert.ok(metrics.rows.length);report.cases.push({name:'actual-resource-snapshot',metricRows:metrics.rows.length,meaning:'Partial OS accounting; not a heap census or overhead measurement'});
  if(simpleperf){
    // Optional native attempt has its own explicit result, never an invented pass.
    try{const trace=await collect(id,'native-simpleperf',{kind:'native_trace',backend:'simpleperf',mode:'capture',targetId:target,appId:bundle,processId:process.pid,durationMs:1000,frequencyHz:99,toolPath:simpleperf});const rows=await analyze(id,trace.cid,{kind:'stack',limit:100});assert.ok(rows.rows.length);report.cases.push({name:'actual-native-simpleperf',stackRows:rows.rows.length});}
    catch(error){report.gaps.push(`Selected simpleperf capture unavailable: ${String(error.message).slice(0,500)}`);report.cases.push({name:'native-simpleperf',status:'unavailable'});}
  }else report.gaps.push('Native profiler not selected; no native capture pass');
  await switchTo('acme_armed',true);await tap('Main-thread hang (6 seconds)');
  await call(['wait','--target',target,JSON.stringify([{kind:'exists',query:{identifier:identifier('acme_result')}}]),'--timeout-ms','15000','--allow-partial']);await tap('Reset');await state('Acme reset');report.cases.push({name:'armed-six-second-sleep-and-responsive-reset',meaning:'No inferred OS ANR or measured causal latency'});
  await act({action:'app_terminate',appId:bundle});await launch();const relaunched=await snapshot(id,'relaunched');assert.equal(relaunched.processes.length,1);assert.notEqual(relaunched.processes[0].generation,process.generation);report.cases.push({name:'native-relaunch-generation',before:process,after:relaunched.processes[0]});
  for(const [key,control,message] of [['java','Java crash','Acme Java fixture failure'],['kotlin','Kotlin crash','Acme Kotlin fixture failure'],['background','Background crash','Acme Java background failure'],['kill','Kill this app',null]]){
    const before=await snapshot(id,`${key}-before`);assert.equal(before.processes.length,1);const prior=before.processes[0];await switchTo('acme_armed',true);await tap(control);
    const until=Date.now()+15000;let absent=false;while(Date.now()<until){const value=await run(adb,['-s',serial,'shell','pidof',bundle],{allowFailure:true});if(value.exitCode===1&&!value.stdout.trim()){absent=true;break;}await delay(100);}assert.ok(absent,'Observed selected process ended');
    const ended=await snapshot(id,`${key}-after`);assert.equal(ended.processes.length,0);assert.equal(ended.boot,before.boot);assert.equal(ended.uid,before.uid);
    const observed=logs(ended).filter(entry=>entry.pid===prior.pid);assert.ok(observed.some(entry=>entry.message.includes(`Acme trigger ${control} corrected=false`)),'Exact Acme native trigger breadcrumb');if(message)assert.ok(observed.some(entry=>entry.message.includes(message)),'Native exception output');
    const failures=await analyze(id,ended.cid,{kind:'failure',limit:100});if(message)assert.ok(failures.rows.length,'Native exception classification');if(message&&!primary)primary=failures.analysisId;
    report.cases.push({name:`actual-${key}-process-end`,pid:prior.pid,generation:prior.generation,failureRows:failures.rows.length,meaning:'Native logs and observed process absence; no inferred signal, OS ANR, or cause'});await launch();
  }
  const agent=await agentWorkflow({cli,cwd:root,env:environment,record,root,investigation:id,primary,baseline:measured.defective,corrected:measured.corrected,platform:'android',preset:'crash'});
  report.cases.push({name:'agent-workflow-over-mcp',...agent});
  report.state='passed';
}catch(error){report.state='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{
  cleanup=true;const errors=[];
  if(connected)try{report.cleanup.disconnect=await call(['disconnect','--target',avdTarget]);assert.equal(report.cleanup.disconnect.closed,true);}catch(error){errors.push(String(error));}
  for(const id of investigations)try{await call(['diagnostics','close',id]);await settle(id);await call(['diagnostics','remove',id]);}catch(error){errors.push(String(error));}
  try{report.cleanup.manager=await call(['manager','stop']);assert.equal(report.cleanup.manager.retired,true,JSON.stringify(report.cleanup.manager));}catch(error){errors.push(String(error));}
  // Delete only our uniquely named AVD from our new set, after manager release.
  try{const devices=await run(adb,['devices','-l']);if(target)assert.ok(!devices.stdout.split('\n').some(line=>line.split(/\s+/)[0]===target.slice(8)),'Owned emulator released from ADB');report.cleanup.emulatorAbsentFromADB=true;await run(join(sdk,'cmdline-tools/latest/bin/avdmanager'),['delete','avd','--name',name],{timeout:60000});const entries=await readdir(avds);assert.equal(entries.length,0);report.cleanup.remainingAVDFiles=0;}catch(error){errors.push(String(error));}
  async function files(path){let count=0;for(const entry of await readdir(path,{withFileTypes:true}).catch(error=>error.code==='ENOENT'?[]:Promise.reject(error)))count+=entry.isDirectory()?await files(join(path,entry.name)):1;return count;}
  report.cleanup.remainingDiagnosticFiles=await files(join(root,'state/diagnostics'));report.cleanup.errors=errors;if(errors.length||report.cleanup.remainingDiagnosticFiles){report.state='failed';process.exitCode=1;}
  await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify({state:report.state,report:reportPath,cases:report.cases.map(row=>({name:row.name,status:row.status??'passed'})),cleanup:{closed:report.cleanup.disconnect?.closed,manager:report.cleanup.manager,remainingAVDFiles:report.cleanup.remainingAVDFiles,remainingDiagnosticFiles:report.cleanup.remainingDiagnosticFiles,errors},error:report.error}));
}
}
