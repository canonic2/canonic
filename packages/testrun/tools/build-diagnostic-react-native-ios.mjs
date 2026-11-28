// Build only a prepared owned Expo or bare React Native fixture for the iOS simulator.
// No simulator, Metro, signing identity or account. Ruby gems stay inside the project.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {cp, mkdir, readFile, stat, writeFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';
import {fixtureFile, installFixtureSources, ownedReactNativeApp, reactNativeFrameworks, verifyIosProject} from './diagnostic-react-native-app.mjs';

const [framework,root,tool,bundler,cocoapodsHome,derived,rubySdk='']=process.argv.slice(2);
assert.ok(reactNativeFrameworks.includes(framework),'Select the expo or react-native build workflow');
// Optional: an installed macOS SDK whose Ruby headers match the host Ruby, used
// only to compile native gems. Xcode's newer SDK can carry another OS's headers.
assert.ok(rubySdk===''||isAbsolute(rubySdk),'Select an absolute macOS SDK for native gem compilation');
assert.equal(process.platform,'darwin','iOS simulator builds require macOS and Xcode');
for(const path of [root,tool,bundler,cocoapodsHome,derived])assert.ok(path&&isAbsolute(path),'Select owned fixture root, pnpm, Bundler, CocoaPods home and new derived-data directory');
const {profile,project}=await ownedReactNativeApp(root,framework);
const exists=path=>stat(path).then(()=>true,e=>e.code==='ENOENT'?false:Promise.reject(e));
await installFixtureSources(project,profile);
await mkdir(derived,{mode:0o700}); // A new directory: never reuse another build's products.
const env={...process.env,CI:'1',LANG:'en_US.UTF-8',LC_ALL:'en_US.UTF-8',EXPO_NO_TELEMETRY:'1',BUNDLE_GEMFILE:join(project,'Gemfile'),BUNDLE_PATH:join(project,'vendor/bundle'),BUNDLE_FORCE_RUBY_PLATFORM:'1',CP_HOME_DIR:cocoapodsHome,COCOAPODS_DISABLE_STATS:'true'};
async function run(program,args,cwd=project,extra={}){await new Promise((accept,reject)=>{
  const child=spawn(program,args,{cwd,env:{...env,...extra},stdio:'inherit',timeout:3600000});
  child.once('error',reject);child.once('exit',(code,signal)=>code===0?accept():reject(Error(`Fixture build failed: ${program} (${code??signal})`)));
});}
await run(tool,['install','--ignore-scripts']);
if(framework==='expo'){
  // Generate only the iOS project; the existing Android project is left alone.
  if(!await exists(join(project,'ios')))await run(tool,['exec','expo','prebuild','--platform','ios','--no-install']);
  // The Expo template has no Gemfile; select the fixture's pinned CocoaPods.
  const gemfile=join(project,'Gemfile'),selected=await readFile(fixtureFile('apps/expo/ios/Gemfile'),'utf8');
  if(await exists(gemfile))assert.equal(await readFile(gemfile,'utf8'),selected,'Expo project Gemfile must be the fixture selection');
  else await writeFile(gemfile,selected,{flag:'wx'});
}
await verifyIosProject(project,profile);
// The fixture breadcrumb module is a local pod, so the generated Xcode project is not edited.
const pods=join(project,'acme-ios');await mkdir(pods,{recursive:true});
for(const file of ['AcmeBreadcrumbs.m','AcmeBreadcrumbs.podspec'])await cp(fixtureFile(`apps/react-native/ios/${file}`),join(pods,file));
// Bare RN calls use_native_modules! without arguments; Expo passes its autolinking command.
const podfile=join(project,'ios/Podfile'),source=await readFile(podfile,'utf8'),hook=/^  config = use_native_modules!.*\n/m,line="  pod 'AcmeBreadcrumbs', :path => '../acme-ios'\n";
if(!source.includes(line)){assert.equal(source.match(new RegExp(hook,'gm'))?.length,1,'Selected template must expose one native modules hook');await writeFile(podfile,source.replace(hook,match=>match+line));}
await run(bundler,['install'],project,rubySdk?{SDKROOT:rubySdk}:{});
await run(bundler,['exec','pod','install'],join(project,'ios'));
await run('/usr/bin/xcrun',['xcodebuild','-quiet','-jobs','2','-workspace','ios/AcmeDiagnostics.xcworkspace','-scheme','AcmeDiagnostics','-configuration','Debug','-sdk','iphonesimulator','-destination','generic/platform=iOS Simulator','-derivedDataPath',derived,`ARCHS=${process.arch==='arm64'?'arm64':'x86_64'}`,
  // Ad-hoc "sign to run locally": no team or account. An unsigned app makes the
  // simulator log entitlement failures for each service request and lacks signature identity.
  'CODE_SIGN_IDENTITY=-','CODE_SIGNING_ALLOWED=YES','CODE_SIGNING_REQUIRED=NO','DEVELOPMENT_TEAM=','build']);
console.log(JSON.stringify({framework,app:join(derived,'Build/Products/Debug-iphonesimulator/AcmeDiagnostics.app'),compiledMetroPort:8081}));
