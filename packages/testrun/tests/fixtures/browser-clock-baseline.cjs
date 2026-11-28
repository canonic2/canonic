// Unchanged pinned SDK reference for the shared numeric Clock scenario.
const path = require('node:path');
const playwright = require(process.argv[2]);
const [engine, mode, browsers, stepsJson] = process.argv.slice(3);
const methods = {install:'install',set_fixed_time:'setFixedTime',set_system_time:'setSystemTime',pause_at:'pauseAt',run_for:'runFor',fast_forward:'fastForward',resume:'resume'};
(async () => {
  let browser;
  try {
    browser = await playwright[engine === 'chrome' ? 'chromium' : engine].launch({headless:mode === 'headless',
      ...(engine === 'chrome' ? {executablePath:path.join(browsers,'chrome-156.0.8078.47/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing')} : {})});
    const context = await browser.newContext();
    const page = await context.newPage();
    const report = [];
    for (const step of JSON.parse(stepsJson)) {
      if (step.action) {
        const action = step.action;
        const argument = action.kind === 'install' ? {time:action.timeMs} : action.timeMs ?? action.durationMs;
        await context.clock[methods[action.kind]](argument);
      } else if (step.future) {
        const future = await context.newPage();
        report.push(JSON.parse(await future.evaluate(`(${step.future})()`)));
        await future.close();
      } else {
        report.push(JSON.parse(await page.evaluate(`(${step.evaluate})()`)));
      }
    }
    process.stdout.write(JSON.stringify(report));
  } finally { if (browser) await browser.close(); }
})().catch(error => { process.stderr.write(String(error.stack || error)); process.exitCode = 1; });
