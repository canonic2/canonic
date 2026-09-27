/* Reaching the implementations
   ----------------------------
   The one place the server talks to anything that isn't this machine's
   filesystem: a Storybook's index. Only addresses the
   config declares ever get here; the routes in server.js see to that.

   Plain node http and https, no dependencies, and every request has a
   deadline: a dev server that isn't running should answer "isn't answering"
   in a moment, not hang the workbench. */

var http = require('http');
var https = require('https');

var DEFAULT_TIMEOUT = 5000;

function client(url) {
  return url.protocol === 'https:' ? https : http;
}

/* One GET, the body collected. Answers { status, headers, body } as text;
   `json` parses it, or throws when it isn't JSON. */
function fetchText(address, options) {
  options = options || {};
  var url = new URL(address);
  var timeout = options.timeout || DEFAULT_TIMEOUT;

  return new Promise(function (resolve, reject) {
    var req = client(url).request(
      url,
      { method: 'GET', headers: { Accept: options.accept || '*/*' } },
      function (res) {
        var chunks = [];
        res.on('data', function (chunk) {
          chunks.push(chunk);
        });
        res.on('end', function () {
          resolve({
            status: res.statusCode,
            headers: res.headers,
            body: Buffer.concat(chunks).toString('utf8'),
          });
        });
        res.on('error', reject);
      }
    );
    req.setTimeout(timeout, function () {
      req.destroy(new Error('timed out after ' + timeout + 'ms'));
    });
    req.on('error', reject);
    req.end();
  });
}

function fetchJson(address, options) {
  return fetchText(address, Object.assign({ accept: 'application/json' }, options || {})).then(function (answer) {
    var parsed = null;
    try {
      parsed = JSON.parse(answer.body);
    } catch (error) {
      parsed = undefined;
    }
    return { status: answer.status, headers: answer.headers, body: parsed };
  });
}

module.exports = { fetchText: fetchText, fetchJson: fetchJson };
