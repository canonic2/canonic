import {defineConfig} from 'vite';
import solid from 'vite-plugin-solid';
export default defineConfig({plugins: [solid()], build: {sourcemap: true}, server: {host: '127.0.0.1', port: 4173, strictPort: true}, preview: {host: '127.0.0.1', port: 4173, strictPort: true}});
