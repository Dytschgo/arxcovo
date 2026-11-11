"""Manual disposable CI only. Import and guard/planning functions have no effects."""
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time

TEST = 'exec::disposable::disposable_linux_cross_account'
TOOLCHAIN = '1.95.0-x86_64-unknown-linux-gnu'
SAFE_ENV = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C.UTF-8'}


def require(ok, code):
    if not ok:
        raise RuntimeError(code)


def identity(env):
    for key in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'):
        require(re.fullmatch(r'[1-9][0-9]{0,19}', env.get(key, '')), 'E_ID')
    return env['GITHUB_RUN_ID'] + '-' + env['GITHUB_RUN_ATTEMPT']


def gate(env, platform):
    require(platform == 'linux', 'E_PLATFORM')
    expected = {'CI': 'true', 'GITHUB_ACTIONS': 'true',
                'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_OS': 'Linux',
                'GITHUB_EVENT_NAME': 'workflow_dispatch',
                'GITHUB_REPOSITORY': 'Dytschgo/arxcovo',
                'GITHUB_ACTOR': 'Dytschgo', 'GITHUB_TRIGGERING_ACTOR': 'Dytschgo',
                'GITHUB_REF': 'refs/heads/main'}
    for key, value in expected.items():
        require(env.get(key) == value, 'E_GATE_' + key)
    sha = env.get('ARX_REVIEWED_SHA', '')
    require(re.fullmatch('[0-9a-f]{40}', sha), 'E_SHA_FORMAT')
    require(env.get('GITHUB_SHA') == sha == env.get('ARX_WORKFLOW_SHA'), 'E_SHA')
    return identity(env)


def plan(run):
    require(re.fullmatch(r'[1-9][0-9]{0,19}-[1-9][0-9]{0,19}', run), 'E_ID')
    return Path('/opt/arx-ci-' + run), 'arx-ci-' + run + '.service'


def protected(path):
    """Match library ancestor checks; never resolve away a link."""
    require(path.is_absolute() and '..' not in path.parts, 'E_PATH')
    for item in [*reversed(path.parents), path]:
        s = item.lstat()
        require(not stat.S_ISLNK(s.st_mode) and s.st_uid == 0 and
                not s.st_mode & 0o022, 'E_PROTECTION')


def call(args, **kw):
    return subprocess.run(args, env=SAFE_ENV, check=True, timeout=30,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kw)


def copy_tree(src, dst, exclude=()):
    # No dereferencing checkout/cache symlinks or special files into root code.
    def ignore(directory, names):
        return set(exclude) & set(names)
    for root, dirs, files in os.walk(src, followlinks=False):
        dirs[:] = [d for d in dirs if d not in exclude]
        for name in dirs + [f for f in files if f not in exclude]:
            s = (Path(root) / name).lstat()
            require(stat.S_ISDIR(s.st_mode) or stat.S_ISREG(s.st_mode), 'E_COPY_TYPE')
    shutil.copytree(src, dst, ignore=ignore)
    for root, dirs, files in os.walk(dst):
        os.chmod(root, 0o700)
        for name in files:
            p = Path(root) / name
            os.chmod(p, 0o700 if p.stat().st_mode & 0o111 else 0o600)


def cg_preflight():
    require(Path('/run/systemd/system').is_dir(), 'E_SYSTEMD')
    protected(Path('/sys/fs/cgroup'))
    fs = subprocess.check_output(['/usr/bin/stat', '-f', '-c', '%T', '/sys/fs/cgroup'],
                                 env=SAFE_ENV, timeout=5).strip()
    require(fs == b'cgroup2fs', 'E_CGROUP2')
    require({'memory', 'pids'} <= set(Path('/sys/fs/cgroup/cgroup.controllers').read_text().split()),
            'E_CONTROLLERS')
    # Conservative whole-hierarchy policy: a new fixture UID must not acquire
    # writable cgroups outside our unit, including ACL grants (mask write bits).
    count = 0
    for root, dirs, files in os.walk('/sys/fs/cgroup', followlinks=False):
        for name in dirs + files:
            count += 1
            require(count <= 20000, 'E_CGROUP_SCAN_BOUND')
            s = (Path(root) / name).lstat()
            require(s.st_uid == 0 and not s.st_mode & 0o022 and
                    not stat.S_ISLNK(s.st_mode), 'E_CGROUP_WRITABLE_HIERARCHY')


def cargo_env(stage):
    return {**SAFE_ENV, 'HOME': str(stage), 'CARGO_HOME': str(stage / 'cargo'),
            'CARGO_NET_OFFLINE': 'true', 'RUSTC': str(stage / 'rust/bin/rustc'),
            'RUSTDOC': str(stage / 'rust/bin/rustdoc')}


def command(stage):
    return [str(stage / 'rust/bin/cargo'), 'test', '-p', 'arxd', '--locked', '--lib', TEST]


def fixture_cleanup(stage):
    """Only the PID recorded by the root-owned Cargo runner, never a glob."""
    import pwd
    import grp
    capture = stage / 'harness-pid'
    if not capture.exists():
        return
    protected(capture)
    pid = capture.read_text()
    require(re.fullmatch('[1-9][0-9]{0,9}', pid), 'E_PID')
    name = 'arxexec' + pid
    base = Path('/var/lib') / name
    try:
        account = pwd.getpwnam(name)
    except KeyError:
        account = None
    if account:
        require((stage / 'fixture-identity').exists(), 'E_QUARANTINE_UID_UNCAPTURED')
        captured = json.loads((stage / 'fixture-identity').read_text())
        group = grp.getgrnam(name)
        require(account.pw_uid == captured['uid'] >= 1000 and
                account.pw_gid == captured['gid'] == group.gr_gid >= 1000 and
                account.pw_shell == '/usr/sbin/nologin' and not group.gr_mem,
                'E_FIXTURE_UID')
        uid = account.pw_uid
        # Unit is already stopped. Any task outside it with this UID quarantines.
        for proc in Path('/proc').iterdir():
            if proc.name.isdigit():
                try:
                    require(proc.stat().st_uid != uid, 'E_UID_LIVE')
                except FileNotFoundError:
                    pass
        call(['/usr/sbin/userdel', name])
    if base.exists() or base.is_symlink():
        protected(base)
        require((stage / 'fixture-directory').exists(), 'E_QUARANTINE_PATH_UNCAPTURED')
        captured = json.loads((stage / 'fixture-directory').read_text())
        require([base.stat().st_dev, base.stat().st_ino] == captured, 'E_FIXTURE_INODE')
        # Descendants can be build-user owned but must never be followed.
        require(shutil.rmtree.avoids_symlink_attacks, 'E_RMTREE')
        shutil.rmtree(base)
    try:
        group = grp.getgrnam(name)
    except KeyError:
        group = None
    if group:
        require(account is not None and group.gr_gid == account.pw_gid and not group.gr_mem,
                'E_GROUP_ORPHAN')
        call(['/usr/sbin/groupdel', name])


def service(stage, run):
    # Only entered from our protected transient unit, never a local CLI shortcut.
    protected(stage)
    _, unit = plan(run)
    cg = Path('/sys/fs/cgroup') / Path('/proc/self/cgroup').read_text().strip().split('0::')[-1].lstrip('/')
    require(cg == Path('/sys/fs/cgroup/system.slice') / unit, 'E_UNIT_CGROUP')
    protected(cg)
    require({'memory', 'pids'} <= set((cg / 'cgroup.controllers').read_text().split()), 'E_DELEGATION')
    control = cg / 'control'
    control.mkdir(mode=0o700)  # exclusive: never reuse another group's child
    (control / 'cgroup.procs').write_text(str(os.getpid()))
    require(not (cg / 'cgroup.procs').read_text().strip(), 'E_INTERNAL_TASKS')
    (cg / 'cgroup.subtree_control').write_text('+memory +pids')
    delegated = cg / 'runs'
    delegated.mkdir(mode=0o700)
    protected(delegated)
    (delegated / 'cgroup.subtree_control').write_text('+memory +pids')
    require(not (delegated / 'cgroup.procs').read_text().strip(), 'E_INTERNAL_TASKS')
    env = {**cargo_env(stage), 'CI': 'true', 'GITHUB_ACTIONS': 'true',
           'RUNNER_ENVIRONMENT': 'github-hosted', 'ARX_DISPOSABLE_EXEC_TEST': '1',
           'ARX_EXEC_CGROUP_PARENT': str(delegated),
           'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER': str(stage / 'runner')}
    # File size limit bounds private diagnostics; output never copied wholesale.
    import resource
    resource.setrlimit(resource.RLIMIT_FSIZE, (1024 * 1024, 1024 * 1024))
    with (stage / 'test-output').open('xb') as out:
        child = subprocess.Popen(['/usr/bin/timeout', '--signal=KILL', '120s'] +
                                 command(stage) + ['--', '--ignored', '--exact'],
                                 cwd=stage / 'source', env=env,
                                 stdout=out, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 120
        try:
            while child.poll() is None:
                capture_fixture(stage)
                require(time.monotonic() < deadline, 'E_TEST_TIMEOUT')
                time.sleep(0.01)
            capture_fixture(stage)
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=5)
    text = (stage / 'test-output').read_text(errors='replace')
    passed = f'test {TEST} ... ok' in text and '1 passed; 0 failed' in text
    require(child.returncode == 0 and passed and (stage / 'harness-pid').exists(), 'E_TEST')
    (stage / 'passed').write_text('1')


def capture_fixture(stage):
    """Capture identities while alive; missing capture quarantines residuals."""
    import pwd
    capture = stage / 'harness-pid'
    if not capture.exists():
        return
    pid = capture.read_text()
    # Runner may have opened but not yet written its marker. Never guess a name.
    if not pid:
        return
    require(re.fullmatch('[1-9][0-9]{0,9}', pid), 'E_PID')
    name = 'arxexec' + pid
    base = Path('/var/lib') / name
    directory = stage / 'fixture-directory'
    if not directory.exists() and base.exists():
        protected(base)
        s = base.stat()
        directory.write_text(json.dumps([s.st_dev, s.st_ino]))
    captured = stage / 'fixture-identity'
    if not captured.exists():
        try:
            account = pwd.getpwnam(name)
        except KeyError:
            return
        require(account.pw_uid >= 1000 and account.pw_gid >= 1000, 'E_FIXTURE_UID')
        captured.write_text(json.dumps({'uid': account.pw_uid, 'gid': account.pw_gid}))


def harness(stage, args):
    import pwd
    import grp
    protected(stage)
    binary = (stage / 'artifact').read_text()
    require(args == [binary, TEST, '--ignored', '--exact'], 'E_TEST_ARGS')
    protected(Path(binary))
    pid = str(os.getpid())
    name = 'arxexec' + pid
    require(not os.path.lexists('/var/lib/' + name), 'E_FIXTURE_COLLISION')
    for lookup in (pwd.getpwnam, grp.getgrnam):
        try:
            lookup(name)
        except KeyError:
            continue
        raise RuntimeError('E_ACCOUNT_COLLISION')
    with (stage / 'harness-pid').open('x') as f:
        f.write(pid)
    os.execve(binary, args, dict(os.environ))


def run(env):
    run_id = gate(env, sys.platform)  # before any mutation or privileged probe
    require(os.getuid() == os.geteuid() == 0, 'E_ROOT')
    cg_preflight()  # read-only; no retry or global controller writes
    stage, unit = plan(run_id)
    protected(stage.parent)
    # Refuse unit collisions before taking ownership of this generated name.
    state = subprocess.check_output(['/usr/bin/systemctl', 'show', unit, '-p', 'LoadState', '--value'],
                                    env=SAFE_ENV, timeout=5).strip()
    require(state == b'not-found', 'E_UNIT_COLLISION')
    stage.mkdir(mode=0o700)
    unit_created = False
    try:
        source = Path.cwd()
        require(subprocess.check_output(['/usr/bin/git', 'rev-parse', 'HEAD'], env=SAFE_ENV,
                                        timeout=5).decode().strip() == env['GITHUB_SHA'], 'E_CHECKOUT')
        # Fixed hosted-runner tool locations; no user supplied paths or config.
        copy_tree(source, stage / 'source', ('.git', 'target', 'node_modules', '__pycache__'))
        copy_tree(Path('/home/runner/.rustup/toolchains') / TOOLCHAIN, stage / 'rust')
        (stage / 'cargo').mkdir(mode=0o700)
        copy_tree(Path('/home/runner/.cargo/registry'), stage / 'cargo/registry')
        driver = stage / 'source/tools/unix-exec-validation/driver.py'
        (stage / 'runner').write_text('#!/bin/sh\nexec /usr/bin/python3 -I ' + str(driver) +
                                    ' harness ' + run_id + ' "$@"\n')
        (stage / 'runner').chmod(0o700)
        build_env = cargo_env(stage)
        # Build before the 120s harness bound, with no fixture account yet.
        with (stage / 'build-output').open('xb') as out:
            result = subprocess.run(command(stage) + ['--no-run', '--message-format=json'],
                                    cwd=stage / 'source', env=build_env, timeout=360,
                                    stdout=out, stderr=subprocess.DEVNULL)
        require(result.returncode == 0, 'E_BUILD')
        require((stage / 'build-output').stat().st_size <= 4 * 1024 * 1024, 'E_BUILD_LOG')
        artifacts = [j['executable'] for line in (stage / 'build-output').read_text().splitlines()
                     if (j := json.loads(line)).get('reason') == 'compiler-artifact'
                     and j.get('executable') and j.get('profile', {}).get('test')]
        require(len(artifacts) == 1, 'E_ARTIFACT')
        artifact = Path(artifacts[0])
        require(artifact.is_relative_to(stage / 'source/target/debug/deps'), 'E_ARTIFACT_PATH')
        protected(artifact)
        (stage / 'artifact').write_text(str(artifact))
        unit_created = True  # name reserved by absence; finally stops even start failure
        result = subprocess.run(['/usr/bin/systemd-run', '--quiet', '--wait', '--collect',
                                 '--unit=' + unit, '--property=Delegate=yes',
                                 '--slice=system.slice',
                                 '--property=RuntimeMaxSec=140', '--property=TimeoutStopSec=10',
                                 '--property=KillMode=control-group', '--property=UMask=0077',
                                 '/usr/bin/python3', '-I', str(driver), 'service', run_id],
                                env=SAFE_ENV, timeout=165, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL)
        require(result.returncode == 0 and (stage / 'passed').exists(), 'E_SERVICE_OR_TEST')
        print('harness: 1 passed; observed UID asserted internally (not emitted)')
    finally:
        if unit_created:
            # Stopping our captured unit kills descendants; never kill host hierarchy.
            subprocess.run(['/usr/bin/systemctl', 'stop', unit], env=SAFE_ENV, timeout=20,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            cg = Path('/sys/fs/cgroup/system.slice') / unit
            require(not cg.exists(), 'E_QUARANTINE_CGROUP')
        fixture_cleanup(stage)
        protected(stage)
        require(shutil.rmtree.avoids_symlink_attacks, 'E_RMTREE')
        shutil.rmtree(stage)
        print('cleanup: confirmed')


def main():
    require(len(sys.argv) >= 2, 'E_MODE')
    mode = sys.argv[1]
    if mode in ('guard', 'run'):
        require(len(sys.argv) == 2, 'E_ARGS')
        gate(os.environ, sys.platform)
        if mode == 'run':
            run(os.environ)
        return
    # Internal modes require root, protected installed path, numeric identity.
    require(sys.platform == 'linux', 'E_PLATFORM')
    require(os.getuid() == os.geteuid() == 0, 'E_ROOT')
    require(len(sys.argv) >= 3, 'E_ARGS')
    stage, _ = plan(sys.argv[2])
    require(Path(__file__) == stage / 'source/tools/unix-exec-validation/driver.py', 'E_INSTALLED')
    protected(Path(__file__))
    if mode == 'service' and len(sys.argv) == 3:
        service(stage, sys.argv[2])
    elif mode == 'harness':
        harness(stage, sys.argv[3:])
    else:
        raise RuntimeError('E_MODE')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # Never expose arbitrary exception text, paths, cargo output or environment.
        code = str(error) if isinstance(error, RuntimeError) and re.fullmatch(r'E_[A-Z_]+', str(error)) else 'E_RUNTIME'
        if isinstance(error, OSError):
            code = 'E_OS_' + str(error.errno or 0)
        elif isinstance(error, subprocess.TimeoutExpired):
            code = 'E_COMMAND_TIMEOUT'
        print('unix experiment refused/failed: ' + code, file=sys.stderr)
        sys.exit(1)
