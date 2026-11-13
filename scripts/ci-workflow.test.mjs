import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
const playwrightConfig = readFileSync(new URL('../playwright.config.ts', import.meta.url), 'utf8');

test('CI installs the headless shell used by its unchanged browser smoke tests', () => {
  assert.match(workflow, /^ {6}- run: npx playwright install --with-deps --only-shell chromium\r?$/m);
  assert.match(workflow, /^ {8}os: \[ubuntu-latest, windows-latest, macos-latest\]\r?$/m);
  assert.doesNotMatch(playwrightConfig, /\bchannel\s*:/, 'a browser channel needs a reviewed install change');
  assert.doesNotMatch(playwrightConfig, /\bheadless\s*:\s*false\b/, 'headed smoke needs a reviewed install change');
});

test('CI replaces superseded PR runs without cancelling main measurements', () => {
  const concurrency = workflow.match(/^concurrency:\r?\n((?:[ \t]+[^\n]*\n)+)/m)?.[1];
  assert.ok(concurrency, 'CI must define concurrency');
  assert.ok(concurrency.includes("  group: ${{ github.event_name == 'pull_request' && format('ci-pr-{0}', github.event.pull_request.number) || format('ci-run-{0}', github.run_id) }}"));
  assert.match(concurrency, /^ {2}cancel-in-progress: \$\{\{ github\.event_name == 'pull_request' \}\}\r?$/m);
  assert.equal((workflow.match(/^\s*concurrency:/gm) ?? []).length, 1, 'job-level concurrency must not override the policy');
  assert.match(workflow, /^ {4}timeout-minutes: 12\r?$/m);
  assert.equal((workflow.match(/^\s*timeout-minutes:/gm) ?? []).length, 1);
});

test('CI disables dev debuginfo job-wide without overriding runtime profile defaults', () => {
  assert.match(workflow, /^ {4}timeout-minutes: 12\r?\n {4}env:\r?\n {6}CARGO_PROFILE_DEV_DEBUG: '0'\r?\n {4}steps:\r?$/m);
  assert.equal((workflow.match(/^\s*env:/gm) ?? []).length, 1, 'the sole env block must be job-wide');
  assert.deepEqual(workflow.match(/\bCARGO_\w+/g), ['CARGO_PROFILE_DEV_DEBUG'], 'no other Cargo overrides are allowed');
  for (const path of ['../Cargo.toml', '../.cargo/config', '../.cargo/config.toml']) {
    const url = new URL(path, import.meta.url);
    if (existsSync(url)) {
      assert.doesNotMatch(readFileSync(url, 'utf8'), /^\s*\[\s*profile\b/m, `${path} must not override Cargo profiles`);
    }
  }
});
