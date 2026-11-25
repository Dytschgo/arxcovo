import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, rmdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { RUST_UBUNTU_RUNNER } from './policy.mjs';
import yaml from 'js-yaml';

const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const playwrightConfig = readFileSync(new URL('../playwright.config.ts', import.meta.url), 'utf8');
const jobs = Object.fromEntries([...workflow.split('\njobs:\n')[1].matchAll(/^ {2}(\w+):\n([\s\S]*?)(?=^ {2}\w+:\n|$(?![\s\S]))/gm)].map(([, id, body]) => [id, body]));
const matrix = '    strategy:\n      fail-fast: false\n      matrix:\n        os: [ubuntu-latest, windows-latest, macos-latest]\n';
const rustTimeout = "    timeout-minutes: ${{ matrix.os == 'windows-latest' && 13 || 12 }}";
const gate = "if ('${{ needs.rust.result }}' -ne 'success' -or '${{ needs.ui.result }}' -ne 'success') { exit 1 }";
const checkout = 'uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262';
const steps = id => jobs[id].split('    steps:\n')[1].trimEnd().split(/^ {6}- /m).slice(1).map(step => step.trimEnd());
const hostedC14Step = `name: Hosted C14 target-authorized delegated build
        if: runner.os == 'Linux'
        timeout-minutes: 2
        continue-on-error: true
        shell: bash
        run: |
          set +e
          python3 -I -B tools/ci/m3-containers/delegated-build-probe.py
          status=$?
          set -e
          if [ "$status" -eq 0 ]; then
            outcome=success
            level=notice
          else
            outcome=failure
            level=warning
          fi
          echo "Hosted C14 target-authorized delegated build quarantined single case: outcome=\${outcome} exit=\${status}" >> "$GITHUB_STEP_SUMMARY"
          echo "::\${level} title=Quarantined hosted C14 probe::outcome=\${outcome} exit=\${status}; non-blocking (docs/ROADMAP.md)"
          exit "$status"`;

function assertHostedC14Quarantine(workflowText) {
  const keys = workflowText.match(/^[ \t]*continue-on-error\s*:\s*\S+/gm) ?? [];
  assert.deepEqual(keys, ['        continue-on-error: true']);
  // Whole-workflow token guard: exactly one spelling anywhere (quoted, flow-style or any case),
  // it sits inside the single exact C14 step, and no quoted mapping key (which could hide an
  // escaped spelling) exists anywhere in the workflow.
  assert.equal((workflowText.match(/continue-on-error/gi) ?? []).length, 1);
  assert.equal(workflowText.split(hostedC14Step).length, 2);
  assert.equal(workflowText.split(hostedC14Step).join('').includes('continue-on-error'), false);
  assert.doesNotMatch(workflowText, /^[ \t]*(?:-[ \t]+)?["'][^\n]*["'][ \t]*:/m);
  const parsed = Object.fromEntries([...workflowText.split('\njobs:\n')[1].matchAll(/^ {2}(\w+):\n([\s\S]*?)(?=^ {2}\w+:\n|$(?![\s\S]))/gm)].map(([, id, body]) => [id, body]));
  const bodies = ['rust', 'ui', 'verify'].flatMap(id => parsed[id].split('    steps:\n')[1].trimEnd().split(/^ {6}- /m).slice(1).map(step => step.trimEnd()));
  const quarantined = bodies.filter(step => step.startsWith('name: Hosted C14 target-authorized delegated build\n'));
  assert.equal(quarantined.length, 1);
  assert.equal(quarantined[0], hostedC14Step);
  for (const step of bodies) {
    if (step !== quarantined[0]) assert.equal(step.includes('continue-on-error'), false);
  }
  assertParsedHostedC14Quarantine(workflowText);
}

// Structural guard on the parsed YAML, so escapes, flow mappings, explicit keys, tags,
// escaped line breaks and aliases cannot add a second non-blocking step or job.
const hostedC14Parsed = {
  name: 'Hosted C14 target-authorized delegated build',
  if: "runner.os == 'Linux'",
  'timeout-minutes': 2,
  'continue-on-error': true,
  shell: 'bash',
  run: `${hostedC14Step.split('run: |\n')[1].split('\n').map(line => line.slice(10)).join('\n')}\n`,
};

function assertParsedHostedC14Quarantine(workflowText) {
  let doc;
  try {
    doc = yaml.load(workflowText, { schema: yaml.CORE_SCHEMA });
  } catch (error) {
    assert.fail(`workflow must parse as plain YAML: ${error.message}`);
  }
  const hits = [];
  const walk = (node, path) => {
    if (Array.isArray(node)) node.forEach((item, i) => walk(item, `${path}[${i}]`));
    else if (node && typeof node === 'object') {
      for (const [key, value] of Object.entries(node)) {
        if (/^continue[\s_-]*on[\s_-]*error$/i.test(key.trim())) hits.push({ path, node });
        walk(value, `${path}.${key}`);
      }
    }
  };
  walk(doc, '$');
  assert.equal(hits.length, 1);
  assert.match(hits[0].path, /^\$\.jobs\.rust\.steps\[\d+\]$/);
  assert.deepEqual(hits[0].node, hostedC14Parsed);
  const named = Object.values(doc.jobs).flatMap(job => job.steps ?? []).filter(step => step?.name === hostedC14Parsed.name);
  assert.equal(named.length, 1);
}

test('CI runs unconditional Rust and UI matrices in parallel behind the existing required checks', () => {
  assert.deepEqual(workflow.split('\n').filter(line => /^\S/.test(line)), ['name: CI', 'on:', 'concurrency:', 'permissions:', 'jobs:']);
  assert.deepEqual(Object.keys(jobs), ['rust', 'ui', 'verify']);
  for (const id of ['rust', 'ui']) {
    const env = id === 'rust' ? "    env:\n      CARGO_PROFILE_DEV_DEBUG: '0'\n" : '';
    const runner = id === 'rust'
      ? `\${{ matrix.os == 'ubuntu-latest' && '${RUST_UBUNTU_RUNNER}' || matrix.os }}`
      : '${{ matrix.os }}';
    const timeout = id === 'rust' ? rustTimeout : '    timeout-minutes: 12';
    assert.equal(jobs[id].split('    steps:\n')[0], `    name: ${id} (\${{ matrix.os }})\n${matrix}    runs-on: ${runner}\n${timeout}\n${env}`);
    assert.doesNotMatch(jobs[id], /^ {4}(?:needs|if|environment|services|concurrency):/m);
  }
  assert.equal(jobs.verify, `    name: verify (\${{ matrix.os }})\n    needs: [rust, ui]\n    if: \${{ always() }}\n${matrix}    runs-on: ubuntu-latest\n    timeout-minutes: 12\n    steps:\n      - name: Require every Rust and UI worker to succeed\n        shell: pwsh\n        run: ${gate}\n`);
  assert.match(workflow, /^permissions:\n {2}contents: read\njobs:/m);
  assertHostedC14Quarantine(workflow);
});

test('hosted C14 quarantine cannot widen past that single step', () => {
  assertHostedC14Quarantine(workflow);
  const probe = 'python3 -I -B tools/ci/m3-containers/delegated-build-probe.py';
  const mutants = [
    workflow.replace('        continue-on-error: true\n', ''),
    workflow.replace(
      '      - name: Hosted C16 live three-container acceptance\n',
      '      - name: Hosted C16 live three-container acceptance\n        continue-on-error: true\n',
    ),
    workflow.replace('      - run: npm run smoke\n', '      - run: npm run smoke\n        continue-on-error: true\n'),
    workflow.replace('continue-on-error: true', 'continue-on-error: false'),
    workflow.replace(probe, 'true'),
    workflow.replace('exit "$status"', 'exit 0'),
    workflow.replace(/ {10}echo "Hosted C14 target-authorized delegated build quarantined single case:.*\n/, ''),
    workflow.replace('timeout-minutes: 2\n        continue-on-error: true', 'timeout-minutes: 9\n        continue-on-error: true'),
    workflow.replace(probe, `# ${probe}`),
    workflow.replace(
      '      - name: Hosted C16 live three-container acceptance\n',
      '      -\n        "continue-on-error": true\n        name: Extra\n        run: "true"\n      - name: Hosted C16 live three-container acceptance\n',
    ),
    workflow.replace(
      '      - name: Hosted C16 live three-container acceptance\n',
      '      - name: Hosted C16 live three-container acceptance\n        "continue\\x2don-error": true\n',
    ),
    workflow.replace(
      '      - name: Hosted C16 live three-container acceptance\n',
      '      - name: Hosted C16 live three-container acceptance\n        Continue-On-Error: true\n',
    ),
  ];
  // YAML spellings that hide the key from text matching; the parsed guard alone must reject each.
  const structural = [
    '      -\n        {name: Extra, "continue\\x2don-error": true, run: "true"}\n',
    '      - ? "continue\\x2don-error"\n        : true\n        name: Extra\n        run: "true"\n',
    '      - !!str continue-on-error: true\n        name: Extra\n        run: "true"\n',
    '      - ? "continue-\\\n          on-error"\n        : true\n        name: Extra\n        run: "true"\n',
    '      - &extra {name: Extra, "continue\\u002don-error": true, run: "true"}\n      - *extra\n',
  ].map(extra => workflow.replace(
    '      - name: Hosted C16 live three-container acceptance\n',
    `${extra}      - name: Hosted C16 live three-container acceptance\n`,
  ));
  for (const mutant of [...mutants, ...structural]) {
    assert.notEqual(mutant, workflow);
    assert.throws(() => assertHostedC14Quarantine(mutant), assert.AssertionError);
  }
  assertParsedHostedC14Quarantine(workflow);
  for (const mutant of structural) {
    assert.doesNotThrow(() => yaml.load(mutant, { schema: yaml.CORE_SCHEMA }));
    assert.throws(() => assertParsedHostedC14Quarantine(mutant), assert.AssertionError);
  }
});

test('required gates accept only both successful matrices, including failed, cancelled, skipped and missing results', () => {
  const actualGate = jobs.verify.match(/^ {8}run: (.+)$/m)?.[1];
  assert.equal(actualGate, gate);
  const cases = [];
  for (const rust of ['success', 'failure', 'cancelled', 'skipped', '']) {
    for (const ui of ['success', 'failure', 'cancelled', 'skipped', '']) {
      const command = actualGate.replace('${{ needs.rust.result }}', rust).replace('${{ needs.ui.result }}', ui);
      cases.push({ rust, ui, command });
    }
  }

  const directory = mkdtempSync(join(tmpdir(), 'arxcovo-workflow-gate-'));
  const gatePath = join(directory, 'gate.ps1');
  try {
    // A script file lets each gate use exit without ending the batch process.
    const batch = `
      $ErrorActionPreference = 'Stop'
      $commands = ConvertFrom-Json -InputObject ([Console]::In.ReadToEnd())
      $statuses = foreach ($command in $commands) {
        [IO.File]::WriteAllText($env:ARXCOVO_TEST_GATE, $command)
        $LASTEXITCODE = 0
        & $env:ARXCOVO_TEST_GATE
        $LASTEXITCODE
      }
      ConvertTo-Json -InputObject @($statuses) -Compress
    `;
    const timeout = 60_000;
    const result = spawnSync('pwsh', ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', batch], {
      encoding: 'utf8',
      env: { ...process.env, ARXCOVO_TEST_GATE: gatePath },
      input: JSON.stringify(cases.map(({ command }) => command)),
      timeout,
    });
    assert.ok(!result.error, `PowerShell gate batch failed (timeout ${timeout} ms): ${result.error}`);
    assert.equal(result.signal, null, result.stderr);
    assert.equal(result.status, 0, result.stderr);
    const statuses = JSON.parse(result.stdout);
    assert.equal(statuses.length, cases.length);
    for (const [index, { rust, ui }] of cases.entries()) {
      assert.equal(statuses[index], rust === 'success' && ui === 'success' ? 0 : 1, `rust=${rust}, ui=${ui}: ${result.stderr}`);
    }
  } finally {
    rmSync(gatePath, { force: true });
    rmdirSync(directory);
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
    `name: Verify pinned SQLite binding build
        shell: pwsh
        run: |
          $ErrorActionPreference = 'Stop'
          $proofRoot = Join-Path $env:RUNNER_TEMP 'arx-bound-sqlite'
          $env:TMP = Join-Path $proofRoot 'tmp'
          $env:TEMP = $env:TMP
          if (-not $env:CARGO_HOME) { $env:CARGO_HOME = Join-Path $HOME '.cargo' }
          if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $env:GITHUB_WORKSPACE 'target' }
          New-Item -ItemType Directory -Force -Path $proofRoot,$env:TMP | Out-Null
          foreach ($name in @('TMP','TEMP','CARGO_HOME','CARGO_TARGET_DIR')) {
            $value = [Environment]::GetEnvironmentVariable($name)
            if ($value.Contains("\`n") -or $value.Contains("\`r")) { throw 'Invalid build path' }
            Add-Content -LiteralPath $env:GITHUB_ENV -Value "$name=$value"
            Write-Output "$name=$value"
          }
          $metadata = Join-Path $proofRoot 'metadata.json'
          $messages = Join-Path $proofRoot 'build-messages.jsonl'
          python -I -B -m unittest discover -s scripts -p 'test_bound_sqlite_loader.py'
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          python -I -B scripts/check-bound-sqlite.py --check-loader-env
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          cargo metadata --locked --format-version 1 > $metadata
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          python -I -B scripts/check-bound-sqlite.py --check-loader-env
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          if ($IsWindows) {
            # Clippy checks the same targets and emits the SQLite build-script proof.
            cargo clippy --workspace --all-targets --locked --message-format=json-render-diagnostics -- -D warnings > $messages
          } else {
            cargo check --workspace --all-targets --locked --message-format=json > $messages
          }
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          python -I -B scripts/check-bound-sqlite.py --metadata $metadata --build-messages $messages --self-test
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          Get-FileHash -Algorithm SHA256 -LiteralPath $metadata,$messages | Format-List
          rustc --version --verbose
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          python -I -B scripts/check-bound-sqlite.py --check-loader-env
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          cargo --version --verbose
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`,
    `name: Preflight loader environment and lint Rust
        if: runner.os != 'Windows'
        shell: pwsh
        run: |
          python -I -B scripts/check-bound-sqlite.py --check-loader-env
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          cargo clippy --workspace --all-targets --locked -- -D warnings
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`,
    `name: Read-only macOS stock-directory observations
        if: runner.os == 'macOS'
        shell: pwsh
        run: |
          python -I -B scripts/check-bound-sqlite.py --check-loader-env
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          cargo test -p arxd --locked --lib service::native::stock_directory_observations::fixed_system_directory_observations -- --exact --nocapture
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`,
    `name: Preflight loader environment and run workspace tests
        shell: pwsh
        run: |
          python -I -B scripts/check-bound-sqlite.py --check-loader-env
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          cargo nextest run --workspace --locked
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          if ($IsLinux) {
            $proofRoot = Join-Path $env:RUNNER_TEMP 'arx-bound-sqlite'
            $messages = Join-Path $proofRoot 'build-messages.jsonl'
            $linkMessages = Join-Path $proofRoot 'arxd-build-messages.jsonl'
            $targetDir = $env:CARGO_TARGET_DIR.TrimEnd('/')
            $arxd = Join-Path $targetDir 'debug/arxd'
            $manifest = Join-Path $env:GITHUB_WORKSPACE 'crates/arxd/Cargo.toml'
            cargo build --locked -p arxd --bin arxd --message-format=json-render-diagnostics > $linkMessages
            if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
            $ErrorActionPreference = 'Stop'
            $linked = @(Get-Content -LiteralPath $linkMessages | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'arxd' -and ($_.target.kind -contains 'bin') -and $_.manifest_path -eq $manifest -and $_.executable })
            if ($linked.Count -ne 1 -or $linked[0].executable -ne $arxd) { throw 'Invalid arxd compiler-artifact' }
            $item = Get-Item -LiteralPath $arxd
            if (($item.LinkType -and $item.LinkType -ne 'HardLink') -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.PSIsContainer) { throw 'Invalid arxd artifact' }
            $receipt = [ordered]@{
              schema = 'arxcovo/r2-artifact/1'
              head = $env:GITHUB_SHA
              run = $env:GITHUB_RUN_ID
              attempt = $env:GITHUB_RUN_ATTEMPT
              target_dir = $targetDir
              executable = $arxd
              sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $arxd).Hash.ToLowerInvariant()
              bytes = [int64]$item.Length
              build_messages_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $messages).Hash.ToLowerInvariant()
              link_messages_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $linkMessages).Hash.ToLowerInvariant()
            }
            $receipt | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $proofRoot 'arxd-receipt.json') -Encoding ascii -NoNewline
          }`,
    `name: Windows authority compile-fail fence
        if: runner.os == 'Windows'
        shell: pwsh
        run: |
          python -I -B scripts/check-bound-sqlite.py --check-loader-env
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          python -I -B -m unittest discover -s tools/ci/authority-fence -p 'test_*.py'
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          python -I -B tools/ci/authority-fence/check.py
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`,
    `name: Hosted distinct-UID custody qualification
        if: runner.os == 'Linux'
        shell: bash
        run: python3 -I -B tools/ci/custody/run.py`,
    `name: Hosted C13 cluster process activation
        if: runner.os == 'Linux'
        timeout-minutes: 3
        shell: bash
        run: python3 -I -B tools/ci/m3-c13/run.py`,
    `name: Hosted A10 Linux executor qualification
        if: runner.os == 'Linux'
        timeout-minutes: 2
        shell: bash
        env:
          ARX_EXEC_QUALIFICATION_MODE: build
        run: python3 -I -B tools/ci/m3-containers/executor-probe.py`,
    hostedC14Step,
    `name: Bind ignored C16 AnchorQuery helper to the Linux build
        if: runner.os == 'Linux'
        shell: pwsh
        run: |
          $ErrorActionPreference = 'Stop'
          $proofRoot = Join-Path $env:RUNNER_TEMP 'arx-bound-sqlite'
          $anchorMessages = Join-Path $proofRoot 'c16-anchor-build.jsonl'
          $targetDir = $env:CARGO_TARGET_DIR.TrimEnd('/')
          $manifest = Join-Path $env:GITHUB_WORKSPACE 'crates/arxd/Cargo.toml'
          cargo test --locked -p arxd --lib --no-run --message-format=json-render-diagnostics > $anchorMessages
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          $linked = @(Get-Content -LiteralPath $anchorMessages | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'arxd' -and ($_.target.kind -contains 'lib') -and $_.profile.test -eq $true -and $_.manifest_path -eq $manifest -and $_.executable })
          if ($linked.Count -ne 1) { throw 'Invalid C16 anchor compiler-artifact' }
          $anchor = $linked[0].executable
          if ((Split-Path -Parent $anchor) -ne "$targetDir/debug/deps" -or (Split-Path -Leaf $anchor) -cnotmatch '^arxd-[a-f0-9]{16}$') { throw 'Invalid C16 anchor artifact path' }
          $item = Get-Item -LiteralPath $anchor
          if (($item.LinkType -and $item.LinkType -ne 'HardLink') -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.PSIsContainer) { throw 'Invalid anchor artifact' }
          $receipt = [ordered]@{
            schema = 'arxcovo/c16-anchor/1'
            head = $env:GITHUB_SHA
            run = $env:GITHUB_RUN_ID
            attempt = $env:GITHUB_RUN_ATTEMPT
            target_dir = $targetDir
            executable = $anchor
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $anchor).Hash.ToLowerInvariant()
            bytes = [int64]$item.Length
            messages_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $anchorMessages).Hash.ToLowerInvariant()
            selector = 'cluster::consensus::bound_start::runtime::c16_probe::c16_distinct_reconnect_anchor_only'
          }
          $receipt | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $proofRoot 'c16-anchor-receipt.json') -Encoding ascii -NoNewline`,
    `name: Hosted C16 live three-container acceptance
        if: runner.os == 'Linux'
        timeout-minutes: 5
        shell: bash
        run: python3 -I -B tools/ci/m3-containers/run.py --mode live`,
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

test('CI replaces superseded PR runs and gives only Windows Rust a 13-minute job limit', () => {
  const concurrency = workflow.match(/^concurrency:\n((?:[ \t]+[^\n]*\n)+)/m)?.[1];
  assert.equal(concurrency, "  group: ${{ github.event_name == 'pull_request' && format('ci-pr-{0}', github.event.pull_request.number) || format('ci-run-{0}', github.run_id) }}\n  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n");
  assert.equal((workflow.match(/^\s*concurrency:/gm) ?? []).length, 1);
  for (const name of ['rust', 'ui', 'verify']) {
    const timeout = name === 'rust' ? rustTimeout : '    timeout-minutes: 12';
    assert.deepEqual(jobs[name].match(/^ {4}timeout-minutes:.*$/gm), [timeout], name);
  }
  assert.deepEqual(workflow.match(/^.*\btimeout-minutes['"]?\s*:.*$/gm), [
    rustTimeout,
    '        timeout-minutes: 3',
    '        timeout-minutes: 2',
    '        timeout-minutes: 2',
    '        timeout-minutes: 5',
    '    timeout-minutes: 12',
    '    timeout-minutes: 12',
  ]);
});

test('CI disables dev debuginfo for Rust without overriding runtime profile defaults', () => {
  assert.ok(jobs.rust.includes(`${rustTimeout}\n    env:\n      CARGO_PROFILE_DEV_DEBUG: '0'\n    steps:\n`));
  assert.equal((workflow.match(/^\s*env:/gm) ?? []).length, 2);
  assert.deepEqual(workflow.match(/\bCARGO_\w+/g), ['CARGO_PROFILE_DEV_DEBUG', 'CARGO_HOME', 'CARGO_HOME', 'CARGO_TARGET_DIR', 'CARGO_TARGET_DIR', 'CARGO_HOME', 'CARGO_TARGET_DIR', 'CARGO_TARGET_DIR', 'CARGO_TARGET_DIR']);
  assertNoCargoProfileOverride(cargoPolicyFiles, path => readFileSync(new URL(path, import.meta.url), 'utf8'));
});

const cargoPolicyFiles = [
  { path: '../Cargo.toml', required: true },
  { path: '../.cargo/config', required: false },
  { path: '../.cargo/config.toml', required: false },
];

function assertNoCargoProfileOverride(files, readFile) {
  for (const { path, required } of files) {
    let text;
    try {
      text = readFile(path);
    } catch (error) {
      if (!required && error?.code === 'ENOENT') continue;
      const code = error?.code ? ` (${error.code})` : '';
      throw new Error(`${path} must be readable to check Cargo profiles${code}`, { cause: error });
    }
    assert.doesNotMatch(text, /^\s*\[\s*profile\b/m, `${path} must not override Cargo profiles`);
  }
}

test('Cargo profile policy skips only a missing optional config and fails closed on every other read', () => {
  const clean = Object.fromEntries(cargoPolicyFiles.map(file => [file.path, '[package]\nname = "arxcovo"\n']));
  const read = results => path => {
    const result = results[path];
    if (result instanceof Error) throw result;
    return result;
  };
  const ioError = code => Object.assign(new Error(code), { code });

  assertNoCargoProfileOverride(cargoPolicyFiles, read(clean));
  assertNoCargoProfileOverride(cargoPolicyFiles, read({
    ...clean,
    '../.cargo/config': ioError('ENOENT'),
    '../.cargo/config.toml': ioError('ENOENT'),
  }));

  for (const code of ['EACCES', 'EPERM', 'EIO']) {
    for (const file of cargoPolicyFiles) {
      assert.throws(
        () => assertNoCargoProfileOverride(cargoPolicyFiles, read({ ...clean, [file.path]: ioError(code) })),
        error => error?.message?.includes(`${file.path} must be readable to check Cargo profiles (${code})`),
        `${file.path} ${code}`,
      );
    }
  }
  assert.throws(
    () => assertNoCargoProfileOverride(cargoPolicyFiles, read({ ...clean, '../Cargo.toml': ioError('ENOENT') })),
    error => error?.message?.includes('../Cargo.toml must be readable to check Cargo profiles (ENOENT)'),
  );

  for (const file of cargoPolicyFiles) {
    assert.throws(
      () => assertNoCargoProfileOverride(cargoPolicyFiles, read({ ...clean, [file.path]: '[profile.dev]\ndebug = 0\n' })),
      error => error?.message?.includes(`${file.path} must not override Cargo profiles`),
      file.path,
    );
  }
});
