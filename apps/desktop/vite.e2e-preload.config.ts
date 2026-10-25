import { resolve } from 'node:path';
import { defineConfig } from 'vite';

export default defineConfig({
  build: {
    emptyOutDir: false,
    lib: {
      entry: resolve(import.meta.dirname, 'e2e/preload/index.cts'),
      fileName: () => 'index.cjs',
      formats: ['cjs'],
    },
    outDir: resolve(import.meta.dirname, 'dist/e2e/preload'),
    rolldownOptions: {
      external: ['electron'],
    },
    sourcemap: true,
    target: 'node22',
  },
});
