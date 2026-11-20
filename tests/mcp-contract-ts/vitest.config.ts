import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'node',
    pool: 'forks',
    isolate: true,
    include: ['mcp-contract.test.ts'],
    testTimeout: 30_000,
    hookTimeout: 20_000,
    teardownTimeout: 10_000,
    sequence: {
      concurrent: false,
    },
  },
});
