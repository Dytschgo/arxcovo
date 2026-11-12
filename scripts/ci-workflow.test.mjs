import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');

test('CI replaces superseded PR runs without cancelling main measurements', () => {
  const concurrency = workflow.match(/^concurrency:\r?\n((?:[ \t]+[^\n]*\n)+)/m)?.[1];
  assert.ok(concurrency, 'CI must define concurrency');
  assert.match(concurrency, /^  group: ci-\$\{\{ github\.event\.pull_request\.number \|\| github\.ref \}\}\r?$/m);
  assert.match(concurrency, /^  cancel-in-progress: \$\{\{ github\.event_name == 'pull_request' \}\}\r?$/m);
});
