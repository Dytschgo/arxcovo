import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

function evaluate(scenario) {
  return JSON.parse(execFileSync(process.execPath, [
    '--experimental-vm-modules', '--disable-warning=ExperimentalWarning',
    fileURLToPath(new URL('./fixtures/ci-budget-buffer.mjs', import.meta.url)), scenario,
  ], { encoding: 'utf8', maxBuffer: 512 * 1024, timeout: 30000 }));
}

test('actual API/evaluator accepts workflow JSON above 1 MiB and selects same-head/repo latest first attempts', () => {
  const result = evaluate('large-candidate');
  assert.equal(result.error, null);
  assert.equal(result.calls.length, 12);
  assert.ok(Math.max(...result.payloadBytes) > 1024 * 1024);
  assert.deepEqual(result.transportErrors, []);
  assert.deepEqual(result.posts.map(post => post.state), ['pending', 'success']);
  assert.equal(result.posts.at(-1).sha, 'a'.repeat(40));
  assert.match(result.posts.at(-1).description, /Main run 20: wall 1200s/);
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

for (const scenario of ['partial-jobs', 'attempt2-main', 'attempt2-candidate', 'attempt2-jobs', 'incomplete-main', 'missing-jobs', 'oversized-jobs']) {
  test(`VM entrypoint fails closed: ${scenario}; bounded attempt1 jobs, no second page`, () => {
    const result = evaluate(scenario);
    assert.equal(result.posts.at(-1).state, 'failure');
    assert.equal(result.posts.some(p => p.state === 'success'), false);
    assert.equal(result.calls.some(p => p.includes('page=2') || p.includes('attempts/2')), false);
    assert.ok(result.calls.filter(p => p.includes('/jobs?')).length <= 2);
    if (scenario === 'oversized-jobs') assert.deepEqual(result.transportErrors, ['ENOBUFS']);
    if (scenario === 'missing-jobs') assert.ok(result.error);
    if (scenario.startsWith('attempt2') || scenario === 'partial-jobs') assert.equal(result.error, null);
    if (scenario === 'incomplete-main') {
      assert.equal(result.error, null);
      assert.match(result.posts.at(-1).description, /wall 1200s, charged n\/a \(run-invalid\)/);
    }
  });
}

test('synthetic record appends charged-v1 without modifying any old record bytes; duplicate is never written', () => {
  const result = evaluate('record-new');
  assert.equal(result.error, null);
  assert.deepEqual(result.posts, []);
  assert.ok(result.written.startsWith(result.initialHistory.slice(0, result.initialHistory.lastIndexOf(']')).trimEnd()));
  const before = JSON.parse(result.initialHistory), after = JSON.parse(result.written);
  assert.equal(before.length, 30);
  assert.deepEqual(after.slice(0, -1), before);
  assert.equal(after.at(-1).metric, 'charged-v1');
  assert.equal(after.at(-1).chargedSeconds, 1200);
  assert.equal(after.at(-1).jobs.length, 3);
  const duplicate = evaluate('record-duplicate');
  assert.equal(duplicate.error, null);
  assert.equal(duplicate.written, null);
});
