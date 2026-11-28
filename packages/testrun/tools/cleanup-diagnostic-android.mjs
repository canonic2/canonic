// Retry only a retained acceptance run's owned release. Never kills the ADB server.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {execFile} from 'node:child_process';
import {readFile, readdir, writeFile} from 'node:fs/promises';
import {isAbsolute, join, resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {promisify} from 'node:util';

export async function cleanupDiagnosticAndroid(root,sdk,javaHome){
for(const path of [root,sdk,javaHome])assert.ok(path&&isAbsolute(path),'Select absolute retained run root, SDK and JDK');
const path=join(root,'acceptance.json'),report=JSON.parse(await readFile(path,'utf8'));
assert.equal(report.schemaVersion,1);assert.equal(report.scope.image,'system-images;android-36;google_apis_playstore;x86_64');
const name=report.identity.avd;assert.match(name,/^Testrun-QA-Diagnostics-[a-f0-9-]{36}$/);
const cli=join(root,'testrun'),avds=join(root,'avd'),adb=join(sdk,'platform-tools/adb');
assert.equal(createHash('sha256').update(await readFile(cli)).digest('hex'),report.identity.cliSha256,'Retained CLI identity');
const entries=await readdir(avds);assert.ok(entries.every(entry=>[`${name}.ini`,`${name}.avd`].includes(entry)),'Only this run owns the selected AVD set');
const env={...process.env,TESTRUN_DATA_ROOT:join(root,'state'),ANDROID_AVD_HOME:avds,ANDROID_HOME:sdk,JAVA_HOME:javaHome,TESTRUN_ADB:adb};
delete env.TESTRUN_TASK_ID;delete env.CODEX_THREAD_ID;
const execute=promisify(execFile),steps=[];
async function run(program,args){const value=await execute(program,args,{env,cwd:root,timeout:60000,maxBuffer:1048576});steps.push({program,args,stdout:value.stdout,stderr:value.stderr});return value.stdout;}
async function call(args){return JSON.parse(await run(cli,['--json',...args]));}
try{
  const closed=await call(['disconnect','--target',`android:avd:${name}`]);assert.equal(closed.closed,true);
  // A refused serial-based install can leave a separate empty pool entry.
  if(report.identity.target){assert.match(report.identity.target,/^android:emulator-\d+$/);assert.equal((await call(['disconnect','--target',report.identity.target])).closed,true);}
  const manager=await call(['manager','stop']);assert.equal(manager.retired,true,JSON.stringify(manager));
  const devices=await run(adb,['devices','-l']);if(report.identity.target)assert.ok(!devices.split('\n').some(line=>line.split(/\s+/)[0]===report.identity.target.slice(8)),'Owned emulator released');
  if(entries.length)await run(join(sdk,'cmdline-tools/latest/bin/avdmanager'),['delete','avd','--name',name]);
  assert.deepEqual(await readdir(avds),[]);
  const recovery={complete:true,closed:true,manager,emulatorAbsentFromADB:true,remainingAVDFiles:0,steps};
  // Preserve the failed acceptance outcome and its first cleanup failure.
  report.cleanup.recovery=recovery;await writeFile(path,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  console.log(JSON.stringify({complete:true,report:path,remainingAVDFiles:0,emulatorAbsentFromADB:true,manager}));
}catch(error){
  report.cleanup.recovery={complete:false,error:String(error),steps};await writeFile(path,JSON.stringify(report,null,2)+'\n',{mode:0o600});throw error;
}
}

if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  await cleanupDiagnosticAndroid(...process.argv.slice(2));
}
