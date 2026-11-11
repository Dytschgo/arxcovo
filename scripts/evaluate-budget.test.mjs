import { test } from 'node:test';
import assert from 'node:assert/strict';
import { decideBudget, evaluatePullRequests } from './evaluate-budget.mjs';

function fixture() {
  const posts = [];
  const pr = { number: 1, state: 'open', head: { sha: 'a'.repeat(40) }, labels: [] };
  const io = { listPullRequests: () => [pr], getPullRequest: () => pr, getMainRun: () => ({ runId: 42, seconds: 40 }), getCandidateRun: () => null, postStatus: (sha, state, description) => posts.push({ sha, state, description }) };
  return { io, posts, pr };
}
test('fast main passes without history or a candidate measurement', async () => {
  const { io, posts } = fixture();
  io.getCandidateRun = () => assert.fail('fast baseline needs no candidate');
  await evaluatePullRequests(io);
  assert.equal(posts.at(-1).state, 'success');
  assert.match(posts.at(-1).description, /42: 40s/);
});
test('measurement failure invalidates old success for every discovered PR', async () => {
  const { io, posts, pr } = fixture();
  io.listPullRequests = () => [pr, { ...pr, number: 2, head: { sha: 'b'.repeat(40) } }];
  io.getMainRun = () => { throw Error('API failed'); };
  await assert.rejects(evaluatePullRequests(io));
  assert.deepEqual(posts.map(post => post.state), ['pending', 'pending', 'failure', 'failure']);
});
test('removed label cannot reuse the old ci-speed snapshot', async () => {
  const { io, posts, pr } = fixture();
  let reads = 0;
  io.getPullRequest = () => ({ ...pr, labels: ++reads === 1 ? [{ name: 'ci-speed' }] : [] });
  io.getMainRun = () => ({ runId: 42, seconds: 901 });
  io.getCandidateRun = () => ({ seconds: 800, conclusion: 'success' });
  await evaluatePullRequests(io);
  assert.equal(posts.at(-1).state, 'failure');
});
test('changed head stays pending and cannot inherit the previous decision', async () => {
  const { io, posts, pr } = fixture();
  let reads = 0;
  io.getPullRequest = () => ++reads === 1 ? pr : { ...pr, head: { sha: 'b'.repeat(40) } };
  await evaluatePullRequests(io);
  assert.equal(posts.at(-1).sha, 'b'.repeat(40));
  assert.equal(posts.at(-1).state, 'pending');
  assert.equal(posts.some(post => post.state === 'success'), false);
});
test('per-PR API errors and discovery bounds publish failure', async () => {
  const { io, posts, pr } = fixture();
  io.getPullRequest = () => { throw Error('unavailable'); };
  await evaluatePullRequests(io);
  assert.equal(posts.at(-1).state, 'failure');
  io.listPullRequests = () => Array.from({ length: 100 }, () => pr);
  await assert.rejects(evaluatePullRequests(io));
  assert.equal(posts.at(-1).state, 'failure');
});
test('speed labels require a successful reduction; absent baseline fails', () => {
  assert.equal(decideBudget({ seconds: 901 }, { seconds: 800, conclusion: 'success' }, [{ name: 'ci-speed' }]).state, 'success');
  assert.equal(decideBudget({ seconds: 901 }, { seconds: 902, conclusion: 'success' }, [{ name: 'ci-speed' }]).state, 'failure');
  assert.equal(decideBudget(null, null, []).state, 'failure');
});
