import { resolve } from 'node:path';
import { defineConfig } from 'vite';

export default defineConfig({
  build: {
    emptyOutDir: false,
    lib: {
      entry: resolve(import.meta.dirname, 'preload/index.cts'),
      fileName: () => 'index.cjs',
      formats: ['cjs'],
    },
    outDir: resolve(import.meta.dirname, 'dist/preload'),
    rolldownOptions: {
      external: ['electron'],
    },
    sourcemap: true,
    target: 'node22',
  },
});
