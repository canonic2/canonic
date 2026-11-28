import test from 'node:test';
import assert from 'node:assert/strict';
import {AcmeFaults} from '../tests/fixtures/diagnostics/runtime.js';

test('Acme defective/corrected runtime faults have bounded work and explicit release', async () => {
  for (const corrected of [false, true]) {
    const fixture = new AcmeFaults({corrected});
    try {
      if (corrected) { assert.equal(fixture.exception(), 'ready'); assert.equal(await fixture.bridge(), 'ready'); assert.deepEqual(await fixture.network(), {status: 200}); }
      else { assert.throws(() => fixture.exception(), /Acme JavaScript/); await assert.rejects(fixture.bridge(), /simulated bridge/); await assert.rejects(fixture.network(), /simulated network/); }
      assert.equal(fixture.expensive(), corrected ? 4950 : 704982704);
      assert.equal(fixture.churn(), corrected ? 1 : 12);
      for (let n = 0; n < 3; n++) assert.equal(fixture.retain(), corrected ? 0 : 1048576);
      fixture.reset(); assert.equal(fixture.retainedBytes, 0); assert.equal(fixture.frames, 0); assert.equal(fixture.generation, 2);
    } finally { fixture.dispose(); }
    assert.equal(fixture.retainedBytes, 0); assert.throws(() => fixture.exception(), /disposed/);
  }
});
