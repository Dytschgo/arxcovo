export const RUST_UBUNTU_RUNNER = 'ubuntu-24.04';
const JOB_TIMEOUT_SECONDS = 12 * 60;
const JOB_TIMEOUT_SETTLE_SECONDS = 30;

export function budget(latest, candidate, speed) {
  if (!latest || !Number.isFinite(latest.seconds) || latest.seconds < 0) throw Error('No valid completed main CI baseline');
  if (latest.conclusion !== 'success' || latest.credit === 'withheld:not-green') {
    throw Error('Main CI did not succeed');
  }
  if ((creditable(latest) ? latest.chargedSeconds : latest.seconds) <= 900) return true;
  if (speed && creditable(latest) && creditable(candidate) && candidate.chargedSeconds < latest.chargedSeconds && candidate.longestJobSeconds < latest.longestJobSeconds) return true;
  throw Error('over 900s; ci-speed needs charged+exec reduction');
}

export function creditable(run) {
  return run?.credit === 'applied' && run.conclusion === 'success' && Number.isFinite(run.chargedSeconds) && run.chargedSeconds >= 0;
}

function timestamp(value) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value)) return NaN;
  const ms = Date.parse(value);
  return Number.isFinite(ms) && new Date(ms).toISOString() === value.replace('Z', '.000Z') ? ms / 1000 : NaN;
}

function isRunnerCancellation(job, split) {
  if (job.conclusion !== 'cancelled' || !Array.isArray(job.steps)) return false;
  if (job.runner_id === 0) return job.steps.length === 0 && job.created_at === job.started_at;
  const seconds = timestamp(job.completed_at) - timestamp(job.started_at);
  // A cancelled or failed step could be a hung or failing test. Only recognize
  // the current split workflow's job deadline after all reported steps settled.
  return split && Number.isSafeInteger(job.runner_id) && job.runner_id > 0 &&
    seconds >= JOB_TIMEOUT_SECONDS && seconds <= JOB_TIMEOUT_SECONDS + JOB_TIMEOUT_SETTLE_SECONDS &&
    job.steps.some(step => step?.conclusion === 'success') &&
    job.steps.every(step => step?.status === 'completed' && ['success', 'skipped'].includes(step.conclusion));
}

function isDependencyFailure(job) {
  // The split verify job has one user step, which checks both matrix results.
  return job.conclusion === 'failure' && Array.isArray(job.steps) && job.steps.length === 3 &&
    ['success', 'failure', 'success'].every((conclusion, i) => {
      const step = job.steps[i];
      return step?.number === i + 1 && step.status === 'completed' && step.conclusion === conclusion;
    });
}

function onlyRunnerCancellations(run, jobs, split) {
  if (!['cancelled', 'failure'].includes(run.conclusion)) return false;
  const cancelled = jobs.filter(job => job.conclusion === 'cancelled');
  if (!cancelled.length || !cancelled.every(job => isRunnerCancellation(job, split))) return false;
  const cancelledWorker = split && cancelled.some(job => !job.name.startsWith('verify ('));
  return jobs.every(job => {
    if (job.conclusion === 'cancelled') return true;
    if (cancelledWorker && job.name.startsWith('verify (')) return isDependencyFailure(job);
    return job.conclusion === 'success';
  });
}

// All validation precedes the sweep. Metadata defects on green runs retain the wall rule.
export function queueMeasurement(wall, run, payload, listed, kind, repositoryId, attempt = 1) {
  const unavailable = code => ({ ...wall, metric: 'charged-v1', chargedSeconds: null, queueCreditSeconds: null, longestJobSeconds: null, credit: `unavailable:${code}`, jobs: [] });
  const S = timestamp(run?.run_started_at), U = timestamp(run?.updated_at);
  if (!run || !Number.isSafeInteger(run.id) || run.id < 1 || run.id !== listed.id || run.run_attempt !== attempt ||
      (attempt !== 1 && (kind !== 'main' || attempt !== listed.run_attempt || !Number.isSafeInteger(attempt) || attempt < 2)) ||
      run.path !== '.github/workflows/ci.yml' || run.status !== 'completed' ||
      (kind === 'main' ? run.event !== 'push' || run.head_branch !== 'main' : run.event !== 'pull_request') ||
      !/^[a-f0-9]{40}$/.test(run.head_sha ?? '') || run.head_sha !== listed.head_sha ||
      (kind !== 'main' && (!Number.isSafeInteger(repositoryId) || repositoryId < 1 || run.head_repository?.id !== repositoryId)) ||
      !Number.isFinite(S) || !Number.isFinite(U) || U - S !== wall.seconds || U - S < 0 || U - S > 604800) return unavailable('run-invalid');
  if (!payload || ![3, 9].includes(payload.total_count) || !Array.isArray(payload.jobs) || payload.jobs.length !== payload.total_count) return unavailable('jobs-invalid');
  const split = payload.total_count === 9;
  const names = new Map((split ? ['rust', 'ui', 'verify'] : ['verify']).flatMap(phase => {
    const gate = split && phase === 'verify';
    return ['ubuntu-latest', 'windows-latest', 'macos-latest'].map(os => {
      let label = gate ? 'ubuntu-latest' : os;
      if (phase === 'rust' && os === 'ubuntu-latest') label = RUST_UBUNTU_RUNNER;
      return [`${phase} (${os})`, { label, gate }];
    });
  }));
  const ids = new Set(), intervals = [];
  for (const job of payload.jobs) {
    const expected = names.get(job?.name);
    if (!job || !Number.isSafeInteger(job.id) || job.id < 1 || ids.has(job.id) || !expected || !names.delete(job.name) ||
        job.run_id !== run.id || job.run_attempt !== attempt || job.head_sha !== run.head_sha ||
        !Array.isArray(job.labels) || job.labels.length !== 1 || job.labels[0] !== expected.label ||
        job.status !== 'completed' || typeof job.conclusion !== 'string' || !job.conclusion || job.conclusion === 'skipped') return unavailable('jobs-invalid');
    ids.add(job.id);
    const c = timestamp(job.created_at), s = timestamp(job.started_at), e = timestamp(job.completed_at);
    if (![c, s, e].every(Number.isFinite) || !(S <= c && c <= s && s <= e && e <= U)) return unavailable('jobs-invalid');
    intervals.push({ c, s, e, gate: expected.gate });
  }
  // The trusted split gates depend on both complete matrices. Early creation is
  // ineligible even when execution starts later; dependency waits earn no credit.
  if (split) {
    const workersCompleted = Math.max(...intervals.filter(({ gate }) => !gate).map(({ e }) => e));
    if (intervals.some(({ c, gate }) => gate && c < workersCompleted)) return unavailable('jobs-invalid');
  }
  const points = [...new Set([S, U, ...intervals.flatMap(({ c, s, e }) => [c, s, e])])].sort((a, b) => a - b);
  let queueCreditSeconds = 0;
  for (let i = 0; i < points.length - 1; i++) {
    const t = points[i];
    if (intervals.some(({ c, s }) => c <= t && t < s) && !intervals.some(({ s, e }) => s <= t && t < e)) queueCreditSeconds += points[i + 1] - t;
  }
  return { ...wall, metric: 'charged-v1', chargedSeconds: wall.seconds - queueCreditSeconds, queueCreditSeconds,
    longestJobSeconds: Math.max(...intervals.map(({ s, e }) => e - s)),
    credit: run.conclusion === 'success' && payload.jobs.every(job => job.conclusion === 'success') ? 'applied' : 'withheld:not-green',
    ...(kind === 'main' && attempt === 1 && onlyRunnerCancellations(run, payload.jobs, split) ? { runnerCancelled: true } : {}),
    jobs: payload.jobs.map(job => ({ id: job.id, name: job.name, conclusion: job.conclusion, created: job.created_at, started: job.started_at, completed: job.completed_at })).sort((a, b) => a.name.localeCompare(b.name)) };
}
export function eligible(author, association, sameRepository, allowlist) {
  return sameRepository && ['OWNER', 'MEMBER', 'COLLABORATOR'].includes(association) && allowlist.includes(author);
}
export function duration(run) {
  const seconds = (Date.parse(run.updated_at) - Date.parse(run.run_started_at)) / 1000;
  if (run.status !== 'completed' || !Number.isFinite(seconds) || seconds < 0) throw Error('Invalid completed workflow run');
  return { runId: run.id, sha: run.head_sha, seconds, conclusion: run.conclusion };
}
