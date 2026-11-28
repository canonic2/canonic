// Unchanged SDK storage for the 5.2 closure against the caller's server:
// creation storageState (inline and from a file), cookies add/read/filtered
// clear, page WebStorage, an exported state with IndexedDB and OPFS restored into
// a new context, and setStorageState clearing a visited origin. Pages are read
// with the caller's probe sources. Each step is bounded and named.
// Arguments: playwright-core, engine, mode, browsers, origin, other origin,
// inline state JSON, state file, export file, then the read, write and visit
// sources.
const path=require('node:path');
const playwright=require(process.argv[2]);
const completed=[];
const step=async(name,run)=>{
  let timer;
  const stalled=new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error(`SDK step ${name} exceeded 60 s after ${JSON.stringify(completed)}`)),60000);});
  try { const value=await Promise.race([run(),stalled]); completed.push(name); return value; } finally { clearTimeout(timer); }
};
(async()=>{
  const [engine,mode,browsers,origin,other,stateJson,stateFile,exportFile,readSource,writeSource,visitSource]=process.argv.slice(3);
  // A string evaluates as an expression, so call the probe function.
  const read=(name,page,source)=>step(name,async()=>JSON.parse(await page.evaluate(`(${source})()`)));
  const cookies=async(name,context,urls)=>(await step(name,()=>context.cookies(urls))).map(cookie=>`${cookie.name}=${cookie.value}`).sort();
  const report={};let browser;
  try {
    browser=await step('launch',()=>playwright[engine==='chrome'?'chromium':engine].launch({headless:mode==='headless',...(engine==='chrome'?{executablePath:path.join(browsers,'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})}));
    const created=await step('inlineContext',()=>browser.newContext({storageState:JSON.parse(stateJson)}));
    const page=await step('inlinePage',()=>created.newPage());
    await step('inlineGoto',()=>page.goto(origin+'/page'));
    report.initial=await read('initial',page,readSource);
    const fromFile=await step('fileContext',()=>browser.newContext({storageState:stateFile}));
    const filePage=await step('filePage',()=>fromFile.newPage());
    await step('fileGoto',()=>filePage.goto(origin+'/page'));
    report.file=await read('file',filePage,readSource);
    await step('fileClose',()=>fromFile.close());
    await step('addCookies',()=>created.addCookies([{name:'extra',value:'1',url:origin},{name:'other',value:'2',url:other}]));
    report.selected=await cookies('selected',created,[origin+'/']);
    await step('clearFiltered',()=>created.clearCookies({name:/^ext/}));
    report.cleared=await cookies('cleared',created,[]);
    await step('localSet',()=>page.localStorage.setItem('k','v'));
    await step('sessionSet',()=>page.sessionStorage.setItem('s','1'));
    report.items=(await step('localItems',()=>page.localStorage.items())).sort((a,b)=>a.name<b.name?-1:1);
    report.sessionItems=(await step('sessionItems',()=>page.sessionStorage.items())).sort((a,b)=>a.name<b.name?-1:1);
    report.missing=(await step('missing',()=>page.localStorage.getItem('missing')))??null;
    await step('localRemove',()=>page.localStorage.removeItem('k'));
    await step('sessionClear',()=>page.sessionStorage.clear());
    report.updated=await read('updated',page,readSource);
    await read('write',page,writeSource);
    await step('export',()=>created.storageState({path:exportFile,indexedDB:true,opfs:engine!=='webkit'}));
    const restored=await step('restoredContext',()=>browser.newContext({storageState:exportFile}));
    const restoredPage=await step('restoredPage',()=>restored.newPage());
    await step('restoredGoto',()=>restoredPage.goto(origin+'/page'));
    report.restored=await read('restored',restoredPage,readSource);
    await step('restoredClose',()=>restored.close());
    const visited=await step('visitedContext',()=>browser.newContext());
    const visitedPage=await step('visitedPage',()=>visited.newPage());
    await step('visitedGoto',()=>visitedPage.goto(other+'/page'));
    await read('visit',visitedPage,visitSource);
    await step('setStorageState',()=>visited.setStorageState(exportFile));
    await step('visitedReload',()=>visitedPage.goto(other+'/page'));
    report.visitedCleared=await read('visitedCleared',visitedPage,readSource);
    await step('visitedRestoredGoto',()=>visitedPage.goto(origin+'/page'));
    report.visitedRestored=await read('visitedRestored',visitedPage,readSource);
    report.visitedCookies=await cookies('visitedCookies',visited,[]);
    await step('visitedClose',()=>visited.close());
    await step('inlineClose',()=>created.close());
  } finally { if(browser)await step('browserClose',()=>browser.close()).catch(()=>{}); }
  process.stdout.write(JSON.stringify(report),()=>process.exit(0));
})().catch(error=>{process.stderr.write(String(error.stack||error));process.exit(1);});
