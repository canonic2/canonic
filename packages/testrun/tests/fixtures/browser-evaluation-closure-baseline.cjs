// Unchanged SDK evaluation and event behavior for the 4.4 closure: a stable
// Frame across child navigation, page/context lifecycle events, and context and
// page extra headers on page and dedicated-worker requests.
// Arguments: playwright-core, engine, mode, browsers.
const http=require('node:http'),path=require('node:path');
const playwright=require(process.argv[2]);
const fixture=require('./browser-evaluation-closure-site.json');
(async()=>{
  const server=http.createServer((request,response)=>{
    const route=new URL(request.url,'http://127.0.0.1').pathname;
    if(route==='/headers'){response.writeHead(200,{'Content-Type':'text/plain'});response.end(request.headers['x-acme']||'none');return;}
    const body=fixture.paths[route];
    if(body===undefined){response.writeHead(404);response.end();return;}
    response.writeHead(200,{'Content-Type':route.endsWith('.js')?'text/javascript':'text/html'});
    response.end(body);
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin=`http://127.0.0.1:${server.address().port}`;
  const engine=process.argv[3],headless=process.argv[4]==='headless';
  const report={};let browser;
  // Shared page functions are source text; compile and call them in the page.
  const run=(page,source,argument)=>page.evaluate(([text,value])=>(0,eval)('('+text+')')(value),[source,argument]);
  try {
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    {
      const context=await browser.newContext();const page=await context.newPage();
      await page.goto(origin+'/frames');
      const frame=page.frames().find(candidate=>candidate!==page.mainFrame());
      const marker=()=>document.getElementById('marker').textContent;
      const one=await frame.evaluate(marker);
      const navigated=page.waitForEvent('framenavigated',candidate=>candidate===frame&&candidate.url().endsWith('/child-two'));
      await page.evaluate(()=>{document.getElementById('child').src='/child-two';});
      await navigated;await frame.waitForLoadState('load');
      report.frames={one,two:await frame.evaluate(marker),same:page.frames().includes(frame),
        selector:await frame.$eval('#marker',element=>element.textContent),
        embedding:await (await frame.frameElement()).evaluate(element=>element.id)};
      await context.close();
    }
    {
      const context=await browser.newContext();const page=await context.newPage();const events=new Set();
      for(const name of ['frameattached','framedetached','load','domcontentloaded','worker'])page.on(name,()=>events.add(name));
      context.on('serviceworker',()=>events.add('serviceworker'));
      await page.goto(origin+'/page');
      await run(page,fixture.lifecycle,engine==='chrome');
      const expected=engine==='chrome'?['worker','serviceworker']:['worker'];
      const end=Date.now()+5000;
      while(!expected.every(name=>events.has(name))&&Date.now()<end)await new Promise(resolve=>setTimeout(resolve,20));
      report.events=[...events].sort();
      await context.close();
    }
    {
      const context=await browser.newContext({extraHTTPHeaders:{'X-Acme':'context'}});const page=await context.newPage();
      await page.goto(origin+'/page');
      await page.setExtraHTTPHeaders({'X-Acme':'page'});
      report.headers=JSON.parse(await run(page,fixture.headers));
      await context.close();
    }
  } finally { if(browser)await browser.close(); }
  process.stdout.write(JSON.stringify(report),()=>process.exit(0));
})().catch(error=>{process.stderr.write(String(error.stack||error));process.exit(1);});
