import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

function evaluate(scenario) {
  return JSON.parse(execFileSync(process.execPath, [
    '--experimental-vm-modules', '--disable-warning=ExperimentalWarning',
    fileURLToPath(new URL('./fixtures/ci-budget-buffer.mjs', import.meta.url)), scenario,
  ], { encoding: 'utf8', maxBuffer: 512 * 1024, timeout: 30000 }));
}

test('an unrelated workflow start cannot cancel a trusted policy evaluation', () => {
  const workflow = readFileSync(new URL('../.github/workflows/policy.yml', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  assert.match(workflow, /types: \[requested, in_progress, completed\]/);
  assert.doesNotMatch(workflow, /^concurrency:/m);
  assert.match(workflow, /  evaluate:\n    if: github\.event\.action != 'in_progress' \|\| \(github\.event\.workflow_run\.event == 'push' && github\.event\.workflow_run\.head_branch == 'main'\)\n    concurrency:\n      group: trusted-ci-policy\n      cancel-in-progress: false\n/);
  assert.doesNotMatch(workflow, /^ {2}checks:/m);
});

test('actual API/evaluator accepts workflow JSON above 1 MiB and selects same-head/repo latest first attempts', () => {
  const result = evaluate('large-candidate');
  assert.equal(result.error, null);
  assert.equal(result.calls.length, 12);
  assert.ok(Math.max(...result.payloadBytes) > 1024 * 1024);
  assert.deepEqual(result.transportErrors, []);
  assert.deepEqual(result.posts.map(post => post.state), ['pending', 'success']);
  assert.equal(result.posts.at(-1).sha, 'a'.repeat(40));
  assert.match(result.posts.at(-1).description, /Main run 20: attempt 1: wall 1200s/);
  assert.deepEqual(result.calls.filter(path => path.includes('/attempts/')), ['actions/runs/20/attempts/1', 'actions/runs/20/attempts/1/jobs?per_page=100', 'actions/runs/11/attempts/1', 'actions/runs/11/attempts/1/jobs?per_page=100']);
});

for (const location of ['main', 'candidate']) {
  test(`actual API/evaluator fails closed for ${location} JSON above 8 MiB`, () => {
    const result = evaluate(`oversized-${location}`);
    assert.ok(result.error);
    assert.ok(Math.max(...result.payloadBytes) > 8 * 1024 * 1024);
    assert.deepEqual(result.transportErrors, ['ENOBUFS']);
    assert.equal(result.posts.at(-1).state, 'failure');
    assert.equal(result.posts.some(post => post.state === 'success'), false);
  });
}

test('first-attempt timing defeats a faster candidate listing', () => {
  const result = evaluate('slow-first-attempt');
  assert.equal(result.error, null);
  assert.equal(result.posts.at(-1).state, 'failure');
});

test('main measures the green first attempt despite a faster rerun', () => {
  const result = evaluate('recovery-green-first');
  assert.equal(result.error, null);
  assert.equal(result.posts.at(-1).state, 'success');
  assert.equal(result.posts.at(-1).description, 'Main run 20: attempt 1: wall 800s, charged 798s; budget satisfied');
  assert.equal(result.calls.some(path => path.includes('/attempts/2')), false);
});

test('main recovers a failed first attempt using only a green full rerun of the same run', () => {
  const result = evaluate('recovery-full-rerun');
  assert.equal(result.error, null);
  assert.deepEqual(result.posts.map(post => post.state), ['pending', 'success']);
  assert.equal(result.posts.at(-1).description, 'Main run 20: attempt 2: wall 613s, charged 611s; budget satisfied');
  assert.deepEqual(result.calls.filter(path => path.includes('/attempts/')), [
    'actions/runs/20/attempts/1', 'actions/runs/20/attempts/1/jobs?per_page=100',
    'actions/runs/20/attempts/2', 'actions/runs/20/attempts/2/jobs?per_page=100',
  ]);
});

test('main finds the latest full rerun behind a newer partial rerun', () => {
  const result = evaluate('recovery-latest-partial');
  assert.equal(result.error, null);
  assert.equal(result.posts.at(-1).state, 'success');
  assert.match(result.posts.at(-1).description, /attempt 2: wall 613s, charged 611s/);
  assert.deepEqual(result.calls.filter(path => /\/attempts\/\d$/.test(path)), [
    'actions/runs/20/attempts/1', 'actions/runs/20/attempts/3', 'actions/runs/20/attempts/2',
  ]);
});

for (const [scenario, readsRetry, reason] of [
  ['slow-first', false, /over 900s/],
  ['awaiting-rerun', false, /did not succeed/],
  ['partial-rerun', true, /did not succeed/],
  ['rerun-over-budget', true, /attempt 2: wall 913s, charged 911s.*over 900s/],
  ['red-rerun', true, /did not succeed/],
  ['mismatched-rerun', true, /did not succeed/],
  ['rerun-topology-changed', true, /did not succeed/],
  ['latest-full-red', false, /attempt 3:.*did not succeed/],
  ['latest-invalid', false, /did not succeed/],
]) {
  test(`main remains blocked with recovery scenario: ${scenario}`, () => {
    const result = evaluate(`recovery-${scenario}`);
    assert.equal(result.error, null);
    assert.equal(result.posts.at(-1).state, 'failure');
    assert.match(result.posts.at(-1).description, reason);
    assert.equal(result.calls.filter(path => path.includes('/attempts/2')).length, readsRetry ? 2 : 0);
    assert.equal(result.calls.some(path => path.includes('/attempts/') && !path.startsWith('actions/runs/20/')), false);
  });
}

for (const [scenario, attempt] of [['rerun-in-progress', 2], ['push-queued', 1]]) {
  test(`the latest main run blocks the older green baseline while ${scenario}`, () => {
    const result = evaluate(`recovery-${scenario}`);
    assert.equal(result.error, null);
    assert.deepEqual(result.posts.map(post => post.state), ['pending']);
    assert.equal(result.posts[0].description, `Main run 20: attempt ${attempt}: waiting for CI completion`);
    assert.equal(result.calls.some(path => path.includes('/attempts/') || path.includes('?event=pull_request')), false);
  });
}

for (const [scenario, attempt] of [['stale-rerun', 2], ['stale-queued', 1]]) {
  test(`the latest main run fails closed when ${scenario}`, () => {
    const result = evaluate(`recovery-${scenario}`);
    assert.equal(result.error, null);
    assert.deepEqual(result.posts.map(post => post.state), ['failure']);
    assert.match(result.posts[0].description, new RegExp(`Main run 20: attempt ${attempt}: stale-main-run.*cancel and re-run all jobs`));
    assert.equal(result.calls.some(path => path.includes('/attempts/') || path.includes('?event=pull_request')), false);
  });
}

test('an unfinished main run with no timestamp fails closed', () => {
  const result = evaluate('recovery-invalid-queue-time');
  assert.equal(result.error, null);
  assert.deepEqual(result.posts.map(post => post.state), ['failure']);
  assert.match(result.posts[0].description, /Main run 20: attempt 1: invalid-main-run-timestamp/);
  assert.equal(result.calls.some(path => path.includes('/attempts/')), false);
});

test('current labels still control the speed exception after a large candidate read', () => {
  const result = evaluate('removed-label');
  assert.equal(result.error, null);
  assert.equal(result.posts.at(-1).state, 'failure');
});

test('fast main first attempt needs no candidate listing', () => {
  const result = evaluate('fast-main');
  assert.equal(result.error, null);
  assert.equal(result.posts.at(-1).state, 'success');
  assert.equal(result.calls.some(path => path.includes('?event=pull_request')), false);
});

test('invalid repository fails before synthetic API access', () => {
  const result = evaluate('invalid-repository');
  assert.equal(result.error, 'Invalid repository');
  assert.deepEqual(result.calls, []);
  assert.deepEqual(result.posts, []);
});

for (const scenario of ['partial-jobs', 'attempt2-main', 'attempt2-candidate', 'attempt2-jobs', 'incomplete-main', 'fast-incomplete-main', 'missing-jobs', 'oversized-jobs']) {
  test(`VM entrypoint fails closed: ${scenario}; bounded attempt1 jobs, no second page`, () => {
    const result = evaluate(scenario);
    assert.equal(result.posts.at(-1).state, 'failure');
    assert.equal(result.posts.some(p => p.state === 'success'), false);
    assert.equal(result.calls.some(p => p.includes('page=2') || p.includes('attempts/2')), false);
    assert.ok(result.calls.filter(p => p.includes('/jobs?')).length <= 2);
    if (scenario === 'oversized-jobs') assert.deepEqual(result.transportErrors, ['ENOBUFS']);
    if (scenario === 'missing-jobs') assert.ok(result.error);
    if (scenario.startsWith('attempt2') || scenario === 'partial-jobs') assert.equal(result.error, null);
    if (scenario.includes('incomplete-main')) {
      assert.match(result.error, /Invalid completed workflow run/);
      assert.match(result.posts.at(-1).description, /Main CI measurement unavailable/);
    }
  });
}

test('synthetic record appends charged-v1 without modifying any old record bytes; duplicate is never written', () => {
  const result = evaluate('record-new');
  assert.equal(result.error, null);
  assert.deepEqual(result.posts, []);
  assert.ok(result.written.startsWith(result.initialHistory.slice(0, result.initialHistory.lastIndexOf(']')).trimEnd()));
  const before = JSON.parse(result.initialHistory), after = JSON.parse(result.written);
  assert.equal(after.length, before.length + 1);
  assert.deepEqual(after.slice(0, -1), before);
  assert.equal(after.at(-1).metric, 'charged-v1');
  assert.equal(after.at(-1).chargedSeconds, 1200);
  assert.equal(after.at(-1).jobs.length, 3);
  const duplicate = evaluate('record-duplicate');
  assert.equal(duplicate.error, null);
  assert.equal(duplicate.written, null);
});


test('unfinished attempt cannot be appended as a completed history baseline', () => {
  const result = evaluate('record-incomplete-main');
  assert.match(result.error, /Invalid completed workflow run/);
  assert.equal(result.written, null);
  assert.deepEqual(result.posts, []);
  const queued = evaluate('record-push-queued');
  assert.match(queued.error, /No completed baseline to record/);
  assert.equal(queued.written, null);
  assert.deepEqual(queued.posts, []);
});
