import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  root: resolve(import.meta.dirname, 'renderer'),
  test: {
    environment: 'jsdom',
    // Waiting on rendered output is a wait on a promise, and this suite runs many
    // files across six workers: the one-second default is short enough that a
    // loaded machine fails a test that is not actually broken (observed as
    // `findByRole` timeouts just past 1000 ms). The per-test timeout is raised to
    // match, so a slow wait has room to finish while a query that can never
    // succeed still fails.
    setupFiles: ['./src/test-setup.ts'],
    globalSetup: [resolve(import.meta.dirname, 'tests/electron-binary.global-setup.ts')],
    testTimeout: 15_000,
    hookTimeout: 15_000,
    include: [
      '**/*.test.{ts,tsx}',
      '../electron-main/**/*.test.{ts,tsx}',
      '../shared/**/*.test.{ts,tsx}',
      '../tests/**/*.unit.ts',
    ],
  },
});
