import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';

const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const playwrightConfig = readFileSync(new URL('../playwright.config.ts', import.meta.url), 'utf8');
const jobs = Object.fromEntries([...workflow.split('\njobs:\n')[1].matchAll(/^ {2}(\w+):\n([\s\S]*?)(?=^ {2}\w+:\n|$(?![\s\S]))/gm)].map(([, id, body]) => [id, body]));
const matrix = '    strategy:\n      fail-fast: false\n      matrix:\n        os: [ubuntu-latest, windows-latest, macos-latest]\n';
const gate = "if ('${{ needs.rust.result }}' -ne 'success' -or '${{ needs.ui.result }}' -ne 'success') { exit 1 }";
const checkout = 'uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262';
const steps = id => jobs[id].split('    steps:\n')[1].trimEnd().split(/^ {6}- /m).slice(1).map(step => step.trimEnd());

test('CI runs unconditional Rust and UI matrices in parallel behind the existing required checks', () => {
  assert.deepEqual(workflow.split('\n').filter(line => /^\S/.test(line)), ['name: CI', 'on:', 'concurrency:', 'permissions:', 'jobs:']);
  assert.deepEqual(Object.keys(jobs), ['rust', 'ui', 'verify']);
  for (const id of ['rust', 'ui']) {
    const env = id === 'rust' ? "    env:\n      CARGO_PROFILE_DEV_DEBUG: '0'\n" : '';
    assert.equal(jobs[id].split('    steps:\n')[0], `    name: ${id} (\${{ matrix.os }})\n${matrix}    runs-on: \${{ matrix.os }}\n    timeout-minutes: 12\n${env}`);
    assert.doesNotMatch(jobs[id], /^ {4}(?:needs|if|environment|services|concurrency):/m);
  }
  assert.equal(jobs.verify, `    name: verify (\${{ matrix.os }})\n    needs: [rust, ui]\n    if: \${{ always() }}\n${matrix}    runs-on: ubuntu-latest\n    timeout-minutes: 12\n    steps:\n      - name: Require every Rust and UI worker to succeed\n        shell: pwsh\n        run: ${gate}\n`);
  assert.match(workflow, /^permissions:\n {2}contents: read\njobs:/m);
  assert.doesNotMatch(workflow, /continue-on-error/);
});

test('required gates accept only both successful matrices, including failed, cancelled, skipped and missing results', () => {
  // Exercise the actual workflow command, including its exit status, rather
  // than reimplementing the boolean decision in the test.
  const actualGate = jobs.verify.match(/^ {8}run: (.+)$/m)?.[1];
  assert.equal(actualGate, gate);
  for (const rust of ['success', 'failure', 'cancelled', 'skipped', '']) {
    for (const ui of ['success', 'failure', 'cancelled', 'skipped', '']) {
      const command = actualGate.replace('${{ needs.rust.result }}', rust).replace('${{ needs.ui.result }}', ui);
      const result = spawnSync('pwsh', ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', command], { encoding: 'utf8', timeout: 10_000 });
      assert.ifError(result.error);
      assert.equal(result.signal, null);
      assert.equal(result.status, rust === 'success' && ui === 'success' ? 0 : 1, `rust=${rust}, ui=${ui}: ${result.stderr}`);
    }
  }
});

test('Rust retains every native, Python and package validation with exact commands, predicates, actions and phase order', () => {
  assert.deepEqual(steps('rust'), [
    `name: Anchor Windows diagnostic sample budget
        if: runner.os == 'Windows'
        shell: pwsh
        run: Add-Content -Path $env:GITHUB_ENV -Value "ARX_DIAG_T0=$([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())"`,
    checkout,
    `uses: dtolnay/rust-toolchain@683b588c487a69d01f6c6ddf40f036ff682cc2c8
        with:
          toolchain: 1.95.0
          components: rustfmt, clippy`,
    `uses: taiki-e/install-action@cfdb7ed4ccc7c7dce37759eae89dc5674b469b39
        with:
          tool: cargo-nextest@0.9.151`,
    `name: Pure Unix experiment guards (no provisioning)
        run: python -I -m unittest discover -s tools/unix-exec-validation -p 'test_*.py'`,
    `name: Compile fixed A4 helper without execution
        if: runner.os == 'Linux'
        run: |
          python3 -I -B -m unittest discover -s tools/unix-exec-validation/a4-helper -p 'test_build.py'
          python3 -I -B tools/unix-exec-validation/a4-helper/build.py linux-candidate`,
    `name: Bounded read-only Unix manager observations
        if: runner.os == 'Linux'
        run: |
          python3 -m unittest discover -s tools/ci -p 'test_observe_manager.py'
          python3 tools/ci/observe_manager.py`,
    `name: Bounded fixed macOS absence observation
        if: runner.os == 'macOS'
        run: |
          python3 -B -m unittest discover -s tools/ci -p 'test_observe_launchd_absence.py'
          python3 -B tools/ci/observe_launchd_absence.py`,
    `name: Bounded read-only Ubuntu DNS observations
        if: runner.os == 'Linux'
        run: |
          python3 -m unittest discover -s tools/ci -p 'test_observe_dns.py'
          python3 tools/ci/observe_dns.py`,
    `name: Pin Windows cluster quarantine scope
        run: python -I -m unittest discover -s tools/ci -p 'test_windows_cluster_quarantine.py'`,
    'run: cargo fmt --all --check',
    'run: cargo clippy --workspace --all-targets --locked -- -D warnings',
    `name: Read-only macOS stock-directory observations
        if: runner.os == 'macOS'
        run: cargo test -p arxd --locked --lib service::native::stock_directory_observations::fixed_system_directory_observations -- --exact --nocapture`,
    'run: cargo nextest run --workspace --locked',
    `name: Read-only service artifact checks
        run: python -I scripts/test-service-artifacts.py`,
    `name: Pure Windows probe guards (no provisioning)
        if: runner.os == 'Windows'
        shell: pwsh
        run: ./tools/windows-identity-probe/Test-CiValidation.ps1`,
    `name: Windows three-node diagnostic sample
        if: runner.os == 'Windows'
        shell: pwsh
        run: python -I -B -m unittest discover -s tools/ci -p 'test_windows_diag_sample.py' && python -I -B tools/ci/windows_diag_sample.py`,
  ]);
  assert.equal((workflow.match(/ARX_DIAG_T0/g) ?? []).length, 1);
  assert.equal((workflow.match(/windows_diag_sample/g) ?? []).length, 2);
});

test('UI retains every Node, browser and Electron validation with exact commands, predicates, actions and phase order', () => {
  assert.deepEqual(steps('ui'), [
    checkout,
    `uses: actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020
        with:
          node-version: 24.18.0
          cache: npm`,
    'run: npm ci',
    'run: npm run lint',
    'run: npm test',
    'run: npm run build',
    `name: Use signed Ubuntu mirror on disposable runner
        if: runner.os == 'Linux'
        run: |
          python3 -B -m unittest discover -s tools/ci -p 'test_ubuntu_mirror.py'
          sudo --preserve-env=GITHUB_ACTIONS,RUNNER_OS,CI,GITHUB_REPOSITORY /usr/bin/python3 -I -B tools/ci/ubuntu_mirror.py`,
    `name: Install Chromium shell with platform dependencies
        if: runner.os != 'Windows'
        run: npx playwright install --with-deps --only-shell chromium`,
    `name: Install Chromium shell on Windows
        if: runner.os == 'Windows'
        run: npx playwright install --only-shell chromium`,
    'run: npm run smoke',
    `name: Electron smoke on Linux
        if: runner.os == 'Linux'
        run: xvfb-run --auto-servernum npm run test:desktop`,
    `name: Electron smoke on Windows and macOS
        if: runner.os != 'Linux'
        run: npm run test:desktop`,
  ]);
  assert.doesNotMatch(playwrightConfig, /\bchannel\s*:/, 'a browser channel needs a reviewed install change');
  assert.doesNotMatch(playwrightConfig, /\bheadless\s*:\s*false\b/, 'headed smoke needs a reviewed install change');
});

test('CI replaces superseded PR runs without cancelling main measurements and retains every 12-minute limit', () => {
  const concurrency = workflow.match(/^concurrency:\n((?:[ \t]+[^\n]*\n)+)/m)?.[1];
  assert.equal(concurrency, "  group: ${{ github.event_name == 'pull_request' && format('ci-pr-{0}', github.event.pull_request.number) || format('ci-run-{0}', github.run_id) }}\n  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n");
  assert.equal((workflow.match(/^\s*concurrency:/gm) ?? []).length, 1);
  assert.deepEqual(workflow.match(/^\s*timeout-minutes:.*$/gm)?.map(line => line.trim()), Array(3).fill('timeout-minutes: 12'));
});

test('CI disables dev debuginfo for Rust without overriding runtime profile defaults', () => {
  assert.match(jobs.rust, /^ {4}timeout-minutes: 12\n {4}env:\n {6}CARGO_PROFILE_DEV_DEBUG: '0'\n {4}steps:$/m);
  assert.equal((workflow.match(/^\s*env:/gm) ?? []).length, 1);
  assert.deepEqual(workflow.match(/\bCARGO_\w+/g), ['CARGO_PROFILE_DEV_DEBUG']);
  for (const path of ['../Cargo.toml', '../.cargo/config', '../.cargo/config.toml']) {
    const url = new URL(path, import.meta.url);
    if (existsSync(url)) {
      assert.doesNotMatch(readFileSync(url, 'utf8'), /^\s*\[\s*profile\b/m, `${path} must not override Cargo profiles`);
    }
  }
});
