import { defineConfig } from 'vite';
export default defineConfig({
  server: {
    host: '127.0.0.1',
    port: Number(process.env.FRONTEND_PORT),
    strictPort: true,
    proxy: { '/api': `http://127.0.0.1:${process.env.BACKEND_PORT}` },
  },
});
