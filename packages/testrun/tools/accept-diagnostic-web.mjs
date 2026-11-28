// Owned loopback fixture, isolated Chrome and public Testrun CLI only.
import assert from 'node:assert/strict';
import {execFile, spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {createHash, randomUUID} from 'node:crypto';
import {createServer} from 'node:http';
import {createServer as portReservation} from 'node:net';
import {appendFile, link, lstat, mkdir, readFile, readdir, writeFile} from 'node:fs/promises';
import {dirname, extname, isAbsolute, join, relative, resolve} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {frameworks} from './prepare-diagnostic-web.mjs';
import {agentWorkflow} from './diagnostic-agent-workflow.mjs';

const [selectedCLI, project, cache, root, pnpm] = process.argv.slice(2);
for(const path of [selectedCLI,project,cache,root,pnpm]) assert.ok(path&&isAbsolute(path),'Select absolute CLI, prepared project, browser cache, new evidence root and pnpm');
assert.equal(process.platform,'darwin','This selected browser acceptance scope is macOS');
const owner=JSON.parse(await readFile(join(dirname(project),'acme-fixture-owner.json'),'utf8'));
const definition=frameworks[owner.framework]; assert.ok(definition,'Registered Acme fixture required');
await mkdir(root,{mode:0o700});
const reportPath=join(root,'acceptance.json'); await writeFile(reportPath,'{"state":"incomplete"}\n',{flag:'wx',mode:0o600});
const cli=join(root,'testrun'); await link(selectedCLI,cli);
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const execute=promisify(execFile), environment={...process.env,TESTRUN_DATA_ROOT:join(root,'state'),TESTRUN_BROWSER_ROOT:cache};
for(const key of ['TESTRUN_TASK_ID','CODEX_THREAD_ID']) delete environment[key];
const report={schemaVersion:1,state:'running',framework:owner.framework,scope:{engine:'chrome',mode:'headless',host:process.platform,architecture:process.arch,server:'owned loopback production build',workBudgetMs:300000},identity:{cliSha256:sha(await readFile(cli)),manifest:JSON.parse(await readFile(join(project,'package.json'),'utf8')),lockSha256:sha(await readFile(join(project,'pnpm-lock.yaml')))},cases:[],cost:[],cleanup:{},gaps:['One selected headless Chrome/host/dependency graph; no other engine/build-mode or causal overhead claim','Fixture buffer byte counts are logical ownership, not heap census or RSS','Production-build scope; HMR, SSR server faults and independent source-map symbolication not certified']};
const deadline=Date.now()+300000; let cleanup=false, logBytes=0, target, investigation, server, serverChild, serverExited, port, url;
const nativeEvidence=[];
async function call(args) {
  if(!cleanup) assert.ok(Date.now()<deadline,'Acceptance work budget');
  const before=performance.now(); const {stdout}=await execute(cli,['--json',...args],{cwd:root,env:environment,timeout:40000,maxBuffer:2097152});
  const result=JSON.parse(stdout), wallMs=performance.now()-before;
  const line=JSON.stringify({args,result,wallMs})+'\n'; logBytes+=Buffer.byteLength(line); assert.ok(logBytes<=16777216,'Private operation log bound');
  await appendFile(join(root,'operations.jsonl'),line,{mode:0o600}); return result;
}
async function operation(request, failure=false) {
  const id=randomUUID(); await call(['browser','send','--target',target,'--operation-id',id,JSON.stringify(request)]);
  const status=await call(['browser','status','--target',target,'--operation-id',id,'--wait-ms','30000']);
  assert.equal(status.operation.settled,true,JSON.stringify(status));
  if(!failure) assert.equal(status.operation.outcome,'succeeded',JSON.stringify(status.operation));
  report.cost.push({operation:request.operation,runWallMs:status.operation.runWallMs});
  return {id,result:status.operation.result,operation:status.operation,request};
}
async function settle(analysis) {
  const until=Date.now()+20000;
  while(Date.now()<until) {
    const value=await call(analysis?['diagnostics','analysis-status',investigation,analysis]:['diagnostics','status',investigation]);
    if(value.settled) return value;
    await delay(50);
  }
  throw Error('Diagnostic settlement deadline');
}
async function retain(submission, source) {
  const value=await call(['diagnostics','start',investigation,'--submission-id',submission,'--source',JSON.stringify(source),'--limits',JSON.stringify({durationMs:15000,maxBytes:8388608,maxEvents:4096})]);
  const cid=value.collectors.at(-1).collectorId, settled=await settle();
  const record=settled.collectors.find(item=>item.collectorId===cid);
  assert.equal(record.phase,source.kind==='browser_operation'?'partial':'complete',JSON.stringify(record));
  assert.equal(record.reason,source.kind==='browser_operation'?'browser_operation_snapshot':'source_complete');
  assert.ok(record.bytes>0&&record.sha256?.length===64);
  return {cid,sha256:record.sha256};
}
async function analyze(cid,filters,metadata) {
  const args=['diagnostics','analyze',investigation,cid,'--submission-id',`parse-${cid}`]; if(metadata) args.push('--metadata',JSON.stringify(metadata));
  const parsed=await call(args), status=await settle(parsed.analysisId); assert.equal(status.phase,'complete',JSON.stringify(status));
  return call(['diagnostics','query',investigation,parsed.analysisId,'--filters',JSON.stringify(filters)]);
}
async function listen(instance) { await new Promise((accept,reject)=>{instance.once('error',reject);instance.listen(0,'127.0.0.1',accept);}); return instance.address().port; }
async function startServer() {
  const staticRoot=join(project,'dist');
  server=createServer(async(req,res)=>{
    try {
      const path=new URL(req.url,'http://127.0.0.1').pathname;
      if(url) res.setHeader('Access-Control-Allow-Origin',new URL(url).origin);
      if(req.method!=='GET') {res.writeHead(405).end();return;}
      if(path==='/acme-fault'||path==='/acme-control') {res.setHeader('Content-Type','application/json');res.writeHead(path==='/acme-fault'?503:200).end(JSON.stringify({state:path==='/acme-fault'?'failed':'ready'}));return;}
      if(!['vite','astro'].includes(definition.builder)) {res.writeHead(404).end();return;}
      const file=resolve(staticRoot,`.${decodeURIComponent(path==='/'?'/index.html':path)}`);
      assert.ok(!relative(staticRoot,file).startsWith('..'));
      const stat=await lstat(file); assert.ok(stat.isFile()&&!stat.isSymbolicLink()&&stat.size<=8388608);
      const bytes=await readFile(file); assert.ok(bytes.length<=8388608);
      const mime={'.html':'text/html','.js':'text/javascript','.css':'text/css','.json':'application/json','.svg':'image/svg+xml','.woff2':'font/woff2'}[extname(file)]??'application/octet-stream';
      res.setHeader('Content-Type',mime);res.writeHead(200).end(bytes);
    } catch {res.writeHead(404).end('Acme fixture resource unavailable');}
  });
  const apiPort=await listen(server); report.apiOrigin=`http://127.0.0.1:${apiPort}`;
  if(['vite','astro'].includes(definition.builder)) {port=apiPort;url=report.apiOrigin;return;}
  const reservation=portReservation(); port=await listen(reservation); await new Promise(accept=>reservation.close(accept));
  url=`http://127.0.0.1:${port}`;
  let output='';
  serverChild=spawn(pnpm,['run','preview','--port',String(port)],{cwd:project,detached:true,env:{...process.env,NO_COLOR:'1',PORT:String(port),NITRO_PORT:String(port),NITRO_HOST:'127.0.0.1',NEXT_TELEMETRY_DISABLED:'1',NUXT_TELEMETRY_DISABLED:'1'},stdio:['ignore','pipe','pipe']});
  serverExited=new Promise(accept=>{serverChild.once('error',error=>accept({error:String(error)}));serverChild.once('close',(code,signal)=>accept({code,signal}));});
  for(const stream of [serverChild.stdout,serverChild.stderr]) stream.on('data',chunk=>{output+=chunk.toString();if(Buffer.byteLength(output)>524288) serverChild.kill();});
  const until=Date.now()+60000;
  while(Date.now()<until) {
    if(serverChild.exitCode!==null||serverChild.signalCode!==null) throw Error(`Fixture server exited: ${output.slice(-4096)}`);
    if(output.includes(`:${port}`)) {try {if((await fetch(url,{signal:AbortSignal.timeout(1000)})).ok) {await writeFile(join(root,'server.log'),output,{mode:0o600});return;}}catch{}}
    await delay(100);
  }
  throw Error(`Owned fixture server readiness deadline: ${output.slice(-4096)}`);
}
try {
  const doctor=await call(['browser','doctor','--engine','chrome']); report.identity.browser=doctor;
  assert.equal(doctor.hostSupported,true); assert.equal(doctor.downloadBytes,0,'Select an already prepared pinned browser cache');
  await startServer(); report.url=url;
  const connected=await call(['connect','--engine','chrome','--mode','headless']); target=connected.targetId; assert.ok(target?.startsWith('browser:'));
  let status;
  const readyUntil=Date.now()+120000;
  while(Date.now()<readyUntil) {status=await call(['browser','status','--target',target]);assert.ok(!status.failure,JSON.stringify(status));if(status.browser?.phase==='ready')break;await delay(100);}
  assert.equal(status.browser.phase,'ready',JSON.stringify(status));
  let page=status.browser.selectedPage; const context=status.browser.selectedContext;
  assert.ok(page?.id&&context?.id);
  const started=await operation({operation:'diagnostics_start',context}), cursor=started.result.latestSequence; assert.ok(Number.isInteger(cursor),JSON.stringify(started.result));
  await operation({operation:'page_navigate',page,navigation:{kind:'goto',url,waitUntil:'load'}});
  async function state(expected) {
    const until=Date.now()+10000; let value;
    while(Date.now()<until) {
      const read=await operation({operation:'page_read',page,read:'text',locator:{kind:'css',selector:'output'},maxBytes:4096});
      try {value=JSON.parse(read.result.result.value);}catch{}
      if(value&&(!expected||expected(value)))return value;
      await delay(25);
    }
    throw Error(`Fixture state deadline: ${JSON.stringify(value)}`);
  }
  const click=title=>operation({operation:'page_action',page,locator:{kind:'role',role:'button',name:title},action:{kind:'click'}});
  const check=(text,value)=>operation({operation:'page_action',page,locator:{kind:'label',text},action:{kind:'check',checked:value}});
  const initial=await state(value=>value.status==='Acme ready'); assert.equal(initial.corrected,false);
  await click('Caught exception'); await state(value=>value.status.includes('Acme JavaScript fixture failure'));
  await click('Retain one MiB'); const defective=await state(value=>value.status==='retain: 1048576');
  await click('Reset'); await state(value=>value.status==='Acme reset'&&value.frames===0);
  await check('Corrected mode',true); await click('Retain one MiB'); const corrected=await state(value=>value.status==='retain: 0');
  await click('Caught exception'); await state(value=>value.status==='exception: "ready"');
  report.cases.push({name:'actual-ui-failure-retention-reset-correction',defective,corrected});
  await check('Corrected mode',false); await click('Frame-state churn'); await state(value=>value.frames===0);
  await check('Arm uncaught JavaScript error',true); await click('Uncaught JavaScript error');
  await operation({operation:'diagnostics_wait',context,after:cursor,conditions:[{event:'page_error',page}]});
  const history=await operation({operation:'diagnostics_read',context,query:{kind:'page_error',page,includeText:true,includeStack:true,limit:20}});
  assert.match(JSON.stringify(history.result),/Acme JavaScript fixture failure/);
  const opened=await call(['diagnostics','open','--submission-id','web','--question','Verify owned Acme web fixture','--binding',JSON.stringify({targetId:target})]); investigation=opened.investigationId;
  const source={kind:'browser_operation',targetId:target,operationId:history.id,request:history.request};
  const retained=await retain('error-history',source), failures=await analyze(retained.cid,{kind:'failure',limit:100}); assert.ok(failures.rows.length>0);
  report.cases.push({name:'real-browser-error-retained-through-investigation',sha256:retained.sha256,failures:failures.rows.length});
  const measured={};
  for(const [mode,observed] of [['defective',defective],['corrected',corrected]]) {
    const value=Number(observed.status.split(': ')[1]); assert.ok(Number.isFinite(value));
    const path=join(root,`${mode}.json`), definition='Acme fixture output retained Uint8Array byte lengths; not a heap census';
    await writeFile(path,JSON.stringify({schemaVersion:1,records:[{kind:'metric',name:'Acme retained buffers',value,unit:'bytes',definition,data:{observedOutput:mode==='defective'?defective:corrected}}]}),{flag:'wx',mode:0o600});
    const copied=await retain(mode,{kind:'import',path,format:'testrun-events-v1'});
    const assertion=await analyze(copied.cid,{view:'assertions',assertions:{checks:[{id:'released',domain:'performance',name:'Acme retained buffers',predicate:{type:'metric',unit:'bytes',definition,max:0,aggregation:'max'}}]}},{observationComplete:true});
    assert.equal(assertion.outcome,mode==='defective'?'assertion_failed':'passed'); report.cases.push({name:`same-condition-${mode}`,outcome:assertion.outcome}); measured[mode]=assertion.analysisId;
  }
  const network=await operation({operation:'evaluate',scope:page,isFunction:true,expression:`async()=>{const failure=await fetch(${JSON.stringify(report.apiOrigin+'/acme-fault')});const control=await fetch(${JSON.stringify(report.apiOrigin+'/acme-control')});return [failure.status,control.status];}`});
  assert.equal(network.result.value.kind,'array'); assert.deepEqual(network.result.value.items.map(item=>{assert.equal(item.kind,'json');return item.value;}),[503,200]);
  await operation({operation:'diagnostics_wait',context,after:cursor,conditions:[{event:'response',page,status:503,url:report.apiOrigin+'/acme-fault'}]});
  report.cases.push({name:'actual-loopback-http-fault-and-control',statuses:[503,200],meaning:'owned HTTP endpoints; fixture network button remains explicitly simulated'});
  const profileOptions={cpu:true,javascriptCoverage:true,cssCoverage:false,samplingIntervalUs:10000};
  report.scope.profileOptions=profileOptions;
  await operation({operation:'profile_start',page,options:profileOptions});
  await operation({operation:'evaluate',scope:page,isFunction:true,expression:"()=>{function find(root){const button=[...root.querySelectorAll('button')].find(item=>item.textContent==='Bounded CPU work');if(button)return button;for(const element of root.querySelectorAll('*'))if(element.shadowRoot){const nested=find(element.shadowRoot);if(nested)return nested;}}const button=find(document);if(!button)throw Error('Acme CPU control missing');for(let n=0;n<128;n++)button.click();return 128;}"});
  const profile=await operation({operation:'profile_stop',page});
  await state(value=>value.status==='expensive: 704982704');
  for(const artifact of profile.result.reports) nativeEvidence.push(artifact.evidenceId);
  const cpu=profile.result.reports.find(item=>item.format==='v8-cpu-profile'); assert.ok(cpu);
  const cpuFile=join(root,'browser.cpuprofile'), cpuBytes=await readFile(cpu.attachment.path);
  assert.equal(cpuBytes.length,cpu.attachment.bytes); assert.equal(sha(cpuBytes),cpu.attachment.sha256);
  await writeFile(cpuFile,cpuBytes,{flag:'wx',mode:0o600});
  const payload=JSON.parse(cpuBytes); assert.ok(payload.nodes.length>0&&payload.endTime>payload.startTime);
  report.cpuProfile={nodes:payload.nodes.length,samples:payload.samples.length,startTime:payload.startTime,endTime:payload.endTime,sha256:sha(cpuBytes)};
  const cpuCopy=await retain('cpu-profile',{kind:'import',path:cpuFile,format:'js-cpu-profile'}), cpuRows=await analyze(cpuCopy.cid,{kind:'stack',limit:20}); assert.ok(cpuRows.rows.length>0);
  report.cases.push({name:'actual-browser-cpu-profile-import',sha256:cpuCopy.sha256,stackRows:cpuRows.rows.length});
  for(const evidenceId of nativeEvidence) await operation({operation:'developer_evidence_remove',evidenceId}); nativeEvidence.length=0;
  await operation({operation:'page_navigate',page,navigation:{kind:'reload',waitUntil:'load'}}); await state(value=>value.status==='Acme ready'&&!value.corrected&&!value.armed&&value.frames===0);
  report.cases.push({name:'reload-fresh-fixture-state',passed:true});
  await operation({operation:'diagnostics_stop',context});
  const disconnected=await call(['browser','disconnect','--target',target]); assert.equal(disconnected.closed,true,JSON.stringify(disconnected)); target=null;
  const replay=await call(['diagnostics','start',investigation,'--submission-id','error-history','--source',JSON.stringify(source),'--limits',JSON.stringify({durationMs:15000,maxBytes:8388608,maxEvents:4096})]); assert.equal(replay.collectors[0].collectorId,retained.cid);
  const preserved=await call(['diagnostics','query',investigation,failures.analysisId,'--filters',JSON.stringify({kind:'failure',limit:100})]); assert.ok(preserved.rows.length>0);
  report.cases.push({name:'retained-replay-and-query-after-browser-close',passed:true});
  const agent=await agentWorkflow({cli,cwd:root,env:environment,record:value=>appendFile(join(root,'operations.jsonl'),JSON.stringify(value)+'\n',{mode:0o600}),root,investigation,primary:failures.analysisId,baseline:measured.defective,corrected:measured.corrected,platform:'browser',preset:'crash'});
  report.cases.push({name:'agent-workflow-over-mcp',...agent}); report.state='passed';
} catch(error) {report.state='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally {
  cleanup=true; const errors=[];
  if(target) {try{await call(['browser','disconnect','--target',target]);}catch(error){errors.push(String(error));}}
  if(investigation) {try{await call(['diagnostics','close',investigation]);await settle();await call(['diagnostics','remove',investigation]);}catch(error){errors.push(String(error));}}
  try{report.cleanup.manager=await call(['manager','stop']);}catch(error){errors.push(String(error));}
  if(serverChild) {
    try{process.kill(-serverChild.pid,'SIGTERM');}catch(error){if(error.code!=='ESRCH')errors.push(String(error));}
    let timeout;try{report.cleanup.server=await Promise.race([serverExited,new Promise((_,reject)=>{timeout=setTimeout(()=>reject(Error('Owned server exit deadline')),5000);})]);}catch(error){try{process.kill(-serverChild.pid,'SIGKILL');}catch{}errors.push(String(error));}finally{clearTimeout(timeout);}
  }
  if(server) {server.closeAllConnections();await new Promise(accept=>server.close(accept));report.cleanup.loopbackServerClosed=!server.listening;}
  async function files(directory){const entries=await readdir(directory,{withFileTypes:true}).catch(error=>error.code==='ENOENT'?[]:Promise.reject(error));let count=0;for(const entry of entries)count+=entry.isDirectory()?await files(join(directory,entry.name)):1;return count;}
  report.cleanup.remainingDiagnosticFiles=await files(join(root,'state/diagnostics'));report.cleanup.errors=errors;
  if(errors.length||report.cleanup.remainingDiagnosticFiles){report.state='failed';process.exitCode=1;}
  await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600}); console.log(JSON.stringify({state:report.state,framework:report.framework,report:reportPath,cases:report.cases.map(value=>value.name),cleanup:report.cleanup,error:report.error}));
}
