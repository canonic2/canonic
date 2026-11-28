// Uses the matching Chrome bundle's Lighthouse worker, controller and reporter.
// This bridge contains no alternative audit rules or fabricated category scores.
async options => {
  if (location.protocol !== 'devtools:') throw Error('Unexpected developer frontend');
  const progressKey=Symbol.for('canonic.testrun.developer.audit-progress.v1');
  const progress={id:options.id,stage:'frontend'};
  globalThis[progressKey]=progress;
  const base='devtools://devtools/bundled/';
  const lighthouse=await import(base+'panels/lighthouse/lighthouse.js');
  const panel=lighthouse.LighthousePanel.LighthousePanel.instance();
  const controller=panel.controller;
  if (!controller || controller.getCurrentRun()) throw Error('canonic:AUDIT_CONFLICT');
  // The frontend target can exist before its SDK has connected to the page.
  // Resolve this outside startLighthouse's serialized UI action promise, whose
  // rejected async executor otherwise leaves its outer promise pending.
  progress.stage='resolve-url';
  const inspectedURL=await controller.getInspectedURL({force:true});
  if(options.mode==='navigation'&&!/^(https?|chrome|chrome-extension):/.test(inspectedURL))throw Error('canonic:AUDIT_URL_UNSUPPORTED');
  const setting=lighthouse.LighthouseController.getRuntimeSettings().find(item=>item.setting.name==='lighthouse.device-type');
  if (!setting) throw Error('canonic:AUDIT_ENGINE_MISMATCH');
  const before=setting.setting.get();
  let ownedFlags;
  try {
    setting.setting.set(options.device);
    progress.stage='start';
    await controller.startLighthouse({categoryIds:['accessibility','seo','best-practices','agentic-browsing'],mode:options.mode,isAIControlled:true});
    const run=controller.getCurrentRun();
    if (!run) throw Error('canonic:AUDIT_ENGINE_MISMATCH');
    ownedFlags=run.flags;
    const mobile=options.device==='mobile';
    Object.assign(run.flags,{maxWaitForLoad:30000,disableStorageReset:true,throttlingMethod:'provided',
      screenEmulation:mobile?{mobile:true,width:412,height:823,deviceScaleFactor:1.75,disabled:false}:{mobile:false,width:1350,height:940,deviceScaleFactor:1,disabled:false},
      emulatedUserAgent:mobile?'Mozilla/5.0 (Linux; Android 11; moto g power (2022)) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Mobile Safari/537.36':'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36'});
    progress.stage='collect';
    const result=await controller.collectLighthouseResults();
    const lhr=result.lhr;
    if (!lhr) throw Error('canonic:AUDIT_ENGINE_MISMATCH');
    progress.stage='report';
    // The official Chrome distribution bundles panel modules into lighthouse.js.
    const json=JSON.stringify(lhr);
    const html=Lighthouse.ReportGenerator.ReportGenerator.generateReportHtml(lhr);
    const encoder=new TextEncoder();
    if (json.length>16777216||html.length>16777216||encoder.encode(json).length>16777216||encoder.encode(html).length>16777216) throw Error('canonic:EVIDENCE_BOUNDS');
    const key=Symbol.for('canonic.testrun.developer.reports.v1');
    globalThis[key] ||= new Map();
    if (globalThis[key].size>=4) throw Error('canonic:EVIDENCE_CAPACITY');
    globalThis[key].set(options.id,{json,html});
    progress.stage='reports-ready';
    const failures=Object.values(lhr.audits).filter(audit=>audit.score!==null&&audit.score<1);
    const categoryMap=new Map();
    for(const category of Object.values(lhr.categories))for(const ref of category.auditRefs){const ids=categoryMap.get(ref.id)||[];ids.push(category.id);categoryMap.set(ref.id,ids);}
    const bounded=value=>typeof value==='string'?value.slice(0,512):value;
    const failedAudits=failures.slice(0,options.limit).map(audit=>{
      const nodes=new Map();
      const collect=(value,depth=0)=>{
        if(depth>32||!value||typeof value!=='object')return;
        if(Array.isArray(value)){for(const item of value)collect(item,depth+1);return;}
        if(value.type==='debugdata'||value.type==='subitems')return;
        if(value.type==='node'){const key=value.lhId||value.path||`${value.selector||''}|${value.snippet||''}`||`node-${nodes.size}`;nodes.set(key,{selector:bounded(value.selector),snippet:bounded(value.snippet),nodeLabel:bounded(value.nodeLabel),explanation:bounded(value.explanation)});return;}
        for(const [name,child]of Object.entries(value))if(name!=='subItems')collect(child,depth+1);
      };
      collect(audit.details);
      return{id:audit.id,title:bounded(audit.title),score:audit.score,categories:categoryMap.get(audit.id)||[],totalNodes:nodes.size,...options.includeNodes?{nodes:[...nodes.values()].slice(0,10)}:{}};
    });
    while(encoder.encode(JSON.stringify(failedAudits)).length>6144)failedAudits.pop();
    return{mode:options.mode,device:options.device,scores:Object.values(lhr.categories).map(category=>({id:category.id,title:category.title,score:category.score})),audits:{failed:failures.length,passed:Object.values(lhr.audits).filter(audit=>audit.score===1).length},failedAudits,hasMore:failures.length>failedAudits.length,runtimeError:lhr.runtimeError||null,formats:['lighthouse-result-json','lighthouse-report-html'],implementation:'bundled-Chrome-Lighthouse',instrumentation:['Lighthouse'],jsonUnits:json.length,htmlUnits:html.length};
  } finally {
    if(ownedFlags&&controller.getCurrentRun()?.flags===ownedFlags)await controller.cancelLighthouse();
    setting.setting.set(before);
  }
}
