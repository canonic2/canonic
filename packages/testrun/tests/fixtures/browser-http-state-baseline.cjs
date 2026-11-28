// Independent pinned isolated APIRequest state and cookie-wire observations.
const playwright = require(process.argv[2]);
(async () => {
  const origin = process.argv[3];
  const context = await playwright.request.newContext({storageState:JSON.parse(process.argv[4])});
  try {
    const initial = await context.storageState({indexedDB:true,opfs:true});
    const echo = await (await context.get(`${origin}/echo`)).json();
    const challenge = await context.get(`${origin}/auth/basic`);
    const report = {initial,echo,challengeStatus:challenge.status(),
      defaults:await context.storageState(),
      indexedDB:await context.storageState({indexedDB:true}),
      opfs:await context.storageState({opfs:true}),
      all:await context.storageState({indexedDB:true,opfs:true})};
    await context.clearCookies();
    report.cleared = await context.storageState({indexedDB:true,opfs:true});
    process.stdout.write(JSON.stringify(report));
  } finally { await context.dispose(); }
})().catch(error => {console.error(error);process.exitCode=1;});
