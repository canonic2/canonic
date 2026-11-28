// Build only a prepared owned Expo or bare React Native fixture for Android. No emulator, Metro or EAS account.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFile, writeFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';
import {fixtureFile, installFixtureSources, nativeSourceDir, ownedReactNativeApp, reactNativeFrameworks} from './diagnostic-react-native-app.mjs';

const [framework,root,tool,sdk,javaHome,cache,port='8097']=process.argv.slice(2);
assert.ok(reactNativeFrameworks.includes(framework),'Select the expo or react-native build workflow');
for(const path of [root,tool,sdk,javaHome,cache])assert.ok(path&&isAbsolute(path),'Select owned fixture root, pnpm, SDK, JDK and Gradle cache');
assert.match(port,/^\d+$/);assert.ok(Number(port)>1024&&Number(port)<65536);
const {profile,project}=await ownedReactNativeApp(root,framework);
await installFixtureSources(project,profile);
const env={...process.env,CI:'1',ANDROID_HOME:sdk,ANDROID_SDK_ROOT:sdk,JAVA_HOME:javaHome,GRADLE_USER_HOME:cache,EXPO_NO_TELEMETRY:'1'};
async function run(program,args,cwd=project){await new Promise((accept,reject)=>{
  const child=spawn(program,args,{cwd,env,stdio:'inherit',timeout:1800000});
  child.once('error',reject);child.once('exit',(code,signal)=>code===0?accept():reject(Error(`Fixture build failed: ${program} (${code??signal})`)));
});}
await run(tool,['install','--ignore-scripts']);
if(framework==='expo')await run(tool,['exec','expo','prebuild','--platform','android','--no-install']);
const native=nativeSourceDir(project,profile),application=join(native,'MainApplication.kt');
const source=await readFile(application,'utf8'),hook='PackageList(this).packages.apply {';
if(!source.includes('add(AcmeBreadcrumbPackage())')){
  assert.ok(source.includes(hook),'Selected template must expose its native package hook');
  await writeFile(application,source.replace(hook,hook+'\n          add(AcmeBreadcrumbPackage())'));
}
// One fixture-owned logging package; its Kotlin package follows the generated app.
const breadcrumbs=await readFile(fixtureFile('apps/expo/AcmeBreadcrumbPackage.kt'),'utf8'),declared='package com.example.acme.expodiagnostics\n';
assert.ok(breadcrumbs.startsWith(declared),'Fixture breadcrumb package declaration');
await writeFile(join(native,'AcmeBreadcrumbPackage.kt'),`package ${profile.appId}\n`+breadcrumbs.slice(declared.length));
await run(join(project,'android/gradlew'),['--no-daemon','--max-workers=2','--console=plain','-PreactNativeArchitectures=x86_64',`-PreactNativeDevServerPort=${port}`,'assembleDebug'],join(project,'android'));
console.log(JSON.stringify({framework,apk:join(project,'android/app/build/outputs/apk/debug/app-debug.apk'),metroPort:Number(port)}));
