// Replay a real retained RN packet stream; no Metro, emulator or app input.
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {createHash} from 'node:crypto';
import {link,mkdir,readFile,writeFile} from 'node:fs/promises';
import {isAbsolute,join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {promisify} from 'node:util';
const [selectedCLI,input,root]=process.argv.slice(2);
for(const path of [selectedCLI,input,root])assert.ok(path&&isAbsolute(path));
const bytes=await readFile(input);assert.ok(bytes.length>0&&bytes.length<=8388608);
const packets=bytes.toString().trim().split('\n').map(line=>JSON.parse(line));
assert.ok(packets.some(p=>p.kind==='probe'&&p.data.hermes===true),'Actual retained Hermes probe required');
await mkdir(root,{mode:0o700});const cli=join(root,'testrun'),path=join(root,'input.ndjson');await link(selectedCLI,cli);await writeFile(path,bytes,{flag:'wx',mode:0o600});
const env={...process.env,TESTRUN_DATA_ROOT:join(root,'state')};delete env.TESTRUN_TASK_ID;delete env.CODEX_THREAD_ID;
const execute=promisify(execFile),report={state:'running',sourceSha256:createHash('sha256').update(bytes).digest('hex'),cleanup:{}};let id;
async function call(args){return JSON.parse((await execute(cli,['--json',...args],{cwd:root,env,timeout:45000,maxBuffer:8388608})).stdout);}
async function settle(analysis){const until=Date.now()+45000;do{const status=await call(analysis?['diagnostics','analysis-status',id,analysis]:['diagnostics','status',id]);if(status.settled)return status;await delay(100);}while(Date.now()<until);throw Error('Replay settlement deadline');}
try{
  id=(await call(['diagnostics','open','--submission-id','replay','--question','Verify retained real RN fixture query'])).investigationId;
  const started=await call(['diagnostics','start',id,'--submission-id','import','--source',JSON.stringify({kind:'import',path,format:'react-native-diagnostics-v1'})]);
  assert.equal((await settle()).collectors.at(-1).phase,'complete');
  const analysis=await call(['diagnostics','analyze',id,started.collectors.at(-1).collectorId,'--submission-id','parse']);assert.equal((await settle(analysis.analysisId)).phase,'complete');
  const result=await call(['diagnostics','query',id,analysis.analysisId,'--filters','{}','--limit','100']);
  await writeFile(join(root,'query.json'),JSON.stringify(result,null,2)+'\n',{mode:0o600});
  assert.ok(result.rows.some(row=>row.record.kind==='metric'&&row.record.name==='Acme diagnostics'),'Reported React commit must survive actual import/query');
  assert.ok(JSON.stringify(result).includes('Acme caught exception: Acme JavaScript fixture failure'));
  const failures=await call(['diagnostics','query',id,analysis.analysisId,'--filters',JSON.stringify({kind:'failure'}),'--limit','100']);
  assert.ok(failures.rows.some(row=>JSON.stringify(row.record).includes('Acme caught exception: Acme JavaScript fixture failure')),'Reported console.error must be queryable without inferring an uncaught crash');
  assert.ok(failures.rows.every(row=>row.record.data.causeEstablished===false&&row.record.data.appId===null));
  report.failureRows=failures.rows.length;
  report.rows=result.rows.length;report.state='passed';
}catch(error){report.state='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{
  try{if(id){await call(['diagnostics','close',id]);await settle();await call(['diagnostics','remove',id]);report.cleanup.investigationRemoved=true;}report.cleanup.manager=await call(['manager','stop']);assert.equal(report.cleanup.manager.retired,true);}catch(error){report.state='failed';report.cleanup.error=String(error);process.exitCode=1;}
  await writeFile(join(root,'replay.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify(report));
}
