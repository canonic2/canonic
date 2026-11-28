// Public pinned SDK binding arguments, returns and thrown values; owned browsers only.
const assert=require('node:assert/strict'),path=require('node:path');
const cases=require('./browser-binding-values.json');
const playwright=require(process.argv[2]),utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const engine=process.argv[3],headless=process.argv[4]==='headless';
(async()=>{
  const report={engine,mode:headless?'headless':'headed',cases:[]};let browser,failure;
  try{
    browser=await playwright[engine==='chrome'?'chromium':engine].launch({headless,...(engine==='chrome'?{executablePath:path.join(process.argv[5],'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')}:{})});
    const context=await browser.newContext(),page=await context.newPage();
    let calls=0;
    await page.exposeBinding('AcmeBinding',(source,index,mode,value)=>{
      assert.equal(source.context,context);assert.equal(source.page,page);assert.equal(source.frame,page.mainFrame());
      const item=cases[index],check=(0,eval)(item.check),make=(0,eval)(item.make);
      if(!check(value))report.argumentMismatch={name:item.name,type:typeof value,tag:Object.prototype.toString.call(value),errorName:value?.name,message:value?.message,stack:value?.stack};
      assert.equal(check(value),true,`Argument ${item.name}`);calls++;
      if(mode==='reject')throw make();return make();
    });
    for(const mode of ['resolve','reject'])for(const [index,item] of cases.entries()){
      const result=await page.evaluate(async({index,mode,make,check})=>{
        const value=(0,eval)(make)(),matches=(0,eval)(check);
        try{const reply=await AcmeBinding(index,mode,value);return {outcome:'resolved',matches:matches(reply)};}
        catch(error){return {outcome:'rejected',matches:matches(error),...(!matches(error)?{mismatch:{type:typeof error,name:error?.name,message:error?.message,stack:error?.stack}}:{})};}
      },{index,mode,make:item.make,check:item.check});
      assert.deepEqual(result,{outcome:mode==='resolve'?'resolved':'rejected',matches:true},`${mode}/${item.name}`);
      report.cases.push({name:item.name,mode,...result});
    }
    assert.equal(calls,cases.length*2);report.calls=calls;
    await context.close();
  }catch(error){failure=error;report.failure={name:error?.name,message:error?.message,stack:error?.stack};}
  finally{try{await browser?.close();}finally{await utils.gracefullyCloseAll();}report.ownedProcesses=utils.gracefullyCloseSet.size;process.stdout.write(JSON.stringify(report));}
  if(failure){console.error(failure);process.exitCode=1;}
})().catch(error=>{console.error(error);process.exitCode=1;});
