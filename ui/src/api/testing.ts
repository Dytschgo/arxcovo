// Test helpers shared by unit tests. Not imported by application code.
import type { Schedule } from './standin';

/** A virtual clock: scheduled callbacks run only when `flush` advances time. */
export function manualClock() {
  let now = 0;
  let queue: { at: number; seq: number; fn: () => void; live: boolean }[] = [];
  let seq = 0;
  const schedule: Schedule = (fn, ms) => {
    const task = { at: now + ms, seq: seq++, fn, live: true };
    queue.push(task);
    return () => {
      task.live = false;
    };
  };
  /** Runs every task due within `ms` of virtual time, in order. */
  function advance(ms: number) {
    const until = now + ms;
    for (;;) {
      queue = queue.filter(t => t.live);
      queue.sort((a, b) => a.at - b.at || a.seq - b.seq);
      const next = queue[0];
      if (!next || next.at > until) break;
      queue.shift();
      now = next.at;
      next.fn();
    }
    now = until;
  }
  return { schedule, advance, pending: () => queue.filter(t => t.live).length };
}

/** Promise that resolves after the pending microtasks. */
export const tick = () => new Promise<void>(resolve => queueMicrotask(resolve));
