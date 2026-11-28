// Pinned caller-file precedence and immutable client certificate snapshots.
const fs=require('node:fs/promises'),path=require('node:path'),assert=require('node:assert/strict');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
(async()=>{
  const input=JSON.parse(await fs.readFile(process.argv[5],'utf8'));
  const root=path.join(process.cwd(),'Acme certificate inputs');await fs.mkdir(root,{mode:0o700});
  const cert=path.join(root,'Acme certificate.pem'),key=path.join(root,'Acme key.pem'),archive=path.join(root,'Acme archive.pfx');
  const bytes=name=>Buffer.from(input[name],'base64');
  await fs.writeFile(cert,bytes('certA'),{mode:0o600});await fs.writeFile(key,bytes('key'),{mode:0o600});
  await fs.writeFile(archive,bytes('pfx'),{mode:0o600});
  let browser,failure;const report={cases:{},lifetime:{}};
  const entry={origin:input.url,certPath:cert,keyPath:key};
  async function identity(client) {const response=await client.get(input.url);assert.equal(response.status(),200);const body=await response.text();await response.dispose();return body;}
  async function probe(name,selected) {
    let client,stage='create';
    try {client=await playwright.request.newContext({ignoreHTTPSErrors:true,clientCertificates:[selected]});stage='fetch';report.cases[name]={ok:true,identity:await identity(client)};}
    catch(error){report.cases[name]={ok:false,stage,name:error.name,message:error.message};}
    finally{await client?.dispose();}
  }
  try {
    await probe('absolutePem',entry);
    await probe('relativePem',{...entry,certPath:path.relative(process.cwd(),cert),keyPath:path.relative(process.cwd(),key)});
    await probe('pfx',{origin:input.url,pfxPath:archive,passphrase:input.passphrase});
    const encrypted=path.join(root,'Acme encrypted key.pem');await fs.writeFile(encrypted,bytes('encryptedKey'),{mode:0o600});
    await probe('encryptedPem',{...entry,keyPath:encrypted,passphrase:input.passphrase});
    const link=path.join(root,'Acme linked certificate.pem');await fs.symlink(cert,link);
    await probe('symlinkPem',{...entry,certPath:link});
    await probe('inlineWins',{...entry,cert:bytes('certA'),key:bytes('key'),certPath:path.join(root,'Acme absent certificate'),keyPath:path.join(root,'Acme absent key')});
    await probe('inlineEmptyWins',{...entry,cert:Buffer.alloc(0)});
    await probe('missing',{...entry,certPath:path.join(root,'Acme absent certificate')});
    const original=await playwright.request.newContext({ignoreHTTPSErrors:true,clientCertificates:[entry]});
    let changed;
    try {
      await fs.writeFile(cert,bytes('certB'));
      report.lifetime.originalAfterReplace=await identity(original);
      changed=await playwright.request.newContext({ignoreHTTPSErrors:true,clientCertificates:[entry]});
      report.lifetime.newAfterReplace=await identity(changed);
      await fs.unlink(cert);await fs.unlink(key);
      report.lifetime.originalAfterDelete=await identity(original);
      report.lifetime.newAfterDelete=await identity(changed);
    }finally{await original.dispose();await changed?.dispose();}
    if(process.argv[3]!=='chrome') {
      await fs.writeFile(cert,bytes('certA'),{mode:0o600});await fs.writeFile(key,bytes('key'),{mode:0o600});
      browser=await playwright[process.argv[3]].launch({headless:process.argv[4]==='headless'});
      const context=await browser.newContext({ignoreHTTPSErrors:true,clientCertificates:[entry]});
      try {
        await fs.unlink(cert);await fs.unlink(key);
        report.lifetime.associatedAfterDelete=await identity(context.request);
        const page=await context.newPage();await page.goto(input.url);
        report.lifetime.browserAfterDelete=await page.locator('body').innerText();
      }finally{await context.close();}
    }
  }catch(error){failure=error;report.failure={name:error.name,message:error.message};}
  finally {
    try{await browser?.close();}finally{await utils.gracefullyCloseAll();}
    report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));
    await fs.rm(root,{recursive:true,force:true});
  }
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
