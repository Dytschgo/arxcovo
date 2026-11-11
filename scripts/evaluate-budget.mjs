import { budget } from './policy.mjs';

export function decideBudget(latest, candidate, labels) {
  try {
    budget(latest, candidate, labels.some(label => label.name === 'ci-speed'));
    return { state: 'success', description: `Main run ${latest.runId}: ${latest.seconds}s; budget satisfied` };
  } catch (error) {
    return { state: 'failure', description: latest ? `Main run ${latest.runId}: ${latest.seconds}s; ${error.message}` : error.message };
  }
}

export async function evaluatePullRequests({ listPullRequests, getPullRequest, getMainRun, getCandidateRun, postStatus }) {
  const prs = await listPullRequests();
  for (const pr of prs) await postStatus(pr.head.sha, 'pending', 'Re-evaluating latest main CI budget');
  let latest;
  try {
    if (prs.length >= 100) throw Error('Open PR limit reached; evaluation refused');
    latest = await getMainRun();
  } catch (error) {
    for (const pr of prs) await postStatus(pr.head.sha, 'failure', 'Main CI measurement unavailable; retry required');
    throw error;
  }
  for (const listed of prs) {
    let head = listed.head.sha;
    try {
      const pr = await getPullRequest(listed.number);
      head = pr.head.sha;
      if (pr.state !== 'open') continue;
      await postStatus(head, 'pending', 'Checking current head and labels');
      const candidate = latest?.seconds > 900 && pr.labels.some(label => label.name === 'ci-speed') ? await getCandidateRun(pr) : null;
      const current = await getPullRequest(pr.number);
      if (current.state !== 'open') continue;
      if (current.head.sha !== head) {
        await postStatus(current.head.sha, 'pending', 'Head changed; waiting for next CI evaluation');
        continue;
      }
      const result = decideBudget(latest, candidate, current.labels);
      await postStatus(head, result.state, result.description);
    } catch {
      await postStatus(head, 'failure', 'CI budget evaluation failed; retry required');
    }
  }
}
