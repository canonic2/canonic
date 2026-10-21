// Delete only reproducible generated state from finished runs owned by this lab.
const fs = require('node:fs');
const path = require('node:path');
function cleanup(root) {
  const removed = [];
  for (const name of fs.readdirSync(root)) {
    const dir = path.join(root, name);
    const file = path.join(dir, 'report.json');
    if (!/^\d{4}-\d{2}-\d{2}T/.test(name) || !fs.existsSync(file)) continue;
    const report = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (!report.finished) continue;
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const folder = path.join(dir, entry.name);
      if (/^product-[012D]$/.test(entry.name)) {
        fs.rmSync(folder, { recursive: true, force: true });
        removed.push(name + '/' + entry.name);
        continue;
      }
      if (!/^\d+-[012D]-(Small|Overview|Code-heavy)-(startup|settled)$/.test(entry.name)) continue;
      for (const generated of ['fixture', 'tmp', 'storage', 'profile']) {
        const generatedPath = path.join(folder, generated);
        if (fs.existsSync(generatedPath)) {
          fs.rmSync(generatedPath, { recursive: true, force: true });
          removed.push(name + '/' + entry.name + '/' + generated);
        }
      }
    }
  }
  return removed;
}
if (require.main === module)
  console.log(JSON.stringify({ removed: cleanup(path.resolve(process.argv[2])).length }));
module.exports = { cleanup };
