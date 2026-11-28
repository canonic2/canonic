// Independent request-default observation through the unchanged pinned SDK.
const {request} = require(process.argv[2]);
(async () => {
  const context = await request.newContext(process.argv[4] === 'empty' ? {userAgent:''} : {});
  try {
    const response = await context.get(process.argv[3]);
    if (response.status() !== 200) throw new Error('Baseline fixture refused');
    process.stdout.write(JSON.stringify({status:response.status(),body:await response.text()}));
  } finally {
    await context.dispose();
  }
})().catch(error => {console.error(error);process.exitCode=1;});
