const fs = require('node:fs');
const path = require('node:path');
const { overlay } = require('./overlay.cjs');
const { generate } = require('./fixtures.cjs');
const { portable } = require('./portable.cjs');
const root = path.resolve(process.argv[2]);
const dir = path.join(root, new Date().toISOString().replace(/[:.]/g, '-') + '-compat');
fs.mkdirSync(dir);
const report = {
  kind: 'portable compatibility smoke; outside performance cells',
  started: new Date().toISOString(),
  variants: [],
};
(async () => {
  for (const variant of ['0', '1', '2', 'D']) {
    const owned = path.join(dir, variant);
    fs.mkdirSync(owned);
    fs.mkdirSync(path.join(owned, 'tmp'));
    const product = overlay(path.join(owned, 'product'), variant, path.join(root, 'assets'));
    const fixture = path.join(owned, 'fixture');
    generate(fixture, 'Overview', path.join(root, 'assets'));
    const result = { variant };
    try {
      result.portable = await portable(product, fixture, owned);
      result.status = 'passed';
    } catch (error) {
      result.status = 'failed';
      result.error = String(error.stack || error);
    }
    report.variants.push(result);
    fs.writeFileSync(path.join(dir, 'report.json'), JSON.stringify(report, null, 2));
    console.log(
      JSON.stringify({ variant, status: result.status, error: result.error?.split('\n')[0] }),
    );
    for (const name of ['product', 'fixture', 'tmp', 'portable-profile'])
      fs.rmSync(path.join(owned, name), { recursive: true, force: true });
  }
  report.finished = new Date().toISOString();
  fs.writeFileSync(path.join(dir, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ report: path.join(dir, 'report.json') }));
})().catch((error) => {
  report.fatal = String(error);
  fs.writeFileSync(path.join(dir, 'report.json'), JSON.stringify(report, null, 2));
  console.error(error);
  process.exitCode = 1;
});
