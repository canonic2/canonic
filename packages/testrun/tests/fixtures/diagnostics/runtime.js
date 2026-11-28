// App-owned Acme fault model. No network, native input or global registrations.
export class AcmeFaults {
  constructor({corrected = false} = {}) { this.corrected = corrected; this.retained = []; this.generation = 1; this.frames = 0; this.disposed = false; }
  check() { if (this.disposed) throw new Error('Acme fixture disposed'); }
  exception() { this.check(); if (!this.corrected) throw new Error('Acme JavaScript fixture failure'); return 'ready'; }
  async bridge() { this.check(); if (!this.corrected) throw new Error('Acme simulated bridge failure'); return 'ready'; }
  async network() { this.check(); if (!this.corrected) throw new Error('Acme simulated network failure'); return {status: 200}; }
  expensive() { this.check(); let checksum = 0; for (let n = 0; n < (this.corrected ? 100 : 100000); n++) checksum = (checksum + n) >>> 0; return checksum; }
  retain() { this.check(); this.retained = Array.from({length: 16}, () => new Uint8Array(65536)); if (this.corrected) this.retained = []; return this.retainedBytes; }
  get retainedBytes() { return this.retained.reduce((n, a) => n + a.byteLength, 0); }
  churn() { this.check(); this.frames = this.corrected ? 1 : 12; return this.frames; }
  reset() { this.check(); this.retained = []; this.frames = 0; this.generation++; }
  dispose() { this.retained = []; this.disposed = true; }
}
