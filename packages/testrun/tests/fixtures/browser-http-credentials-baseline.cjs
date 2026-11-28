// Independent pinned SDK observation of browser auth and APIRequest auth.
const playwright = require(process.argv[2]);
(async () => {
  const origin = process.argv[5];
  const browser = await playwright[process.argv[3]].launch({headless:process.argv[4] === 'headless'});
  try {
    const report = [];
    for (const path of ['/auth/basic','/auth/quoted']) {
      const context = await browser.newContext({httpCredentials:[
        {origin:'http://example.invalid',username:'Acme',password:'pass',send:'always'},
        {origin,username:'Acme',password:'pass',send:'unauthorized'},
      ]});
      try {
        const api = await context.request.get(`${origin}${path}`);
        const apiRecord = await api.json();
        const page = await context.newPage();
        const response = await page.goto(`${origin}${path}`);
        report.push({path,apiStatus:api.status(),apiRecord,browserStatus:response.status(),browserRecord:JSON.parse(await response.text())});
      } finally { await context.close(); }
    }
    process.stdout.write(JSON.stringify(report));
  } finally { await browser.close(); }
})().catch(error => {console.error(error);process.exitCode=1;});
