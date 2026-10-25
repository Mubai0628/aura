import { defineConfig } from '@playwright/test';

export default defineConfig({
  forbidOnly: Boolean(process.env.CI),
  fullyParallel: false,
  outputDir: 'test-results',
  reporter: process.env.CI ? [['line'], ['html', { open: 'never' }]] : 'line',
  retries: process.env.CI ? 1 : 0,
  testDir: 'tests',
  timeout: 30_000,
  use: {
    trace: 'retain-on-failure',
  },
  workers: 1,
});
