import { defineConfig } from 'vitest/config';

export default defineConfig({ define: { __ARX_STANDIN__: 'true' }, test: { include: ['src/**/*.test.{ts,tsx}'], environment: 'jsdom', setupFiles: ['src/test-setup.ts'] } });
