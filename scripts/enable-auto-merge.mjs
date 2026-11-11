import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { readFileSync } from 'node:fs';
import { eligible } from './policy.mjs';

export function enableAutoMerge({ repository, number, head, allowlist }, run) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository) || !/^[1-9]\d*$/.test(String(number)) || !/^[a-f0-9]{40}$/.test(head)) {
    throw Error('Expected repository, positive PR number and exact head SHA');
  }
  if (!Array.isArray(allowlist) || allowlist.length === 0) throw Error('Owner-agent allowlist is empty');
  const api = (path) => JSON.parse(run(['api', `repos/${repository}/${path}`]));
  const pr = api(`pulls/${number}`);
  if (pr.state !== 'open' || pr.draft || pr.head.sha !== head || pr.base.ref !== 'main') throw Error('PR is not the expected open, ready main-branch candidate');
  if (!eligible(pr.user.login, pr.author_association, pr.head.repo?.full_name.toLowerCase() === repository.toLowerCase(), allowlist)) {
    throw Error('PR author or source is not eligible for agent auto-merge');
  }
  const protection = api('branches/main/protection');
  const required = protection.required_status_checks;
  const contexts = new Set([...(required?.contexts ?? []), ...(required?.checks ?? []).map(check => check.context)]);
  const expected = ['verify (ubuntu-latest)', 'verify (windows-latest)', 'verify (macos-latest)', 'ci-time-budget', 'agent-review'];
  if (!required?.strict || !protection.enforce_admins?.enabled || !expected.every(name => contexts.has(name))) {
    throw Error('Required CI/review protection is not installed');
  }
  if (!protection.required_pull_request_reviews || protection.allow_force_pushes?.enabled || protection.allow_deletions?.enabled) {
    throw Error('PR-only protected merge policy is not installed');
  }
  return run(['pr', 'merge', String(number), '--repo', repository, '--auto', '--squash', '--match-head-commit', head]);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [repository, number, head] = process.argv.slice(2);
  const allowlist = JSON.parse(readFileSync(new URL('../.github/agent-authors.json', import.meta.url), 'utf8'));
  enableAutoMerge({ repository, number, head, allowlist },
    args => execFileSync('gh', args, { encoding: 'utf8', maxBuffer: 1024 * 1024 }));
}
