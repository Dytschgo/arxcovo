import { test } from 'node:test';
import assert from 'node:assert/strict';
import { budget, eligible, duration, queueMeasurement } from './policy.mjs';
import { actualRuns } from './fixtures/ci-queue-credit-runs.mjs';
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
    const listed = structuredClone(data.run);
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
  const tooMany = sample(); tooMany.jobs.jobs.push(structuredClone(tooMany.jobs.jobs[0]));
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
