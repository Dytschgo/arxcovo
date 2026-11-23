import { lstat as fsLstat, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

// Global teardown: runs before Playwright stops the web server. It asks this
// run's launcher (crates/arxd/tests/dashboard_live.rs) to stop through a stop
// file named by the per-run token. The launcher then kills arxd, removes only
// its own run directory and deletes the stop file to confirm.
//
// Confirmation needs positive evidence: only an inspection that fails with
// ENOENT proves the launcher removed the stop file. A stop file that is still
// present means the launcher has not confirmed (keep waiting until the
// deadline), and any other inspection error is a teardown failure.

export const CLEANUP_UNCONFIRMED = 'The live launcher did not confirm cleanup; its run directory under ARX_LIVE_ROOT may remain.';
export const CLEANUP_UNINSPECTABLE = 'The live launcher stop file cannot be inspected, so cleanup is not confirmed; its run directory under ARX_LIVE_ROOT may remain.';

export interface ConfirmOptions {
  timeoutMs?: number;
  intervalMs?: number;
  /** Test seam; defaults to node:fs/promises lstat. */
  lstat?: (file: string) => Promise<unknown>;
}

const isNotFound = (error: unknown) => (error as { code?: unknown } | null)?.code === 'ENOENT';

/**
 * Resolves only once inspecting `stop` fails with ENOENT. Rejects with a
 * fixed message if the file is still present at the deadline, or if an
 * inspection fails for any other reason.
 */
export async function waitForStopConfirmation(stop: string, options: ConfirmOptions = {}): Promise<void> {
  const inspect = options.lstat ?? fsLstat;
  const deadline = Date.now() + (options.timeoutMs ?? 20_000);
  for (;;) {
    try {
      await inspect(stop);
    } catch (error) {
      if (isNotFound(error)) return;
      throw new Error(CLEANUP_UNINSPECTABLE);
    }
    // Still present: the launcher has not confirmed yet.
    if (Date.now() > deadline) throw new Error(CLEANUP_UNCONFIRMED);
    await new Promise(resolve => setTimeout(resolve, options.intervalMs ?? 100));
  }
}

export default async function stopLauncher() {
  const root = process.env.ARX_LIVE_ROOT!;
  const token = process.env.ARX_LIVE_RUN!;
  const stop = path.join(root, `stop-${token}`);
  await writeFile(stop, '');
  try {
    await waitForStopConfirmation(stop);
  } catch (error) {
    // Withdraw this run's own stop request so it cannot affect a later run,
    // then report the fixed failure.
    await rm(stop, { force: true }).catch(() => undefined);
    throw error;
  }
}
