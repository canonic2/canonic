// Independent pinned SDK certificate selection and initial-context inheritance.
const fs=require('node:fs'),assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const input=JSON.parse(fs.readFileSync(process.argv[5],'utf8'));
const decode=entry=>Object.fromEntries(Object.entries(entry).map(([key,value])=>[key,['cert','key','pfx'].includes(key)?Buffer.from(value,'base64'):value]));
(async()=>{
  let browser,failure;const report={cases:{},initial:{}};
  async function observe(client,url) {
    const response=await client.get(url);assert.equal(response.status(),200);
    const identity=await response.text();await response.dispose();return identity;
  }
  try {
    for(const [name,certificates] of Object.entries(input.cases)) {
      const client=await playwright.request.newContext({ignoreHTTPSErrors:true,clientCertificates:certificates.map(decode)});
      try{report.cases[name]=await observe(client,input.first);}finally{await client.dispose();}
    }
    report.algorithmCases={};
    for(const [name,{url,certificates}] of Object.entries(input.algorithmCases||{})) {
      const client=await playwright.request.newContext({ignoreHTTPSErrors:true,clientCertificates:certificates.map(decode)});
      try{report.algorithmCases[name]=await observe(client,url);}finally{await client.dispose();}
    }
    if(process.argv[3]!=='chrome') {
      browser=await playwright[process.argv[3]].launch({headless:process.argv[4]==='headless'});
      for(const [name,certificates] of Object.entries(input.formats)) {
        const context=await browser.newContext({ignoreHTTPSErrors:true,clientCertificates:certificates.map(decode)});
        try {
          report.initial[name]={first:await observe(context.request,input.first),redirect:await observe(context.request,input.first+'/redirect')};
          const page=await context.newPage();await page.goto(input.first);
          report.initial[name].page=await page.locator('body').innerText();
        }finally{await context.close();}
      }
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally {
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
