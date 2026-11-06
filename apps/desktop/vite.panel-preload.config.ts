import { resolve } from 'node:path';
import { defineConfig } from 'vite';

// Plugin panel pages have a preload of their own, built as one file: a sandboxed preload cannot
// load a chunk shared with the window's preload.
export default defineConfig({
  build: {
    emptyOutDir: false,
    lib: {
      entry: resolve(import.meta.dirname, 'preload/panel.cts'),
      fileName: () => 'panel.cjs',
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
