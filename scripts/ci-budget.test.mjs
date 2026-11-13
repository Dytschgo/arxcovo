import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

function evaluate(scenario) {
  return JSON.parse(execFileSync(process.execPath, [
    '--experimental-vm-modules', '--disable-warning=ExperimentalWarning',
    fileURLToPath(new URL('./fixtures/ci-budget-buffer.mjs', import.meta.url)), scenario,
  ], { encoding: 'utf8', maxBuffer: 128 * 1024, timeout: 30000 }));
}

test('actual API/evaluator accepts workflow JSON above 1 MiB and selects same-head/repo latest first attempts', () => {
  const result = evaluate('large-candidate');
  assert.equal(result.error, null);
  assert.ok(Math.max(...result.payloadBytes) > 1024 * 1024);
  assert.deepEqual(result.transportErrors, []);
  assert.deepEqual(result.posts.map(post => post.state), ['pending', 'success']);
  assert.equal(result.posts.at(-1).sha, 'a'.repeat(40));
  assert.match(result.posts.at(-1).description, /Main run 20: 1200s/);
  assert.deepEqual(result.calls.filter(path => path.includes('/attempts/')), ['actions/runs/20/attempts/1', 'actions/runs/11/attempts/1']);
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
