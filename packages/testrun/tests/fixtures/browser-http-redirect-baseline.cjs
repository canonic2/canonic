// Independent redirect wire effects through the unchanged pinned APIRequest client.
const {request} = require(process.argv[2]);
const options = JSON.parse(process.argv[4]);
const names = ['authorization','proxy-authorization','cookie','host','user-agent','accept','accept-encoding','x-acme','x-empty'];
(async () => {
  const report = {};
  for (const [name, url] of [['same',process.argv[3]+'/rewrite/307'],['cross',process.argv[3]+'/cross']]) {
    const context = await request.newContext(options);
    try {
      const response = await context.get(url);
      if (response.status() !== 200) throw new Error('Acme redirect baseline refused');
      const row = await response.json();
      report[name] = Object.fromEntries(names.filter(key => key in row.headers).map(key => [key,row.headers[key]]));
    } finally { await context.dispose(); }
  }
  process.stdout.write(JSON.stringify(report));
})().catch(error => {console.error(error);process.exitCode=1;});
