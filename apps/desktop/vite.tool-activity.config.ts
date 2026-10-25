import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';
import { defineConfig } from 'vite';

// Standalone test fixture: no desktop lifecycle, preload bridge or real data root.
export default defineConfig({
  plugins: [react()],
  root: resolve(import.meta.dirname, 'tests/tool-activity-preview'),
  server: {
    host: '127.0.0.1',
    port: 5176,
    strictPort: true,
    fs: { allow: [resolve(import.meta.dirname, '../..')] },
  },
});
