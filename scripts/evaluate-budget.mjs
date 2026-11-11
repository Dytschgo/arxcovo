import { budget } from './policy.mjs';

export function decideBudget(latest, candidate, labels) {
  try {
    budget(latest, candidate, labels.some(label => label.name === 'ci-speed'));
    return { state: 'success', description: `Main run ${latest.runId}: ${latest.seconds}s; budget satisfied` };
  } catch (error) {
    return { state: 'failure', description: latest ? `Main run ${latest.runId}: ${latest.seconds}s; ${error.message}` : error.message };
  }
}

export function statusPublisher({ readStatus, createStatus }) {
  const cache = new Map();
  const get = async sha => {
    if (!cache.has(sha)) cache.set(sha, await readStatus(sha));
    return cache.get(sha);
  };
  const post = async (sha, state, description) => {
    description = description.slice(0, 140);
    let previous;
    try { previous = await get(sha); } catch { /* A read failure must not prevent publishing failure. */ }
    if (previous?.state === state && previous.description === description) return;
    await createStatus(sha, state, description);
    cache.set(sha, { state, description });
  };
  return { get, post };
}

export async function evaluatePullRequests({ listPullRequests, getPullRequest, getMainRun, getCandidateRun, getStatus, postStatus }) {
  const prs = await listPullRequests();
  let latest;
  try {
    if (prs.length >= 100) throw Error('Open PR limit reached; evaluation refused');
    latest = await getMainRun();
  } catch (error) {
    await Promise.allSettled(prs.map(pr => postStatus(pr.head.sha, 'failure', 'Main CI measurement unavailable; retry required')));
    throw error;
  }
  const errors = [];
  for (const listed of prs) {
    let head = listed.head.sha;
    try {
      const pr = await getPullRequest(listed.number);
      head = pr.head.sha;
      if (pr.state !== 'open') continue;
      const previous = await getStatus(head);
      const marker = latest ? `Main run ${latest.runId}:` : 'No valid completed main CI baseline';
      // Unchanged final results stay final. Only invalidate an old success when
      // its authoritative main measurement changed, or initialize a new head.
      if (!previous || (previous.state === 'success' && !(previous.description ?? '').startsWith(marker))) {
        await postStatus(head, 'pending', `${marker} evaluating budget`);
      }
      const candidate = latest?.seconds > 900 && pr.labels.some(label => label.name === 'ci-speed') ? await getCandidateRun(pr) : null;
      const current = await getPullRequest(pr.number);
      if (current.state !== 'open') continue;
      if (current.head.sha !== head) {
        await postStatus(current.head.sha, 'pending', 'Head changed; waiting for next CI evaluation');
        continue;
      }
      const result = decideBudget(latest, candidate, current.labels);
      await postStatus(head, result.state, result.description);
    } catch (error) {
      errors.push(error);
      try { await postStatus(head, 'failure', 'CI budget evaluation failed; retry required'); } catch (postError) { errors.push(postError); }
    }
  }
  if (errors.length) throw new AggregateError(errors, 'One or more PR budgets could not be evaluated');
}
