// Unchanged SDK context emulation for the 5.1 closure against the caller's
// server: creation options, a cross-site child frame, a page-opened popup, a
// plain second context, offline, geolocation, permissions, page media and a
// pinned device descriptor, read with the caller's probe sources. Each step
// is bounded and named, so a stalled SDK call fails with that step and the
// steps completed before it instead of hanging.
// Arguments: playwright-core, engine, mode, browsers, origin, options JSON,
// device name, then the probe, online, geolocation, permission, media, frame,
// plain-context, popup and device sources.
const path=require('node:path');
const playwright=require(process.argv[2]);
const completed=[];
const step=async(name,run)=>{
  let timer;
  const stalled=new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error(`SDK step ${name} exceeded 60 s after ${JSON.stringify(completed)}`)),60000);});
  try { const value=await Promise.race([run(),stalled]); completed.push(name); return value; } finally { clearTimeout(timer); }
};
(async()=>{
  const [engine,mode,browsers,origin,optionsJson,device,probe,online,geolocation,permission,media,frame,plainProbe,popup,deviceProbe]=process.argv.slice(3);
  // A string evaluates as an expression, so call the probe function.
  const read=(name,page,source)=>step(name,async()=>JSON.parse(await page.evaluate(`(${source})()`)));
  const report={};let browser;
  try {
    browser=await step('launch',()=>playwright[engine==='chrome'?'chromium':engine].launch({headless:mode==='headless',...(engine==='chrome'?{executablePath:path.join(browsers,'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})}));
    const context=await step('newContext',()=>browser.newContext(JSON.parse(optionsJson)));
    const page=await step('newPage',()=>context.newPage());
    await step('goto',()=>page.goto(origin+'/page'));
    report.created=await read('created',page,probe);
    report.frame=await read('frame',page,frame);
    report.popup=await read('popup',page,popup);
    const plain=await step('plainContext',()=>browser.newContext({viewport:{width:1280,height:720},deviceScaleFactor:1}));
    const plainPage=await step('plainPage',()=>plain.newPage());
    await step('plainGoto',()=>plainPage.goto(origin+'/page'));
    report.plain=await read('plain',plainPage,plainProbe);
    await step('setOffline',()=>context.setOffline(true));
    report.offline=await read('offline',page,online);
    report.plainOnline=await read('plainOnline',plainPage,online);
    await step('plainClose',()=>plain.close());
    await step('setOnline',()=>context.setOffline(false));
    report.online=await read('online',page,online);
    await step('setGeolocation',()=>context.setGeolocation({latitude:48.85,longitude:2.35}));
    report.geolocation=await read('geolocation',page,geolocation);
    await step('clearPermissions',()=>context.clearPermissions());
    report.cleared=await read('cleared',page,permission);
    await step('emulateMedia',()=>page.emulateMedia({media:'print',colorScheme:'light'}));
    report.media=await read('media',page,media);
    await step('contextClose',()=>context.close());
    const described=await step('deviceContext',()=>browser.newContext({...playwright.devices[device]}));
    const describedPage=await step('devicePage',()=>described.newPage());
    await step('deviceGoto',()=>describedPage.goto(origin+'/page'));
    report.device=await read('device',describedPage,deviceProbe);
    await step('deviceClose',()=>described.close());
  } finally { if(browser)await step('browserClose',()=>browser.close()).catch(()=>{}); }
  process.stdout.write(JSON.stringify(report),()=>process.exit(0));
})().catch(error=>{process.stderr.write(String(error.stack||error));process.exit(1);});
