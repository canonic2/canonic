// Owned Expo/bare React Native fixture profiles: identity checks and Metro entry points.
import assert from 'node:assert/strict';
import {cp,readFile,writeFile} from 'node:fs/promises';
import {isAbsolute,join} from 'node:path';

const profiles={
  expo:{
    framework:'expo',reportFramework:'expo-native-android',appId:'com.example.acme.expodiagnostics',clientMode:'generated-native',
    description:'Expo generated native Debug/Hermes; app-owned probe explicitly enabled',
    scopeGap:'Selected Expo Android Debug only; bare RN, Expo Go, iOS, physical and release builds unverified',
    iosScopeGap:'Selected Expo generated-native iOS simulator Debug only; simulator processes run on the host; Expo Go, physical/release and other Expo/RN versions unverified',
    metroArgs:port=>['exec','expo','start','--localhost','--port',port],
    bundlePath:platform=>`/.expo/.virtual-metro-entry.bundle?platform=${platform}&dev=true&minify=false`,
  },
  'react-native':{
    framework:'react-native',reportFramework:'react-native-android',appId:'com.example.acme.rndiagnostics',clientMode:'bare-native',
    description:'Bare React Native Community CLI native Debug/Hermes; app-owned probe explicitly enabled',
    scopeGap:'Selected bare RN Android Debug only; Expo, iOS, physical, release and other RN versions unverified',
    iosScopeGap:'Selected bare RN iOS simulator Debug only; simulator processes run on the host; physical/release, Expo and other RN versions unverified',
    metroArgs:port=>['exec','react-native','start','--host','127.0.0.1','--port',port],
    bundlePath:platform=>`/index.bundle?platform=${platform}&dev=true&minify=false`,
  },
};
export const reactNativeFrameworks=Object.keys(profiles);
// Both frameworks also have an iOS simulator workflow.
const iosReportFrameworks={expo:'expo-native-ios','react-native':'react-native-ios'};
export function reportFramework(profile,platform){
  const name=platform==='ios'?iosReportFrameworks[profile.framework]:profile.reportFramework;
  assert.ok(name,`No ${platform} acceptance workflow for ${profile.framework}`);return name;
}
export function profileForReport(name){
  const profile=Object.values(profiles).find(p=>p.reportFramework===name||iosReportFrameworks[p.framework]===name);
  assert.ok(profile,`Unknown React Native acceptance framework ${name}`);return profile;
}
/** Kotlin source directory owned by the generated Android application package. */
export function nativeSourceDir(project,profile){return join(project,'android/app/src/main/java',...profile.appId.split('.'));}
/**
 * Verify the owner marker and the generated project's Android application identity.
 * `expected` selects one framework; omit it only for an already identified run.
 */
export async function ownedReactNativeApp(appRoot,expected){
  assert.ok(appRoot&&isAbsolute(appRoot),'Select an absolute owned fixture root');
  const owner=JSON.parse(await readFile(join(appRoot,'acme-fixture-owner.json'),'utf8'));
  const profile=profiles[owner.framework];assert.ok(profile,`Owned fixture is not Expo/React Native: ${owner.framework}`);
  if(expected!==undefined)assert.equal(owner.framework,expected,'Owned fixture framework must match the selected workflow');
  const project=join(appRoot,'project');
  if(profile.framework==='expo'){
    const {expo}=JSON.parse(await readFile(join(project,'app.json'),'utf8'));
    assert.equal(expo.android.package,profile.appId);assert.equal(expo.ios?.bundleIdentifier,profile.appId,`Expo iOS bundleIdentifier must be ${profile.appId}`);
  }else{
    assert.equal(JSON.parse(await readFile(join(project,'app.json'),'utf8')).name,'AcmeDiagnostics');
    const gradle=await readFile(join(project,'android/app/build.gradle'),'utf8');
    for(const key of ['namespace','applicationId'])assert.match(gradle,new RegExp(`^\\s*${key}\\s+"${profile.appId.replaceAll('.','\\.')}"\\s*$`,'m'),`Generated Android ${key} must be ${profile.appId}`);
  }
  return {owner,profile,project};
}
const fixtures=new URL('../tests/fixtures/diagnostics/',import.meta.url);
/** Refresh the checked-in app entry, shared JS faults and probe; leave the probe disabled. */
export async function installFixtureSources(project,profile){
  const entry=profile.framework==='expo'?'App.js':'App.tsx';
  await cp(new URL(`apps/${profile.framework}/${entry}`,fixtures),join(project,entry));
  for(const file of ['AcmeDiagnostics.jsx','AcmeDiagnostics.d.ts','DiagnosticApp.jsx','DiagnosticApp.d.ts','runtime.js'])await cp(new URL(file,fixtures),join(project,'diagnostics',file));
  await cp(new URL('../probes/react-native.js',import.meta.url),join(project,'diagnostics/probe.cjs'));
  await writeFile(join(project,'diagnostics/acceptance.json'),'{"enabled":false}\n');
}
export function fixtureFile(path){return new URL(path,fixtures);}
/** Every generated (Expo: prebuilt) iOS bundle identifier must be the fixture's. */
export async function verifyIosProject(project,profile){
  assert.ok(iosReportFrameworks[profile.framework],`No iOS fixture workflow for ${profile.framework}`);
  const pbx=await readFile(join(project,'ios/AcmeDiagnostics.xcodeproj/project.pbxproj'),'utf8');
  const ids=[...pbx.matchAll(/PRODUCT_BUNDLE_IDENTIFIER = "?([^";]+)"?;/g)].map(match=>match[1]);
  assert.ok(ids.length&&ids.every(id=>id===profile.appId),`Generated iOS bundle identifiers must be ${profile.appId}: ${ids.join(', ')}`);
}
