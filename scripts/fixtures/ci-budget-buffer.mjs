// Runs only synthetic IO. No gh executable, credentials, or network are used.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempDisposableSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { SourceTextModule, SyntheticModule, createContext } from 'node:vm';

const scenario = process.argv[2];
const probesBuffer = ['large-candidate', 'removed-label'].includes(scenario) || scenario.startsWith('oversized-');
const incompleteMain = ['incomplete-main', 'fast-incomplete-main', 'record-incomplete-main'].includes(scenario);
const mainSeconds = ['fast-main', 'fast-incomplete-main', 'record-incomplete-main'].includes(scenario) ? 800 : 1200;
const recovery = scenario.startsWith('recovery-');
const activeMain = scenario === 'recovery-rerun-in-progress' ? 'in_progress'
  : ['recovery-push-queued', 'record-push-queued'].includes(scenario) ? 'queued' : null;
const head = 'a'.repeat(40);
const repository = scenario === 'invalid-repository' ? 'https://unsafe' : 'example/arxcovo';
const prefix = 'repos/example/arxcovo/';
const calls = [];
const posts = [];
const payloadBytes = [];
const transportErrors = [];
let prReads = 0;
const pr = { number: 1, state: 'open', head: { sha: head, repo: { id: 7 } }, labels: recovery ? [] : [{ name: 'ci-speed' }] };
const run = (id, seconds, extra = {}) => ({
  run_attempt: 1, path: '.github/workflows/ci.yml', event: id === 20 || id === 19 ? 'push' : 'pull_request', head_branch: 'main',
  id, run_number: id, head_sha: head, head_repository: { id: 7 },
  status: 'completed', conclusion: 'success',
  run_started_at: '2026-01-01T00:00:00Z',
  updated_at: new Date(Date.parse('2026-01-01T00:00:00Z') + seconds * 1000).toISOString().replace('.000Z', 'Z'),
  ...extra,
});

const at = seconds => new Date(Date.UTC(2026, 0, 1) + seconds * 1000).toISOString().replace('.000Z', 'Z');
const completedStep = name => ({ name, status: 'completed', conclusion: 'success' });
function recoveryRun(attempt) {
  if (attempt === 1) return run(20, 913, { conclusion: scenario === 'recovery-slow-first' ? 'success' : 'failure' });
  return run(20, 613, { run_attempt: 2, run_started_at: at(2000), updated_at: at(2613) });
}

function recoveryJobs(attempt) {
  const offset = attempt === 1 ? 0 : 2000;
  const firstRed = attempt === 1 && scenario !== 'recovery-slow-first';
  const workerEnd = attempt === 1 ? 903 : 600;
  const jobs = ['rust', 'ui', 'verify'].flatMap((phase, matrix) => ['ubuntu-latest', 'windows-latest', 'macos-latest'].map((os, index) => {
    const gate = phase === 'verify';
    const job = {
      id: matrix * 3 + index + 1, run_id: 20, run_attempt: attempt, head_sha: head,
      name: `${phase} (${os})`,
      labels: [gate ? 'ubuntu-latest' : phase === 'rust' && index === 0 ? 'ubuntu-24.04' : os],
      status: 'completed', conclusion: 'success', runner_id: 1,
      created_at: at(offset + (gate ? workerEnd : 0)),
      started_at: at(offset + (gate ? workerEnd + 2 : 0)),
      completed_at: at(offset + (gate ? workerEnd + 6 : firstRed ? 400 : workerEnd)),
      steps: [{ number: 1, status: 'completed', conclusion: 'success' }],
    };
    if (firstRed && gate) {
      job.conclusion = 'failure';
      job.steps = ['success', 'failure', 'success'].map((conclusion, i) => ({ number: i + 1, status: 'completed', conclusion }));
    }
    if (firstRed && phase === 'rust' && index === 1) {
      job.conclusion = scenario === 'recovery-worker-failure' ? 'failure' : 'cancelled';
      job.completed_at = at(scenario === 'recovery-unknown-cancellation' ? 719 : 749);
      job.steps = ['Set up job', 'Verify pinned SQLite binding build', 'Preflight loader environment and run workspace tests', 'Complete job'].map(completedStep);
      if (scenario === 'recovery-setup-only') job.steps = job.steps.slice(0, 1);
      if (scenario === 'recovery-test-cancelled') job.steps[2].conclusion = 'cancelled';
    }
    if (firstRed && phase === 'rust' && index === 2) {
      Object.assign(job, { conclusion: 'cancelled', runner_id: 0, steps: [], completed_at: at(workerEnd) });
    }
    job.check_run_url = `https://api.github.com/${prefix}check-runs/${job.id}`;
    if (job.id === 2 && scenario === 'recovery-wrong-check-run') job.check_run_url += '9';
    return job;
  }));
  if (attempt === 2 && scenario === 'recovery-partial-rerun') jobs[0].started_at = at(0);
  if (attempt === 2 && scenario === 'recovery-red-rerun') jobs[0].conclusion = 'failure';
  if (attempt === 2 && scenario === 'recovery-mismatched-rerun') jobs[0].run_attempt = 1;
  if (attempt === 2 && scenario === 'recovery-rerun-topology-changed') {
    const gates = jobs.slice(6).map((job, i) => ({ ...job, labels: [`${['ubuntu', 'windows', 'macos'][i]}-latest`] }));
    return { total_count: gates.length, jobs: gates };
  }
  return { total_count: jobs.length, jobs };
}

function response(path, args) {
  if (path === 'pulls?state=open&per_page=100') return [pr];
  if (path === 'pulls/1') {
    prReads++;
    return { ...pr, labels: scenario === 'removed-label' && prReads > 1 ? [] : pr.labels };
  }
  if (path === `commits/${head}/status?per_page=100`) return {
    statuses: activeMain ? [{ context: 'ci-time-budget', state: 'success', description: 'Main run 19: attempt 1: wall 600s, charged 600s; budget satisfied' }] : [],
  };
  if (path === `statuses/${head}`) {
    assert.deepEqual(Array.from(args.slice(0, 2)), ['-X', 'POST']);
    assert.equal(args[5], 'context=ci-time-budget');
    posts.push({ sha: head, state: args[3].slice('state='.length), description: args[7].slice('description='.length) });
    return {};
  }
  if (/^actions\/workflows\/ci\.yml\/runs\?branch=main&event=push(?:&status=completed)?&per_page=100$/.test(path)) {
    const latest = activeMain ? run(20, 1, { status: activeMain, conclusion: null, run_attempt: activeMain === 'queued' ? 1 : 2 })
      : scenario === 'recovery-awaiting-rerun' ? recoveryRun(1) : run(20, 1, {run_attempt: 2});
    const workflow_runs = activeMain && path.includes('&status=completed') ? [run(19, 600)] : [run(19, 600), latest];
    return { workflow_runs, ...(scenario === 'oversized-main' ? { padding: 'x'.repeat(9 * 1024 * 1024) } : {}) };
  }
  const checkRun = recovery && path.match(/^check-runs\/([23])\/annotations\?per_page=100$/);
  if (checkRun) {
    const unassigned = checkRun[1] === '3';
    if ((unassigned && scenario === 'recovery-no-acquisition-evidence') ||
        (!unassigned && scenario === 'recovery-no-deadline-evidence')) return [];
    return [{
      path: '.github', start_line: 1, end_line: 1, start_column: null, end_column: null,
      annotation_level: 'failure', title: '', raw_details: '',
      message: unassigned ? 'The job was not acquired by Runner of type hosted even after multiple attempts'
        : 'The job has exceeded the maximum execution time of 12m0s',
    }];
  }
  const recoveryPath = recovery && path.match(/^actions\/runs\/20\/attempts\/([12])(\/jobs\?per_page=100)?$/);
  if (recoveryPath) {
    const attempt = Number(recoveryPath[1]);
    return recoveryPath[2] ? recoveryJobs(attempt) : recoveryRun(attempt);
  }
  if (path === 'actions/runs/20/attempts/1') return run(20, mainSeconds, scenario === 'attempt2-main' ? {run_attempt: 2} : incompleteMain ? {status: 'in_progress'} : {});
  if (path === 'actions/runs/19/attempts/1') return run(19, 600);
  const jobsPath = path.match(/^actions\/runs\/(19|20|11)\/attempts\/1\/jobs\?per_page=100$/);
  if (jobsPath) {
    if (scenario === 'missing-jobs') throw Error('Synthetic unavailable jobs transport');
    const id = Number(jobsPath[1]);
    const seconds = id === 20 ? (mainSeconds) : (scenario === 'slow-first-attempt' ? 1300 : 600);
    const r = run(id, seconds);
    const jobs = ['ubuntu', 'windows', 'macos'].map((os, i) => ({id: i+1, run_id: id, run_attempt: scenario === 'attempt2-jobs' ? 2 : 1, head_sha: head, name: `verify (${os}-latest)`, labels: [`${os}-latest`], status: 'completed', conclusion: 'success', created_at: r.run_started_at, started_at: r.run_started_at, completed_at: r.updated_at}));
    return {total_count: scenario === 'partial-jobs' ? 101 : 3, jobs, ...(scenario === 'oversized-jobs' ? {padding:'x'.repeat(9*1024*1024)} : {})};
  }
  if (path === 'actions/workflows/ci.yml/runs?event=pull_request&status=completed&per_page=100') {
    return {
      workflow_runs: [run(10, 50), run(13, 50, { head_sha: 'b'.repeat(40) }), run(12, 50, { head_repository: { id: 8 } }), run(11, 50, {run_attempt: 2})],
      padding: 'x'.repeat((scenario === 'oversized-candidate' ? 9 : 1.25) * 1024 * 1024),
    };
  }
  if (path === 'actions/runs/11/attempts/1') return run(11, scenario === 'slow-first-attempt' ? 1300 : 600, scenario === 'attempt2-candidate' ? {run_attempt:2} : {});
  throw Error(`Unexpected synthetic API path: ${path}`);
}

function fakeExec(command, args, options) {
  assert.equal(command, 'gh');
  assert.equal(args[0], 'api');
  assert.ok(args[1].startsWith(prefix));
  const path = args[1].slice(prefix.length);
  calls.push(path);
  const payload = JSON.stringify(response(path, args.slice(2)));
  const bytes = Buffer.byteLength(payload);
  payloadBytes.push(bytes);
  // Only large reads in buffer scenarios need the subprocess transport.
  if (!probesBuffer || bytes <= 1024 * 1024) return payload;
  try {
    // Use a finite input file so the child never waits for stdin EOF.
    // Keep stdout piped to exercise the real execFileSync byte bound.
    using directory = mkdtempDisposableSync(join(tmpdir(), 'arxcovo-ci-budget-'));
    const file = join(directory.path, 'response.json');
    writeFileSync(file, payload);
    return execFileSync(process.execPath, [
      '-e', "const {readFileSync, writeFileSync} = require('node:fs'); writeFileSync(1, readFileSync(process.argv[1]));", file,
    ], {
      ...options, stdio: ['ignore', 'pipe', 'pipe'], timeout: 10000,
    });
  } catch (error) {
    transportErrors.push(error.code);
    throw error;
  }
}

const history = readFileSync(new URL('../../docs/ci-duration-history.json', import.meta.url), 'utf8');
const initialHistory = scenario === 'record-duplicate' ? JSON.stringify([{runId:20, marker:'unchanged'}], null, 2) + '\n' : history;
let written = null;
const context = createContext({ process: { env: { GITHUB_REPOSITORY: repository, GITHUB_EVENT_PATH: 'synthetic-event' }, argv: scenario.startsWith('record') ? ['--record'] : [] } });
const modules = new Map();
modules.set('node:child_process', new SyntheticModule(['execFileSync'], function () { this.setExport('execFileSync', fakeExec); }, { context }));
modules.set('node:fs', new SyntheticModule(['readFileSync', 'writeFileSync'], function () {
  this.setExport('readFileSync', path => {
    if (path === 'docs/ci-duration-history.json') return initialHistory;
    assert.equal(path, 'synthetic-event');
    return JSON.stringify({ workflow_run: { event: 'pull_request', head_sha: head } });
  });
  this.setExport('writeFileSync', (path, value) => { assert.ok(scenario.startsWith('record')); assert.equal(path, 'docs/ci-duration-history.json'); written = value; });
}, { context }));
for (const file of ['ci-budget.mjs', 'evaluate-budget.mjs', 'policy.mjs']) {
  modules.set(`./${file}`, new SourceTextModule(readFileSync(new URL(`../${file}`, import.meta.url), 'utf8'), { context, identifier: file }));
}
const entry = modules.get('./ci-budget.mjs');
await entry.link(specifier => {
  assert.ok(modules.has(specifier), `Unexpected module ${specifier}`);
  return modules.get(specifier);
});
let error = null;
try { await entry.evaluate(); } catch (caught) { error = caught.message; }
process.stdout.write(JSON.stringify({ calls, posts, payloadBytes, transportErrors, error, written, initialHistory }));
