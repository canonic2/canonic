/* One compatibility script for every project page served by Workbench.
   Keep the source modules separate: keys.js is also used by the shell. */
var fs = require('node:fs');
var path = require('node:path');

var TAG = '<script src="/_workbench/preview-compat.js"></script>';
var source = ['keys.js', 'actions.js', 'states.js'].map(function (name) {
  return fs.readFileSync(path.join(__dirname, 'workbench', name), 'utf8');
}).join('\n;\n');

function withPreviewScripts(html) {
  var at = /<head(\s[^>]*)?>/i.exec(html) || /<html(\s[^>]*)?>/i.exec(html);
  if (!at) return TAG + html;
  var end = at.index + at[0].length;
  return html.slice(0, end) + TAG + html.slice(end);
}

module.exports = { source: source, withPreviewScripts: withPreviewScripts };
