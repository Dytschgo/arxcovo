import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { duration, queueMeasurement, unfinishedMainRun } from './policy.mjs';
import { evaluateEvent, statusPublisher } from './evaluate-budget.mjs';

const repository = process.env.GITHUB_REPOSITORY;
if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository ?? '')) throw Error('Invalid repository');
const api = (path, args = []) => JSON.parse(execFileSync('gh', ['api', `repos/${repository}/${path}`, ...args], { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 }));
const runs = query => api(`actions/workflows/ci.yml/runs?${query}&per_page=100`).workflow_runs;
const MAX_ATTEMPT = 51; // GitHub allows 50 reruns after the first attempt.
const measure = (listed, kind, repositoryId, options = {}) => {
  const { attempt = 1 } = options;
  const path = `actions/runs/${listed.id}/attempts/${attempt}`;
  const run = api(path);
  const jobs = api(`${path}/jobs?per_page=100`);
  // Keep the existing completed-attempt baseline validation before optional credit.
  return queueMeasurement(duration(run), run, jobs, listed, kind, repositoryId, options);
};
const getMainRun = () => {
  const mainRuns = runs('branch=main&event=push');
  if (!Array.isArray(mainRuns) || mainRuns.some(run => !Number.isSafeInteger(run?.run_number) || run.run_number < 1)) {
    throw Error('Invalid main CI listing');
  }
  const listed = mainRuns.sort((a, b) => b.run_number - a.run_number)[0];
  if (!listed) return null;
  const latestAttempt = listed.run_attempt;
  if (!Number.isSafeInteger(listed.id) || listed.id < 1 ||
      !Number.isSafeInteger(latestAttempt) || latestAttempt < 1 || latestAttempt > MAX_ATTEMPT) throw Error('Invalid main CI listing');
  if (listed.status !== 'completed') return unfinishedMainRun(listed);
  const first = measure(listed, 'main');
  if (first.credit === 'unavailable:run-invalid' ||
      (first.conclusion === 'success' && first.credit !== 'withheld:not-green')) return first;
  for (let attempt = latestAttempt; attempt > 1; attempt--) {
    const retry = measure(listed, 'main', undefined, { attempt, expectedJobCount: first.jobs.length });
    if (retry.credit === 'unavailable:partial-rerun') continue;
    // Only a proven partial can be skipped. A full red or invalid attempt blocks.
    if (!retry.jobs.length) break;
    return { ...retry, attempt };
  }
  return first;
};
const statuses = statusPublisher({
  readStatus: sha => api(`commits/${sha}/status?per_page=100`).statuses.find(status => status.context === 'ci-time-budget') ?? null,
  createStatus: (sha, state, description) => api(`statuses/${sha}`, ['-X', 'POST', '-f', `state=${state}`, '-f', 'context=ci-time-budget', '-f', `description=${description}`]),
});
const postStatus = statuses.post;

if (process.argv.includes('--record')) {
  const latest = getMainRun();
  if (!latest || latest.pending) throw Error('No completed baseline to record');
  const path = 'docs/ci-duration-history.json';
  const records = JSON.parse(readFileSync(path, 'utf8'));
  if (!records.some(record => record.runId === latest.runId)) {
    // Preserve every existing record's bytes, including its whitespace.
    const original = readFileSync(path, 'utf8');
    const end = original.lastIndexOf(']');
    writeFileSync(path, original.slice(0, end).trimEnd() + (records.length ? ',' : '') + '\n' + JSON.stringify(latest, null, 2) + '\n' + original.slice(end));
  }
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
        return run ? measure(run, 'candidate', pr.head.repo?.id) : null;
      },
      postStatus,
  }, triggeringHead);
}
