import { test } from 'node:test';
import assert from 'node:assert/strict';
import { budget, eligible, duration, queueMeasurement } from './policy.mjs';
import { actualRuns } from './fixtures/ci-queue-credit-runs.mjs';
import { historicalSplitFirstAttempt } from './fixtures/ci-split-first-attempt.mjs';
test('slow main blocks normal PR and unmeasured speed claims', () => {
  assert.throws(() => budget({ seconds: 901 }, null, false));
  assert.throws(() => budget({ seconds: 901 }, null, true));
  assert.throws(() => budget({ seconds: 901 }, { seconds: 902, conclusion: 'success' }, true));
  assert.throws(() => budget({ seconds: 901 }, { seconds: 800, conclusion: 'failure' }, true));
  assert.throws(() => budget({ seconds: 901 }, { seconds: 900, conclusion: 'success' }, true));
  assert.equal(budget({ seconds: 900 }, null, false), true);
  assert.throws(() => budget(null, null, false));
});
test('outsiders and forks never qualify for auto-merge', () => {
  assert.equal(eligible('agent', 'OWNER', true, ['agent']), true);
  assert.equal(eligible('agent', 'NONE', true, ['agent']), false);
  assert.equal(eligible('agent', 'OWNER', false, ['agent']), false);
  assert.equal(eligible('outsider', 'OWNER', true, ['agent']), false);
});
test('wall clock includes the entire completed run', () => {
  assert.equal(duration({ id: 1, status: 'completed', run_started_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:16:00Z' }).seconds, 960);
  assert.throws(() => duration({ status: 'in_progress' }));
});

const iso = seconds => new Date(Date.UTC(2026, 0, 1) + seconds * 1000).toISOString().replace('.000Z', 'Z');
function sample(spans = [[0, 400, 500], [0, 400, 500], [0, 400, 500]], seconds = 1000) {
  const run = { id: 42, head_sha: 'a'.repeat(40), run_attempt: 1, path: '.github/workflows/ci.yml', event: 'push', head_branch: 'main', status: 'completed', conclusion: 'success', run_started_at: iso(0), updated_at: iso(seconds) };
  const jobs = { total_count: 3, jobs: spans.map(([c, s, e], i) => ({ id: i + 1, run_id: 42, run_attempt: 1, head_sha: run.head_sha, name: `verify (${['ubuntu', 'windows', 'macos'][i]}-latest)`, labels: [`${['ubuntu', 'windows', 'macos'][i]}-latest`], status: 'completed', conclusion: 'success', created_at: iso(c), started_at: iso(s), completed_at: iso(e) })) };
  return { run, jobs };
}
const measure = ({ run, jobs }, listed = run, kind = 'main', repositoryId) => queueMeasurement(duration(run), run, jobs, listed, kind, repositoryId);

function splitSample(workerSpans = Array(6).fill([0, 400, 500]), gateSpans = Array(3).fill([500, 600, 700]), seconds = 1000) {
  const data = sample(undefined, seconds);
  const spans = [...workerSpans, ...gateSpans];
  data.jobs.jobs = ['rust', 'ui', 'verify'].flatMap((phase, phaseIndex) => data.jobs.jobs.map((job, osIndex) => {
    const i = phaseIndex * 3 + osIndex;
    const [c, s, e] = spans[i];
    return { ...job, id: i + 1, name: job.name.replace('verify', phase),
      labels: phase === 'verify' ? ['ubuntu-latest'] : job.labels,
      created_at: iso(c), started_at: iso(s), completed_at: iso(e) };
  }));
  data.jobs.total_count = 9;
  return data;
}

function assertSplitInvalid(data, detail) {
  const result = measure(data);
  assert.equal(result.seconds, duration(data.run).seconds, detail);
  assert.equal(result.credit, 'unavailable:jobs-invalid', detail);
  for (const key of ['queueCreditSeconds', 'chargedSeconds', 'longestJobSeconds']) assert.equal(result[key], null, detail);
  assert.deepEqual(result.jobs, [], detail);
  assert.throws(() => budget(result, measure(sample()), true), detail);
  assert.throws(() => budget(measure(sample([[0, 0, 1000], [0, 0, 1000], [0, 0, 1000]], 1000)), result, true), detail);
}

test('early-created nine-job gates are invalid and retain the original 900-second wall rule', () => {
  for (const seconds of [900, 901, 1000]) {
    const data = sample(undefined, seconds);
    const original = data.jobs.jobs;
    data.jobs.jobs = ['rust', 'ui', 'verify'].flatMap((phase, phaseIndex) => original.map((job, osIndex) => ({
      ...job, id: phaseIndex * 3 + osIndex + 1,
      name: job.name.replace('verify', phase),
      labels: phase === 'verify' ? ['ubuntu-latest'] : job.labels,
    })));
    data.jobs.total_count = 9;
    const result = measure(data);
    assert.equal(result.seconds, seconds);
    assert.equal(result.credit, 'unavailable:jobs-invalid');
    for (const key of ['queueCreditSeconds', 'chargedSeconds', 'longestJobSeconds']) assert.equal(result[key], null);
    assert.deepEqual(result.jobs, []);
    if (seconds === 900) assert.equal(budget(result, null, false), true);
    else {
      assert.throws(() => budget(result, null, false));
      assert.throws(() => budget(result, measure(sample()), true));
    }
    assert.throws(() => budget(measure(sample([[0, 0, 1000], [0, 0, 1000], [0, 0, 1000]], 1000)), result, true));
  }
});

test('historical split metadata simulates prospective exact-nine geometry without retroactive acceptance', () => {
  const result = measure(historicalSplitFirstAttempt, historicalSplitFirstAttempt.run, 'candidate', 1453422996);
  assert.deepEqual([result.seconds, result.queueCreditSeconds, result.chargedSeconds, result.longestJobSeconds, result.credit],
    [949, 165, 784, 441, 'applied']);
  assert.equal(result.jobs.length, 9);
  assert.deepEqual(result.jobs.map(job => job.name), [...result.jobs.map(job => job.name)].sort((a, b) => a.localeCompare(b)));
  // This is only the new rule's regression geometry. The original run remains held;
  // 784 charged seconds also exceeds the authoritative main's 603 seconds.
  assert.ok(result.chargedSeconds > 603);
});

test('exact-nine geometry retains overlapping waits, pre-creation gaps, no-pending idle and prefix/tail', () => {
  for (const [workers, gates, seconds, Q, longest, executionUnion] of [
    [Array(6).fill([0, 400, 500]), Array(3).fill([500, 600, 700]), 1000, 500, 100, 200],
    [[[0, 500, 600], [0, 100, 400], ...Array(4).fill([400, 400, 400])], Array(3).fill([600, 610, 620]), 650, 210, 300, 410],
    [Array(6).fill([10, 20, 30]), Array(3).fill([50, 60, 70]), 100, 20, 10, 20],
    [Array(6).fill([0, 0, 10]), [[10, 20, 30], [40, 50, 60], [70, 80, 90]], 100, 30, 10, 40],
    [Array(6).fill([0, 0, 0]), Array(3).fill([0, 0, 0]), 0, 0, 0, 0],
  ]) {
    const data = splitSample(workers, gates, seconds);
    for (const jobs of [data.jobs.jobs, [...data.jobs.jobs].reverse()]) {
      const result = measure({ run: data.run, jobs: { total_count: 9, jobs } });
      assert.equal(result.credit, 'applied');
      assert.equal(result.queueCreditSeconds, Q);
      assert.equal(result.chargedSeconds, seconds - Q);
      assert.equal(result.longestJobSeconds, longest);
      assert.equal(result.jobs.length, 9);
      assert.ok(result.chargedSeconds >= executionUnion && result.chargedSeconds <= seconds);
    }
  }
});

test('each of the three gates contributes execution to the sweep and longest-job maximum', () => {
  for (let i = 0; i < 3; i++) {
    const gates = Array(3).fill([30, 40, 50]);
    gates[i] = [30, 40, 340];
    const result = measure(splitSample(Array(6).fill([0, 10, 30]), gates, 350));
    assert.equal(result.credit, 'applied');
    assert.equal(result.queueCreditSeconds, 20);
    assert.equal(result.chargedSeconds, 330);
    assert.equal(result.longestJobSeconds, 300);
  }
  const overlapping = measure(splitSample(Array(6).fill([0, 10, 30]), [[30, 40, 100], [30, 200, 210], [30, 200, 210]], 220));
  assert.equal(overlapping.queueCreditSeconds, 120); // gate execution at 40..100 stays charged
  assert.equal(overlapping.chargedSeconds, 100);
  assert.equal(overlapping.longestJobSeconds, 60);
});

test('exact-nine charges slow worker and gate execution after idle queue, with unchanged 900/901 boundary', () => {
  for (const execution of [900, 901, 1000]) {
    const end = 600 + execution;
    const result = measure(splitSample(Array(6).fill([0, 600, end]), Array(3).fill([end, end, end]), end));
    assert.equal(result.queueCreditSeconds, 600);
    assert.equal(result.chargedSeconds, execution);
    assert.equal(result.longestJobSeconds, execution);
    if (execution === 900) assert.equal(budget(result, null, false), true);
    else assert.throws(() => budget(result, null, false));
  }
  const slowGate = measure(splitSample(Array(6).fill([0, 600, 700]), Array(3).fill([700, 800, 1801]), 1801));
  assert.equal(slowGate.queueCreditSeconds, 700);
  assert.equal(slowGate.chargedSeconds, 1101);
  assert.equal(slowGate.longestJobSeconds, 1001);
  assert.throws(() => budget(slowGate, null, false));
});

test('every gate must be created after all six workers complete, even when its start is later', () => {
  for (let worker = 0; worker < 6; worker++) for (let gate = 6; gate < 9; gate++) {
    const data = splitSample();
    data.jobs.jobs[worker].completed_at = iso(550);
    // Gates start at 600, but creation at 500 predates this one worker.
    assertSplitInvalid(data, `worker ${worker}, gate ${gate}: whole-matrix completion`);
    for (let i = 6; i < 9; i++) data.jobs.jobs[i].created_at = iso(550);
    assert.equal(measure(data).credit, 'applied', 'equality at last worker completion is valid');
    data.jobs.jobs[gate].created_at = iso(549);
    assertSplitInvalid(data, `worker ${worker}, early-created gate ${gate}`);
  }
});

test('exact-nine rejects counts, partial arrays, duplicate or unknown names, wrong labels and invalid IDs', () => {
  const mutations = [
    ...[0, 2, 3, 4, 6, 8, 10, 101, '9'].map(count => data => { data.jobs.total_count = count; }),
    data => { data.jobs.jobs.pop(); },
    data => { data.jobs.jobs.push({ ...data.jobs.jobs[0], id: 10 }); },
    data => { data.jobs.jobs = null; },
    data => { data.jobs.jobs = data.jobs.jobs.slice(0, 3); data.jobs.total_count = 3; },
    data => { data.jobs.jobs = data.jobs.jobs.slice(6); data.jobs.total_count = 3; },
  ];
  for (const mutate of mutations) {
    const data = splitSample(); mutate(data); assertSplitInvalid(data, 'count or closed layout');
  }
  for (let i = 0; i < 9; i++) {
    const changes = [
      ['id', 0], ['id', -1], ['id', 1.5], ['id', Number.MAX_SAFE_INTEGER + 1], ['id', (i + 1) % 9 + 1],
      ['name', 'other'], ['name', `prefix ${splitSample().jobs.jobs[i].name}`],
      ['name', splitSample().jobs.jobs[(i + 1) % 9].name],
      ['labels', []], ['labels', null], ['labels', ['other']], ['labels', ['ubuntu-latest', 'other']],
    ];
    for (const [key, value] of changes) {
      const data = splitSample(); data.jobs.jobs[i][key] = value;
      assertSplitInvalid(data, `job ${i}.${key}=${value}`);
    }
    const wrongOS = splitSample();
    wrongOS.jobs.jobs[i].labels = [wrongOS.jobs.jobs[i].labels[0] === 'ubuntu-latest' ? 'windows-latest' : 'ubuntu-latest'];
    assertSplitInvalid(wrongOS, `job ${i} mismatched runner label`);
    const missing = splitSample(); missing.jobs.jobs[i] = null;
    assertSplitInvalid(missing, `job ${i} missing`);
  }
});

test('exact-nine preserves attempt, run, repository and SHA trust for run and all job roles', () => {
  for (const [key, value] of [
    ['id', 43], ['id', 0], ['id', 1.5], ['run_attempt', 2], ['path', 'other'], ['status', 'queued'],
    ['event', 'pull_request'], ['head_branch', 'other'], ['head_sha', 'b'.repeat(40)], ['head_sha', 'A'.repeat(40)],
    ['run_started_at', null], ['run_started_at', '2026-02-30T00:00:00Z'],
    ['updated_at', iso(604801)], ['updated_at', iso(-1)], ['updated_at', '2026-01-01T00:20:00.000Z'],
  ]) {
    const data = splitSample(); const listed = structuredClone(data.run); data.run[key] = value;
    const result = queueMeasurement(duration(listed), data.run, data.jobs, listed, 'main');
    assert.equal(result.credit, 'unavailable:run-invalid', `run.${key}`);
    assert.equal(result.chargedSeconds, null);
    assert.deepEqual(result.jobs, []);
  }
  for (let i = 0; i < 9; i++) for (const [key, value] of [
    ['run_id', 43], ['run_attempt', 2], ['head_sha', 'b'.repeat(40)], ['head_sha', 'A'.repeat(40)],
  ]) {
    const data = splitSample(); data.jobs.jobs[i][key] = value;
    assertSplitInvalid(data, `job ${i}.${key}`);
  }
  const candidate = splitSample(); candidate.run.event = 'pull_request'; candidate.run.head_repository = { id: 7 };
  assert.equal(measure(candidate, candidate.run, 'candidate', 7).credit, 'applied');
  for (const repositoryId of [undefined, 0, 7.5, 8]) assert.equal(measure(candidate, candidate.run, 'candidate', repositoryId).credit, 'unavailable:run-invalid');
  assert.equal(measure(candidate, { ...candidate.run, head_sha: 'b'.repeat(40) }, 'candidate', 7).credit, 'unavailable:run-invalid');
});

test('strict integer-second timestamps and within-run ordering apply to every worker and gate', () => {
  for (let i = 0; i < 9; i++) {
    for (const key of ['created_at', 'started_at', 'completed_at']) for (const value of [
      null, '', 0, '2026-02-30T00:00:00Z', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:60Z',
    ]) {
      const data = splitSample(); data.jobs.jobs[i][key] = value;
      assertSplitInvalid(data, `job ${i}.${key}=${value}`);
    }
    for (const [key, value] of [
      ['created_at', iso(-1)], ['created_at', iso(i < 6 ? 401 : 601)],
      ['started_at', iso(i < 6 ? 501 : 701)], ['started_at', iso(i < 6 ? -1 : 499)],
      ['completed_at', iso(i < 6 ? 399 : 599)], ['completed_at', iso(1001)],
    ]) {
      const data = splitSample(); data.jobs.jobs[i][key] = value;
      assertSplitInvalid(data, `job ${i}.${key}=${value}`);
    }
  }
  const maxSpan = splitSample(undefined, undefined, 604800);
  assert.equal(measure(maxSpan).credit, 'applied');
});

test('only a successful run and every successful worker and gate receive exact-nine credit', () => {
  for (const seconds of [800, 1000]) for (const conclusion of ['failure', 'cancelled']) for (let i = -1; i < 9; i++) {
    const data = splitSample(undefined, undefined, seconds);
    (i < 0 ? data.run : data.jobs.jobs[i]).conclusion = conclusion;
    const result = measure(data);
    assert.equal(result.credit, 'withheld:not-green', `job ${i}: ${conclusion}`);
    assert.equal(result.queueCreditSeconds, 500);
    assert.equal(result.chargedSeconds, seconds - 500);
    if (seconds === 800) assert.equal(budget(result, null, false), true);
    else assert.throws(() => budget(result, null, false));
  }
  for (let i = 0; i < 9; i++) for (const [key, value] of [
    ['conclusion', 'skipped'], ['conclusion', null], ['conclusion', ''], ['status', 'queued'], ['status', 'in_progress'],
    ['created_at', null], ['started_at', null], ['completed_at', null],
  ]) {
    const data = splitSample(); data.jobs.jobs[i][key] = value;
    assertSplitInvalid(data, `unfinished/skipped job ${i}.${key}`);
  }
});

test('exact-nine ci-speed still requires both strict charged and longest reductions above slow main', () => {
  const baseline = measure(splitSample(Array(6).fill([0, 600, 1600]), Array(3).fill([1600, 1600, 1600]), 1600));
  const queueOnly = measure(splitSample(Array(6).fill([0, 0, 1000]), Array(3).fill([1000, 1000, 1000]), 1000));
  assert.throws(() => budget(baseline, queueOnly, true));
  const faster = measure(splitSample(Array(6).fill([0, 600, 1500]), Array(3).fill([1500, 1500, 1500]), 1500));
  assert.equal(budget(baseline, faster, true), true);
  assert.throws(() => budget(baseline, faster, false));
  for (const change of [{ chargedSeconds: 1000 }, { longestJobSeconds: 1000 }, { credit: 'withheld:not-green' }, { conclusion: 'failure' }]) {
    assert.throws(() => budget(baseline, { ...faster, ...change }, true));
    assert.throws(() => budget({ ...baseline, ...change, credit: 'withheld:not-green' }, faster, true));
  }
});

test('geometry: concurrent waits, execution overlap, serial gaps, prefix/tail and no-pending idle', () => {
  for (const [spans, seconds, Q] of [
    [[[0, 400, 500], [0, 400, 500], [0, 400, 500]], 1000, 400],
    [[[0, 500, 600], [0, 100, 400], [600, 600, 600]], 600, 200],
    [[[10, 20, 30], [30, 50, 60], [60, 80, 90]], 100, 50],
    [[[10, 20, 30], [40, 50, 60], [70, 80, 90]], 100, 30],
    [[[0, 0, 100], [100, 100, 100], [100, 100, 100]], 100, 0],
    [[[0, 0, 0], [0, 0, 0], [0, 0, 0]], 0, 0],
  ]) {
    const result = measure(sample(spans, seconds));
    assert.equal(result.queueCreditSeconds, Q);
    assert.equal(result.chargedSeconds, seconds - Q);
  }
});

test('slow execution is charged even after 600 seconds of idle queue; exact 900 boundary', () => {
  const slow = measure(sample([[0, 600, 1600], [0, 600, 1600], [0, 600, 1600]], 1600));
  assert.equal(slow.chargedSeconds, 1000);
  assert.throws(() => budget(slow, null, false));
  const boundary = measure(sample([[0, 600, 1500], [0, 600, 1500], [0, 600, 1500]], 1500));
  assert.equal(budget(boundary, null, false), true);
});

test('every metadata defect withholds credit and cannot authorize ci-speed', () => {
  const changes = [
    ['run', 'id', 43], ['run', 'id', 1.5], ['run', 'run_attempt', 2], ['run', 'path', 'other'],
    ['run', 'status', 'queued'], ['run', 'event', 'pull_request'], ['run', 'head_branch', 'other'],
    ['run', 'head_sha', 'b'.repeat(40)], ['run', 'head_sha', 'x'.repeat(40)],
    ['run', 'run_started_at', null], ['run', 'run_started_at', '2026-02-30T00:00:00Z'],
    ['run', 'updated_at', '2026-01-01T00:20:00.000Z'], ['run', 'updated_at', iso(604801)],
    ['run', 'updated_at', iso(-1)],
    ['payload', 'total_count', 2], ['payload', 'total_count', 4], ['payload', 'total_count', 101],
    ['payload', 'jobs', []], ['payload', 'jobs', null],
    ['job', 'id', 0], ['job', 'id', 1.5], ['job', 'id', 2],
    ['job', 'run_id', 43], ['job', 'run_attempt', 2], ['job', 'head_sha', 'b'.repeat(40)],
    ['job', 'name', 'verify (windows-latest)'], ['job', 'name', 'other'],
    ['job', 'labels', []], ['job', 'labels', ['macos-latest']], ['job', 'labels', ['ubuntu-latest', 'other']],
    ['job', 'status', 'queued'], ['job', 'conclusion', ''], ['job', 'conclusion', null], ['job', 'conclusion', 'skipped'],
    ...['created_at', 'started_at', 'completed_at'].flatMap(key => [[ 'job', key, null ], ['job', key, '2026-02-30T00:00:00Z'], ['job', key, '2026-01-01T00:00:00.000Z']]),
    ['job', 'created_at', iso(-1)], ['job', 'created_at', iso(401)], ['job', 'started_at', iso(501)], ['job', 'completed_at', iso(399)], ['job', 'completed_at', iso(1001)],
  ];
  for (const [target, key, value] of changes) {
    const data = sample();
    const listed = globalThis.structuredClone(data.run);
    (target === 'run' ? data.run : target === 'payload' ? data.jobs : data.jobs.jobs[0])[key] = value;
    const result = queueMeasurement({ ...duration(listed), seconds: 1000 }, data.run, data.jobs, listed, 'main');
    assert.equal(result.chargedSeconds, null, `${target}.${key}=${value}`);
    assert.equal(result.credit, `unavailable:${target === 'run' ? 'run' : 'jobs'}-invalid`);
    assert.deepEqual(result.jobs, []);
    assert.throws(() => budget(result, measure(sample()), true));
    assert.throws(() => budget({ ...measure(sample()), seconds: 1200, chargedSeconds: 1000 }, result, true));
  }
  const data = sample();
  data.run.event = 'pull_request'; data.run.head_repository = { id: 7 };
  assert.equal(measure(data, data.run, 'candidate', 8).credit, 'unavailable:run-invalid');
  assert.equal(measure(data, data.run, 'candidate', 7).credit, 'applied');
  data.run.head_repository = {};
  assert.equal(measure(data, data.run, 'candidate', undefined).credit, 'unavailable:run-invalid');
  data.run.event = 'push';
  assert.equal(measure(data, data.run, 'candidate', 7).credit, 'unavailable:run-invalid');
  const duplicatePlatform = sample();
  duplicatePlatform.jobs.jobs[0].name = duplicatePlatform.jobs.jobs[1].name;
  duplicatePlatform.jobs.jobs[0].labels = duplicatePlatform.jobs.jobs[1].labels;
  assert.equal(measure(duplicatePlatform).credit, 'unavailable:jobs-invalid');
  const tooMany = sample(); tooMany.jobs.jobs.push(globalThis.structuredClone(tooMany.jobs.jobs[0]));
  assert.equal(measure(tooMany).credit, 'unavailable:jobs-invalid');
  const missing = sample(); missing.jobs.jobs[0] = null;
  assert.equal(measure(missing).credit, 'unavailable:jobs-invalid');
});

test('failed run or any failed/cancelled complete job retains wall gate', () => {
  for (const seconds of [800, 1475]) for (const target of ['run', 'job']) {
    const data = sample(undefined, seconds);
    (target === 'run' ? data.run : data.jobs.jobs[0]).conclusion = 'failure';
    const result = measure(data);
    assert.equal(result.credit, 'withheld:not-green');
    if (seconds === 800) assert.equal(budget(result, null, false), true);
    else assert.throws(() => budget(result, null, false));
  }
  const data = sample(); data.jobs.jobs[0].conclusion = 'cancelled';
  assert.equal(measure(data).credit, 'withheld:not-green');
  data.jobs.jobs[0].started_at = null;
  assert.equal(measure(data).credit, 'unavailable:jobs-invalid');
});

test('ci-speed rejects queue-only reduction, failed baseline even with faster green candidate, and requires both strict reductions', () => {
  const baseline = measure(sample([[0, 0, 1000], [0, 100, 1000], [0, 0, 1000]], 1500));
  const candidate = { ...baseline, seconds: 1200, chargedSeconds: 1200 };
  assert.throws(() => budget(baseline, candidate, true)); // tail/queue-only change
  candidate.chargedSeconds = 900;
  assert.throws(() => budget(baseline, candidate, true)); // longest unchanged
  candidate.longestJobSeconds = 999;
  assert.equal(budget(baseline, candidate, true), true);
  assert.throws(() => budget(baseline, candidate, false));
  assert.throws(() => budget({ ...baseline, conclusion: 'failure', credit: 'withheld:not-green' }, candidate, true));
  assert.throws(() => budget(baseline, { ...candidate, credit: 'unavailable:jobs-invalid', chargedSeconds: null }, true));
  assert.throws(() => budget(baseline, { ...candidate, chargedSeconds: baseline.chargedSeconds }, true));
});

test('exact public attempt-1 fixtures reproduce ADR0025; PR162 cannot claim ci-speed', () => {
  const results = actualRuns.map(({ run, jobs }) => measure({ run, jobs }, run, run.event === 'push' ? 'main' : 'candidate', 1453422996));
  assert.deepEqual(results.map(r => [r.seconds, r.queueCreditSeconds, r.chargedSeconds, r.longestJobSeconds, r.credit]), [
    [1475, 737, 738, 372, 'withheld:not-green'], [1311, 434, 877, 585, 'applied'],
  ]);
  assert.throws(() => budget(results[0], results[1], true));
  assert.throws(() => budget(results[0], null, false));
});


test('successful run with one failed job receives no idle credit below wall budget', () => {
  const data = sample([[0, 600, 1000], [0, 600, 1000], [0, 600, 1000]], 1000);
  data.jobs.jobs[0].conclusion = 'failure';
  const result = measure(data);
  assert.equal(result.conclusion, 'success');
  assert.equal(result.chargedSeconds, 400);
  assert.equal(result.credit, 'withheld:not-green');
  assert.throws(() => budget(result, null, false));
});
