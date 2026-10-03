/* One release's notes from CHANGELOG.md
   -------------------------------------
   `node scripts/release-notes.cjs <version>` prints the body of that
   version's `## <version>` entry, for the GitHub Release. It fails when the
   entry is missing or empty, so a tag without notes stops before the builds. */
var fs = require('node:fs');
var path = require('node:path');

var CHANGELOG = path.resolve(__dirname, '..', 'CHANGELOG.md');

function notesFor(changelog, version) {
  var lines = String(changelog).split(/\r?\n/);
  var heading = /^##\s+v?(\S+)\s*$/;
  var start = lines.findIndex(function (line) {
    var match = heading.exec(line);
    return match && match[1] === version;
  });
  if (start === -1) throw new Error('CHANGELOG.md has no entry for ' + version + '. Add a "## ' + version + '" section.');
  var end = lines.findIndex(function (line, index) { return index > start && /^##?\s/.test(line); });
  var body = lines.slice(start + 1, end === -1 ? lines.length : end).join('\n').trim();
  if (!body) throw new Error('The CHANGELOG.md entry for ' + version + ' is empty.');
  return body + '\n';
}

module.exports = { notesFor: notesFor };

if (require.main === module) {
  try {
    var version = process.argv[2];
    if (!/^\d+\.\d+\.\d+$/.test(version || '')) throw new Error('Usage: node scripts/release-notes.cjs <major.minor.patch>');
    process.stdout.write(notesFor(fs.readFileSync(CHANGELOG, 'utf8'), version));
  } catch (error) {
    console.error(String(error.message || error));
    process.exitCode = 1;
  }
}
