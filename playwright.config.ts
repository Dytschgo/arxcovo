import { defineConfig } from '@playwright/test';
export default defineConfig({ testDir: './ui/tests', use: { baseURL: 'http://127.0.0.1:5173' }, webServer: { command: 'npm run dev -w ui -- --port 5173 --strictPort', url: 'http://127.0.0.1:5173', reuseExistingServer: false } });
