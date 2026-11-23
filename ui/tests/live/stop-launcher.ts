import { existsSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';

// Global teardown: runs before Playwright stops the web server. It asks this
// run's launcher (crates/arxd/tests/dashboard_live.rs) to stop through a stop
// file named by the per-run token. The launcher then kills arxd, removes only
// its own run directory and deletes the stop file to confirm.
export default async function stopLauncher() {
  const root = process.env.ARX_LIVE_ROOT!;
  const token = process.env.ARX_LIVE_RUN!;
  const stop = path.join(root, `stop-${token}`);
  writeFileSync(stop, '');
  const deadline = Date.now() + 20_000;
  while (existsSync(stop)) {
    if (Date.now() > deadline) {
      rmSync(stop, { force: true });
      throw new Error('The live launcher did not confirm cleanup; its run directory under ARX_LIVE_ROOT may remain.');
    }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
}
