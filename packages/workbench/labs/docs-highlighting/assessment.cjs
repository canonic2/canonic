const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),crypto=require('node:crypto');
assert(process.argv[2], 'Usage: node assessment.cjs <reviewed-results-directory>');
const root=path.resolve(process.argv[2]);
const names=['set1','set2','confirmation'];
const data=names.map(name=>({name,report:JSON.parse(fs.readFileSync(path.join(root,name,'report.json'))),summary:JSON.parse(fs.readFileSync(path.join(root,name,'summary.json')))}));
const key=r=>[r.variant,r.shape,r.cache,r.metric].join('|');
function checkStatus(name,value){
  if(value===undefined)return 'unobserved';
  if(typeof value==='boolean')return value?'pass':'fail';
  if(name==='tokenRoles')return String(value);
  if(['cleanConsole','externalRequests'].includes(name))return value.length===0?'pass':'fail';
  if(name==='initial')return value.geometry&&value.typography&&value.images&&value.fonts==='loaded'&&value.examples&&!value.unexpectedMarkup&&!value.workerErrors.length?'pass':'fail';
  if(name==='copyIsolation')return value.copy&&value.independent&&value.supersession?'pass':'fail';
  if(name==='lifecycle')return value.stale&&value.disposed?'pass':'fail';
  if(name==='capture')return value.bytes>0&&value.docs?'observed':'fail';
  if(name==='exampleFailure')return value.contained&&value.neighbour?'pass':'fail';
  if(name==='faults')return value.diagnostic?'diagnostic':value.length===2&&value.every(fault=>fault.plain&&fault.latePlain)?'pass':'fail';
  if(name==='renderParity')return value.pass?'pass':'fail';
  return 'recorded';
}
const rows=[];
for(const d of data){
  const groups=new Map();
  for(const s of d.report.samples)for(const [metric,value]of Object.entries(s.observations)){
    assert(Number.isFinite(value));const k=key({...s,metric});if(!groups.has(k))groups.set(k,[]);groups.get(k).push(value);
  }
  assert.equal(groups.size,d.summary.metrics.length);
  for(const row of d.summary.metrics){const v=groups.get(key(row)).sort((a,b)=>a-b);assert.deepEqual([v.length,v[Math.ceil(v.length/2)-1],v[Math.ceil(v.length*.95)-1],v[0],v.at(-1)],[row.n,row.p50,row.p95,row.min,row.max]);}
  assert.deepEqual(d.report.cleanupFailures,[]);
  const plan=JSON.parse(fs.readFileSync(path.join(root,d.name,'plan-provenance.json')));
  assert.equal(plan.originalSha256,d.report.planSha256);
  assert.equal(plan.reviewedSha256,crypto.createHash('sha256').update(fs.readFileSync(path.join(root,d.name,'plan.md'))).digest('hex'));
  for(const variant of d.name==='confirmation'?['0','1']:['0','1','2','D'])for(const shape of ['Small','Overview','Code-heavy'])for(const cache of ['cold-startup','fresh-profile-after-examples-settled']){
    const samples=d.report.samples.filter(s=>s.variant===variant&&s.shape===shape&&s.cache===cache);
    assert.deepEqual(samples.map(s=>s.index).sort((a,b)=>a-b),Array.from({length:20},(_,i)=>i));
  }
  assert.equal(d.report.source.binary,data[0].report.source.binary);
  const product=source=>Object.fromEntries(Object.entries(source.files).filter(([name])=>!name.startsWith('labs/')));
  assert.deepEqual(product(d.report.source),product(data[0].report.source));
  assert.deepEqual(d.report.fixtures,data[0].report.fixtures);
  if(d.name!=='set1')assert.equal(d.report.source.inheritedFromReportSha256,data[0].report.originalReportSha256);
  const loads=d.report.samples.flatMap(s=>[s.loadBefore?.[0],s.loadAfter?.[0]]).filter(Number.isFinite);
  rows.push({set:d.name,trials:d.report.samples.length,completeNavigation:d.report.samples.filter(s=>s.navigationMeasurementsComplete).length,
    executionFailures:d.report.samples.filter(s=>s.status==='failed').map(s=>({variant:s.variant,shape:s.shape,cache:s.cache,index:s.index,error:s.error.split('\n')[0]})),
    browserVersions:[...new Set(d.report.samples.map(s=>s.browserVersion?.product))],loadMin:Math.min(...loads),loadMax:Math.max(...loads),
    admission:d.report.hostAdmission,started:d.report.started,finished:d.report.finished,
    fallbacks:d.report.samples.flatMap(s=>Object.entries(s.actionOutcomes).filter(([,outcome])=>outcome.colouring==='plain-fallback').map(([action])=>({variant:s.variant,shape:s.shape,cache:s.cache,index:s.index,action}))),
    correctness:Object.fromEntries([...new Set(d.report.samples.map(s=>s.variant))].map(variant=>{
      const ss=d.report.samples.filter(s=>s.variant===variant);
      const flags={};for(const name of ['initial','retained','evicted','source','tokenRoles','cleanConsole','externalRequests','copyIsolation','lifecycle','capture','exampleFailure','faults','runtimeCacheReused','fixtureRestored','renderParity'])
        flags[name]=ss.reduce((counts,s)=>{const label=checkStatus(name,s.checks[name]);counts[label]=(counts[label]||0)+1;return counts;},{});
      return[variant,{trials:ss.length,flags,failedRoles:ss.reduce((n,s)=>n+(s.failedRoles?.length||0),0),glyphMax:Math.max(...ss.map(s=>s.glyphGeometry?.maxDelta||0))}];
    }))});
}
assert.equal(data[1].report.source.fixedInputsSha256,data[2].report.source.fixedInputsSha256);
const phases=JSON.parse(fs.readFileSync(path.join(root,'pipeline-phases.json')));
const phaseGroups=new Map();
for(const sample of phases.samples)for(const [metric,value]of Object.entries(sample.observations)){
  assert(Number.isFinite(value));const k=sample.set+'|'+key({...sample,metric});
  if(!phaseGroups.has(k))phaseGroups.set(k,[]);phaseGroups.get(k).push(value);
}
assert.equal(phaseGroups.size,phases.metrics.length);
for(const row of phases.metrics){const values=phaseGroups.get(row.set+'|'+key(row)).sort((a,b)=>a-b);
  assert.deepEqual([values.length,values[Math.ceil(values.length/2)-1],values[Math.ceil(values.length*.95)-1],values[0],values.at(-1)],
    [row.n,row.p50,row.p95,row.min,row.max]);}
for(const input of phases.inputs){const d=data.find(set=>set.name===input.set);
  assert.equal(input.originalReportSha256,d.report.originalReportSha256);
  assert.equal(input.reviewedReportSha256,crypto.createHash('sha256').update(fs.readFileSync(path.join(root,input.set,'report.json'))).digest('hex'));}
assert.equal(phases.analysisToolSha256,crypto.createHash('sha256').update(fs.readFileSync(path.join(__dirname,'pipeline-phases.cjs'))).digest('hex'));
const userLatency=/^(first|second|retained|evicted|sourceFirst|sourceReopen|edit\.[^.]+)\.(visibleMs|colouredMs|visibleAndColouredMs|readableMs|highlightSettledMs)$/;
const comparisons=[];
for(const d of data)for(const row of d.summary.metrics.filter(r=>['1','2'].includes(r.variant)&&(userLatency.test(r.metric)||r.metric==='memory.settledCombinedRSS'))){
  const control=d.summary.metrics.find(r=>r.variant==='0'&&r.shape===row.shape&&r.cache===row.cache&&r.metric===row.metric);if(!control)continue;
  const controls=data.flatMap(set=>set.summary.metrics.filter(r=>r.variant==='0'&&r.shape===row.shape&&r.cache===row.cache&&r.metric===row.metric).map(r=>r.p50));
  const controlRange=Math.max(...controls)-Math.min(...controls),gain=control.p50-row.p50;
  const memory=row.metric==='memory.settledCombinedRSS',allowance=Math.max(memory?20*1024**2:5,control.p50*(memory?.2:.1));
  comparisons.push({set:d.name,variant:row.variant,shape:row.shape,cache:row.cache,metric:row.metric,n:row.n,controlN:control.n,controlP50:control.p50,candidateP50:row.p50,
    delta:row.p50-control.p50,relativeDelta:control.p50?(row.p50-control.p50)/control.p50:null,allowance,guardPass:memory?row.p50-control.p50<allowance:row.p50-control.p50<=allowance,controlRange,
    gainThresholdPass:!memory&&gain>=10&&gain>=control.p50*.2&&gain>controlRange});
}
const output={aggregationVerified:true,pipelineAggregationVerified:true,sourceAndFixturesVerified:true,thresholds:{latency:'delta <= max(5ms, 10% control p50)',settledCombinedRSS:'delta < max(20MiB, 20% control p50)',gain:'>=10ms and >=20% and greater than the cross-set control p50 range'},
  sets:rows,comparisons,analysisSha256:crypto.createHash('sha256').update(fs.readFileSync(__filename)).digest('hex')};
fs.writeFileSync(path.join(root,'assessment.json'),JSON.stringify(output,null,2));
console.log(JSON.stringify({verified:true,sets:rows.map(({set,trials,completeNavigation,executionFailures,loadMin,loadMax,browserVersions,fallbacks})=>({set,trials,completeNavigation,executionFailures,loadMin,loadMax,browserVersions,fallbacks})),
  guards:names.map(set=>({set,candidates:['1','2'].filter(variant=>comparisons.some(row=>row.set===set&&row.variant===variant)).map(variant=>({variant,latencyFailures:comparisons.filter(r=>r.set===set&&r.variant===variant&&r.metric!=='memory.settledCombinedRSS'&&!r.guardPass).length,memoryFailures:comparisons.filter(r=>r.set===set&&r.variant===variant&&r.metric==='memory.settledCombinedRSS'&&!r.guardPass)}))}))}));
