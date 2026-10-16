/** One producer per validated source/codec, with independent subscribers. */
interface Producer {
  start(request: Request): Promise<unknown>; stop(): void; close(): Promise<void>;
  accept(request: unknown, socket: unknown): boolean;
  acceptHttp(request: unknown, response: unknown): boolean;
  subscribers(): number;
}
interface Request { id: string; app?: string; source?: string; codec?: string }
export function createPool(create: () => Producer, options: { idleMs?: number } = {}) {
  const entries = new Map<string, { producer: Producer; idleSince: number; pending: number }>();
  const idleMs = options.idleMs ?? 30000;
  let closed = false;
  const timer = setInterval(() => {
    const now = Date.now();
    for (const [key, entry] of entries) {
      if (entry.pending || entry.producer.subscribers()) entry.idleSince = now;
      else if (now - entry.idleSince > idleMs) { entry.producer.stop(); entries.delete(key); }
    }
  }, Math.min(idleMs, 5000)); timer.unref();
  return {
    async start(request: Request) {
      if (closed) throw new Error('Native stream pool is closed.');
      const key = JSON.stringify([request.id, request.app, request.source, request.codec === 'jpeg' ? 'jpeg' : 'h264']);
      let entry = entries.get(key);
      if (!entry) { entry = { producer: create(), idleSince: Date.now(), pending: 0 }; entries.set(key, entry); }
      entry.idleSince = Date.now();
      entry.pending++;
      try { const result = await entry.producer.start(request); if (closed) throw new Error('Native stream pool is closed.'); return result; }
      catch (error) { if (entry.pending === 1 && !entry.producer.subscribers()) { entry.producer.stop(); entries.delete(key); } throw error; }
      finally { entry.pending--; }
    },
    accept(req: unknown, socket: unknown) { return [...entries.values()].some(e => e.producer.accept(req, socket)); },
    acceptHttp(req: unknown, res: unknown) { return [...entries.values()].some(e => e.producer.acceptHttp(req, res)); },
    stop(id?: string) { for (const [key, entry] of entries) if (!id || JSON.parse(key)[0] === id) { if (!entry.pending && !entry.producer.subscribers()) { entry.producer.stop(); entries.delete(key); } } },
    async close() { if (closed) return; closed = true; clearInterval(timer); await Promise.all([...entries.values()].map(e => e.producer.close())); entries.clear(); },
  };
}
