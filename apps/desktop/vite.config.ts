import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import {
  DEVELOPMENT_RENDERER_HEADER,
  isDevelopmentRendererInstance,
} from './shared/development-renderer.js';

const developmentInstance = process.env.AURA_DEV_RENDERER_INSTANCE;

export default defineConfig({
  base: './',
  plugins: [
    react(),
    tailwindcss(),
    {
      name: 'aura-development-renderer',
      configureServer(server) {
        if (!isDevelopmentRendererInstance(developmentInstance)) return;
        server.middlewares.use((_, response, next) => {
          response.setHeader(DEVELOPMENT_RENDERER_HEADER, developmentInstance);
          next();
        });
      },
    },
  ],
  root: resolve(import.meta.dirname, 'renderer'),
  server: {
    host: '127.0.0.1',
    port: 5175,
    strictPort: true,
  },
  build: {
    emptyOutDir: true,
    outDir: resolve(import.meta.dirname, 'dist/renderer'),
    // `font-src 'self'` refuses `data:` fonts, so no font file is inlined, however small.
    assetsInlineLimit: (file) => (/\.woff2?$/u.test(file) ? false : undefined),
  },
});
