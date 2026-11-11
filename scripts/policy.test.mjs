import { test } from 'node:test';
import assert from 'node:assert/strict';
import { budget, eligible, duration } from './policy.mjs';
test('slow main blocks normal PR and unmeasured speed claims', () => {
  assert.throws(() => budget({ seconds: 901 }, null, false));
  assert.throws(() => budget({ seconds: 901 }, null, true));
  assert.throws(() => budget({ seconds: 901 }, { seconds: 902, conclusion: 'success' }, true));
  assert.throws(() => budget({ seconds: 901 }, { seconds: 800, conclusion: 'failure' }, true));
  assert.equal(budget({ seconds: 901 }, { seconds: 900, conclusion: 'success' }, true), true);
  assert.equal(budget({ seconds: 900 }, null, false), true);
  assert.throws(() => budget(null, null, false));
});
test('outsiders and forks never qualify for auto-merge', () => {
  assert.equal(eligible('agent', 'OWNER', true, ['agent']), true);
  assert.equal(eligible('agent', 'NONE', true, ['agent']), false);
  assert.equal(eligible('agent', 'OWNER', false, ['agent']), false);
  assert.equal(eligible('outsider', 'OWNER', true, ['agent']), false);
});
test('wall clock includes the entire completed run', () => {
  assert.equal(duration({ id: 1, status: 'completed', run_started_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:16:00Z' }).seconds, 960);
  assert.throws(() => duration({ status: 'in_progress' }));
});
