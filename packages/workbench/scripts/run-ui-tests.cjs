/* UI acceptance requires a real browser; missing Chrome must not look green.
 * Keep this runner outside Node's test-*.cjs discovery pattern. */
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { findChrome } = require('./chrome.cjs');

if (!findChrome()) {
  console.error(
    'UI tests require Chrome/Chromium. Install it or set CHROME_PATH to its executable.',
  );
  process.exit(1);
}
const root = path.resolve(__dirname, '..');
const tests = fs
  .readdirSync(path.join(root, 'ui/src'), { recursive: true })
  .filter((file) => file.endsWith('.test.js'))
  .sort()
  .map((file) => path.join('ui/src', file));
if (!tests.length) throw new Error('No UI tests discovered.');
const result = spawnSync(
  process.execPath,
  ['--test', '--test-concurrency=2', 'scripts/ui-quality.test.cjs', ...tests],
  {
    cwd: root,
    stdio: 'inherit',
  },
);
if (result.error) throw result.error;
process.exit(result.status ?? 1);
