import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';
import { defineConfig } from 'vite';

export default defineConfig({
  base: './',
  plugins: [react(), tailwindcss()],
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
