import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');

test('CI replaces superseded PR runs without cancelling main measurements', () => {
  const concurrency = workflow.match(/^concurrency:\r?\n((?:[ \t]+[^\n]*\n)+)/m)?.[1];
  assert.ok(concurrency, 'CI must define concurrency');
  assert.ok(concurrency.includes("  group: ${{ github.event_name == 'pull_request' && format('ci-pr-{0}', github.event.pull_request.number) || format('ci-run-{0}', github.run_id) }}"));
  assert.match(concurrency, /^ {2}cancel-in-progress: \$\{\{ github\.event_name == 'pull_request' \}\}\r?$/m);
  assert.equal((workflow.match(/^\s*concurrency:/gm) ?? []).length, 1, 'job-level concurrency must not override the policy');
  assert.match(workflow, /^ {4}timeout-minutes: 12\r?$/m);
  assert.equal((workflow.match(/^\s*timeout-minutes:/gm) ?? []).length, 1);
});
