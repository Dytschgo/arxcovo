export function budget(latest, candidate, speed) {
  if (!latest || !Number.isFinite(latest.seconds) || latest.seconds < 0) throw Error('No valid completed main CI baseline');
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

// All validation precedes the sweep. Defects retain the original wall rule.
export function queueMeasurement(wall, run, payload, listed, kind, repositoryId) {
  const unavailable = code => ({ ...wall, metric: 'charged-v1', chargedSeconds: null, queueCreditSeconds: null, longestJobSeconds: null, credit: `unavailable:${code}`, jobs: [] });
  const S = timestamp(run?.run_started_at), U = timestamp(run?.updated_at);
  if (!run || !Number.isSafeInteger(run.id) || run.id < 1 || run.id !== listed.id || run.run_attempt !== 1 ||
      run.path !== '.github/workflows/ci.yml' || run.status !== 'completed' ||
      (kind === 'main' ? run.event !== 'push' || run.head_branch !== 'main' : run.event !== 'pull_request') ||
      !/^[a-f0-9]{40}$/.test(run.head_sha ?? '') || run.head_sha !== listed.head_sha ||
      (kind !== 'main' && (!Number.isSafeInteger(repositoryId) || repositoryId < 1 || run.head_repository?.id !== repositoryId)) ||
      !Number.isFinite(S) || !Number.isFinite(U) || U - S !== wall.seconds || U - S < 0 || U - S > 604800) return unavailable('run-invalid');
  if (!payload || payload.total_count !== 3 || !Array.isArray(payload.jobs) || payload.jobs.length !== 3) return unavailable('jobs-invalid');
  const names = new Set(['verify (ubuntu-latest)', 'verify (windows-latest)', 'verify (macos-latest)']);
  const ids = new Set(), intervals = [];
  for (const job of payload.jobs) {
    if (!job || !Number.isSafeInteger(job.id) || job.id < 1 || ids.has(job.id) || !names.delete(job.name) ||
        job.run_id !== run.id || job.run_attempt !== 1 || job.head_sha !== run.head_sha ||
        !Array.isArray(job.labels) || job.labels.length !== 1 || job.labels[0] !== job.name.slice(8, -1) ||
        job.status !== 'completed' || typeof job.conclusion !== 'string' || !job.conclusion || job.conclusion === 'skipped') return unavailable('jobs-invalid');
    ids.add(job.id);
    const c = timestamp(job.created_at), s = timestamp(job.started_at), e = timestamp(job.completed_at);
    if (![c, s, e].every(Number.isFinite) || !(S <= c && c <= s && s <= e && e <= U)) return unavailable('jobs-invalid');
    intervals.push({ c, s, e });
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
