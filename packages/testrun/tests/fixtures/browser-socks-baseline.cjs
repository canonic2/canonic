// Browser SOCKS creation, launch/context precedence and associated HTTP use an owned proxy only.
const http=require('node:http');
const path=require('node:path');
const {listen,socks}=require('./browser-socks-wire.cjs');
const playwright=require(process.argv[2]);
(async()=>{
  const records=[];let port;
  const pageBody='<!doctype html><link rel="icon" href="data:,"><title>Acme SOCKS</title><p>Acme</p>';
  const origin=await listen(http.createServer((req,res)=>{
    records.push({path:req.url,host:req.headers.host.replace(String(port),'PORT'),authorization:req.headers.authorization??'',proxyAuthorization:req.headers['proxy-authorization']??'',custom:req.headers['x-acme']??''});
    const page=req.url==='/Acme-page';const body=page?pageBody:'Acme';
    res.writeHead(200,{'Content-Type':page?'text/html':'text/plain','Content-Length':Buffer.byteLength(body),'Connection':'close'});res.end(body);
  }));port=origin.port;
  const primary=await socks(port,false,null,false,false,true),override=await socks(port,false,null,false,false,true);
  const endpoint=(owner,scheme)=>`${scheme}://AcmeIgnored:example@127.0.0.1:${owner.port}/ignored?Acme=1`;
  const engine=process.argv[3],headless=process.argv[4]==='headless';let browser;
  const launch=scheme=>playwright[engine==='chrome'?'chromium':engine].launch({headless,proxy:{server:endpoint(primary,scheme)},...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
  const cases=[],validation=[];
  try {
    for(const launchScheme of ['socks4','socks5']) {
      browser=await launch(launchScheme);
      for(const kind of ['initial','context_override','context_bypass','later']) {
        const contextScheme=launchScheme==='socks4'?'socks5':'socks4';
        const before={origin:records.length,primary:primary.connections.length,override:override.connections.length,primaryErrors:primary.protocolErrors.length,overrideErrors:override.protocolErrors.length,primaryPrefixes:primary.protocolPrefixes.length,overridePrefixes:override.protocolPrefixes.length};
        const row={name:`${launchScheme}-${kind}`};let context,explicit;let phase='context_create';
        const effective=kind.startsWith('context_')?contextScheme:launchScheme;
        // The reserved virtual fixture name resolves only in this loopback proxy.
        const host=effective==='socks5'&&kind!=='context_bypass'?'acme.invalid':'127.0.0.1';
        const base=`http://${host}:${port}`;
        try {
          context=await browser.newContext({baseURL:base,extraHTTPHeaders:{'X-Acme':'browser'},...(kind.startsWith('context_')?{proxy:{server:endpoint(override,contextScheme),...(kind==='context_bypass'?{bypass:' 127.0.0.1 '}:{})}}:{})});
          phase='page_navigate';const page=await context.newPage();
          row.pageStatus=(await page.goto(base+'/Acme-page')).status();row.pageText=await page.locator('p').innerText();
          phase='inherited_http';const response=await context.request.get('/Acme-http');row.httpStatus=response.status();row.httpBody=await response.text();await response.dispose();
          phase='explicit_http';const inherited=context._initializer.options;
          explicit=await playwright.request.newContext({proxy:{server:`${contextScheme}://127.0.0.1:${primary.port}`},extraHTTPHeaders:Object.fromEntries((inherited.extraHTTPHeaders??[]).map(header=>[header.name,header.value]))});
          const changed=await explicit.get(base+'/Acme-override');row.overrideStatus=changed.status();await changed.dispose();
          phase='source_close';await context.close();const refused=await context.request.get(base+'/Acme-after-close').then(()=>false,()=>true);row.sourceClosed=refused;context=undefined;
        } catch(error) {row.failure={phase,message:String(error.message).split('\n')[0]};}
        finally {await explicit?.dispose();await context?.close();}
        row.origin=records.slice(before.origin);row.primary=primary.connections.slice(before.primary);row.override=override.connections.slice(before.override);
        row.protocolErrors={primary:primary.protocolErrors.slice(before.primaryErrors),override:override.protocolErrors.slice(before.overrideErrors)};
        row.protocolPrefixes={primary:primary.protocolPrefixes.slice(before.primaryPrefixes),override:override.protocolPrefixes.slice(before.overridePrefixes)};
        cases.push(row);
      }
      for(const scheme of ['socks4','socks5'])for(const credentials of [{username:'Acme'},{password:'example'},{username:'',password:''}]) {
        const row={kind:'context',scheme,credentials};let context;
        try {context=await browser.newContext({proxy:{server:`${scheme}://127.0.0.1:${override.port}`,...credentials}});row.accepted=true;}
        catch(error){row.accepted=false;row.error=String(error.message).split('\n')[0];}
        finally{await context?.close();}validation.push(row);
      }
      await browser.close();browser=undefined;
    }
    for(const scheme of ['socks4','socks5'])for(const credentials of [{username:'Acme'},{password:'example'}]) {
      const row={kind:'launch',scheme,credentials};let unexpected;
      try {unexpected=await playwright[engine==='chrome'?'chromium':engine].launch({headless,proxy:{server:`${scheme}://127.0.0.1:${primary.port}`,...credentials},...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});row.accepted=true;}
      catch(error){row.accepted=false;row.error=String(error.message).split('\n')[0];}
      finally{await unexpected?.close();}validation.push(row);
    }
  } finally {await browser?.close();await primary.close();await override.close();await origin.close();}
  process.stdout.write(JSON.stringify({cases,validation,unselectedRefused:{primary:primary.unselected(),override:override.unselected()}}));
})().catch(error=>{console.error(error);process.exitCode=1;});
