import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFile} from 'node:child_process';
import {chmod, mkdir, mkdtemp, readFile, realpath, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {promisify} from 'node:util';

const execute=promisify(execFile),script=resolve(import.meta.dirname,'cleanup-diagnostic-android.mjs');
async function fixture(t){
  const root=await realpath(await mkdtemp(join(tmpdir(),'testrun-cleanup-guard-')));
  t.after(()=>rm(root,{recursive:true,force:true}));
  const bytes=Buffer.from('Acme non-executable CLI identity peer');
  await writeFile(join(root,'testrun'),bytes);await mkdir(join(root,'avd'));
  const report={schemaVersion:1,state:'failed',scope:{image:'system-images;android-36;google_apis_playstore;x86_64'},identity:{avd:'Testrun-QA-Diagnostics-01234567-0123-0123-0123-012345678901',cliSha256:createHash('sha256').update(bytes).digest('hex')},cleanup:{errors:['original failure']}};
  async function refuse(pattern){
    const original=JSON.stringify(report);await writeFile(join(root,'acceptance.json'),original);
    for(const entry of [[script],[resolve(import.meta.dirname,'accept-diagnostic-android.mjs'),'--cleanup']]){
      await assert.rejects(execute(process.execPath,[...entry,root,join(root,'sdk'),join(root,'jdk')],{timeout:10000,maxBuffer:1048576}),error=>pattern.test(error.stderr));
      assert.equal(await readFile(join(root,'acceptance.json'),'utf8'),original,'Ownership refusal precedes any cleanup/report mutation');
    }
  }
  return {root,report,refuse};
}
test('cleanup refuses non-fixture scope before executing a retained CLI',async t=>{
  const f=await fixture(t);f.report.scope.image='unselected system image';await f.refuse(/AssertionError/);
});
test('cleanup refuses a foreign AVD name before executing a retained CLI',async t=>{
  const f=await fixture(t);f.report.identity.avd='Personal-Phone';await f.refuse(/Testrun-QA-Diagnostics/);
});
test('cleanup refuses changed CLI bytes before any release',async t=>{
  const f=await fixture(t);await writeFile(join(f.root,'testrun'),'changed bytes');await f.refuse(/Retained CLI identity/);
});
test('cleanup refuses an AVD set containing another target',async t=>{
  const f=await fixture(t);await writeFile(join(f.root,'avd','Personal-Phone.ini'),'foreign');await f.refuse(/Only this run owns the selected AVD set/);
});

test('acceptance cleanup mode releases only the retained run and preserves its failure on repeat',async t=>{
  const f=await fixture(t),sdk=join(f.root,'sdk'),cli=join(f.root,'testrun');
  async function executable(path,body){await writeFile(path,`#!${process.execPath}\n${body}\n`);await chmod(path,0o700);}
  await executable(cli,`const fs=require('node:fs');const path=require('node:path');
const root=process.env.TESTRUN_DATA_ROOT;
if(root!==path.join(__dirname,'state'))throw Error('Foreign state root');
const args=process.argv.slice(2);
fs.appendFileSync(path.join(__dirname,'calls.jsonl'),JSON.stringify(args)+'\\n');
if(args[1]==='disconnect'&&args[3]==='android:avd:${f.report.identity.avd}')console.log(JSON.stringify({closed:true}));
else if(args[1]==='manager'&&args[2]==='stop')console.log(JSON.stringify({retired:true}));
else throw Error('Unexpected operation');`);
  f.report.identity.cliSha256=createHash('sha256').update(await readFile(cli)).digest('hex');
  await writeFile(join(f.root,'acceptance.json'),JSON.stringify(f.report));
  await mkdir(join(sdk,'platform-tools'),{recursive:true});
  await executable(join(sdk,'platform-tools/adb'),"if(process.argv.slice(2).join(' ')!=='devices -l')throw Error('Unexpected ADB input');console.log('List of devices attached\\n');");
  await mkdir(join(sdk,'cmdline-tools/latest/bin'),{recursive:true});
  await executable(join(sdk,'cmdline-tools/latest/bin/avdmanager'),`const fs=require('node:fs');const path=require('node:path');
const name='${f.report.identity.avd}';
if(process.argv.slice(2).join(' ')!=='delete avd --name '+name)throw Error('Foreign AVD');
fs.unlinkSync(path.join(process.env.ANDROID_AVD_HOME,name+'.ini'));`);
  await writeFile(join(f.root,'avd',f.report.identity.avd+'.ini'),'owned fixture');
  for(let n=0;n<2;n++){
    const result=await execute(process.execPath,[resolve(import.meta.dirname,'accept-diagnostic-android.mjs'),'--cleanup',f.root,sdk,join(f.root,'jdk')],{timeout:10000,maxBuffer:1048576});
    assert.equal(JSON.parse(result.stdout).complete,true);
    const report=JSON.parse(await readFile(join(f.root,'acceptance.json'),'utf8'));
    assert.equal(report.state,'failed');assert.deepEqual(report.cleanup.errors,['original failure']);
    assert.equal(report.cleanup.recovery.remainingAVDFiles,0);
  }
  assert.equal((await readFile(join(f.root,'calls.jsonl'),'utf8')).trim().split('\n').length,4,'Only release and manager retirement; no acceptance work');
});
