import { test } from 'node:test';
import assert from 'node:assert/strict';
import { enableAutoMerge } from './enable-auto-merge.mjs';

const head = 'a'.repeat(40);
const input = { repository: 'example/arxcovo', number: 1, head, allowlist: ['agent'] };
function fixture({ author = 'agent', source = 'example/arxcovo', sha = head, protectedBranch = true } = {}) {
  const calls = [];
  const run = args => {
    calls.push(args);
    if (args[0] === 'pr') return 'enabled';
    if (args[1].endsWith('/pulls/1')) return JSON.stringify({ state: 'open', draft: false, user: { login: author }, author_association: 'OWNER', head: { sha, repo: { full_name: source } }, base: { ref: 'main' } });
    return JSON.stringify({ required_status_checks: { strict: true, contexts: protectedBranch ? ['verify (ubuntu-latest)', 'verify (windows-latest)', 'verify (macos-latest)', 'ci-time-budget', 'agent-review'] : [] }, enforce_admins: { enabled: true }, required_pull_request_reviews: {} });
  };
  return { calls, run };
}
test('only an allowlisted same-repository author can enable auto-merge at the expected head', () => {
  const { calls, run } = fixture();
  assert.equal(enableAutoMerge(input, run), 'enabled');
  assert.deepEqual(calls.at(-1), ['pr', 'merge', '1', '--repo', 'example/arxcovo', '--auto', '--squash', '--match-head-commit', head]);
});
test('outsiders, forks, stale heads and missing gates never invoke merge', () => {
  for (const options of [{ author: 'outsider' }, { source: 'outsider/arxcovo' }, { sha: 'b'.repeat(40) }, { protectedBranch: false }]) {
    const { calls, run } = fixture(options);
    assert.throws(() => enableAutoMerge(input, run));
    assert.equal(calls.some(args => args[0] === 'pr'), false);
  }
});
test('empty allowlist fails before accessing GitHub', () => {
  assert.throws(() => enableAutoMerge({ ...input, allowlist: [] }, () => assert.fail('must not access GitHub')));
});
