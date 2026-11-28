// Compare unchanged WebKit with an independently owned Rust proxy endpoint.
const playwright=require(process.argv[2]);
const utils=require(process.argv[2]+'/lib/coreBundle.js').utils;
const proxy=new URL(process.argv[3]),destination=new URL(process.argv[4]);
if(proxy.protocol!=='socks4:'||proxy.hostname!=='127.0.0.1'||destination.protocol!=='http:'||destination.hostname!=='127.0.0.1')throw Error('Acme owned loopback endpoints required');
(async()=>{
  const started=performance.now();let browser,context;const report={};
  try {
    browser=await playwright.webkit.launch({headless:false,proxy:{server:proxy.href}});
    context=await browser.newContext({viewport:{width:640,height:480},deviceScaleFactor:1,extraHTTPHeaders:{'X-Acme':'browser'}});
    const page=await context.newPage();const navigation=performance.now();
    try {const response=await page.goto(destination.href);report.navigation={outcome:'succeeded',status:response?.status()};}
    catch(error){report.navigation={outcome:'failed',message:error.message};}
    report.navigation.elapsedMs=performance.now()-navigation;
  } finally {
    try {await context?.close();}
    finally {try {await browser?.close();} finally {await utils.gracefullyCloseAll();}}
    report.ownedProcesses=utils.gracefullyCloseSet.size;report.elapsedMs=performance.now()-started;
    process.stdout.write(JSON.stringify(report));
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
