import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { budget, duration } from './policy.mjs';
const repository = process.env.GITHUB_REPOSITORY;
if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository ?? '')) throw Error('Invalid repository');
const get = (query) => JSON.parse(execFileSync('gh', ['api', `repos/${repository}/actions/workflows/ci.yml/runs?${query}&per_page=100`], { encoding: 'utf8' })).workflow_runs;
const runs = get('branch=main&event=push&status=completed');
const latestRun = runs.sort((a,b) => b.run_number - a.run_number)[0];
const latest = latestRun ? duration(latestRun) : null;
const history = readFileSync('docs/ci-duration-history.json', 'utf8');
if (process.argv.includes('--record')) {
  if (!latest) throw Error('No baseline to record');
  const records = JSON.parse(history);
  if (!records.some(r => r.runId === latest.runId)) records.push(latest);
  writeFileSync('docs/ci-duration-history.json', JSON.stringify(records, null, 2) + '\n');
} else {
  const api = (path, args = []) => JSON.parse(execFileSync('gh', ['api', `repos/${repository}/${path}`, ...args], { encoding: 'utf8', maxBuffer: 1024 * 1024 }));
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  const prs = event.pull_request ? [event.pull_request] : api('pulls?state=open&per_page=100');
  if (prs.length >= 100) throw Error('Too many open PRs; bounded evaluation refused');
  for (const pr of prs) {
    let state = 'failure';
    let description = 'CI time budget evaluation failed';
    try {
      const candidates = get('event=pull_request&status=completed');
      const run = candidates.filter(r => r.head_sha === pr.head.sha && r.pull_requests?.some(p => p.number === pr.number)).sort((a,b) => b.run_number - a.run_number)[0];
      budget(latest, run ? duration(run) : null, pr.labels.some(l => l.name === 'ci-speed'));
      const file = api(`contents/docs/ci-duration-history.json?ref=${pr.head.sha}`);
      if (file.size > 65536) throw Error('History exceeds bound');
      const records = JSON.parse(Buffer.from(file.content, 'base64').toString('utf8'));
      if (!records.some(r => r.runId === latest.runId && r.seconds === latest.seconds)) throw Error('Latest baseline missing from committed history');
      state = 'success'; description = 'Latest main CI meets budget or measured reduction';
    } catch (error) { console.error(pr.number, error.message); }
    api(`statuses/${pr.head.sha}`, ['-X', 'POST', '-f', `state=${state}`, '-f', 'context=ci-time-budget', '-f', `description=${description}`]);
  }
}
