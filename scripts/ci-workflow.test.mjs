import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
const playwrightConfig = readFileSync(new URL('../playwright.config.ts', import.meta.url), 'utf8');

test('trusted queue contract pins a single unconditional three-platform matrix job', () => {
  const lines = workflow.split(/\r?\n/);
  assert.deepEqual(lines.filter(l => /^\S/.test(l)), ['name: CI', 'on:', 'concurrency:', 'permissions:', 'jobs:']);
  const jobs = workflow.slice(workflow.indexOf('\njobs:'));
  assert.deepEqual(jobs.match(/^ {2}\S.*$/gm), ['  verify:']);
  assert.deepEqual(jobs.match(/^ {4}\S[^\r\n]*$/gm), [
    '    name: verify (${{ matrix.os }})', '    strategy:', '    runs-on: ${{ matrix.os }}',
    '    timeout-minutes: 12', '    env:', '    steps:',
  ]);
  assert.doesNotMatch(jobs, /^ {4}(?:needs|if|environment|services|concurrency):/m);
  assert.match(jobs, /^ {4}name: verify \(\$\{\{ matrix.os \}\}\)\r?$/m);
  assert.match(jobs, /^ {4}runs-on: \$\{\{ matrix.os \}\}\r?$/m);
  assert.match(jobs, /^ {4}strategy:\r?\n {6}fail-fast: false\r?\n {6}matrix:\r?\n {8}os: \[ubuntu-latest, windows-latest, macos-latest\]\r?\n {4}runs-on:/m);
  assert.match(workflow, /^permissions:\r?\n {2}contents: read\r?\njobs:/m);
});

test('CI installs the headless shell used by its unchanged browser smoke tests', () => {
  assert.match(workflow, /^ {6}- name: Install Chromium shell with platform dependencies\r?\n {8}if: runner.os != 'Windows'\r?\n {8}run: npx playwright install --with-deps --only-shell chromium\r?$/m);
  assert.match(workflow, /^ {6}- name: Install Chromium shell on Windows\r?\n {8}if: runner.os == 'Windows'\r?\n {8}run: npx playwright install --only-shell chromium\r?$/m);
  assert.equal((workflow.match(/run: npx playwright install /g) ?? []).length, 2);
  for (const command of ['npm run smoke', 'npm run test:desktop']) {
    assert.ok(workflow.includes(command), 'browser and Electron smoke remain required');
  }
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

test('CI anchors and runs the bounded Windows diagnostic sample around the unchanged post-C1 steps', () => {
  const lines = workflow.split(/\r?\n/);
  const start = lines.indexOf('    steps:');
  assert.ok(start > 0, 'one job-level steps list');
  const ids = [];
  for (const line of lines.slice(start + 1)) {
    const step = line.match(/^ {6}- (name|uses|run): (.*)$/);
    if (step) ids.push(`${step[1]}: ${step[2]}`);
    else assert.doesNotMatch(line, /^ {6}- /, 'every step starts with name, uses or run');
  }
  assert.deepEqual(ids, [
    'name: Anchor Windows diagnostic sample budget',
    'uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262',
    'uses: dtolnay/rust-toolchain@683b588c487a69d01f6c6ddf40f036ff682cc2c8',
    'uses: actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020',
    'uses: taiki-e/install-action@cfdb7ed4ccc7c7dce37759eae89dc5674b469b39',
    'name: Pure Unix experiment guards (no provisioning)',
    'name: Compile fixed A4 helper without execution',
    'name: Bounded read-only Unix manager observations',
    'name: Bounded fixed macOS absence observation',
    'name: Bounded read-only Ubuntu DNS observations',
    'name: Pin Windows cluster quarantine scope',
    'run: cargo fmt --all --check',
    'run: cargo clippy --workspace --all-targets --locked -- -D warnings',
    'name: Read-only macOS stock-directory observations',
    'run: cargo nextest run --workspace --locked',
    'name: Read-only service artifact checks',
    'name: Pure Windows probe guards (no provisioning)',
    'run: npm ci',
    'run: npm run lint',
    'run: npm test',
    'run: npm run build',
    'name: Use signed Ubuntu mirror on disposable runner',
    'name: Install Chromium shell with platform dependencies',
    'name: Install Chromium shell on Windows',
    'run: npm run smoke',
    'name: Electron smoke on Linux',
    'name: Electron smoke on Windows and macOS',
    'name: Windows three-node diagnostic sample',
  ], 'the 26 post-C1 steps keep their order between the anchor and the sample');
  assert.match(workflow, /^ {4}steps:\r?\n {6}- name: Anchor Windows diagnostic sample budget\r?\n {8}if: runner\.os == 'Windows'\r?\n {8}shell: pwsh\r?\n {8}run: Add-Content -Path \$env:GITHUB_ENV -Value "ARX_DIAG_T0=\$\(\[DateTimeOffset\]::UtcNow\.ToUnixTimeSeconds\(\)\)"\r?\n {6}- uses: actions\/checkout@/m);
  assert.match(workflow, /\n {6}- name: Windows three-node diagnostic sample\r?\n {8}if: runner\.os == 'Windows'\r?\n {8}shell: pwsh\r?\n {8}run: python -I -B -m unittest discover -s tools\/ci -p 'test_windows_diag_sample\.py' && python -I -B tools\/ci\/windows_diag_sample\.py\r?\n?$/);
  assert.equal((workflow.match(/ARX_DIAG_T0/g) ?? []).length, 1);
  assert.equal((workflow.match(/windows_diag_sample/g) ?? []).length, 2);
  assert.doesNotMatch(workflow, /continue-on-error/);
});
