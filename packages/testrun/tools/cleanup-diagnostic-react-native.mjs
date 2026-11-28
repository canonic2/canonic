// Recovery checks the recorded process birth/group before signalling owned Metro.
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFile,writeFile} from 'node:fs/promises';
import {isAbsolute,join} from 'node:path';
import {promisify} from 'node:util';
import {setTimeout as delay} from 'node:timers/promises';
import {cleanupDiagnosticAndroid} from './cleanup-diagnostic-android.mjs';
import {profileForReport,reportFramework} from './diagnostic-react-native-app.mjs';
const execute=promisify(execFile);
async function groupExists(pid){
  const value=await execute('/bin/ps',['-ax','-o','pid=','-o','pgid='],{timeout:5000,maxBuffer:2097152});
  return value.stdout.trim().split('\n').some(line=>Number(line.trim().split(/\s+/)[1])===pid);
}
export async function metroWitness(pid){
  assert.ok(Number.isSafeInteger(pid)&&pid>1);
  try{return (await execute('/bin/ps',['-p',String(pid),'-o','pgid=','-o','lstart='],{timeout:5000,maxBuffer:65536})).stdout.trim().split(/\s+/).join(' ');}
  catch(error){if(error.code===1&&!error.stdout?.trim())return null;throw error;}
}
export async function stopOwnedMetro(metro){
  if(!metro)return;
  // Earlier records appended comm, which legitimately changes when the pnpm
  // shebang launcher execs Node. Preserve the recorded group and process birth.
  const recorded=metro.witness.trim().split(/\s+/).slice(0,6).join(' ');
  const witness=await metroWitness(metro.pid);
  if(witness!==null){assert.equal(witness,recorded,'Metro process birth/group must match the retained run');assert.equal(Number(witness.split(/\s+/)[0]),metro.pid,'Only a new owned process group');}
  else {
    // A missing leader cannot authenticate surviving/reused group members.
    if(!await groupExists(metro.pid))return;
    throw Error('Metro leader absent but group remains; ownership requires inspection');
  }
  process.kill(-metro.pid,'SIGTERM');
  const until=Date.now()+10000;
  while(Date.now()<until){if(!await groupExists(metro.pid))return;await delay(100);}
  assert.equal(await metroWitness(metro.pid),recorded,'Recheck Metro birth before forced cleanup');
  process.kill(-metro.pid,'SIGKILL');
  const forced=Date.now()+5000;
  while(Date.now()<forced){if(!await groupExists(metro.pid))return;await delay(100);}
  throw Error('Owned Metro group did not disappear');
}
/**
 * Release one retained iOS run: its simulator target, private manager and only the
 * devices in its own simulator set. Repeating after a partial release is safe.
 */
export async function cleanupOwnedSimulator(root,xcrun='/usr/bin/xcrun'){
  assert.ok(root&&isAbsolute(root));
  const path=join(root,'acceptance.json'),report=JSON.parse(await readFile(path,'utf8'));
  assert.equal(report.scope.platform,'ios');
  const cli=join(root,'testrun'),set=join(root,'simulators');
  assert.equal(createHash('sha256').update(await readFile(cli)).digest('hex'),report.identity.cliSha256,'Retained CLI identity');
  const env={...process.env,TESTRUN_DATA_ROOT:join(root,'state'),TESTRUN_SIMULATOR_DEVICE_SET:set};
  for(const key of ['TESTRUN_TASK_ID','CODEX_THREAD_ID','TESTRUN_XCRUN','TESTRUN_APPLE_PS','TESTRUN_CODESIGN','TESTRUN_ADB'])delete env[key];
  const steps=[];
  async function run(program,args){const value=await execute(program,args,{env,cwd:root,timeout:60000,maxBuffer:2097152});steps.push({program,args,stdout:value.stdout,stderr:value.stderr});return value.stdout;}
  async function call(args){return JSON.parse(await run(cli,['--json',...args]));}
  async function devices(){return Object.values(JSON.parse(await run(xcrun,['simctl','--set',set,'list','devices','--json'])).devices).flat();}
  try{
    let closed=null;
    if(report.identity.target){assert.match(report.identity.target,/^simulator:([a-f0-9]{64}:)?[A-Fa-f0-9-]{36}$/);if(report.identity.simulatorUUID)assert.ok(report.identity.target.endsWith(':'+report.identity.simulatorUUID));closed=(await call(['disconnect','--target',report.identity.target])).closed;assert.equal(closed,true);}
    const manager=await call(['manager','stop']);assert.equal(manager.retired,true,JSON.stringify(manager));
    // Discover the private set's contents even if create's reply was lost; never create again.
    const owned=await devices();assert.ok(owned.length<=1,'Only this run owns the private simulator set');
    for(const device of owned){
      assert.ok(device.name.startsWith('Testrun-QA-Diagnostics-'));if(report.identity.simulatorUUID)assert.equal(device.udid,report.identity.simulatorUUID);
      if(device.state!=='Shutdown')await run(xcrun,['simctl','--set',set,'shutdown',device.udid]);
      await run(xcrun,['simctl','--set',set,'delete',device.udid]);
    }
    assert.deepEqual(await devices(),[]);
    report.cleanup.recovery={complete:true,closed,manager,remainingSimulators:0,steps};await writeFile(path,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  }catch(error){report.cleanup.recovery={complete:false,error:String(error),steps};await writeFile(path,JSON.stringify(report,null,2)+'\n',{mode:0o600});throw error;}
}
export async function cleanupReactNative(root,sdk,javaHome,{xcrun}={}){
  assert.ok(root&&isAbsolute(root));
  const path=join(root,'acceptance.json'),report=JSON.parse(await readFile(path,'utf8'));
  const ios=report.scope.platform==='ios';
  if(!ios)for(const value of [sdk,javaHome])assert.ok(value&&isAbsolute(value),'Select the Android SDK and JDK used by the retained run');
  const profile=profileForReport(report.scope.framework);
  // Reports written before iOS support carry no platform; they are Android runs.
  assert.equal(reportFramework(profile,ios?'ios':'android'),report.scope.framework,'Report platform must match its framework workflow');
  assert.equal(createHash('sha256').update(await readFile(join(root,'testrun'))).digest('hex'),report.identity.cliSha256);
  const appRoot=report.identity.appRoot;assert.ok(isAbsolute(appRoot));
  // Recovery checks only the owner marker: a failed generation may lack native sources.
  assert.equal(JSON.parse(await readFile(join(appRoot,'acme-fixture-owner.json'),'utf8')).framework,profile.framework);
  if(report.identity.investigationId&&!report.cleanup.investigationRemoved){
    const id=report.identity.investigationId;assert.match(id,/^diag-[a-f0-9]{32}$/);
    const env=ios?{...process.env,TESTRUN_DATA_ROOT:join(root,'state'),TESTRUN_SIMULATOR_DEVICE_SET:join(root,'simulators')}:{...process.env,TESTRUN_DATA_ROOT:join(root,'state'),ANDROID_AVD_HOME:join(root,'avd'),ANDROID_HOME:sdk,JAVA_HOME:javaHome,TESTRUN_ADB:join(sdk,'platform-tools/adb')};
    delete env.TESTRUN_TASK_ID;delete env.CODEX_THREAD_ID;
    async function call(args){return JSON.parse((await execute(join(root,'testrun'),['--json',...args],{env,cwd:root,timeout:60000,maxBuffer:2097152})).stdout);}
    await call(['diagnostics','close',id]);const until=Date.now()+45000;let settled=false;
    while(Date.now()<until){if((await call(['diagnostics','status',id])).settled){settled=true;break;}await delay(100);}
    assert.ok(settled,'Owned investigation settled before removal');await call(['diagnostics','remove',id]);report.cleanup.investigationRemoved=true;
  }
  await stopOwnedMetro(report.identity.metro);
  await writeFile(join(appRoot,'project/diagnostics/acceptance.json'),'{"enabled":false}\n');
  report.cleanup.metroStopped=true;report.cleanup.probeConfigDisabled=true;
  await writeFile(path,JSON.stringify(report,null,2)+'\n',{mode:0o600});
  if(ios)await cleanupOwnedSimulator(root,xcrun);else await cleanupDiagnosticAndroid(root,sdk,javaHome);
}
