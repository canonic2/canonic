/* Optional disk cache owned by one project's compiler. Build results contain
   Maps and Buffers, so V8 serialization avoids base64 copies of large bundles. */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const v8 = require('node:v8');
const hash = value => crypto.createHash('sha256').update(value).digest('hex');
const implementation = () => hash(fs.readdirSync(__dirname).filter(name => /\.(?:cjs|js)$/.test(name)).sort()
  .map(name => name + ':' + hash(fs.readFileSync(path.join(__dirname, name)))).join('\n'));

class BuildCache {
  constructor(root, options) {
    const identity = JSON.stringify([fs.realpathSync(root), options, implementation(), process.versions.v8,
      process.platform, process.arch, require('esbuild').version]);
    this.directory = path.join(os.tmpdir(), 'canonic-workbench-previews', hash(identity));
  }
  file(key) { return path.join(this.directory, hash(key) + '.bin'); }
  read(key) {
    try { return v8.deserialize(fs.readFileSync(this.file(key))); }
    catch (error) {
      // Missing, damaged or incompatible artifacts are cache misses. Source
      // compilation remains authoritative and replaces the artifact on success.
      if (error.code !== 'ENOENT') console.warn('Workbench build cache could not be read: ' + error.message);
      return null;
    }
  }
  write(key, value) {
    const temporary = this.file(key) + '.' + crypto.randomUUID() + '.tmp';
    try {
      fs.mkdirSync(this.directory, { recursive: true });
      fs.writeFileSync(temporary, v8.serialize(value));
      fs.renameSync(temporary, this.file(key));
    } catch (error) {
      console.warn('Workbench build cache could not be written: ' + error.message);
    } finally {
      try { fs.unlinkSync(temporary); } catch (error) { if (error.code !== 'ENOENT') console.warn('Workbench cache cleanup failed: ' + error.message); }
    }
  }
}
module.exports = { BuildCache };
