// Real, owned Node/V8 CPU profile and actual fault-model observations.
import {Session} from 'node:inspector';
import {mkdir, writeFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';
import {AcmeFaults} from './runtime.js';

const directory = process.argv[2];
if (process.argv.length !== 3 || !directory || !isAbsolute(directory)) throw new Error('Select a new absolute owned corpus directory');
await mkdir(directory, {mode: 0o700}); // Refuse an existing destination; never overwrite.
const write = (name, value) => writeFile(join(directory, name), JSON.stringify(value), {flag: 'wx', mode: 0o600});
const session = new Session(); session.connect();
const post = (method) => new Promise((accept, reject) => session.post(method, (error, value) => error ? reject(error) : accept(value)));
try {
  await post('Profiler.enable'); await post('Profiler.start');
  const workload = new AcmeFaults();
  try { for (let n = 0; n < 64; n++) workload.expensive(); }
  finally { workload.dispose(); }
  const {profile} = await post('Profiler.stop'); await write('profile.cpuprofile', profile);
  await post('Profiler.disable');
} finally { session.disconnect(); }
for (const mode of ['defective', 'corrected', 'insufficient']) {
  const fixture = new AcmeFaults({corrected: mode === 'corrected'});
  try {
    const records = [];
    if (mode !== 'insufficient') {
      let state = 'ready'; try { fixture.exception(); } catch { state = 'failed'; }
      records.push({kind: state === 'failed' ? 'failure' : 'event', name: 'Acme checkout', data: {state, runtime: 'node', buildId: `acme-${mode}`}});
      records.push({kind: 'metric', name: 'Acme retained buffers', value: fixture.retain(), unit: 'bytes', definition: 'Acme fixture-owned Uint8Array byte lengths; not a heap census', data: {runtime: 'node', buildId: `acme-${mode}`}});
    }
    await write(`${mode}.json`, {schemaVersion: 1, records});
    fixture.reset(); if (fixture.retainedBytes !== 0) throw new Error('Acme reset did not release its references');
  } finally { fixture.dispose(); }
}
console.log(JSON.stringify({runtime: 'node', version: process.version, v8: process.versions.v8, cpuProfile: 'native inspector output', observations: 'actual bounded JS fixture model; not RN/Expo native capture'}));
