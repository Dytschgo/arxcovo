import { defineConfig } from '@playwright/test';
import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Live-backend M6 acceptance (docs/m6-acceptance.md): a real `arxd` serves the
// production ui/dist and the dashboard API on loopback HTTP. The Rust launcher
// crates/arxd/tests/dashboard_live.rs builds the private state, provisions the
// password through the real CLI and runs the daemon until Playwright stops it.
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const dist = path.join(repo, 'ui', 'dist');
if (!existsSync(path.join(dist, 'index.html'))) throw new Error('Build the production UI first: npm run build');

// The runner evaluates this file before it forks workers and the web server,
// so both inherit the same values. The password is synthetic, per run and
// never printed.
process.env.ARX_LIVE_PORT ??= '47613';
process.env.ARX_LIVE_PASSWORD ??= `synthetic-${randomBytes(18).toString('base64url')}`;
process.env.ARX_LIVE_ROOT ??= path.join(tmpdir(), 'arx-live-dashboard');
process.env.ARX_LIVE_SHOTS ??= path.join(repo, 'test-results', 'm6-live-screenshots');
mkdirSync(process.env.ARX_LIVE_ROOT, { recursive: true });

const origin = `http://localhost:${process.env.ARX_LIVE_PORT}`;

export default defineConfig({
  testDir: '.',
  testMatch: '*.live.ts',
  outputDir: path.join(repo, 'test-results', 'm6-live'),
  workers: 1,
  fullyParallel: false,
  timeout: 30_000,
  use: { baseURL: origin, viewport: { width: 1280, height: 800 }, colorScheme: 'light' },
  webServer: {
    command: 'cargo test -p arxd --test dashboard_live -- --ignored --exact live_dashboard_server --nocapture',
    cwd: repo,
    url: `${origin}/`,
    // A cold Rust build of the launcher can take minutes; a warm one is seconds.
    timeout: 900_000,
    reuseExistingServer: false,
    stdout: 'pipe',
    stderr: 'pipe',
    env: {
      ARX_LIVE_DIST: dist,
      ARX_LIVE_PORT: process.env.ARX_LIVE_PORT,
      ARX_LIVE_PASSWORD: process.env.ARX_LIVE_PASSWORD,
      ARX_LIVE_ROOT: process.env.ARX_LIVE_ROOT,
    },
  },
});
