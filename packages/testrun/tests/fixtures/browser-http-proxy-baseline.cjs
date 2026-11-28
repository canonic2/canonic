// Independent CONNECT observations use the unchanged pinned APIRequest implementation.
const {request} = require(process.argv[2]);
(async () => {
  const context = await request.newContext(JSON.parse(process.argv[4]));
  try {
    const report = [];
    for (let i = 0; i < Number(process.argv[5]); ++i) {
      const response = await context.get(process.argv[3]);
      if (response.status() !== 200) throw new Error('Acme proxy baseline refused');
      const row = await response.json();
      report.push({host: row.headers.host, proxyAuthorization: row.headers['proxy-authorization'] ?? null});
    }
    process.stdout.write(JSON.stringify(report));
  } finally { await context.dispose(); }
})().catch(error => {console.error(error);process.exitCode = 1;});
