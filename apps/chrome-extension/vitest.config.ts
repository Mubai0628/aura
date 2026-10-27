import { defineConfig } from 'vitest/config';

export default defineConfig({
  define: { __AURA_HOST_NAME__: JSON.stringify('com.aura.chrome_control.ttest') },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
