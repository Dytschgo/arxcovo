export function budget(latest, candidate, speed) {
  if (!latest || !Number.isFinite(latest.seconds) || latest.seconds < 0) throw Error('No valid completed main CI baseline');
  if (latest.seconds <= 900) return true;
  if (speed && candidate?.conclusion === 'success' && candidate.seconds >= 0 && candidate.seconds < latest.seconds) return true;
  throw Error('Latest completed main CI exceeds 15 minutes; ci-speed requires a successful measured reduction');
}
export function eligible(author, association, sameRepository, allowlist) {
  return sameRepository && ['OWNER', 'MEMBER', 'COLLABORATOR'].includes(association) && allowlist.includes(author);
}
export function duration(run) {
  const seconds = (Date.parse(run.updated_at) - Date.parse(run.run_started_at)) / 1000;
  if (run.status !== 'completed' || !Number.isFinite(seconds) || seconds < 0) throw Error('Invalid completed workflow run');
  return { runId: run.id, sha: run.head_sha, seconds, conclusion: run.conclusion };
}
