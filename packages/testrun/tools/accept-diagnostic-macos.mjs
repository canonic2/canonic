// Explicit live AppKit acceptance. Only owned children and a new private root.
import assert from 'node:assert/strict';
import {spawn, execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createHash} from 'node:crypto';
import {createInterface} from 'node:readline';
import {appendFile, link, mkdir, readFile, readdir, writeFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';
import {arch, release} from 'node:os';
import {setTimeout as delay} from 'node:timers/promises';

const [selectedCLI, app, root, selectedCase = 'all', selectedIPS, selectedIPSPid] = process.argv.slice(2);
assert.equal(process.platform, 'darwin');
assert.ok(['all','objc','faults','ips','sample-probe','sample-probe-late-symbols'].includes(selectedCase),'Select all, faults, objc, ips or a diagnostic-only sample probe');
for (const value of [selectedCLI, app, root]) assert.ok(value && isAbsolute(value), 'Select absolute CLI, app bundle and new evidence root');
await mkdir(root, {mode: 0o700});
const reportPath = join(root, 'acceptance.json');
await writeFile(reportPath, '{"state":"incomplete"}\n', {flag:'wx', mode:0o600});
const cli = join(root, 'testrun'); await link(selectedCLI, cli);
const executable = join(app, 'Contents/MacOS/AcmeDiagnostics');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const execute = promisify(execFile), children = new Set(), investigations = [];
const environment = {...process.env, TESTRUN_DATA_ROOT:join(root, 'state')};
for (const name of ['TESTRUN_TASK_ID','CODEX_THREAD_ID','TESTRUN_XCRUN','TESTRUN_APPLE_PS','TESTRUN_CODESIGN','TESTRUN_ADB']) delete environment[name];
const report = {schemaVersion:1,state:'running',host:{os:'macos',kernel:release(),architecture:arch(),node:process.version,contention:'shared host'},identity:{cliSha256:sha(await readFile(cli)),appSha256:sha(await readFile(executable)),configuration:'optimized with debug symbols'},scope:{app:'com.example.acme.diagnostics.macos',driver:'explicit bounded stdin; real NSButton.performClick, not desktop input automation',sampler:'selected owned PID, stack_sample 1000 ms',budget:'five-minute work deadline; per-call time/output limits; own processes/state only'},cases:[],cost:[],gaps:[],cleanup:{}};
report.scope.selectedCase=selectedCase;
const deadline = Date.now()+300000;
let logBytes = 0, cleanup = false;
async function call(args) {
  if (!cleanup) assert.ok(Date.now()<deadline, 'Acceptance work deadline');
  const before = performance.now();
  const {stdout} = await execute(cli, ['--json',...args], {cwd:root,env:environment,timeout:20000,maxBuffer:2097152});
  const value = JSON.parse(stdout);
  const row = JSON.stringify({args,result:value,wallMs:performance.now()-before})+'\n';
  logBytes += Buffer.byteLength(row); assert.ok(logBytes<=8388608, 'Bounded private operation log');
  await appendFile(join(root,'operations.jsonl'),row,{mode:0o600}); return value;
}
async function settle(id, analysis) {
  const until=Date.now()+20000;
  while (Date.now()<until) {
    const value=await call(analysis?['diagnostics','analysis-status',id,analysis]:['diagnostics','status',id]);
    if (value.settled) return value;
    await delay(50);
  }
  throw Error('Diagnostic settlement deadline');
}
async function open(target, submission) {
  const value=await call(['diagnostics','open','--submission-id',submission,'--question','Verify owned Acme macOS fixture','--binding',JSON.stringify({targetId:target,appId:'com.example.acme.diagnostics.macos'})]);
  investigations.push(value.investigationId); return value.investigationId;
}
async function collect(id, submission, source) {
  const before=performance.now();
  const value=await call(['diagnostics','start',id,'--submission-id',submission,'--source',JSON.stringify(source),'--limits',JSON.stringify({durationMs:15000,maxBytes:8388608,maxEvents:4096})]);
  const cid=value.collectors.at(-1).collectorId;
  const status=await settle(id); const collector=status.collectors.find(row=>row.collectorId===cid);
  const windowReason={host_process:'host_process_snapshot',native_trace:'native_trace_window'}[source.kind];
  assert.equal(collector.phase,windowReason?'partial':'complete',JSON.stringify(collector));
  assert.equal(collector.reason,windowReason??'source_complete',JSON.stringify(collector));
  assert.ok(collector.bytes>0&&collector.sha256?.length===64,JSON.stringify(collector));
  if(source.kind==='host_process') assert.equal(collector.sourceReport.signalsSent,false);
  if(source.kind==='native_trace') {
    const identity=collector.sourceReport.identity;
    assert.ok(identity.kernelBirth); assert.deepEqual(identity.kernelBirth,identity.after.kernelBirth);
    assert.equal(identity.executable,identity.after.executable);
    assert.equal(collector.sourceReport.cleanup,'owned_host_files_removed');
  }
  report.cost.push({submission,collectorId:cid,sha256:collector.sha256,artifactSha256:collector.sourceReport?.artifactSha256,phase:collector.phase,reason:collector.reason,settledCliWallMs:performance.now()-before,bytes:collector.bytes,cost:collector.cost});
  return cid;
}
async function query(id,cid,filters,metadata) {
  const args=['diagnostics','analyze',id,cid,'--submission-id',`parse-${cid}`];
  if(metadata) args.push('--metadata',JSON.stringify(metadata));
  const parsed=await call(args); const status=await settle(id,parsed.analysisId);
  assert.equal(status.phase,'complete',JSON.stringify(status));
  return call(['diagnostics','query',id,parsed.analysisId,'--filters',JSON.stringify(filters)]);
}
async function startApp() {
  const child=spawn(executable,['--acme-driver'],{cwd:root,stdio:['pipe','pipe','pipe']}); children.add(child);
  let pending, queue=[], stderr=Buffer.alloc(0), stdoutBytes=0;
  const lines=createInterface({input:child.stdout});
  lines.on('line',line=>{
    stdoutBytes+=Buffer.byteLength(line); if(stdoutBytes>65536) { pending?.reject(Error('Fixture stdout bound')); child.kill(); return; }
    try { const row=JSON.parse(line); if(pending) { const waiter=pending; pending=null; waiter.resolve(row); } else queue.push(row); }
    catch(error) { pending?.reject(error); child.kill(); }
  });
  child.stderr.on('data',bytes=>{ if(stderr.length+bytes.length<=524288) stderr=Buffer.concat([stderr,bytes]); else child.kill(); });
  const exited=new Promise(resolve=>{
    child.once('error',error=>{ pending?.reject(error); resolve({error:error.message}); });
    child.once('close',(code,signal)=>{ pending?.reject(Error(`Fixture exited ${code??signal}`)); resolve({code,signal}); });
  });
  async function receive(command) {
    let timeout;
    try {
      const value=queue.length?queue.shift():await new Promise((resolve,reject)=>{ pending={resolve,reject}; timeout=setTimeout(()=>{pending=null;reject(Error(`Fixture ${command} deadline`));},13000); });
      assert.equal(value.command,command); assert.equal(value.pid,child.pid); return value;
    } finally { clearTimeout(timeout); }
  }
  const ready=await receive('ready');
  const owner={child,ready,exited,async command(command){child.stdin.write(command+'\n');return receive(command);},async finish(){const result=await exited;children.delete(child);await writeFile(join(root,`app-${child.pid}.stderr`),stderr,{flag:'wx',mode:0o600});return result;}};
  return owner;
}
async function snapshot(owner, id, name, generation) {
  const source={kind:'host_process',targetId:`process:Acme:${owner.child.pid}`,hostId:'Acme',serviceId:'com.example.acme.diagnostics.macos',processId:owner.child.pid};
  if(generation) source.expectedGeneration=generation;
  const cid=await collect(id,name,source);
  const result=await query(id,cid,{kind:'metric',limit:100});
  assert.ok(result.rows.some(row=>row.record.unit==='bytes'));
  assert.ok(result.rows.every(row=>row.record.data.pid===owner.child.pid));
  const raw=await call(['diagnostics','read',id,cid,'--limit','65536']);
  const packet=JSON.parse(Buffer.from(raw.dataBase64,'base64').toString());
  return {collectorId:cid,generation:packet.processGeneration,metrics:packet.metrics};
}
async function fatalCase(native, action) {
  const crashed=await startApp(); await crashed.command('arm'); crashed.child.stdin.write(action+'\n');
  let timeout; let outcome;
  try { outcome=await Promise.race([crashed.finish(),new Promise((_,reject)=>{timeout=setTimeout(()=>reject(Error(`${action} exit deadline`)),15000);})]); }
  finally { clearTimeout(timeout); }
  const stderrPath=join(root,`app-${crashed.child.pid}.stderr`), stderr=await readFile(stderrPath);
  if(action==='objc-crash') {
    assert.match(stderr.toString(),/Acme Objective-C background exception requested/,'Intended native exception trigger reached');
    assert.ok(['SIGABRT','SIGSEGV'].includes(outcome.signal),JSON.stringify(outcome));
    if(outcome.signal==='SIGSEGV') report.gaps.push('Objective-C exception trigger exits with SIGSEGV on this host; selected OS evidence shows native exception unwinding, not a certified healthy unwinder or a normalized SIGABRT');
  } else assert.ok((action==='cpp-crash'?['SIGABRT']:['SIGILL','SIGTRAP','SIGABRT']).includes(outcome.signal),JSON.stringify(outcome));
  const path=join(root,`${action}-exit.json`);
  await writeFile(path,JSON.stringify({schemaVersion:1,records:[{kind:'failure',name:'Acme native process exit',data:{pid:crashed.child.pid,signal:outcome.signal,code:outcome.code,action,observation:'owned child close; not an OS crash report'}}]}),{flag:'wx',mode:0o600});
  const cid=await collect(native,`${action}-exit`,{kind:'import',path,format:'testrun-events-v1'});
  const failure=await query(native,cid,{kind:'failure',limit:10});
  assert.equal(failure.rows.length,1); assert.equal(failure.rows[0].record.data.signal,outcome.signal);
  let stderrCollector;
  if(stderr.length) stderrCollector=await collect(native,`${action}-stderr`,{kind:'import',path:stderrPath,format:'utf8'});
  report.cases.push({name:action,exit:outcome,exitCollector:cid,stderrCollector,rawSha256:failure.rawSha256,meaning:'actual owned app exit observed via normalized event and optional raw stderr retained through Testrun; not an OS IPS/report transfer or symbolication pass'});
}
try {
  report.identity.capabilities=await call(['diagnostics','capabilities']);
  for(const kind of ['host_process','native_trace']) assert.ok(report.identity.capabilities.sources.some(source=>source.kind===kind),`Selected CLI lacks ${kind}; build the current CLI before live acceptance`);
  if(selectedCase==='ips') {
    assert.ok(isAbsolute(selectedIPS??'')&&/^\d+$/.test(selectedIPSPid??''),'Select one exact owned IPS and its observed PID');
    const text=await readFile(selectedIPS,'utf8'); assert.ok(Buffer.byteLength(text)<=8388608);
    const body=JSON.parse(text.slice(text.indexOf('\n')+1));
    assert.equal(body.pid,Number(selectedIPSPid)); assert.equal(body.procName,'AcmeDiagnostics');
    const native=await open('host:macos','ips'); const cid=await collect(native,'owned-ips',{kind:'import',path:selectedIPS,format:'apple-ips'});
    const failures=await query(native,cid,{kind:'failure',limit:100});
    assert.ok(failures.rows.length>0); assert.match(JSON.stringify(failures),/SIGSEGV/);
    const stacks=await call(['diagnostics','query',native,failures.analysisId,'--filters',JSON.stringify({kind:'stack',limit:100})]);
    assert.match(JSON.stringify(stacks),/objc_exception_throw/);
    report.cases.push({name:'real-owned-ips',pid:body.pid,exception:body.exception,termination:body.termination,stackRows:stacks.rows.length,meaning:'selected actual OS report parsed through Testrun; producer symbols, not independent dSYM verification'}); report.state='passed';
  } else if(selectedCase.startsWith('sample-probe')) {
    const owner=await startApp(), before=performance.now();
    let probe, toolExit;
    const task=execute('/usr/bin/sample',[String(owner.child.pid),'1.000','10',...(selectedCase==='sample-probe'?['-mayDie']:[]),'-file',join(root,'sample.native')],{timeout:20000,maxBuffer:4194304});
    task.child.once('exit',(code,signal)=>{toolExit={code,signal,elapsedMs:performance.now()-before};});
    try { const value=await task; probe={outcome:'tool_completed',stdout:value.stdout,stderr:value.stderr}; }
    catch(error) { probe={outcome:'tool_failed',code:error.code,signal:error.signal,killed:error.killed,stdout:error.stdout??'',stderr:error.stderr??''}; }
    const elapsedMs=performance.now()-before;
    await writeFile(join(root,'sample.stdout'),probe.stdout,{flag:'wx',mode:0o600});
    await writeFile(join(root,'sample.stderr'),probe.stderr,{flag:'wx',mode:0o600});
    report.cases.push({name:'diagnostic-only-direct-sample',elapsedMs,toolExit,outcome:probe.outcome,code:probe.code,signal:probe.signal,killed:probe.killed,stdoutBytes:Buffer.byteLength(probe.stdout),stderr:probe.stderr,toolSha256:sha(await readFile('/usr/bin/sample')),meaning:'direct tool diagnosis with a 20-second bound; not a Testrun collector pass'});
    await owner.command('quit'); assert.equal((await owner.finish()).code,0);
    report.state='diagnostic_completed';
  } else if(selectedCase==='objc'||selectedCase==='faults') {
    const actions=selectedCase==='objc'?['objc-crash']:['swift-crash','objc-crash','cpp-crash','background-crash'];
    const control=await startApp(); await control.command('corrected-on'); await control.command('arm');
    for(const action of actions) { await control.command('arm'); assert.equal((await control.command(action)).status,'Corrected control ready'); }
    await control.command('quit'); assert.equal((await control.finish()).code,0);
    const native=await open('host:macos','native'); for(const action of actions) await fatalCase(native,action);
    report.state='passed';
  } else {
  const owner=await startApp(), target=`process:Acme:${owner.child.pid}`;
  const host=await open(target,'host'), native=await open('host:macos','native');
  const baseline=await snapshot(owner,host,'baseline');
  report.identity.processGeneration=baseline.generation;
  const retained=await owner.command('retain'); assert.equal(retained.retainedBytes,1048576);
  const held=await snapshot(owner,host,'held',baseline.generation);
  const reset=await owner.command('reset'); assert.equal(reset.retainedBytes,0);
  const released=await snapshot(owner,host,'released',baseline.generation);
  report.cases.push({name:'live-process-memory',baseline,held,released,fixtureBytes:[retained.retainedBytes,reset.retainedBytes],meaning:'fixture byte ownership differs from process RSS; release does not require RSS to fall'});
  const definition='Acme Swift fixture-owned Data byte lengths; not a heap census';
  for(const [mode,observed] of [['defective',retained],['corrected',await owner.command('corrected-on').then(()=>owner.command('retain'))]]) {
    assert.equal(observed.retainedBytes,mode==='defective'?1048576:0);
    const path=join(root,`${mode}.json`);
    await writeFile(path,JSON.stringify({schemaVersion:1,records:[{kind:'metric',name:'Acme retained Data',value:observed.retainedBytes,unit:'bytes',definition,data:{pid:owner.child.pid,processGeneration:baseline.generation}}]}),{flag:'wx',mode:0o600});
    const cid=await collect(host,mode,{kind:'import',path,format:'testrun-events-v1'});
    const result=await query(host,cid,{view:'assertions',assertions:{checks:[{id:'released',domain:'performance',name:'Acme retained Data',predicate:{type:'metric',unit:'bytes',definition,max:0,aggregation:'max'}}]}},{observationComplete:true});
    assert.equal(result.outcome,mode==='defective'?'assertion_failed':'passed',JSON.stringify(result));
    report.cases.push({name:`same-condition-${mode}`,outcome:result.outcome,rawSha256:result.rawSha256});
  }
  const source={kind:'native_trace',backend:'stack_sample',mode:'capture',targetId:'host:macos',appId:'com.example.acme.diagnostics.macos',processId:owner.child.pid,allowAttach:true,durationMs:1000};
  // Same app and control path; action time is measured inside the app, not CLI admission.
  const pairs=[];
  for(let n=0;n<3;n++) {
    const order=n%2===0?['baseline','sampled']:['sampled','baseline'], pair={};
    for(const mode of order) {
      let capture;
      if(mode==='sampled') capture=collect(native,`sample-${n}`,source).then(value=>({value}),error=>({error}));
      const times=[];
      for(let k=0;k<4;k++) { const receipt=await owner.command('cpu'); assert.equal(receipt.status,'4950'); times.push(receipt.actionNanoseconds); }
      if(capture) {
        const result=await capture; if(result.error) throw result.error; const cid=result.value;
        const stacks=await query(native,cid,{kind:'stack',limit:100}); assert.ok(stacks.rows.length>0,'Actual native stack rows');
        pair.capture={collectorId:cid,rawSha256:stacks.rawSha256,stackRows:stacks.rows.length};
      }
      pair[mode]=times;
    }
    pairs.push(pair);
  }
  report.cases.push({name:'matched-capture-action-observations',pairs,meaning:'short corrected CPU control; app action nanoseconds include NSButton.performClick/AppKit processing; native attach overlap with each action is not witnessed, so no causal overhead percentage or latency budget is established'});
  for(const action of ['swift-crash','objc-crash','cpp-crash','hang','background-crash']) { await owner.command('arm'); const value=await owner.command(action); assert.equal(value.status,'Corrected control ready'); }
  report.cases.push({name:'corrected-fatal-hang-controls',passed:true});
  await owner.command('corrected-off'); await owner.command('arm'); await owner.command('hang');
  const hang=await collect(native,'hang',source), stacks=await query(native,hang,{kind:'stack',limit:100});
  const texts=stacks.rows.map(row=>row.record.data.observation.text);
  const mainStart=texts.findIndex(text=>text.includes('Thread_')&&text.includes('com.apple.main-thread'));
  assert.ok(mainStart>=0,'Observed main-thread group');
  const nextThread=texts.findIndex((text,index)=>index>mainStart&&text.includes('Thread_'));
  const mainThread=texts.slice(mainStart,nextThread<0?undefined:nextThread).join('\n');
  assert.match(mainThread,/in AcmeDiagnostics/,'Owned app image in the main-thread call graph');
  assert.match(mainThread,/NSThread sleepForTimeInterval/,'Actual main-thread sleep; helper can be inlined');
  const responsive=await owner.command('cpu'); assert.equal(responsive.status,'704982704');
  report.cases.push({name:'native-hang-capture-and-recovery',rawSha256:stacks.rawSha256,stackRows:stacks.rows.length,recovered:true});
  await owner.command('quit'); assert.equal((await owner.finish()).code,0);
  const reborn=await startApp(); const host2=await open(`process:Acme:${reborn.child.pid}`,'relaunch');
  const next=await snapshot(reborn,host2,'new-birth'); assert.notEqual(next.generation,baseline.generation);
  report.cases.push({name:'relaunch-new-native-generation',passed:true});
  await reborn.command('quit'); assert.equal((await reborn.finish()).code,0);
  for(const action of ['swift-crash','objc-crash','cpp-crash','background-crash']) await fatalCase(native,action);
  assert.equal(sha(await readFile(executable)),report.identity.appSha256,'Fixture build unchanged');
  report.state='passed';
  }
} catch(error) { report.state='failed';report.error=String(error.stack??error);process.exitCode=1; }
finally {
  cleanup=true;
  for(const child of children) { if(child.exitCode===null&&child.signalCode===null) child.kill('SIGTERM'); }
  for(const child of children) { await Promise.race([new Promise(resolve=>child.once('close',resolve)),delay(3000)]); if(child.exitCode===null&&child.signalCode===null) child.kill('SIGKILL'); }
  report.cleanup.appChildrenAlive=[...children].filter(child=>child.exitCode===null&&child.signalCode===null).length;
  const errors=[];
  for(const id of investigations) {
    try { await call(['diagnostics','close',id]);await settle(id);await call(['diagnostics','remove',id]); }
    catch(error){errors.push(String(error));}
  }
  try { const result=await call(['manager','stop']);report.cleanup.manager=result; } catch(error){errors.push(String(error));}
  report.cleanup.errors=errors;
  async function retainedFiles(directory) {
    const entries=await readdir(directory,{withFileTypes:true}).catch(error=>error.code==='ENOENT'?[]:Promise.reject(error));
    let count=0;
    for(const entry of entries) count+=entry.isDirectory()?await retainedFiles(join(directory,entry.name)):1;
    return count;
  }
  report.cleanup.remainingDiagnosticFiles=await retainedFiles(join(root,'state/diagnostics'));
  if(errors.length||report.cleanup.appChildrenAlive||report.cleanup.remainingDiagnosticFiles) {report.state='failed';process.exitCode=1;}
  report.gaps.push('Automatic OS report discovery/creation/transfer, independent symbolication, full heap proof, system-wide capture and GUI input automation remain unverified; ips mode checks one explicitly selected existing report','Capture-overhead attribution/tails remain unverified; all-mode shared-host action pairs have uncertain per-action attach overlap and do not certify them');
  await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  console.log(JSON.stringify({state:report.state,report:reportPath,cases:report.cases.map(value=>value.name),cleanup:report.cleanup,error:report.error}));
}
