/* The workbench-server binary for this platform, as the package builds it. */
var path = require('path');

function binaryPath() {
  return path.join(__dirname, '..', 'bin', process.platform + '-' + process.arch,
    'workbench-server' + (process.platform === 'win32' ? '.exe' : ''));
}

module.exports = { binaryPath: binaryPath };
