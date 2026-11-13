import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { duration } from './policy.mjs';
import { evaluateEvent, statusPublisher } from './evaluate-budget.mjs';

const repository = process.env.GITHUB_REPOSITORY;
if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository ?? '')) throw Error('Invalid repository');
const api = (path, args = []) => JSON.parse(execFileSync('gh', ['api', `repos/${repository}/${path}`, ...args], { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 }));
const runs = query => api(`actions/workflows/ci.yml/runs?${query}&per_page=100`).workflow_runs;
const firstAttempt = run => api(`actions/runs/${run.id}/attempts/1`);
const getMainRun = () => {
  const run = runs('branch=main&event=push&status=completed').sort((a, b) => b.run_number - a.run_number)[0];
  return run ? duration(firstAttempt(run)) : null;
};
const statuses = statusPublisher({
  readStatus: sha => api(`commits/${sha}/status?per_page=100`).statuses.find(status => status.context === 'ci-time-budget') ?? null,
  createStatus: (sha, state, description) => api(`statuses/${sha}`, ['-X', 'POST', '-f', `state=${state}`, '-f', 'context=ci-time-budget', '-f', `description=${description}`]),
});
const postStatus = statuses.post;

if (process.argv.includes('--record')) {
  const latest = getMainRun();
  if (!latest) throw Error('No baseline to record');
  const path = 'docs/ci-duration-history.json';
  const records = JSON.parse(readFileSync(path, 'utf8'));
  if (!records.some(record => record.runId === latest.runId)) records.push(latest);
  writeFileSync(path, JSON.stringify(records, null, 2) + '\n');
} else {
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  const triggeringHead = event.workflow_run?.event === 'pull_request' ? event.workflow_run.head_sha : event.pull_request?.head.sha;
  await evaluateEvent({
      listPullRequests: () => api('pulls?state=open&per_page=100'),
      getPullRequest: number => api(`pulls/${number}`),
      getMainRun,
      getStatus: statuses.get,
      getCandidateRun: pr => {
        const run = runs('event=pull_request&status=completed')
          .filter(run => run.head_sha === pr.head.sha && run.head_repository?.id === pr.head.repo?.id)
          .sort((a, b) => b.run_number - a.run_number)[0];
        return run ? duration(firstAttempt(run)) : null;
      },
      postStatus,
  }, triggeringHead);
}
