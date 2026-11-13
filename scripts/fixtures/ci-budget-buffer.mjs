// Runs only synthetic IO. No gh executable, credentials, or network are used.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { SourceTextModule, SyntheticModule, createContext } from 'node:vm';

const scenario = process.argv[2];
const head = 'a'.repeat(40);
const repository = scenario === 'invalid-repository' ? 'https://unsafe' : 'example/arxcovo';
const prefix = 'repos/example/arxcovo/';
const calls = [];
const posts = [];
const payloadBytes = [];
const transportErrors = [];
let prReads = 0;
const pr = { number: 1, state: 'open', head: { sha: head, repo: { id: 7 } }, labels: [{ name: 'ci-speed' }] };
const run = (id, seconds, extra = {}) => ({
  id, run_number: id, head_sha: head, head_repository: { id: 7 },
  status: 'completed', conclusion: 'success',
  run_started_at: '2026-01-01T00:00:00Z',
  updated_at: new Date(Date.parse('2026-01-01T00:00:00Z') + seconds * 1000).toISOString(),
  ...extra,
});

function response(path, args) {
  if (path === 'pulls?state=open&per_page=100') return [pr];
  if (path === 'pulls/1') {
    prReads++;
    return { ...pr, labels: scenario === 'removed-label' && prReads > 1 ? [] : pr.labels };
  }
  if (path === `commits/${head}/status?per_page=100`) return { statuses: [] };
  if (path === `statuses/${head}`) {
    assert.deepEqual(Array.from(args.slice(0, 2)), ['-X', 'POST']);
    assert.equal(args[5], 'context=ci-time-budget');
    posts.push({ sha: head, state: args[3].slice('state='.length), description: args[7].slice('description='.length) });
    return {};
  }
  if (path === 'actions/workflows/ci.yml/runs?branch=main&event=push&status=completed&per_page=100') {
    return { workflow_runs: [run(19, 1), run(20, 1)], ...(scenario === 'oversized-main' ? { padding: 'x'.repeat(9 * 1024 * 1024) } : {}) };
  }
  if (path === 'actions/runs/20/attempts/1') return run(20, scenario === 'fast-main' ? 800 : 1200);
  if (path === 'actions/workflows/ci.yml/runs?event=pull_request&status=completed&per_page=100') {
    return {
      workflow_runs: [run(10, 50), run(13, 50, { head_sha: 'b'.repeat(40) }), run(12, 50, { head_repository: { id: 8 } }), run(11, 50)],
      padding: 'x'.repeat((scenario === 'oversized-candidate' ? 9 : 1.25) * 1024 * 1024),
    };
  }
  if (path === 'actions/runs/11/attempts/1') return run(11, scenario === 'slow-first-attempt' ? 1300 : 600);
  throw Error(`Unexpected synthetic API path: ${path}`);
}

function fakeExec(command, args, options) {
  assert.equal(command, 'gh');
  assert.equal(args[0], 'api');
  assert.ok(args[1].startsWith(prefix));
  const path = args[1].slice(prefix.length);
  calls.push(path);
  const payload = JSON.stringify(response(path, args.slice(2)));
  payloadBytes.push(Buffer.byteLength(payload));
  try {
    // Exercise the real execFileSync byte bound, with synthetic JSON on stdin.
    return execFileSync(process.execPath, ['-e', "process.stdout.write(require('node:fs').readFileSync(0))"], {
      ...options, input: payload, stdio: ['pipe', 'pipe', 'pipe'],
    });
  } catch (error) {
    transportErrors.push(error.code);
    throw error;
  }
}

const context = createContext({ process: { env: { GITHUB_REPOSITORY: repository, GITHUB_EVENT_PATH: 'synthetic-event' }, argv: [] } });
const modules = new Map();
modules.set('node:child_process', new SyntheticModule(['execFileSync'], function () { this.setExport('execFileSync', fakeExec); }, { context }));
modules.set('node:fs', new SyntheticModule(['readFileSync', 'writeFileSync'], function () {
  this.setExport('readFileSync', path => {
    assert.equal(path, 'synthetic-event');
    return JSON.stringify({ workflow_run: { event: 'pull_request', head_sha: head } });
  });
  this.setExport('writeFileSync', () => assert.fail('No record writes allowed'));
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
process.stdout.write(JSON.stringify({ calls, posts, payloadBytes, transportErrors, error }));
