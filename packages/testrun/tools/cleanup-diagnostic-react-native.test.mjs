import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {chmod,mkdir,mkdtemp,readFile,realpath,rm,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {cleanupReactNative,metroWitness,stopOwnedMetro} from './cleanup-diagnostic-react-native.mjs';
import {ownedReactNativeApp,profileForReport,reportFramework,verifyIosProject} from './diagnostic-react-native-app.mjs';

test('owned app selection verifies the workflow framework and generated Android identity',async t=>{
  const root=await mkdtemp(join(tmpdir(),'testrun-rn-identity-')),project=join(root,'project');t.after(()=>rm(root,{recursive:true,force:true}));
  await mkdir(join(project,'android/app'),{recursive:true});
  await writeFile(join(root,'acme-fixture-owner.json'),'{"framework":"react-native"}');
  await writeFile(join(project,'app.json'),'{"name":"AcmeDiagnostics","displayName":"Acme Diagnostics"}');
  const gradle=id=>`android {\n    namespace "com.example.acme.rndiagnostics"\n    defaultConfig {\n        applicationId "${id}"\n    }\n}\n`;
  await writeFile(join(project,'android/app/build.gradle'),gradle('com.example.acme.rndiagnostics'));
  assert.equal((await ownedReactNativeApp(root,'react-native')).profile.appId,'com.example.acme.rndiagnostics');
  assert.equal((await ownedReactNativeApp(root)).profile.reportFramework,'react-native-android');
  await assert.rejects(ownedReactNativeApp(root,'expo'),/framework must match/);
  await writeFile(join(project,'android/app/build.gradle'),gradle('com.example.acme.other'));
  await assert.rejects(ownedReactNativeApp(root,'react-native'),/applicationId must be com\.example\.acme\.rndiagnostics/);
  await writeFile(join(root,'acme-fixture-owner.json'),'{"framework":"flutter"}');
  await assert.rejects(ownedReactNativeApp(root),/not Expo\/React Native/);
});

async function owned(t){
  const root=await mkdtemp(join(tmpdir(),'testrun-metro-owner-')),script=join(root,'metro-peer.cjs'),ready=join(root,'ready');
  await writeFile(script,"require('node:fs').writeFileSync(process.argv[2],'ready');setInterval(()=>{},1000);\n");
  const child=spawn(process.execPath,[script,ready],{detached:true,stdio:'ignore'});
  const exited=new Promise(resolve=>child.once('exit',(code,signal)=>resolve({code,signal})));
  t.after(async()=>{try{process.kill(-child.pid,'SIGKILL');}catch(error){if(error.code!=='ESRCH')throw error;}await exited;await rm(root,{recursive:true,force:true});});
  const until=Date.now()+5000;while(Date.now()<until){if(await readFile(ready,'utf8').catch(error=>error.code==='ENOENT'?null:Promise.reject(error)))break;await delay(10);}
  assert.equal(await readFile(ready,'utf8'),'ready');
  return {metro:{pid:child.pid,witness:await metroWitness(child.pid)},exited};
}
test('Metro cleanup accepts legacy launcher exec metadata, waits for its owned group and safely repeats',async t=>{
  const {metro,exited}=await owned(t);await stopOwnedMetro({...metro,witness:metro.witness+' /usr/bin/env'});
  assert.equal((await exited).signal,'SIGTERM');assert.equal(await metroWitness(metro.pid),null);
  await stopOwnedMetro(metro);
});
test('Metro cleanup refuses a changed birth witness without signalling the process',async t=>{
  const {metro}=await owned(t);
  await assert.rejects(stopOwnedMetro({...metro,witness:metro.witness.replace(/^\d+/,String(metro.pid+1))}),/Metro process birth/);
  assert.equal(await metroWitness(metro.pid),metro.witness);
  await stopOwnedMetro(metro);
});
for(const [framework,reportFramework] of [['expo','expo-native-android'],['react-native','react-native-android']])
test(`${framework} recovery settles/removes the recorded investigation and disables only its fixture probe`,async t=>{
  const root=await realpath(await mkdtemp(join(tmpdir(),'testrun-rn-recovery-'))),appRoot=join(root,'app'),sdk=join(root,'sdk');
  t.after(()=>rm(root,{recursive:true,force:true}));
  await mkdir(join(appRoot,'project/diagnostics'),{recursive:true});await mkdir(join(root,'avd'));await mkdir(join(sdk,'platform-tools'),{recursive:true});
  await writeFile(join(appRoot,'acme-fixture-owner.json'),JSON.stringify({framework}));
  await writeFile(join(appRoot,'project/diagnostics/acceptance.json'),'{"enabled":true}');
  const id='diag-'+ 'a'.repeat(32),name='Testrun-QA-Diagnostics-01234567-0123-0123-0123-012345678901';
  const body=`#!${process.execPath}\nconst fs=require('node:fs'),path=require('node:path'),args=process.argv.slice(3);
fs.appendFileSync(path.join(__dirname,'calls.jsonl'),JSON.stringify(args)+'\\n');
if(args[0]==='diagnostics'&&['close','status','remove'].includes(args[1])&&args[2]==='${id}')console.log(JSON.stringify({settled:true,removed:true}));
else if(args[0]==='disconnect'&&args[2]==='android:avd:${name}')console.log(JSON.stringify({closed:true}));
else if(args.join(' ')==='manager stop')console.log(JSON.stringify({retired:true}));
else throw Error('Unexpected recovery operation');\n`;
  await writeFile(join(root,'testrun'),body);await chmod(join(root,'testrun'),0o700);
  await writeFile(join(sdk,'platform-tools/adb'),`#!${process.execPath}\nconsole.log('List of devices attached\\n');\n`);await chmod(join(sdk,'platform-tools/adb'),0o700);
  const report={schemaVersion:1,state:'failed',scope:{framework:reportFramework,image:'system-images;android-36;google_apis_playstore;x86_64'},identity:{appRoot,avd:name,investigationId:id,cliSha256:createHash('sha256').update(body).digest('hex')},cleanup:{errors:['original failure']}};
  // A report naming the other framework must not release this app's resources.
  const other=reportFramework==='expo-native-android'?'react-native-android':'expo-native-android';
  await writeFile(join(root,'acceptance.json'),JSON.stringify({...report,scope:{...report.scope,framework:other}}));
  await assert.rejects(cleanupReactNative(root,sdk,join(root,'jdk')),/Expected values to be strictly equal/);
  await assert.rejects(readFile(join(root,'calls.jsonl')),{code:'ENOENT'});
  await writeFile(join(root,'acceptance.json'),JSON.stringify(report));
  await cleanupReactNative(root,sdk,join(root,'jdk'));
  const recovered=JSON.parse(await readFile(join(root,'acceptance.json'),'utf8'));
  assert.equal(recovered.state,'failed');assert.deepEqual(recovered.cleanup.errors,['original failure']);
  assert.equal(recovered.cleanup.investigationRemoved,true);assert.equal(recovered.cleanup.recovery.complete,true);
  assert.deepEqual(JSON.parse(await readFile(join(appRoot,'project/diagnostics/acceptance.json'),'utf8')),{enabled:false});
  const calls=(await readFile(join(root,'calls.jsonl'),'utf8')).trim().split('\n').map(JSON.parse);
  assert.deepEqual(calls.map(args=>args.slice(0,2)),[['diagnostics','close'],['diagnostics','status'],['diagnostics','remove'],['disconnect','--target'],['manager','stop']]);
});
test('iOS recovery releases only its private simulator set and refuses another set member',async t=>{
  const root=await realpath(await mkdtemp(join(tmpdir(),'testrun-rn-ios-recovery-'))),appRoot=join(root,'app'),udid='01234567-89AB-4CDE-8F01-23456789ABCD';
  t.after(()=>rm(root,{recursive:true,force:true}));
  await mkdir(join(appRoot,'project/diagnostics'),{recursive:true});await mkdir(join(root,'simulators'));
  await writeFile(join(appRoot,'acme-fixture-owner.json'),'{"framework":"react-native"}');
  await writeFile(join(appRoot,'project/diagnostics/acceptance.json'),'{"enabled":true}');
  const id='diag-'+'b'.repeat(32),target=`simulator:${'c'.repeat(64)}:${udid}`;
  const body=`#!${process.execPath}\nconst fs=require('node:fs'),path=require('node:path'),args=process.argv.slice(3);
fs.appendFileSync(path.join(__dirname,'calls.jsonl'),JSON.stringify(args)+'\\n');
if(process.env.TESTRUN_SIMULATOR_DEVICE_SET!==path.join(__dirname,'simulators'))throw Error('Private set');
if(args[0]==='diagnostics'&&args[2]==='${id}')console.log(JSON.stringify({settled:true,removed:true}));
else if(args.join(' ')==='disconnect --target ${target}')console.log(JSON.stringify({closed:true}));
else if(args.join(' ')==='manager stop')console.log(JSON.stringify({retired:true}));
else throw Error('Unexpected recovery operation');\n`;
  await writeFile(join(root,'testrun'),body);await chmod(join(root,'testrun'),0o700);
  // The owned xcrun peer reports and mutates only the selected set's state file.
  const xcrun=join(root,'xcrun'),state=join(root,'devices.json');
  await writeFile(xcrun,`#!${process.execPath}\nconst fs=require('node:fs'),args=process.argv.slice(2),state=${JSON.stringify(state)};
fs.appendFileSync(${JSON.stringify(join(root,'xcrun.jsonl'))},JSON.stringify(args)+'\\n');
if(args[0]!=='simctl'||args[1]!=='--set'||args[2]!==${JSON.stringify(join(root,'simulators'))})throw Error('Unowned set');
let devices=JSON.parse(fs.readFileSync(state,'utf8'));
if(args[3]==='list')console.log(JSON.stringify({devices:{runtime:devices}}));
else if(args[3]==='shutdown')devices=devices.map(d=>d.udid===args[4]?{...d,state:'Shutdown'}:d);
else if(args[3]==='delete')devices=devices.filter(d=>d.udid!==args[4]);
else throw Error('Unexpected simctl');
fs.writeFileSync(state,JSON.stringify(devices));\n`);await chmod(xcrun,0o700);
  const report={schemaVersion:1,state:'failed',scope:{platform:'ios',framework:'react-native-ios'},identity:{appRoot,target,simulatorUUID:udid,investigationId:id,cliSha256:createHash('sha256').update(body).digest('hex')},cleanup:{errors:['original failure']}};
  await writeFile(join(root,'acceptance.json'),JSON.stringify(report));
  await writeFile(state,JSON.stringify([{udid:'FEDCBA98-7654-4321-8FED-CBA987654321',name:'Testrun-QA-Diagnostics-other',state:'Booted'}]));
  await assert.rejects(cleanupReactNative(root,undefined,undefined,{xcrun}),/Expected values to be strictly equal/);
  assert.ok(!(await readFile(join(root,'xcrun.jsonl'),'utf8')).includes('delete'),'Another set member is never deleted');
  await writeFile(state,JSON.stringify([{udid,name:'Testrun-QA-Diagnostics-run',state:'Booted'}]));
  await cleanupReactNative(root,undefined,undefined,{xcrun});
  const recovered=JSON.parse(await readFile(join(root,'acceptance.json'),'utf8'));
  assert.equal(recovered.state,'failed');assert.deepEqual(recovered.cleanup.errors,['original failure']);
  assert.equal(recovered.cleanup.recovery.complete,true);assert.equal(recovered.cleanup.recovery.remainingSimulators,0);
  assert.deepEqual(JSON.parse(await readFile(join(appRoot,'project/diagnostics/acceptance.json'),'utf8')),{enabled:false});
  assert.deepEqual(JSON.parse(await readFile(state,'utf8')),[]);
  // A report whose platform does not match its framework releases nothing.
  await writeFile(join(root,'acceptance.json'),JSON.stringify({...report,scope:{platform:'android',framework:'react-native-ios'}}));
  await assert.rejects(cleanupReactNative(root,'/sdk','/jdk',{xcrun}),/Report platform must match/);
});
test('iOS workflow requires every generated bundle identifier to be the selected fixture app',async t=>{
  const project=await mkdtemp(join(tmpdir(),'testrun-rn-ios-identity-'));t.after(()=>rm(project,{recursive:true,force:true}));
  await mkdir(join(project,'ios/AcmeDiagnostics.xcodeproj'),{recursive:true});
  const pbx=ids=>ids.map(id=>`\t\t\t\tPRODUCT_BUNDLE_IDENTIFIER = "${id}";\n`).join('');
  const bare=profileForReport('react-native-ios'),expo=profileForReport('expo-native-ios'),path=join(project,'ios/AcmeDiagnostics.xcodeproj/project.pbxproj');
  await writeFile(path,pbx(['com.example.acme.rndiagnostics','com.example.acme.rndiagnostics']));await verifyIosProject(project,bare);
  await assert.rejects(verifyIosProject(project,expo),/bundle identifiers must be com\.example\.acme\.expodiagnostics/);
  await writeFile(path,pbx(['com.example.acme.rndiagnostics','org.reactjs.native.example.AcmeDiagnostics']));
  await assert.rejects(verifyIosProject(project,bare),/bundle identifiers must be com\.example\.acme\.rndiagnostics/);
  await writeFile(path,pbx(['com.example.acme.expodiagnostics']));await verifyIosProject(project,expo);
  assert.equal(reportFramework(bare,'ios'),'react-native-ios');assert.equal(reportFramework(expo,'ios'),'expo-native-ios');
  assert.equal(profileForReport('expo-native-android'),expo);
});
test('Expo selection also requires the fixture iOS bundle identifier',async t=>{
  const root=await mkdtemp(join(tmpdir(),'testrun-expo-identity-'));t.after(()=>rm(root,{recursive:true,force:true}));
  await mkdir(join(root,'project'));await writeFile(join(root,'acme-fixture-owner.json'),'{"framework":"expo"}');
  const app=ios=>JSON.stringify({expo:{android:{package:'com.example.acme.expodiagnostics'},ios}});
  await writeFile(join(root,'project/app.json'),app({bundleIdentifier:'com.example.acme.expodiagnostics'}));
  assert.equal((await ownedReactNativeApp(root,'expo')).profile.appId,'com.example.acme.expodiagnostics');
  await writeFile(join(root,'project/app.json'),app({bundleIdentifier:'com.example.acme.other'}));
  await assert.rejects(ownedReactNativeApp(root,'expo'),/Expo iOS bundleIdentifier must be com\.example\.acme\.expodiagnostics/);
});
