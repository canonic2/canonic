#!/usr/bin/env node
// Strict fake Apple tools. No device, simulator, log store or installed Xcode is touched.
import fs from 'node:fs';
import path from 'node:path';
const root=path.dirname(process.argv[1]);
const plan=JSON.parse(fs.readFileSync(path.join(root,'apple-plan.json'),'utf8'));
const args=process.argv.slice(2);
fs.appendFileSync(path.join(root,'apple-commands.jsonl'),JSON.stringify({tool:path.basename(process.argv[1]),args})+'\n');
const app=path.join(root,'Acme.app');
const exe=path.join(app,'Acme');
function emit(v){process.stdout.write(typeof v==='string'?v:JSON.stringify(v));}
function fail(v){process.stderr.write(v);process.exit(1);}
if(args.join(' ')==='--version')emit('xcrun version fixture-1');
else if(path.basename(process.argv[1])==='apple-ps'){
  if(JSON.stringify(args)!==JSON.stringify(['-p','42','-o','pid=,rss=,%cpu=,time=,state=,comm=']))fail('Unexpected ps request');
  emit(`42 1024 2.0 0:00.01 S ${exe}\n`);
}else if(path.basename(process.argv[1])==='codesign'){
  if(JSON.stringify(args)!==JSON.stringify(['-d','--entitlements',':-',exe]))fail('Unexpected codesign operation');
  emit('<?xml version="1.0"?><plist version="1.0"><dict><key>get-task-allow</key><true/></dict></plist>');
}else if(args[0]==='simctl'){
  if(plan.simUnavailable)fail('Selected simulator unavailable');
  const a=args.slice(1);
  if(JSON.stringify(a)===JSON.stringify(['list','devices','--json']))emit({devices:{'com.apple.CoreSimulator.SimRuntime.iOS-27-0':[{udid:'ACME',state:'Booted',isAvailable:true}]}});
  else if(JSON.stringify(a)===JSON.stringify(['getenv','ACME','SIMULATOR_UDID']))emit('ACME\n');
  else if(JSON.stringify(a)===JSON.stringify(['get_app_container','ACME','com.example.acme','app']))emit(app+'\n');
  else if(JSON.stringify(a)===JSON.stringify(['spawn','ACME','launchctl','list']))emit('PID Status Label\n42 0 UIKitApplication:com.example.acme[abc]\n99 0 UIKitApplication:com.example.acme.extra[def]\n');
  else if(a.slice(0,7).join(' ')==='spawn ACME log show --style ndjson --last' && a.length===10 && a[8]==='--predicate'){
    if(!a[9].includes('processID == 42')||!a[9].includes('com.example.acme'))fail('Wrong log predicate');
    emit([{processID:42,processImagePath:exe,eventMessage:'simulator app private-token',timestamp:'2026-11-07 10:30:00.000+0000'},{processID:8,processImagePath:'/usr/libexec/runningboardd',eventMessage:'watchdog com.example.acme'},{processID:99,processImagePath:'/Other.app/Other',eventMessage:'private-neighbor'}].map(v=>JSON.stringify(v)).join('\n')+'\n');
  }else fail('Unexpected simctl operation');
}else if(args[0]==='devicectl'){
  const i=args.indexOf('--json-output');if(i<0||args.at(-2)!=='--timeout'||args.at(-1)!=='3')fail('Missing bounded private JSON output');
  const a=args.slice(0,i);let value;
  if(a.join(' ')==='devicectl list devices')value={result:{devices:[{hardwareProperties:{udid:'ACME',platform:'iOS'},deviceProperties:{osVersionNumber:'27.0',isLocked:!!plan.locked,developerModeStatus:plan.noSupport?'disabled':'enabled',ddiServicesAvailable:!plan.noSupport},connectionProperties:{transportType:plan.network?'localNetwork':'wired',pairingState:plan.untrusted?'unpaired':'paired'}}]}};
  else if(a.join(' ')==='devicectl device info apps --device ACME --include-all-apps')value={result:{apps:[{bundleIdentifier:'com.example.acme',version:'1.0',bundleVersion:'42',url:'file:///private/var/containers/Bundle/Application/ABC/Acme.app'}]}};
  else if(a.join(' ')==='devicectl device info processes --device ACME'){
    if(plan.noSupport)fail('Developer support DDI unavailable');
    value={result:{processes:[{processIdentifier:42,executable:'file:///private/var/containers/Bundle/Application/ABC/Acme.app/Acme'},{processIdentifier:99,executable:'file:///private/var/containers/Bundle/Application/OTHER/Other.app/Other'}]}};
  }else fail('Unexpected CoreDevice command');
  fs.writeFileSync(args[i+1],JSON.stringify(value));
}else if(args[0]==='dwarfdump'){
  if(args[1]!=='--uuid'||args.length!==3)fail('Unexpected dwarfdump operation');
  const contents=fs.readFileSync(args[2],'utf8');
  emit(`UUID: ${contents.includes('mismatch')?'BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB':'AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE'} (arm64) image\n`);
}else if(args[0]==='atos'){
  if(args.slice(0,4).join(' ')!=='atos -arch arm64 -o'||args[5]!=='-l'||args[6]!=='0x1000')fail('Unverified atos invocation');
  emit(args.slice(7).map(()=> 'Acme.crashHere() (in Acme) (Main.swift:42)').join('\n')+'\n');
}else fail('Unexpected Apple tool');
