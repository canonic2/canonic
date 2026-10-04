import express from 'express';
import { createClient } from 'redis';
import { readFile } from 'node:fs/promises';
const sessionId = process.env.STUDIO_SESSION_ID;
const redis = createClient({
  url: `redis://127.0.0.1:${process.env.REDIS_PORT}`,
  socket: { reconnectStrategy: false },
});
redis.on('error', (error: unknown) =>
  console.error(error instanceof Error ? error.message : String(error)),
);
await redis.connect();
const app = express();
const info = async () => ({
  sessionId,
  checkout: process.env.STUDIO_CHECKOUT,
  pid: process.pid,
  backendPort: Number(process.env.BACKEND_PORT),
  redisPort: Number(process.env.REDIS_PORT),
  title: (await readFile(new URL('../../title.txt', import.meta.url), 'utf8')).trim(),
  counter: Number((await redis.get('counter')) || 0),
});
app.get('/api/health', async (request, response) => {
  await redis.ping();
  response.json(await info());
});
app.post('/api/increment', async (request, response) => {
  await redis.incr('counter');
  response.json(await info());
});
const server = app.listen(Number(process.env.BACKEND_PORT), '127.0.0.1', () =>
  console.log('EXPRESS_READY', sessionId, process.pid),
);
process.on('SIGTERM', () =>
  server.close(() => {
    void redis.close().then(
      () => process.exit(0),
      (error) => {
        console.error(error);
        process.exit(1);
      },
    );
  }),
);
