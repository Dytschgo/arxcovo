"""Manual disposable CI only. Import and guard/planning functions have no effects."""
import json
import hashlib
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time
import threading

TEST = 'exec::disposable::sudo_nnp::disposable_linux_a7_a11'
TOOLCHAIN = '1.95.0-x86_64-unknown-linux-gnu'
SAFE_ENV = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C.UTF-8'}


def require(ok, code):
    if not ok:
        raise RuntimeError(code)


def standard_profile():
    require(TEST in ('exec::disposable::disposable_linux_a1_a2',
                     'exec::disposable::disposable_linux_a3_standard',
                     'exec::disposable::disposable_linux_a3_none',
                     'exec::disposable::a4::disposable_linux_a4',
                     'exec::disposable::a4::disposable_linux_a4_omitted_kill',
                     'exec::disposable::disposable_linux_a5',
                     'exec::a12::native::disposable_linux_a12',
                     'exec::disposable::sudo_nnp::disposable_linux_a7_a11'), 'E_TEST_ARGS')
    return TEST != 'exec::disposable::disposable_linux_a3_none'


# Neither delivery preparation nor execution is authorized by selecting a name.
A12_DELIVERY_ENABLED = False
A12_EXECUTION_ENABLED = False
A12_TEST = 'exec::a12::native::disposable_linux_a12'


def a12_selected():
    return TEST == A12_TEST


def a12_open(path, directory=False, owner=None):
    require(path.is_absolute() and '..' not in path.parts, 'E_PATH')
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    fd = os.open('/', flags | os.O_DIRECTORY)
    try:
        for part in path.parts[1:]:
            info = os.fstat(fd)
            require(stat.S_ISDIR(info.st_mode) and
                    (info.st_uid == 0 and not info.st_mode & 0o022 or
                     owner is not None and info.st_uid == owner and stat.S_IMODE(info.st_mode) == 0o700), 'E_PATH')
            child = os.open(part, flags, dir_fd=fd)
            os.close(fd)
            fd = child
        if directory:
            require(stat.S_ISDIR(os.fstat(fd).st_mode), 'E_PATH')
        result, fd = fd, None
        return result
    finally:
        if fd is not None:
            os.close(fd)


def a12_pin(path, binary=False, owner=None):
    fd = a12_open(path, not binary, owner)
    try:
        info = os.fstat(fd)
        digest, count = hashlib.sha256(), 0
        if binary:
            require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and
                    4 <= info.st_size <= 128 * 1024 * 1024, 'E_ARTIFACT')
            while chunk := os.read(fd, 65536):
                count += len(chunk)
                require(count <= info.st_size, 'E_ARTIFACT')
                digest.update(chunk)
            require(count == info.st_size, 'E_ARTIFACT')
        require(_stable_stat(info) == _stable_stat(os.fstat(fd)), 'E_FIXTURE_INODE')
        return dict(dev=info.st_dev, ino=info.st_ino, mode=info.st_mode,
                    uid=info.st_uid, gid=info.st_gid, nlink=info.st_nlink,
                    size=count, sha256=digest.hexdigest() if binary else '')
    finally:
        os.close(fd)


def a12_name(run):
    plan(run)
    name = 'arxa12' + run.replace('-', 'x')
    require(len(name) <= 32 and len(run.split('-')[1]) <= 6, 'E_ID')
    return name


def build_a12(stage):
    # Single libtest invocation, bounded while reading, with owned wait/kill/join.
    build_env = dict(cargo_env(stage), CARGO_PROFILE_DEV_DEBUG="0")
    exceeded = threading.Event()
    start = time.monotonic()
    with (stage / 'a12-build-output').open('xb') as out:
        with subprocess.Popen(command(stage) + ['--no-run', '--message-format=json'],
                cwd=stage / 'source', env=build_env, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL) as child:
            def capture():
                try:
                    drain_output(child.stdout, out, exceeded, 4 * 1024 * 1024)
                except Exception:
                    exceeded.set()
            drain = threading.Thread(target=capture)
            drain.start()
            try:
                child.wait(timeout=360)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=5)
                drain.join(timeout=5)
            require(not drain.is_alive() and not exceeded.is_set(), 'E_BUILD_LOG')
            require(child.returncode == 0, 'E_BUILD')
    require(0 <= time.monotonic() - start <= 360, 'E_BUILD')
    data = (stage / 'a12-build-output').read_bytes()
    require(len(data) <= 4 * 1024 * 1024, 'E_BUILD_LOG')
    artifacts = [Path(j['executable']) for line in data.splitlines()
                 if (j := json.loads(line)).get('reason') == 'compiler-artifact'
                 and j.get('executable') and j.get('profile', {}).get('test')]
    require(len(artifacts) == 1, 'E_ARTIFACT')
    artifact = artifacts[0]
    require(artifact.parent == stage / 'source/target/debug/deps' and
            re.fullmatch('arxd-[0-9a-f]+', artifact.name), 'E_ARTIFACT_PATH')
    return artifact


def a12_local_identity(item):
    for path, width in (('/etc/passwd', 7), ('/etc/group', 4)):
        fd = a12_open(Path(path))
        try:
            info = os.fstat(fd)
            require(stat.S_ISREG(info.st_mode) and info.st_uid == 0 and
                    not info.st_mode & 0o022 and info.st_size <= 1024 * 1024, 'E_FIXTURE_UID')
            data = bytearray()
            while chunk := os.read(fd, min(65536, 1024 * 1024 + 1 - len(data))):
                data.extend(chunk)
                require(len(data) <= 1024 * 1024, 'E_FIXTURE_UID')
            require(len(data) == info.st_size and _stable_stat(info) == _stable_stat(os.fstat(fd)), 'E_FIXTURE_UID')
            rows = [line.split(':') for line in data.decode('utf-8').splitlines()]
            require(all(len(row) == width for row in rows), 'E_FIXTURE_UID')
            key = item['uid'] if width == 7 else item['gid']
            require([row[0] for row in rows if row[2] == str(key)] == [item['name']], 'E_FIXTURE_UID')
            if width == 7:
                require([row[0] for row in rows if row[3] == str(item['gid'])] == [item['name']], 'E_FIXTURE_UID')
            else:
                require(all(item['name'] not in row[3].split(',') for row in rows), 'E_FIXTURE_UID')
        finally:
            os.close(fd)


def a12_account(item):
    import pwd
    import grp
    account = pwd.getpwnam(item['name'])
    group = grp.getgrnam(item['name'])
    a12_local_identity(item)
    require(account.pw_uid == item['uid'] >= 1000 and
            account.pw_gid == group.gr_gid == item['gid'] >= 1000 and
            account.pw_shell == '/usr/sbin/nologin' and
            account.pw_dir == str(item['workspace']) and not group.gr_mem and
            os.getgrouplist(item['name'], item['gid']) == [item['gid']], 'E_FIXTURE_UID')


def prepare_a12(env, owned):
    require(A12_DELIVERY_ENABLED and a12_selected(), 'E_TEST_ARGS')
    import pwd
    import grp
    run = gate(env, sys.platform)
    stage, _ = plan(run)
    name = a12_name(run)
    require(not owned, 'E_FIXTURE_COLLISION')
    for lookup in (pwd.getpwnam, grp.getgrnam):
        try:
            lookup(name)
        except KeyError:
            continue
        raise RuntimeError('E_ACCOUNT_COLLISION')
    # Flags precede mutation: uncertainty always quarantines, never adopts.
    root = Path('/var/lib') / name
    require(not os.path.lexists(root), 'E_FIXTURE_COLLISION')
    artifact = build_a12(stage)
    artifact.chmod(0o700)
    root.mkdir(mode=0o700)  # collision owns nothing
    owned.update(name=name, workspace=root / 'workspace', pending=True)
    owned['root'] = root.lstat()
    call(['/usr/sbin/groupadd', name])
    owned['gid'] = grp.getgrnam(name).gr_gid
    call(['/usr/sbin/useradd', '--no-create-home', '--no-user-group', '--gid', name,
          '--shell', '/usr/sbin/nologin', '--home-dir', str(owned['workspace']), name])
    owned['uid'] = pwd.getpwnam(name).pw_uid
    owned['workspace'].mkdir(mode=0o700)
    os.chown(root, owned['uid'], owned['gid'])
    owned['root'] = root.lstat()
    os.chown(owned['workspace'], owned['uid'], owned['gid'])
    owned['workspace_info'] = owned['workspace'].lstat()
    a12_account(owned)
    owned['pending'] = False
    data = dict(version=1, run=run, source_sha=env['GITHUB_SHA'],
                workflow_sha=env['ARX_WORKFLOW_SHA'], uid=owned['uid'], gid=owned['gid'],
                root=a12_pin(root), workspace=a12_pin(owned['workspace'], owner=owned['uid']),
                harness=a12_pin(artifact, True))
    encoded = json.dumps(data, separators=(',', ':')).encode('ascii')
    require(len(encoded) <= 4096, 'E_ARTIFACT')
    exclusive_file(stage / 'a12-source-sha', env['GITHUB_SHA'].encode('ascii'))
    exclusive_file(stage / 'a12-delivery.json', encoded)
    exclusive_file(stage / 'artifact', str(artifact).encode('ascii'))
    return artifact


def a12_uid_released(uid):
    count = 0
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        count += 1
        require(count <= 20000, 'E_UID_LIVE')
        fd = None
        try:
            require(proc.stat().st_uid != uid, 'E_UID_LIVE')
            fd = os.open(proc / 'status', os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            data = os.read(fd, 4097)
            require(len(data) <= 4096, 'E_UID_LIVE')
            rows = [line.split()[1:] for line in data.splitlines() if line.startswith(b'Uid:')]
            require(len(rows) == 1 and len(rows[0]) == 4 and
                    all(value.isdigit() and int(value) != uid for value in rows[0]), 'E_UID_LIVE')
        except FileNotFoundError:
            pass
        finally:
            if fd is not None:
                os.close(fd)


def cleanup_a12(owned, unit_gone=False):
    if not owned:
        return
    require(unit_gone and not owned.get('pending'), 'E_QUARANTINE_CGROUP')
    a12_account(owned)
    # Confirm live UID release BEFORE account, workspace, stage or runtime deletion.
    a12_uid_released(owned['uid'])
    same_object(owned['workspace'].parent, owned['root'])
    same_object(owned['workspace'], owned['workspace_info'])
    call(['/usr/sbin/userdel', owned['name']])
    call(['/usr/sbin/groupdel', owned['name']])
    require(shutil.rmtree.avoids_symlink_attacks, 'E_RMTREE')
    shutil.rmtree(owned['workspace'].parent)


def a4_selected():
    standard_profile()
    return TEST in ('exec::disposable::a4::disposable_linux_a4',
                    'exec::disposable::a4::disposable_linux_a4_omitted_kill')


A4_IMAGE_SHA = 'cee895412d58718016dd0db9883526ef8e0b13f8ed00cf213c6f77536b206369'
A4_IMAGE_SIZE = 3680


def a4_image(data):
    require(len(data) == A4_IMAGE_SIZE and
            hashlib.sha256(data).hexdigest() == A4_IMAGE_SHA, 'E_ARTIFACT')
    return data


def a4_input(name, bound):
    # Fixed checkout-relative assets only, each opened root-down without links.
    # Checkout is the existing reviewed-source boundary; not owner attestation.
    require(name in ('helper.rs', 'recipe.json', 'linux-image-pin.json',
                     'out/candidate-pin.json', 'out/a4-helper'), 'E_PATH')
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
    fd = os.open('/', flags | os.O_DIRECTORY)
    try:
        parts = (*Path.cwd().parts[1:], 'tools', 'unix-exec-validation', 'a4-helper',
                 *name.split('/'))
        for part in parts[:-1]:
            child = os.open(part, flags | os.O_DIRECTORY, dir_fd=fd)
            os.close(fd)
            fd = child
        image = os.open(parts[-1], flags, dir_fd=fd)
        try:
            info = os.fstat(image)
            require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and
                    info.st_size <= bound, 'E_ARTIFACT')
            with os.fdopen(os.dup(image), 'rb') as stream:
                data = stream.read(bound + 1)
            require(len(data) <= bound, 'E_ARTIFACT')
            return data
        finally:
            os.close(image)
    finally:
        os.close(fd)


def a4_delivery():
    require(a4_selected(), 'E_TEST_ARGS')
    # Source/recipe authorization is independent of image authorization. The
    # Both independent host pins stay fixed; candidates get no self-authorization.
    inputs = {}
    for name, digest in (
        ('helper.rs', '770ab397264ab78deb5d54854bd00d4cfabb1b3660b0e975ee751472fbeada5a'),
        ('recipe.json', '90a3d497971ce504470c6bbead42e4a1b37f77ec0cd56b11cbb6312be739b47d'),
        ('linux-image-pin.json', '6427856805b5fa362de01cace73930c6f1f9fa78ee08f32a8da61a252b0a13d8')):
        inputs[name] = a4_input(name, 16384)
        require(hashlib.sha256(inputs[name]).hexdigest() == digest, 'E_ARTIFACT')
    reviewed = json.loads(inputs['linux-image-pin.json'])
    candidate = json.loads(a4_input('out/candidate-pin.json', 16384), object_pairs_hook=unique_pairs)
    require(type(candidate) is dict and set(candidate) == set(reviewed), 'E_ARTIFACT')
    # Compiler hashes are observations of the unprivileged Linux build, not a
    # replacement trust pin. Only the independent source/recipe/image may admit.
    for key in ('compiler_sha256', 'linker_sha256'):
        require(type(candidate[key]) is str and re.fullmatch('[0-9a-f]{64}', candidate[key]), 'E_ARTIFACT')
        reviewed[key] = candidate[key]
    reviewed['compiler_identity'] = reviewed['compiler_identity'].replace(
        'host: x86_64-pc-windows-msvc', 'host: x86_64-unknown-linux-gnu')
    require(candidate == reviewed, 'E_ARTIFACT')
    return a4_image(a4_input('out/a4-helper', A4_IMAGE_SIZE))


def a4_directory(fd):
    info = os.fstat(fd)
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and
            not info.st_mode & 0o022, 'E_PATH')
    return info


def install_a4_helper(data, owned):
    require(a4_selected(), 'E_TEST_ARGS')
    a4_image(data)  # before even opening the privileged parent
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
    parent = os.open('/', flags | os.O_DIRECTORY)
    owned['parent'] = parent
    for part in ('usr', 'libexec'):
        a4_directory(parent)
        child = os.open(part, flags | os.O_DIRECTORY, dir_fd=parent)
        os.close(parent)
        parent = child
        owned['parent'] = parent
    a4_directory(parent)
    # Existing directories, links and any differing parent refuse: no adoption.
    os.mkdir('arxcovo', mode=0o755, dir_fd=parent)
    owned['created'] = True
    owned['directory'] = os.stat('arxcovo', dir_fd=parent, follow_symlinks=False)
    directory = os.open('arxcovo', flags | os.O_DIRECTORY, dir_fd=parent)
    owned['fd'] = directory
    info = a4_directory(directory)
    require((info.st_dev, info.st_ino) ==
            (owned['directory'].st_dev, owned['directory'].st_ino), 'E_FIXTURE_INODE')
    fd = os.open('a4-helper', os.O_RDWR | os.O_CREAT | os.O_EXCL |
                 os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=directory)
    try:
        owned['image'] = os.fstat(fd)
        require(os.write(fd, data) == len(data), 'E_ARTIFACT')
        os.fchmod(fd, 0o755)
        owned['image'] = os.fstat(fd)
        info = owned['image']
        require(stat.S_ISREG(info.st_mode) and info.st_uid == 0 and
                stat.S_IMODE(info.st_mode) == 0o755 and info.st_nlink == 1 and
                info.st_size == A4_IMAGE_SIZE, 'E_ARTIFACT')
        os.lseek(fd, 0, os.SEEK_SET)
        a4_image(os.read(fd, A4_IMAGE_SIZE + 1))
    finally:
        os.close(fd)


def remove_a4_helper(owned):
    if 'directory' not in owned:
        require(not owned.get('created'), 'E_QUARANTINE_CGROUP')
        return  # collision owns no asset
    require('fd' in owned, 'E_QUARANTINE_CGROUP')
    parent, directory = owned['parent'], owned['fd']
    a4_directory(parent)
    a4_directory(directory)
    def same(name, fd, expected):
        now = os.stat(name, dir_fd=fd, follow_symlinks=False)
        require((now.st_dev, now.st_ino, now.st_mode, now.st_uid) ==
                (expected.st_dev, expected.st_ino, expected.st_mode, expected.st_uid),
                'E_FIXTURE_INODE')
    same('arxcovo', parent, owned['directory'])
    require(set(os.listdir(directory)) == ({'a4-helper'} if 'image' in owned else set()), 'E_PATH')
    if 'image' in owned:
        same('a4-helper', directory, owned['image'])
        os.unlink('a4-helper', dir_fd=directory)
    os.rmdir('arxcovo', dir_fd=parent)


# Fixed combined candidate. The profile labels reviewed facts, never host proof.
A7_PROFILE = '99e7f5b4b3b63ff13955432bf9b7cde0436c9ab57afbc93fbe99bb45eb17017e'


def a7_selected():
    return TEST == 'exec::disposable::sudo_nnp::disposable_linux_a7_a11'


def a7_package_read():
    # Isolated Python has no sibling import path. Load only the reviewed fixed
    # comparator bytes root-down without links. The fixed source hash supplies
    # trust for checkout bytes; installed payload ancestry is checked by _read_linux.
    path = Path(__file__).absolute().parent / 'sudo_package_profile.py'
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
    fd = os.open('/', flags | os.O_DIRECTORY)
    try:
        for part in path.parts[1:-1]:
            child = os.open(part, flags | os.O_DIRECTORY, dir_fd=fd)
            os.close(fd)
            fd = child
        child = os.open(path.name, flags, dir_fd=fd)
        os.close(fd)
        fd = child
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1
                and info.st_size == 11180, 'E_ARTIFACT')
        data = os.read(fd, 11181)
        require(len(data) == 11180 and os.read(fd, 1) == b'' and
                hashlib.sha256(data).hexdigest() ==
                '12a122db7d30be04dd7523db046d49f4083882c318b375ccf0986ec0eb338e75', 'E_ARTIFACT')
        scope = {'__name__': 'a7_fixed_package'}
        exec(compile(data, '<fixed-package>', 'exec'), scope)
        return scope['_read_linux']()
    finally:
        os.close(fd)


def _stable_stat(info):
    # Reads may update atime; identity, permissions and content metadata must hold.
    return tuple(getattr(info, field) for field in (
        "st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink",
        "st_size", "st_mtime_ns", "st_ctime_ns"))


def a7_alias_c2():
    # Missing final C2 is allowed; missing/unsafe ancestors and dangling links refuse.
    def open_role(path, directory, role):
        try:
            return sudo_nnp_open(path, directory)
        except RuntimeError as error:
            # Attribute only this initial pre-mutation E_PATH fence. OS errors,
            # later admissions and all other refusal codes keep their old form.
            if (type(error) is RuntimeError and error.args == ('E_PATH',) and
                    feedback_mutation == 'none'):
                raise A7PathRefusal(role) from None
            raise
    held = []
    try:
        parent = open_role(Path('/usr/local/bin'), True, 'a7_c2_parent')
        held.append(parent)
        root = open_role(Path('/'), True, 'a7_alias_root')
        held.append(root)
        usrbin = open_role(Path('/usr/bin'), True, 'a7_alias_target')
        held.append(usrbin)
        try:
            info = os.stat('sudo', dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError:
            info = None
        if info is not None:
            require(stat.S_ISREG(info.st_mode) and info.st_uid == 0 and
                    info.st_nlink == 1 and not info.st_mode & 0o022, 'E_C2')
        before = os.stat('bin', dir_fd=root, follow_symlinks=False)
        require(stat.S_ISLNK(before.st_mode) and before.st_uid == 0 and
                before.st_nlink == 1 and before.st_size in (7, 8), 'E_ALIAS')
        import ctypes
        call = ctypes.CDLL(None).readlinkat
        call.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_void_p, ctypes.c_size_t)
        call.restype = ctypes.c_ssize_t
        buffer = ctypes.create_string_buffer(9)
        length = call(root, b'bin', buffer, 9)
        require(0 <= length < 9 and buffer.raw[:length] in (b'usr/bin', b'/usr/bin'), 'E_ALIAS')
        after = os.stat('bin', dir_fd=root, follow_symlinks=False)
        require(_stable_stat(before) == _stable_stat(after) and
                _stable_stat(os.stat('bin', dir_fd=root)) == _stable_stat(os.fstat(usrbin)), 'E_ALIAS')
        if info is not None:
            fd = open_role(Path('/usr/local/bin/sudo'), False, 'a7_c2_image')
            try:
                pin = sudo_nnp_digest(fd)
                require(pin['size'] == 282032 and pin['sha256'] ==
                        '6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff'
                        and _stable_stat(os.stat('sudo', dir_fd=parent, follow_symlinks=False)) == _stable_stat(info),
                        'E_C2')
            finally:
                os.close(fd)
    finally:
        for fd in reversed(held):
            os.close(fd)


def a7_alias_controlled():
    # ADR0020 D9c controlled fixture ONLY: protected / and /usr/bin ancestry plus
    # the merged-/usr alias. Host /usr/local/bin is never opened or claimed safe
    # or absent here; the unit proves its closed empty C2 view instead. The
    # strict a7_alias_c2 gate above stays unchanged for every other caller.
    def open_role(path, role):
        try:
            return sudo_nnp_open(path, True)
        except RuntimeError as error:
            if (type(error) is RuntimeError and error.args == ('E_PATH',) and
                    feedback_mutation == 'none'):
                raise A7PathRefusal(role) from None
            raise
    held = []
    try:
        root = open_role(Path('/'), 'a7_alias_root')
        held.append(root)
        usrbin = open_role(Path('/usr/bin'), 'a7_alias_target')
        held.append(usrbin)
        before = os.stat('bin', dir_fd=root, follow_symlinks=False)
        require(stat.S_ISLNK(before.st_mode) and before.st_uid == 0 and
                before.st_nlink == 1 and before.st_size in (7, 8), 'E_ALIAS')
        import ctypes
        call = ctypes.CDLL(None).readlinkat
        call.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_void_p, ctypes.c_size_t)
        call.restype = ctypes.c_ssize_t
        buffer = ctypes.create_string_buffer(9)
        length = call(root, b'bin', buffer, 9)
        require(0 <= length < 9 and buffer.raw[:length] in (b'usr/bin', b'/usr/bin'), 'E_ALIAS')
        after = os.stat('bin', dir_fd=root, follow_symlinks=False)
        require(_stable_stat(before) == _stable_stat(after) and
                _stable_stat(os.stat('bin', dir_fd=root)) == _stable_stat(os.fstat(usrbin)), 'E_ALIAS')
    finally:
        for fd in reversed(held):
            os.close(fd)


def a7_admission():
    require(a7_selected() and sys.platform == 'linux' and
            os.getuid() == os.geteuid() == 0 and os.uname().machine == 'x86_64', 'E_POLICY_UNATTESTED')
    result = a7_package_read()
    require(result == dict(schema=1, status='candidate_match', candidate_match=True,
            qualified=False, revision='1.9.15p5-3ubuntu5.24.04.4', architecture='amd64',
            digests=dict(sudo='6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff',
                         sudoers='151d681d51293a040dc479cb0175d1a6d461a40ff455661cf6ad485adf305d4c',
                         util='aae167b9da2436a31eba12d9fb20b36120fd204b2c720d62881d09e89eecd26d')),
            'E_ARTIFACT')
    a7_alias_controlled()
    cg_preflight()
    return A7_PROFILE


def sudo_nnp_host_attested():
    # Legacy original-host-policy path stays closed.
    return False


def sudo_nnp_eligible(host_attested, policy_attested):
    return host_attested is True and policy_attested is True


def sudo_nnp_names(run):
    require(re.fullmatch('[1-9][0-9]{0,19}-[1-9][0-9]{0,5}', run), 'E_IDENTITY')
    names = tuple('arxsn' + run.replace('-', 'x') + suffix for suffix in ('r', 'n'))
    require(all(len(name) <= 32 for name in names), 'E_IDENTITY')
    return names


def sudo_nnp_accounts(run, owned):
    """Root controller only, before service start. Caller retains owned on failure.

    No adoption: pre-existing accounts/groups, roots or policy names refuse before
    the first mutation. Unknown identity after a partial command quarantines.
    """
    require(a7_admission() == A7_PROFILE, 'E_POLICY_UNATTESTED')
    import pwd
    import grp
    require(os.getuid() == os.geteuid() == 0 and not owned, 'E_FIXTURE_UID')
    names = sudo_nnp_names(run)
    for parent in (Path('/var/lib'),):
        protected(parent, 'run_parent')
    for name in names:
        for lookup in (pwd.getpwnam, grp.getgrnam):
            try:
                lookup(name)
            except KeyError:
                continue
            raise RuntimeError('E_FIXTURE_COLLISION')
        for path in (Path('/var/lib') / name,):
            require(not os.path.lexists(path), 'E_FIXTURE_COLLISION')
    for name in names:
        base = Path('/var/lib') / name
        item = dict(name=name, base=base, created=False, identity=None, policy=None)
        owned.append(item)
        base.mkdir(mode=0o700)
        item['base_created'] = True
        item['base_info'] = base.lstat()
        # Mark before command: failure may have partially created account/group.
        item['created'] = True
        call(['/usr/sbin/useradd', '--no-create-home', '--user-group',
              '--shell', '/usr/sbin/nologin', name])
        account, group = pwd.getpwnam(name), grp.getgrnam(name)
        require(account.pw_uid >= 1000 and account.pw_gid == group.gr_gid >= 1000 and
                account.pw_shell == '/usr/sbin/nologin' and not group.gr_mem and
                os.getgrouplist(name, account.pw_gid) == [account.pw_gid], 'E_FIXTURE_UID')
        item['identity'] = (account.pw_uid, account.pw_gid)
        cg_scan(uid=account.pw_uid, gid=account.pw_gid)
        workspace = base / 'workspace'
        workspace.mkdir(mode=0o700)
        os.chown(workspace, account.pw_uid, account.pw_gid, follow_symlinks=False)
        item['workspace_info'] = workspace.lstat()
        # Private build-owned root; retain captured inode for owned-only cleanup.
        os.chown(base, account.pw_uid, account.pw_gid, follow_symlinks=False)
        item['base_info'] = base.lstat()
        yield item
    require(owned[0]['identity'][0] != owned[1]['identity'][0] and
            owned[0]['identity'][1] != owned[1]['identity'][1], 'E_FIXTURE_UID')


def prepare_sudo_nnp(run, owned):
    """Legacy dormant host-policy path; never called by controlled preparation."""
    require(sudo_nnp_eligible(sudo_nnp_host_attested(), False), 'E_POLICY_UNATTESTED')
    require(os.getuid() == os.geteuid() == 0 and not owned, 'E_FIXTURE_UID')
    names = sudo_nnp_names(run)
    os.close(sudo_nnp_open(Path('/etc/sudoers.d'), True))
    for name in names:
        require(not os.path.lexists(Path('/etc/sudoers.d') / name), 'E_FIXTURE_COLLISION')
    for index, item in enumerate(sudo_nnp_accounts(run, owned)):
        name = item['name']
        if index == 0:
            policy = Path('/etc/sudoers.d') / name
            data = (name + ' ALL=(root) NOPASSWD: /usr/bin/true\n').encode('ascii')
            fd = os.open(policy, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o440)
            # Capture immediately, including short-write/fchmod/syntax failure.
            item['policy_pending'] = True
            try:
                item['policy'] = (policy, os.fstat(fd))
                item['policy_pending'] = False
                require(os.write(fd, data) == len(data), 'E_POLICY_WRITE')
                os.fchmod(fd, 0o440)
                item['policy'] = (policy, os.fstat(fd))
            finally:
                os.close(fd)
            # Fixed bounded syntax refusal only, never an effective-policy check.
            subprocess.run(['/usr/sbin/visudo', '-c', '-f', str(policy)], env=SAFE_ENV,
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)


def cleanup_sudo_nnp(owned, unit_gone=False):
    """Only after existing driver stop/cgroup confirmation. Uncertainty retains.

    All identity and live-task checks precede destructive cleanup. No wildcard
    paths, policy discovery, account adoption, or foreign-state deletion.
    """
    import pwd
    import grp
    require(unit_gone is True, 'E_QUARANTINE_CGROUP')
    for item in owned:
        require(not item.get('policy_pending') and
                (not item.get('base_created') or item.get('base_info') is not None),
                'E_QUARANTINE_PATH_UNCAPTURED')
        require(item.get('identity') is not None or not item['created'], 'E_QUARANTINE_UID_UNCAPTURED')
        if item.get('base_info') is not None:
            same_object(item['base'], item['base_info'])
        if item['created']:
            account, group = pwd.getpwnam(item['name']), grp.getgrnam(item['name'])
            require((account.pw_uid, account.pw_gid) == item['identity'] and
                    group.gr_gid == account.pw_gid and not group.gr_mem and
                    account.pw_shell == '/usr/sbin/nologin' and
                    os.getgrouplist(item['name'], account.pw_gid) == [account.pw_gid], 'E_FIXTURE_UID')
            for count, proc in enumerate(Path('/proc').iterdir()):
                require(count < 20000, 'E_PROC_BOUND')
                if proc.name.isdigit():
                    try:
                        require(proc.stat().st_uid != account.pw_uid, 'E_UID_LIVE')
                    except FileNotFoundError:
                        pass
        if item['policy']:
            policy, info = item['policy']
            same_object(policy, info)
            require(stat.S_ISREG(policy.lstat().st_mode) and policy.lstat().st_uid == 0
                    and policy.lstat().st_nlink == 1, 'E_POLICY_IDENTITY')
    require(shutil.rmtree.avoids_symlink_attacks, 'E_RMTREE')
    for item in reversed(owned):
        if item['policy']:
            policy, info = item['policy']
            same_object(policy, info)
            policy.unlink()
            require(not os.path.lexists(policy), 'E_POLICY_RESIDUE')
        if item['created']:
            call(['/usr/sbin/userdel', item['name']])
            try:
                pwd.getpwnam(item['name'])
            except KeyError:
                pass
            else:
                raise RuntimeError('E_FIXTURE_RESIDUE')
            try:
                group = grp.getgrnam(item['name'])
            except KeyError:
                group = None
            if group is not None:
                require(group.gr_gid == item['identity'][1] and not group.gr_mem, 'E_GROUP_ORPHAN')
                call(['/usr/sbin/groupdel', item['name']])
            try:
                grp.getgrnam(item['name'])
            except KeyError:
                pass
            else:
                raise RuntimeError('E_GROUP_ORPHAN')
        if item.get('base_info') is not None:
            same_object(item['base'], item['base_info'])
            shutil.rmtree(item['base'])
            require(not os.path.lexists(item['base']), 'E_FIXTURE_RESIDUE')


# Candidate only: finite package/default checks precede effects; live view remains required.
SUDO_NNP_PLUGIN = '/usr/libexec/sudo/sudoers.so'


def controlled_literals(run):
    case0, _case1 = sudo_nnp_names(run)
    return {'sudoers': b'#includedir /etc/sudoers.d\n',
            'sudo.conf': ('Plugin sudoers_policy ' + SUDO_NNP_PLUGIN + '\n').encode('ascii'),
            'nsswitch.conf': b'passwd: files\ngroup: files\nsudoers: files\n',
            'sudoers.d/10-a7-case0': (case0 + ' ALL=(root) NOPASSWD: /usr/bin/true\n').encode('ascii')}


# ADR0020 D9c: exactly the four D9b policy views plus the closed empty C2 view.
CONTROLLED_VIEWS = (('sudoers', '/etc/sudoers'), ('sudo.conf', '/etc/sudo.conf'),
                    ('nsswitch.conf', '/etc/nsswitch.conf'), ('sudoers.d', '/etc/sudoers.d'),
                    ('c2-empty', '/usr/local/bin'))
CONTROLLED_DIRECTORIES = ('fx', 'fx/sudoers.d', 'fx/c2-empty')
CONTROLLED_ENTRIES = {'fx': {'sudoers', 'sudo.conf', 'nsswitch.conf', 'sudoers.d', 'c2-empty'},
                      'fx/sudoers.d': {'10-a7-case0'}, 'fx/c2-empty': set()}


def controlled_kind(name):
    # (directory, empty C2 view source); the view source is root 0755, never 0700.
    return name in CONTROLLED_DIRECTORIES, name == 'fx/c2-empty'


def controlled_mappings(run):
    sudo_nnp_names(run)
    stage, _ = plan(run)
    return tuple((stage / 'fx' / name).as_posix() + ':' + target
                 for name, target in CONTROLLED_VIEWS)


def controlled_mapping_set(run, mappings):
    expected = controlled_mappings(run)
    require(len(mappings) == 5 and set(mappings) == set(expected), 'E_POLICY_MAPPING')


def controlled_override(run):
    stage, _ = plan(run)
    return fixture_override(stage, run) + ''.join(
        'BindReadOnlyPaths=' + mapping + '\n' for mapping in controlled_mappings(run))


def prepare_controlled_sudo_nnp(run, accounts, fixture):
    # No original-policy collector, legacy writer or runtime selector edge.
    require(a7_admission() == A7_PROFILE, 'E_POLICY_UNATTESTED')
    stage_controlled_sudo_nnp(run, fixture)
    for _ in sudo_nnp_accounts(run, accounts):
        pass


def controlled_pin(fd, directory=False, view=False):
    info = os.fstat(fd)
    # The empty C2 source must be searchable by the low-UID probe (ENOENT, not
    # EACCES/unknown); policy directories stay private 0700 and files 0440.
    directory = directory or view
    mode = 0o755 if view else 0o700 if directory else 0o440
    require(info.st_uid == info.st_gid == 0 and stat.S_IMODE(info.st_mode) == mode and
            (stat.S_ISDIR(info.st_mode) if directory else
             stat.S_ISREG(info.st_mode) and info.st_nlink == 1), 'E_POLICY_IDENTITY')
    digest = None
    if not directory:
        os.lseek(fd, 0, os.SEEK_SET)
        data = os.read(fd, 4097)
        require(len(data) <= 4096 and os.read(fd, 1) == b'', 'E_POLICY_BOUND')
        digest = hashlib.sha256(data).hexdigest()
    after = os.fstat(fd)
    require((info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid, info.st_nlink, info.st_size) ==
            (after.st_dev, after.st_ino, after.st_mode, after.st_uid, after.st_gid, after.st_nlink, after.st_size),
            'E_POLICY_IDENTITY')
    return (info.st_dev, info.st_ino, digest)


def stage_controlled_sudo_nnp(run, owned):
    require(a7_admission() == A7_PROFILE, 'E_POLICY_UNATTESTED')
    sudo_nnp_names(run)
    require(not owned and os.getuid() == os.geteuid() == 0, 'E_FIXTURE_UID')
    stage, _ = plan(run)
    parent = sudo_nnp_open(stage, True)
    try:
        require(stat.S_IMODE(os.fstat(parent).st_mode) == 0o700, 'E_POLICY_IDENTITY')
        owned['entries'] = []
        for name in CONTROLLED_DIRECTORIES:
            _, view = controlled_kind(name)
            owned['pending'] = True
            # Exclusive creation: an existing name raises and is never adopted.
            os.mkdir(name, mode=0o755 if view else 0o700, dir_fd=parent)
            fd = sudo_nnp_open(stage / name, True)
            owned['entries'].append((name, fd, None))
            if view:
                os.fchmod(fd, 0o755)  # held new inode; umask-independent exact mode
            owned['entries'][-1] = (name, fd, controlled_pin(fd, True, view))
            owned['pending'] = False
        for name, data in controlled_literals(run).items():
            owned['pending'] = True
            fd = os.open('fx/' + name, os.O_RDWR | os.O_CREAT | os.O_EXCL |
                         os.O_NOFOLLOW | os.O_CLOEXEC, 0o440, dir_fd=parent)
            # Retain descriptor immediately: short writes and uncertain capture quarantine.
            owned['entries'].append(('fx/' + name, fd, None))
            require(os.write(fd, data) == len(data), 'E_POLICY_WRITE')
            os.fchmod(fd, 0o440)
            owned['entries'][-1] = ('fx/' + name, fd, controlled_pin(fd))
            owned['pending'] = False
        recheck_controlled_sudo_nnp(run, owned)
    finally:
        os.close(parent)


def recheck_controlled_sudo_nnp(run, owned):
    stage, _ = plan(run)
    literals = controlled_literals(run)
    require(not owned.get('pending') and len(owned.get('entries', [])) == 7, 'E_POLICY_IDENTITY')
    require({n for n, _, _ in owned['entries']} ==
            set(CONTROLLED_DIRECTORIES) | {'fx/' + n for n in literals}, 'E_POLICY_ENTRIES')
    for name, held, pin in owned['entries']:
        directory, view = controlled_kind(name)
        require(pin is not None and controlled_pin(held, directory, view) == pin, 'E_POLICY_IDENTITY')
        fd = sudo_nnp_open(stage / name, directory)
        try:
            require(controlled_pin(fd, directory, view) == pin, 'E_POLICY_IDENTITY')
            if not directory:
                require(pin[2] == hashlib.sha256(literals[name[3:]]).hexdigest(), 'E_POLICY_LITERAL')
            else:
                expected = CONTROLLED_ENTRIES[name]
                require(set(os.listdir(fd)) == expected, 'E_POLICY_ENTRIES')
        finally:
            os.close(fd)


def a7_uids_released(accounts):
    """Read-only, after unit/cgroup removal and before ANY fixture deletion.

    An uncaptured created identity or a surviving task of a captured fixture UID
    retains every fixture path, including the empty C2 view source.
    """
    uids = set()
    for item in accounts:
        require(item.get('identity') is not None or not item['created'], 'E_QUARANTINE_UID_UNCAPTURED')
        if item.get('identity') is not None:
            uids.add(item['identity'][0])
    if not uids:
        return
    for count, proc in enumerate(Path('/proc').iterdir()):
        require(count < 20000, 'E_PROC_BOUND')
        if proc.name.isdigit():
            try:
                require(proc.stat().st_uid not in uids, 'E_UID_LIVE')
            except FileNotFoundError:
                pass


def cleanup_controlled_sudo_nnp(run, owned, unit_gone=False):
    require(unit_gone is True, 'E_QUARANTINE_CGROUP')
    # Partial/unidentified creation retains everything; never guess ownership.
    recheck_controlled_sudo_nnp(run, owned)
    stage, _ = plan(run)
    parent = sudo_nnp_open(stage, True)
    try:
        for name, held, pin in reversed(owned['entries']):
            directory, view = controlled_kind(name)
            fd = sudo_nnp_open(stage / name, directory)
            try:
                require(controlled_pin(fd, directory, view) == pin, 'E_POLICY_IDENTITY')
            finally:
                os.close(fd)
            if directory:
                os.rmdir(name, dir_fd=parent)
            else:
                os.unlink(name, dir_fd=parent)
            os.close(held)
        owned.clear()
    finally:
        os.close(parent)


def sudo_nnp_artifact(output, stage, libtest):
    require(len(output) <= 4 * 1024 * 1024, 'E_BUILD_LOG')
    artifacts = []
    for line in output.splitlines():
        j = json.loads(line, object_pairs_hook=unique_pairs)
        if j.get('reason') != 'compiler-artifact' or not j.get('executable'):
            continue
        require(j.get('target', {}).get('name') == 'arxd' and
                j.get('profile', {}).get('test') is libtest and
                j.get('target', {}).get('kind') == (['lib'] if libtest else ['bin']), 'E_ARTIFACT')
        artifacts.append(Path(j['executable']))
    require(len(artifacts) == 1, 'E_ARTIFACT')
    artifact = artifacts[0]
    target = stage / 'source/target/debug'
    require((libtest and artifact.parent == target / 'deps' and
             re.fullmatch('arxd-[0-9a-f]+', artifact.name)) or
            (not libtest and artifact == target / 'arxd'), 'E_ARTIFACT_PATH')
    return artifact


def build_sudo_nnp(env):
    """Dormant: one libtest (also harness), one production build; shared budget.

    The existing root-private offline copy/prebuild must already be complete.
    No account preparation, selector or run_body edge is added here.
    """
    run = gate(env, sys.platform)
    sudo_nnp_names(run)
    stage, _ = plan(run)
    stage_fd = sudo_nnp_open(stage, True)
    os.close(stage_fd)
    deadline = time.monotonic() + 360
    exceeded = threading.Event()
    artifacts = []
    with (stage / 'a7-a11-build-output').open('xb') as out:
        for libtest in (True, False):
            start = out.tell()
            args = [str(stage / 'rust/bin/cargo'), 'test' if libtest else 'build',
                    '-p', 'arxd', '--locked']
            args += ['--lib', '--no-run'] if libtest else ['--bin', 'arxd']
            args += ['--message-format=json']
            remaining = deadline - time.monotonic()
            require(remaining > 0, 'E_BUILD')
            with subprocess.Popen(args, cwd=stage / 'source', env=cargo_env(stage),
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL) as child:
                def capture():
                    try:
                        drain_output(child.stdout, out, exceeded, 4 * 1024 * 1024 - start)
                    except Exception:
                        exceeded.set()
                drain = threading.Thread(target=capture, daemon=True)
                drain.start()
                try:
                    child.wait(timeout=remaining)
                except BaseException:
                    child.kill()
                    child.wait(timeout=5)
                    raise
                finally:
                    drain.join(timeout=5)
                require(not drain.is_alive() and not exceeded.is_set(), 'E_BUILD_LOG')
                require(child.returncode == 0, 'E_BUILD')
                require(time.monotonic() <= deadline, 'E_BUILD')
            out.flush()
            with (stage / 'a7-a11-build-output').open('rb') as capture:
                capture.seek(start)
                data = capture.read(4 * 1024 * 1024 + 1)
            artifacts.append(sudo_nnp_artifact(data, stage, libtest))
    # Return harness first, production binary second. Never rebuild the harness.
    return tuple(artifacts)


def sudo_nnp_open(path, directory=False):
    """Fixed derived callers only. Check ALL ancestors including /usr/libexec."""
    require(path.is_absolute() and '..' not in path.parts, 'E_PATH')
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
    fd = os.open('/', flags | os.O_DIRECTORY)
    try:
        for index, part in enumerate(path.parts[1:]):
            a4_directory(fd)
            child = os.open(part, flags | (os.O_DIRECTORY if directory or
                            index < len(path.parts) - 2 else 0), dir_fd=fd)
            os.close(fd)
            fd = child
        if directory:
            a4_directory(fd)
        result, fd = fd, None
        return result
    finally:
        if fd is not None:
            os.close(fd)


def sudo_nnp_digest(fd):
    info = os.fstat(fd)
    require(stat.S_ISREG(info.st_mode) and info.st_uid == 0 and
            info.st_nlink == 1 and 4 <= info.st_size <= 128 * 1024 * 1024,
            'E_ARTIFACT')
    os.lseek(fd, 0, os.SEEK_SET)
    digest, count = hashlib.sha256(), 0
    while True:
        data = os.read(fd, 65536)
        if not data:
            break
        require(count != 0 or data.startswith(b'\x7fELF'), 'E_ARTIFACT')
        count += len(data)
        require(count <= info.st_size, 'E_ARTIFACT')
        digest.update(data)
    require(count == info.st_size, 'E_ARTIFACT')
    return dict(size=count, sha256=digest.hexdigest(), dev=info.st_dev, ino=info.st_ino)


def stage_sudo_nnp(env, owned):
    """Held-fd read -> O_EXCL copy -> destination rehash, never copy sudo.

    Fixed cargo outputs only. Libtest bytes ARE the harness bytes from the single
    test invocation, not an executable inferred from the subsequent bin build.
    Partial captures stay in owned for stop-gated cleanup; no adoption.
    """
    run = gate(env, sys.platform)
    sudo_nnp_names(run)
    require(a7_admission() == A7_PROFILE, 'E_POLICY_UNATTESTED')
    require(not owned, 'E_FIXTURE_COLLISION')
    stage, _ = plan(run)
    # Build capture defines exactly one output from each invocation.
    # The caller records the single libtest path as the existing harness artifact.
    harness, production = build_sudo_nnp(env)
    exclusive_file(stage / 'artifact', str(harness).encode('ascii'))
    require(harness.parent == stage / 'source/target/debug/deps' and
            re.fullmatch('arxd-[0-9a-f]+', harness.name), 'E_ARTIFACT_PATH')
    require(production == stage / 'source/target/debug/arxd', 'E_ARTIFACT_PATH')
    sources = (production, harness)
    parent = sudo_nnp_open(Path('/usr/libexec'), True)
    owned['parent'] = parent
    name = 'arxcovo-a7-a11-' + run
    owned['name'] = name
    os.mkdir(name, mode=0o755, dir_fd=parent)
    owned['created'] = True
    owned['directory'] = os.stat(name, dir_fd=parent, follow_symlinks=False)
    directory = sudo_nnp_open(Path('/usr/libexec') / name, True)
    owned['fd'] = directory
    info = os.fstat(directory)
    require((info.st_dev, info.st_ino) ==
            (owned['directory'].st_dev, owned['directory'].st_ino), 'E_FIXTURE_INODE')
    os.fchmod(directory, 0o755)
    owned['directory'] = os.fstat(directory)
    owned['images'] = []
    artifacts = []
    for source, target in zip(sources, ('arxd', 'reentry')):
        src = sudo_nnp_open(source)
        try:
            pin = sudo_nnp_digest(src)
            dst = os.open(target, os.O_RDWR | os.O_CREAT | os.O_EXCL |
                          os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=directory)
            owned['images'].append((target, os.fstat(dst)))
            try:
                os.lseek(src, 0, os.SEEK_SET)
                remaining = pin['size']
                while remaining:
                    data = os.read(src, min(65536, remaining))
                    require(data and os.write(dst, data) == len(data), 'E_ARTIFACT')
                    remaining -= len(data)
                os.fchmod(dst, 0o755)
                owned['images'][-1] = (target, os.fstat(dst))
                copied = sudo_nnp_digest(dst)
                require((copied['size'], copied['sha256']) == (pin['size'], pin['sha256']) and
                        sudo_nnp_digest(src) == pin, 'E_ARTIFACT')
                artifacts.append(copied)
            finally:
                os.close(dst)
        finally:
            os.close(src)
    return artifacts


def handoff_sudo_nnp(env, accounts, artifacts):
    run = gate(env, sys.platform)
    names = sudo_nnp_names(run)
    expected_profile = a7_admission()
    require(expected_profile == A7_PROFILE, 'E_POLICY_UNATTESTED')
    require(len(accounts) == len(artifacts) == 2, 'E_ARTIFACT')
    captured = []
    for item, name in zip(accounts, names):
        base = Path('/var/lib') / name
        require(item['name'] == name and item['base'] == base and item['created'], 'E_FIXTURE_UID')
        same_object(base, item['base_info'])
        same_object(base / 'workspace', item['workspace_info'])
        uid, gid = item['identity']
        require(uid >= 1000 and gid >= 1000, 'E_FIXTURE_UID')
        captured.append(dict(uid=uid, gid=gid, base_dev=item['base_info'].st_dev,
            base_ino=item['base_info'].st_ino, workspace_dev=item['workspace_info'].st_dev,
            workspace_ino=item['workspace_info'].st_ino))
    require(captured[0]['uid'] != captured[1]['uid'] and captured[0]['gid'] != captured[1]['gid'], 'E_FIXTURE_UID')
    data = canonical(dict(run=run, source_sha=env['GITHUB_SHA'],
        host_profile_sha256=expected_profile, accounts=captured, artifacts=artifacts))
    require(len(data) <= 4096, 'E_ARTIFACT')
    stage, _ = plan(run)
    exclusive_file(stage / 'a7-a11-delivery.json', data)


def cleanup_sudo_nnp_delivery(owned, unit_gone=False):
    require(unit_gone is True, 'E_QUARANTINE_CGROUP')
    if not owned.get('created'):
        return  # collision owns nothing
    require('directory' in owned and 'fd' in owned, 'E_QUARANTINE_PATH_UNCAPTURED')
    parent, directory = owned['parent'], owned['fd']
    a4_directory(parent)
    a4_directory(directory)
    def same(name, fd, expected):
        now = os.stat(name, dir_fd=fd, follow_symlinks=False)
        require((now.st_dev, now.st_ino, now.st_mode, now.st_uid, now.st_nlink) ==
                (expected.st_dev, expected.st_ino, expected.st_mode, expected.st_uid, expected.st_nlink), 'E_FIXTURE_INODE')
    same(owned['name'], parent, owned['directory'])
    images = owned.get('images', [])
    require(set(os.listdir(directory)) == {name for name, _ in images}, 'E_QUARANTINE_PATH_UNCAPTURED')
    for name, info in images:
        same(name, directory, info)
    for name, info in images:
        same(name, directory, info)
        os.unlink(name, dir_fd=directory)
    os.rmdir(owned['name'], dir_fd=parent)


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
    return Path('/var/lib/arx-ci-' + run), 'arxd.service'


# F0 mechanism only: no experiment is authorized by this instrumentation.
SITES = ('cg_preflight', 'cleanup_capture', 'cleanup_base', 'service_stage',
         'service_cg', 'service_runs', 'capture_base', 'harness_stage',
         'harness_binary', 'run_parent', 'run_artifact', 'run_cleanup', 'installed')
PHASES = ('initial', 'final')
MUTATIONS = ('none', 'staged', 'service', 'cgroup', 'account')
CLEANUPS = ('confirmed', 'unconfirmed', 'not_reached')
CODES = ('E_ACCOUNT_COLLISION', 'E_ARGS', 'E_ARTIFACT', 'E_ARTIFACT_PATH', 'E_BUILD', 'E_BUILD_LOG', 'E_CGROUP2', 'E_CGROUP_FIXTURE_OWNER', 'E_CGROUP_SCAN_BOUND', 'E_CGROUP_UNIT_OWNER', 'E_CGROUP_WRITABLE_HIERARCHY', 'E_CHECKOUT', 'E_CONTROLLERS', 'E_COPY_RACE', 'E_COPY_TYPE', 'E_DELEGATION', 'E_FIXTURE_COLLISION', 'E_FIXTURE_INODE', 'E_FIXTURE_UID', 'E_GROUP_ORPHAN', 'E_ID', 'E_INSTALLED', 'E_INTERNAL_TASKS', 'E_MODE', 'E_OUTPUT_DRAIN', 'E_PATH', 'E_PID', 'E_PLATFORM', 'E_QUARANTINE_CGROUP', 'E_QUARANTINE_PATH_UNCAPTURED', 'E_QUARANTINE_UID_UNCAPTURED', 'E_RECORD', 'E_RECORD_BOUND', 'E_RMTREE', 'E_ROOT', 'E_RUNTIME', 'E_SERVICE_OR_TEST', 'E_SHA', 'E_SHA_FORMAT', 'E_SITE', 'E_SYSTEMD', 'E_TEST', 'E_TEST_ARGS', 'E_TEST_LOG', 'E_TEST_TIMEOUT', 'E_UID_LIVE', 'E_UNIT_CGROUP', 'E_UNIT_COLLISION', 'E_GATE_CI', 'E_GATE_GITHUB_ACTIONS', 'E_GATE_RUNNER_ENVIRONMENT', 'E_GATE_RUNNER_OS', 'E_GATE_GITHUB_EVENT_NAME', 'E_GATE_GITHUB_REPOSITORY', 'E_GATE_GITHUB_ACTOR', 'E_GATE_GITHUB_TRIGGERING_ACTOR', 'E_GATE_GITHUB_REF', 'E_COMMAND_TIMEOUT', 'E_COMMAND_FAILED', 'E_OS')
MAX_TRIPLES = 32
MAX_LINE = 4096
feedback_phase = 'initial'
feedback_mutation = 'none'
feedback_cleanup = 'not_reached'
feedback_first = None
feedback_internal = None
internal_trusted = False


def triples_valid(triples):
    require(type(triples) is list and 1 <= len(triples) <= MAX_TRIPLES, 'E_RECORD')
    for t in triples:
        require(type(t) is list and len(t) == 3, 'E_RECORD')
        require(type(t[0]) is str and t[0] in ('dir', 'link', 'other') and
                type(t[1]) is bool and type(t[2]) is int and
                t[2] in (0, 2, 16, 18), 'E_RECORD')


def decide(triples):
    """Pure historical predicate; other (including regular files) stays allowed."""
    triples_valid(triples)
    for ordinal, (kind, owner_zero, write_bits) in enumerate(triples):
        for conjunct, failed in (('link', kind == 'link'),
                                   ('owner', not owner_zero), ('write', write_bits != 0)):
            if failed:
                return ordinal, conjunct
    return None


class Refusal(RuntimeError):
    def __init__(self, site, phase, triples):
        require(site in SITES and phase in PHASES, 'E_RECORD')
        result = decide(triples)
        require(result is not None and result[0] == len(triples) - 1, 'E_RECORD')
        self.record = dict(code='E_PROTECTION', site=site, phase=phase,
                           ordinal=result[0], kind=triples[-1][0],
                           owner_zero=triples[-1][1], write_bits=triples[-1][2],
                           predecessors=[list(t) for t in triples[:-1]])
        super().__init__('E_PROTECTION')


A7_PATH_ROLES = ('a7_c2_parent', 'a7_alias_root', 'a7_alias_target', 'a7_c2_image')


class A7PathRefusal(RuntimeError):
    def __init__(self, role):
        require(type(role) is str and role in A7_PATH_ROLES, 'E_RECORD')
        self.role = role
        super().__init__('E_PATH')


def exception_record(error):
    # Never stringify an arbitrary payload, even a RuntimeError payload.
    if type(error) is Refusal:
        return dict(error.record)
    if type(error) is A7PathRefusal:
        require(type(error.role) is str and error.role in A7_PATH_ROLES, 'E_RECORD')
        return dict(code='E_PATH', role=error.role)
    if type(error) is RuntimeError and len(error.args) == 1:
        code = error.args[0]
        if type(code) is str and code in CODES:
            return dict(code=code)
    if type(error) is subprocess.TimeoutExpired:
        return dict(code='E_COMMAND_TIMEOUT')
    if type(error) is subprocess.CalledProcessError:
        return dict(code='E_COMMAND_FAILED')
    if isinstance(error, OSError):
        return dict(code='E_OS')
    return dict(code='E_RUNTIME')


def validate_refusal(e):
    require(set(e) == {'code', 'site', 'phase', 'ordinal', 'kind', 'owner_zero',
                       'write_bits', 'predecessors'}, 'E_RECORD')
    require(e['code'] == 'E_PROTECTION' and type(e['site']) is str and e['site'] in SITES and
            type(e['phase']) is str and e['phase'] in PHASES and
            type(e['ordinal']) is int and type(e['predecessors']) is list, 'E_RECORD')
    triples = e['predecessors'] + [[e['kind'], e['owner_zero'], e['write_bits']]]
    result = decide(triples)
    require(result is not None and result[0] == e['ordinal'] == len(triples) - 1, 'E_RECORD')
    return e


def validate_record(record):
    require(type(record) is dict and set(record) ==
            {'schema', 'version', 'provenance', 'first', 'final', 'mutation_lower_bound', 'cleanup', 'internal'}, 'E_RECORD')
    require(record['schema'] == 'unix-protection-f0' and type(record['schema']) is str and
            type(record['version']) is int and record['version'] == 2, 'E_RECORD')
    p = record['provenance']
    require(type(p) is dict and set(p) == {'source_sha', 'workflow_sha', 'run', 'attempt'}, 'E_RECORD')
    for key in ('source_sha', 'workflow_sha'):
        require(type(p[key]) is str and re.fullmatch('[0-9a-f]{40}', p[key]), 'E_RECORD')
    require(p['source_sha'] == p['workflow_sha'], 'E_RECORD')
    for key in ('run', 'attempt'):
        require(type(p[key]) is str and re.fullmatch('[1-9][0-9]{0,19}', p[key]), 'E_RECORD')
    require(type(record['mutation_lower_bound']) is str and record['mutation_lower_bound'] in MUTATIONS and
            type(record['cleanup']) is str and record['cleanup'] in CLEANUPS, 'E_RECORD')
    for key in ('first', 'final'):
        e = record[key]
        if e is None:
            continue
        require(type(e) is dict, 'E_RECORD')
        if set(e) == {'code', 'role'}:
            require(type(e['code']) is str and e['code'] == 'E_PATH' and
                    type(e['role']) is str and e['role'] in A7_PATH_ROLES and
                    record['mutation_lower_bound'] == 'none', 'E_RECORD')
            continue
        if set(e) == {'code'} and type(e['code']) is str and e['code'] in CODES:
            continue
        validate_refusal(e)
    first, final = record['first'], record['final']
    mutation, cleanup = record['mutation_lower_bound'], record['cleanup']
    internal = record['internal']
    require((internal is None) == (mutation in ('none', 'staged')), 'E_RECORD')
    if internal is not None:
        validate_internal_summary(internal)
    if final is None:
        require(first is None and cleanup == 'confirmed' and mutation in ('service', 'cgroup', 'account') and
                all(internal[m] == 'absent' for m in PRECEDENCE), 'E_RECORD')
    if cleanup == 'confirmed':
        require(first == final, 'E_RECORD')
    if mutation == 'none':
        require(cleanup == 'not_reached', 'E_RECORD')
    if cleanup == 'not_reached':
        require(first == final and final is not None, 'E_RECORD')
    if final is not None and first is None:
        require(cleanup == 'unconfirmed' and mutation != 'none', 'E_RECORD')
    if first and first.get('code') == 'E_PROTECTION':
        require(first['phase'] == 'initial', 'E_RECORD')
    if final and final.get('code') == 'E_PROTECTION':
        require((final['phase'] == 'final' and cleanup == 'unconfirmed') or
                (final['phase'] == 'initial' and first == final), 'E_RECORD')
    return record


def encode_record(record):
    validate_record(record)
    line = json.dumps(record, separators=(',', ':'), ensure_ascii=True) + '\n'
    require(len(line.encode('ascii')) <= MAX_LINE, 'E_RECORD_BOUND')
    return line


def replay(line):
    require(type(line) is str and len(line) <= MAX_LINE and line.isascii() and
            line.endswith('\n') and line.count('\n') == 1, 'E_RECORD_BOUND')
    def pairs(items):
        out = {}
        for key, value in items:
            require(key not in out, 'E_RECORD')
            out[key] = value
        return out
    try:
        record = json.loads(line, object_pairs_hook=pairs)
        validate_record(record)
    except (ValueError, TypeError, RecursionError, KeyError):
        raise RuntimeError('E_RECORD') from None
    return {key: decide(e['predecessors'] + [[e['kind'], e['owner_zero'], e['write_bits']]])
            for key in ('first', 'final') if (e := record[key]) and e['code'] == 'E_PROTECTION'}


CHANNELS = {'service': 'feedback-service', 'harness': 'feedback-harness',
            'fixture-check': 'feedback-fixture-check'}
INTERNAL_SITES = {'service': ('service_stage', 'service_cg', 'service_runs', 'capture_base'),
                  'harness': ('harness_stage', 'harness_binary'),
                  'fixture-check': ('capture_base',)}
PRECEDENCE = ('service', 'fixture-check', 'harness')
STATUSES = ('absent', 'malformed', 'captured', 'unknown')
MAX_INTERNAL = 1024


def canonical(value):
    return (json.dumps(value, separators=(',', ':'), ensure_ascii=True) + '\n').encode('ascii')


def unique_pairs(items):
    out = {}
    for key, value in items:
        require(key not in out, 'E_RECORD')
        out[key] = value
    return out


def internal_refusal(mode, refusal):
    validate_refusal(refusal)
    require(refusal['site'] in INTERNAL_SITES[mode] and refusal['phase'] == 'initial', 'E_RECORD')


def validate_internal(value, mode):
    require(type(value) is dict and set(value) == {'schema', 'version', 'mode', 'refusal'}, 'E_RECORD')
    require(type(value['schema']) is str and value['schema'] == 'unix-protection-f0-internal' and
            type(value['version']) is int and value['version'] == 1 and
            type(value['mode']) is str and value['mode'] == mode, 'E_RECORD')
    internal_refusal(mode, value['refusal'])
    return dict(schema=value['schema'], version=1, mode=mode,
                refusal={k: value['refusal'][k] for k in
                         ('code', 'site', 'phase', 'ordinal', 'kind', 'owner_zero', 'write_bits', 'predecessors')})


def validate_internal_summary(value):
    require(type(value) is dict and set(value) == {*PRECEDENCE, 'carried', 'refusal'}, 'E_RECORD')
    require(all(type(value[m]) is str and value[m] in STATUSES for m in PRECEDENCE), 'E_RECORD')
    require(all(value[m] == 'unknown' for m in PRECEDENCE) or
            all(value[m] != 'unknown' for m in PRECEDENCE), 'E_RECORD')
    carried = next((m for m in PRECEDENCE if value[m] == 'captured'), None)
    require(value['carried'] == carried and (value['refusal'] is None) == (carried is None), 'E_RECORD')
    if carried is not None:
        internal_refusal(carried, value['refusal'])


def open_stage(run_id):
    fd = None
    try:
        stage, _ = plan(run_id)
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        for name in ('/', 'var', 'lib', stage.name):
            child = os.open(name, flags, dir_fd=fd)
            parent, fd = fd, child
            if parent is not None:
                os.close(parent)
            info = os.fstat(fd)
            require(stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and
                    info.st_mode & 0o022 == 0, 'E_RECORD')
        require(info.st_mode & 0o077 == 0, 'E_RECORD')
        result, fd = fd, None
        return result
    except Exception:
        return None
    finally:
        if fd is not None:
            try:
                os.close(fd)
            except Exception:
                pass


def create_channel(run_id, name, data):
    stage = fd = None
    try:
        stage = open_stage(run_id)
        if stage is None:
            return
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=stage)
        os.fchmod(fd, 0o600)
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == 0 and
                info.st_nlink == 1 and info.st_size == 0, 'E_RECORD')
        if data is not None:
            require(os.write(fd, data) == len(data), 'E_RECORD')
    finally:
        try:
            if fd is not None:
                os.close(fd)
        finally:
            if stage is not None:
                os.close(stage)


def write_internal(error):
    try:
        mode = sys.argv[1]
        if not internal_trusted or mode not in CHANNELS or type(error) is not Refusal:
            return
        internal_refusal(mode, error.record)
        value = dict(schema='unix-protection-f0-internal', version=1, mode=mode, refusal=error.record)
        data = canonical(validate_internal(value, mode))
        require(len(data) <= MAX_INTERNAL, 'E_RECORD')
        create_channel(sys.argv[2], CHANNELS[mode], data)
    except Exception:
        pass


def mark_cgroup(run_id):
    try:
        create_channel(run_id, 'progress-cgroup', None)
    except Exception:
        pass


def read_internal(stage, mode):
    fd = None
    try:
        try:
            fd = os.open(CHANNELS[mode], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                         dir_fd=stage)
        except FileNotFoundError:
            return 'absent', None
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode), 'E_RECORD')
        require(info.st_uid == 0, 'E_RECORD')
        require(stat.S_IMODE(info.st_mode) == 0o600, 'E_RECORD')
        require(info.st_nlink == 1, 'E_RECORD')
        require(0 < info.st_size <= MAX_INTERNAL, 'E_RECORD')
        data = b''
        while len(data) < MAX_INTERNAL + 1:
            chunk = os.read(fd, MAX_INTERNAL + 1 - len(data))
            if not chunk:
                break
            data += chunk
        require(len(data) == info.st_size and len(data) <= MAX_INTERNAL, 'E_RECORD')
        line = data.decode('ascii')
        require(line.endswith('\n') and line.count('\n') == 1, 'E_RECORD')
        value = validate_internal(json.loads(line, object_pairs_hook=unique_pairs), mode)
        require(canonical(value) == data, 'E_RECORD')
        return 'captured', value['refusal']
    except Exception:
        return 'malformed', None
    finally:
        if fd is not None:
            os.close(fd)


def snapshot_internal(run_id, unit, stopped):
    # Conditional snapshot within the existing containment assumption only.
    global feedback_internal, feedback_mutation
    feedback_internal = dict.fromkeys(PRECEDENCE, 'unknown')
    feedback_internal.update(carried=None, refusal=None)
    stage = None
    try:
        cgroup_absent = False
        if stopped is not None:
            try:
                os.lstat(Path('/sys/fs/cgroup/system.slice') / unit)
            except FileNotFoundError:
                cgroup_absent = True
            except Exception:
                pass
        snapshot_available = stopped is not None and cgroup_absent
        stage = open_stage(run_id)
        if stage is None:
            return
        for name, bound in (('progress-cgroup', 'cgroup'), ('fixture-identity', 'account')):
            try:
                info = os.stat(name, dir_fd=stage, follow_symlinks=False)
                if stat.S_ISREG(info.st_mode) and info.st_uid == 0:
                    if MUTATIONS.index(bound) > MUTATIONS.index(feedback_mutation):
                        feedback_mutation = bound
            except Exception:
                pass
        if snapshot_available:
            value = {}
            for mode in PRECEDENCE:
                status, refusal = read_internal(stage, mode)
                value[mode] = status
                if status == 'captured' and 'carried' not in value:
                    value.update(carried=mode, refusal=refusal)
            value.setdefault('carried', None)
            value.setdefault('refusal', None)
            feedback_internal = value
    except Exception:
        pass
    finally:
        if stage is not None:
            try:
                os.close(stage)
            except Exception:
                pass


def protected(path, site):
    """Match library ancestor checks; never resolve away a link."""
    require(site in SITES, 'E_SITE')
    require(path.is_absolute() and '..' not in path.parts, 'E_PATH')
    items = [*reversed(path.parents), path]
    require(len(items) <= MAX_TRIPLES, 'E_RECORD_BOUND')
    triples = []
    for item in items:
        s = item.lstat()
        kind = 'link' if stat.S_ISLNK(s.st_mode) else 'dir' if stat.S_ISDIR(s.st_mode) else 'other'
        triples.append([kind, s.st_uid == 0, s.st_mode & 0o022])
        if decide(triples) is not None:
            raise Refusal(site, feedback_phase, triples)


def call(args, **kw):
    return subprocess.run(args, env=SAFE_ENV, check=True, timeout=30,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kw)


def copy_tree(src, dst, exclude=()):
    # Open every component relative to a pinned directory; never reopen a checked
    # pathname for copying. Destination is exclusively created private staging.
    def copy_dir(fd, target):
        target.mkdir(mode=0o700)
        for name in os.listdir(fd):
            if name in exclude:
                continue
            s = os.stat(name, dir_fd=fd, follow_symlinks=False)
            require(stat.S_ISDIR(s.st_mode) or stat.S_ISREG(s.st_mode), 'E_COPY_TYPE')
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
            if stat.S_ISDIR(s.st_mode):
                flags |= os.O_DIRECTORY
            child = os.open(name, flags, dir_fd=fd)
            try:
                opened = os.fstat(child)
                require((s.st_dev, s.st_ino, stat.S_IFMT(s.st_mode)) ==
                        (opened.st_dev, opened.st_ino, stat.S_IFMT(opened.st_mode)), 'E_COPY_RACE')
                if stat.S_ISDIR(opened.st_mode):
                    copy_dir(child, target / name)
                else:
                    with os.fdopen(os.dup(child), 'rb') as source, (target / name).open('xb') as out:
                        shutil.copyfileobj(source, out)
                    (target / name).chmod(0o700 if opened.st_mode & 0o111 else 0o600)
            finally:
                os.close(child)
    fd = os.open(src, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        copy_dir(fd, dst)
    finally:
        os.close(fd)


def cg_preflight():
    require(Path('/run/systemd/system').is_dir(), 'E_SYSTEMD')
    protected(Path('/sys/fs/cgroup'), 'cg_preflight')
    fs = subprocess.check_output(['/usr/bin/stat', '-f', '-c', '%T', '/sys/fs/cgroup'],
                                 env=SAFE_ENV, timeout=5, stderr=subprocess.DEVNULL).strip()
    require(fs == b'cgroup2fs', 'E_CGROUP2')
    require({'memory', 'pids'} <= set(Path('/sys/fs/cgroup/cgroup.controllers').read_text().split()),
            'E_CONTROLLERS')
    version = subprocess.check_output(['/usr/bin/systemctl', '--version'], env=SAFE_ENV,
        timeout=5, stderr=subprocess.DEVNULL).splitlines()[0].split()
    require(len(version) >= 2 and version[1].isdigit() and int(version[1]) >= 254, 'E_SYSTEMD')
    kernel = re.match(r'([0-9]+)\.([0-9]+)', os.uname().release)
    require(kernel is not None and tuple(map(int, kernel.groups())) >= (5, 14), 'E_SYSTEMD')
    cg_scan()


def cg_scan(unit=None, uid=None, gid=None):
    # v2 migration needs write access to destination AND common ancestor procs.
    # Unrelated user delegation is safe; our ancestors/subtree remain root-owned.
    # ACL write masks appear in group mode bits and therefore refuse globally.
    count = 0
    for root, dirs, files in os.walk('/sys/fs/cgroup', followlinks=False):
        for name in dirs + files:
            count += 1
            require(count <= 20000, 'E_CGROUP_SCAN_BOUND')
            s = (Path(root) / name).lstat()
            path = Path(root) / name
            require(not s.st_mode & 0o022 and
                    not stat.S_ISLNK(s.st_mode), 'E_CGROUP_WRITABLE_HIERARCHY')
            if unit is not None and (path == unit or unit in path.parents or path in unit.parents):
                require(s.st_uid == 0, 'E_CGROUP_UNIT_OWNER')
            if uid is not None:
                require(s.st_uid != uid and s.st_gid != gid, 'E_CGROUP_FIXTURE_OWNER')


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
    protected(capture, 'cleanup_capture')
    pid = capture.read_text()
    require(re.fullmatch('[1-9][0-9]{0,9}', pid), 'E_PID')
    name = 'arxexec' + pid
    base = Path('/var/lib') / name
    try:
        account = pwd.getpwnam(name)
    except KeyError:
        account = None
    if base.exists() or base.is_symlink():
        protected(base, 'cleanup_base')
        require((stage / 'fixture-directory').exists(), 'E_QUARANTINE_PATH_UNCAPTURED')
        captured = json.loads((stage / 'fixture-directory').read_text())
        require([base.stat().st_dev, base.stat().st_ino] == captured, 'E_FIXTURE_INODE')
        # Descendants can be build-user owned but must never be followed.
        require(shutil.rmtree.avoids_symlink_attacks, 'E_RMTREE')
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
        shutil.rmtree(base)
    try:
        group = grp.getgrnam(name)
    except KeyError:
        group = None
    if group:
        require(account is not None and group.gr_gid == account.pw_gid and not group.gr_mem,
                'E_GROUP_ORPHAN')
        call(['/usr/sbin/groupdel', name])


def drain_output(source, target, exceeded, limit=1024 * 1024):
    written = 0
    while chunk := source.read(65536):
        keep = min(len(chunk), limit - written)
        target.write(chunk[:keep])
        written += keep
        if keep < len(chunk):
            exceeded.set()


def test_argv(binary):
    standard_profile()
    return [binary, TEST, '--ignored', '--exact', '--test-threads=1', '--color=never']


def test_env(notify):
    require(bool(notify), 'E_SERVICE_OR_TEST')
    return dict(CI='true', GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted',
                ARX_DISPOSABLE_EXEC_TEST='1', NOTIFY_SOCKET=notify)


def fixture_override(stage, run):
    require(plan(run)[0] == stage, 'E_PATH')
    return ('[Service]\nExecStart=\nExecStart=/usr/bin/python3 -I ' +
            str(stage / 'source/tools/unix-exec-validation/driver.py') +
            ' service ' + run + '\nRestart=no\nRuntimeMaxSec=140\n')


def exclusive_file(path, data):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        info = os.fstat(fd)
        with os.fdopen(os.dup(fd), 'wb') as out:
            out.write(data)
        return info
    finally:
        os.close(fd)


def private_read(path, bound=4096):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == 0 and
                stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1 and
                info.st_size <= bound, 'E_PATH')
        data = os.read(fd, bound + 1)
        require(len(data) <= bound, 'E_PATH')
        return data.decode('ascii')
    finally:
        os.close(fd)


def service(stage, run):
    protected(stage, 'service_stage')
    require(plan(run)[0] == stage, 'E_PATH')
    binary = private_read(stage / 'artifact')
    # Same process, next existing feedback channel; no new token or transport.
    sys.argv[1] = 'harness'
    harness(stage, test_argv(binary))


def unit_properties():
    data = subprocess.check_output(['/usr/bin/systemctl', 'show', 'arxd.service',
        '-p', 'LoadState,Id,Names,FragmentPath,DropInPaths,MainPID,ActiveState,Result,ExecMainStatus'],
        env=SAFE_ENV, timeout=5, stderr=subprocess.DEVNULL)
    require(len(data) <= 16384, 'E_UNIT_COLLISION')
    return dict(line.split('=', 1) for line in data.decode('ascii').splitlines() if '=' in line)


def capture_size(stage):
    try:
        info = (stage / 'test-output').lstat()
    except FileNotFoundError:
        return 0
    require(stat.S_ISREG(info.st_mode) and info.st_uid == 0 and
            stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1, 'E_TEST_LOG')
    return info.st_size


def monitor_a1a2(stage, artifact):
    deadline = time.monotonic() + 120
    monitor_start = deadline - 120
    acknowledged = False
    while True:
        require(time.monotonic() < deadline, 'E_TEST_TIMEOUT')
        require(capture_size(stage) <= 1024 * 1024, 'E_TEST_LOG')
        state = unit_properties()
        if state['ActiveState'] == 'active' and not acknowledged:
            ack_start = time.monotonic()
            if a7_selected() or a12_selected():
                require(ack_start - monitor_start <= 15, 'E_TEST_TIMEOUT')
            verify_runtime(stage, stage.name.removeprefix('arx-ci-'), state)
            pid = private_read(stage / 'harness-pid')
            require(re.fullmatch('[1-9][0-9]{0,9}', pid), 'E_PID')
            require(state['MainPID'] == pid, 'E_SERVICE_OR_TEST')
            require(os.readlink('/proc/' + pid + '/exe') == artifact, 'E_ARTIFACT')
            # procfs reads are separately bounded; these are not private stage files.
            fd = os.open('/proc/' + pid + '/cgroup', os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                membership = os.read(fd, 4097)
            finally:
                os.close(fd)
            require(membership == b'0::/system.slice/arxd.service/supervisor\n', 'E_UNIT_CGROUP')
            fd = os.open('/sys/fs/cgroup/system.slice/arxd.service/supervisor/cgroup.procs',
                         os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                tasks = os.read(fd, 4097)
            finally:
                os.close(fd)
            require(tasks == (pid + '\n').encode('ascii'), 'E_UNIT_CGROUP')
            if a7_selected() or a12_selected():
                require(time.monotonic() - ack_start <= 5, 'E_TEST_TIMEOUT')
            exclusive_file(stage / 'identity-ack', b'1')
            acknowledged = True
        if state['ActiveState'] in ('inactive', 'failed'):
            require(acknowledged and state['Result'] == 'success' and
                    state['ExecMainStatus'] == '0', 'E_SERVICE_OR_TEST')
            verify_test_result(stage, 0)
            require(deadline > time.monotonic(), 'E_TEST_TIMEOUT')
            return
        time.sleep(0.01)


def verify_test_result(stage, returncode):
    require((stage / 'test-output').stat().st_size <= 1024 * 1024, 'E_TEST_LOG')
    text = private_read(stage / 'test-output', 1024 * 1024)
    if a12_selected():
        for marker in ('ARX_A12_NORMAL_OK', 'ARX_A12_CANCEL_OK', 'ARX_A12_COMPLETE_OK'):
            require(text.count(marker + '\n') == 1, 'E_TEST')
            text = text.replace(marker + '\n', '')
        require(len(re.findall(r'^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9]+\.[0-9]+s$', text, re.M)) == 1, 'E_TEST')
    if a7_selected():
        for marker in ('ARX_A7_A11_CASE0_OK', 'ARX_A7_A11_CASE1_OK', 'ARX_A7_A11_COMPLETE_OK'):
            require(text.count(marker + '\n') == 1, 'E_TEST')
            text = text.replace(marker + '\n', '')
        require(len(re.findall(r'^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9]+\.[0-9]+s$', text, re.M)) == 1, 'E_TEST')
    if TEST == 'exec::disposable::a4::disposable_linux_a4_omitted_kill':
        require(text.count('ARX_A4_OMITTED_S7_REJECTED\n') == 1, 'E_TEST')
        # Raw fd output may interrupt libtest's in-progress test-ok line.
        text = text.replace('ARX_A4_OMITTED_S7_REJECTED\n', '')
    passed = (text.splitlines().count(f'test {TEST} ... ok') == 1 and
              re.findall(r'^test result: ok\. ([0-9]+) passed; ([0-9]+) failed;', text, re.M) == [('1', '0')])
    require(returncode == 0 and passed and (stage / 'harness-pid').exists(), 'E_TEST')


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
        protected(base, 'capture_base')
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
    protected(stage, 'harness_stage')
    binary = private_read(stage / 'artifact')
    require(args == test_argv(binary), 'E_TEST_ARGS')
    artifact = Path(binary)
    require(artifact.parent == stage / 'source/target/debug/deps' and
            re.fullmatch(r'arxd-[0-9a-f]+', artifact.name), 'E_ARTIFACT_PATH')
    protected(artifact, 'harness_binary')
    info = artifact.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_mode & 0o111, 'E_ARTIFACT')
    env = test_env(os.environ.get('NOTIFY_SOCKET'))
    exclusive_file(stage / 'harness-pid', str(os.getpid()).encode('ascii'))
    fd = os.open(stage / 'test-output', os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                 os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        os.dup2(fd, 1)
        os.dup2(fd, 2)
    finally:
        os.close(fd)
    os.chdir(stage)
    mark_cgroup(stage.name.removeprefix('arx-ci-'))
    os.execve(binary, args, env)


def same_object(path, info):
    now = path.lstat()
    require((now.st_dev, now.st_ino, now.st_mode, now.st_uid) ==
            (info.st_dev, info.st_ino, info.st_mode, info.st_uid), 'E_FIXTURE_INODE')


def refuse_unit_collision():
    props = unit_properties()
    require(props.get('LoadState') == 'not-found' and not props.get('FragmentPath') and
            not props.get('DropInPaths') and props.get('Id') == 'arxd.service' and
            set(props.get('Names', '').split()) <= {'arxd.service'}, 'E_UNIT_COLLISION')
    # All manager search paths, including linked aliases and generator output.
    paths = subprocess.check_output(['/usr/bin/systemd-analyze', 'unit-paths'],
        env=SAFE_ENV, timeout=5, stderr=subprocess.DEVNULL)
    require(len(paths) <= 16384, 'E_UNIT_COLLISION')
    for root in paths.decode('ascii').splitlines():
        require(root.startswith('/'), 'E_UNIT_COLLISION')
        for name in ('arxd.service', 'arxd.service.d'):
            require(not os.path.lexists(Path(root) / name), 'E_UNIT_COLLISION')
    require(not os.path.lexists('/run/arxcovo'), 'E_UNIT_COLLISION')
    count = 0
    for root, dirs, files in os.walk('/sys/fs/cgroup', followlinks=False):
        count += len(dirs) + len(files)
        require(count <= 20000, 'E_CGROUP_SCAN_BOUND')
        require('arxd.service' not in dirs + files, 'E_UNIT_COLLISION')


def install_runtime(stage, run, owned):
    standard_profile()
    root = Path('/run/systemd/system')
    protected(root, 'service_stage')
    for target, source in ((root / 'arxd.service', 'arxd.service'),):
        info = exclusive_file(target, (stage / 'source/packaging/linux' / source).read_bytes())
        owned.append((target, info))
    drop = root / 'arxd.service.d'
    drop.mkdir(mode=0o700)
    owned.append((drop, drop.lstat()))
    for target, data in runtime_files(stage, run)[1:]:
        owned.append((target, exclusive_file(target, data)))


def runtime_files(stage, run):
    present = standard_profile()
    root = Path('/run/systemd/system')
    package = stage / 'source/packaging/linux'
    files = [(root / 'arxd.service', (package / 'arxd.service').read_bytes())]
    if present:
        files.append((root / 'arxd.service.d/50-arxcovo-standard-rights.conf',
                      (package / '50-arxcovo-standard-rights.conf').read_bytes()))
    files.append((root / 'arxd.service.d/90-disposable-a1a2.conf',
                  (fixture_override(stage, run) +
                   (controlled_override(run) if a7_selected() else '')).encode('ascii')))
    return files


def verify_runtime(stage, run, state=None):
    files = runtime_files(stage, run)
    props = unit_properties() if state is None else state
    # systemctl show renders this space-separated, ordered list without brackets.
    expected = ' '.join(path.as_posix() for path, _ in files[1:])
    require(props.get('LoadState') == 'loaded' and props.get('Id') == 'arxd.service' and
            props.get('Names') == 'arxd.service' and
            props.get('FragmentPath') == '/run/systemd/system/arxd.service' and
            props.get('DropInPaths') == expected, 'E_UNIT_COLLISION')
    drop = Path('/run/systemd/system/arxd.service.d')
    info = drop.lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and
            stat.S_IMODE(info.st_mode) == 0o700, 'E_UNIT_COLLISION')
    require(set(drop.iterdir()) == {path for path, _ in files[1:]}, 'E_UNIT_COLLISION')
    for path, data in files:
        require(private_read(path).encode('ascii') == data, 'E_UNIT_COLLISION')


def remove_runtime(owned):
    # Validate the whole set before removing any part. Never remove replacements.
    for path, info in owned:
        same_object(path, info)
    for path, info in reversed(owned):
        if stat.S_ISDIR(info.st_mode):
            path.rmdir()
        else:
            path.unlink()
    if owned:
        call(['/usr/bin/systemctl', 'daemon-reload'])


def run(env):
    global feedback_phase, feedback_mutation, feedback_cleanup, feedback_first, feedback_internal
    feedback_internal = None
    feedback_phase, feedback_mutation, feedback_cleanup, feedback_first = 'initial', 'none', 'not_reached', None
    try:
        return run_body(env)
    except Exception as error:
        if feedback_cleanup == 'not_reached':
            feedback_first = exception_record(error)
        raise


def reserve_unit():
    import fcntl
    lock = Path('/run/arxcovo-disposable-a1a2.lock')
    protected(lock.parent, 'run_parent')
    fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_RDWR, 0o600)
    lock_info = os.fstat(fd)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BaseException:
        # Keep the reservation on uncertainty; never unlock a competing owner.
        os.close(fd)
        raise
    return fd, lock, lock_info


def run_body(env):
    global feedback_phase, feedback_mutation, feedback_cleanup, feedback_first
    standard_profile()
    run_id = gate(env, sys.platform)  # before any mutation or privileged probe
    if a12_selected():
        require(A12_DELIVERY_ENABLED and A12_EXECUTION_ENABLED, 'E_TEST_ARGS')
    selected_a7 = a7_selected()
    if selected_a7:
        require(env['GITHUB_RUN_ATTEMPT'] == '1', 'E_ID')
        a7_admission()  # before root/host probes, reservation or staging
    helper_data = a4_delivery() if a4_selected() else None
    require(os.getuid() == os.geteuid() == 0, 'E_ROOT')
    cg_preflight()  # read-only; no retry or global controller writes
    stage, unit = plan(run_id)
    protected(stage.parent, 'run_parent')
    # Exclusive fixed-name reservation. A collision owns nothing and cleans nothing.
    fd, lock, lock_info = reserve_unit()
    owned = []
    helper_owned = {}
    a7_accounts, a7_artifacts, a7_policy = [], {}, {}
    a12_owned = {}
    unit_created = False
    stage_created = False
    stage_info = None
    installation_pending = False
    try:
        refuse_unit_collision()
        feedback_mutation = 'staged'
        stage.mkdir(mode=0o700)
        stage_created = True
        stage_info = stage.lstat()
        if helper_data is not None:
            install_a4_helper(helper_data, helper_owned)
        source = Path.cwd()
        require(subprocess.check_output(['/usr/bin/git', '-c', 'safe.directory=' + str(source), 'rev-parse', 'HEAD'], env=SAFE_ENV,
                                        timeout=5, stderr=subprocess.DEVNULL).decode().strip() == env['GITHUB_SHA'], 'E_CHECKOUT')
        # Fixed hosted-runner tool locations; no user supplied paths or config.
        copy_tree(source, stage / 'source', ('.git', 'target', 'node_modules', '__pycache__'))
        copy_tree(Path('/home/runner/.rustup/toolchains') / TOOLCHAIN, stage / 'rust')
        (stage / 'cargo').mkdir(mode=0o700)
        copy_tree(Path('/home/runner/.cargo/registry'), stage / 'cargo/registry')
        driver = stage / 'source/tools/unix-exec-validation/driver.py'
        build_env = cargo_env(stage)
        if a12_selected():
            artifact = prepare_a12(env, a12_owned)
        elif selected_a7:
            pins = stage_sudo_nnp(env, a7_artifacts)
            artifact = Path(private_read(stage / 'artifact'))
            prepare_controlled_sudo_nnp(run_id, a7_accounts, a7_policy)
            handoff_sudo_nnp(env, a7_accounts, pins)
        else:
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
            require(artifact.parent == stage / 'source/target/debug/deps' and
                    re.fullmatch(r'arxd-[0-9a-f]+', artifact.name), 'E_ARTIFACT_PATH')
            protected(artifact, 'run_artifact')
            exclusive_file(stage / 'artifact', str(artifact).encode('ascii'))
        installation_pending = True
        install_runtime(stage, run_id, owned)
        installation_pending = False
        feedback_mutation = 'service'
        unit_created = True  # exact files captured; stop even when start fails
        call(['/usr/bin/systemctl', 'daemon-reload'])
        verify_runtime(stage, run_id)
        call(['/usr/bin/systemctl', 'start', '--no-block', unit])
        monitor_a1a2(stage, str(artifact))

    except Exception as error:
        feedback_first = exception_record(error)
        raise
    finally:
        feedback_phase = 'final'
        feedback_cleanup = 'unconfirmed'
        try:
            require(not installation_pending, 'E_QUARANTINE_CGROUP')
            if stage_created or owned:
                if a12_selected():
                    cleanup_a1a2(run_id, unit, stage, stage_created, stage_info, unit_created, owned, a12_owned=a12_owned)
                elif selected_a7:
                    cleanup_a1a2(run_id, unit, stage, stage_created, stage_info,
                                 unit_created, owned, a7_owned=(a7_accounts, a7_artifacts, a7_policy))
                elif helper_data is not None:
                    cleanup_a1a2(run_id, unit, stage, stage_created, stage_info,
                                 unit_created, owned, helper_owned)
                else:
                    cleanup_a1a2(run_id, unit, stage, stage_created, stage_info, unit_created, owned)
            else:
                feedback_cleanup = 'not_reached'
        finally:
            # Keep the reservation on uncertain cleanup: this environment cannot be reused.
            if feedback_cleanup == 'confirmed' or (not stage_created and not owned):
                same_object(lock, lock_info)
                lock.unlink()
            os.close(fd)
            for assets in (helper_owned, a7_artifacts):
                for key in ('fd', 'parent'):
                    if key in assets:
                        os.close(assets[key])


def cleanup_a1a2(run_id, unit, stage, stage_created, stage_info, unit_created, owned, helper_owned=None, a7_owned=None, a12_owned=None):
    global feedback_cleanup
    if unit_created:
        require(unit == 'arxd.service', 'E_UNIT_COLLISION')
        for path, info in owned:
            same_object(path, info)
        verify_runtime(stage, run_id)
        stopped = None
        try:
            stopped = subprocess.run(['/usr/bin/systemctl', 'stop', unit], env=SAFE_ENV, timeout=40,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            require(stopped.returncode == 0, 'E_QUARANTINE_CGROUP')
        finally:
            snapshot_internal(run_id, unit, stopped)
        cg = Path('/sys/fs/cgroup/system.slice') / unit
        require(not os.path.lexists(cg), 'E_QUARANTINE_CGROUP')
    if a12_owned is not None:
        require(not os.path.lexists('/sys/fs/cgroup/system.slice/arxd.service'), 'E_QUARANTINE_CGROUP')
        cleanup_a12(a12_owned, unit_gone=True)
    if a7_owned is not None:
        accounts, artifacts, policy = a7_owned
        a7_uids_released(accounts)
        if policy:
            cleanup_controlled_sudo_nnp(run_id, policy, unit_gone=True)
        cleanup_sudo_nnp(accounts, unit_gone=True)
        cleanup_sudo_nnp_delivery(artifacts, unit_gone=True)
    if helper_owned is not None:
        remove_a4_helper(helper_owned)
    remove_runtime(owned)
    if stage_created:
        same_object(stage, stage_info)
        protected(stage, 'run_cleanup')
        require(shutil.rmtree.avoids_symlink_attacks, 'E_RMTREE')
        shutil.rmtree(stage)
    feedback_cleanup = 'confirmed'


def main():
    global internal_trusted
    internal_trusted = False
    standard_profile()
    require(len(sys.argv) >= 2, 'E_MODE')
    if a12_selected():
        require(A12_DELIVERY_ENABLED and A12_EXECUTION_ENABLED, 'E_TEST_ARGS')
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
    protected(Path(__file__), 'installed')
    internal_trusted = True
    if mode == 'service' and len(sys.argv) == 3:
        service(stage, sys.argv[2])
    elif mode == 'fixture-check' and len(sys.argv) == 3:
        capture_fixture(stage)
        captured = json.loads((stage / 'fixture-identity').read_text())
        _, unit = plan(sys.argv[2])
        cg_scan(Path('/sys/fs/cgroup/system.slice') / unit, captured['uid'], captured['gid'])
    elif mode == 'harness':
        harness(stage, sys.argv[3:])
    else:
        raise RuntimeError('E_MODE')


def feedback_line(env, first, final):
    # Gate validates the association; this envelope is association, not attestation.
    gate(env, sys.platform)
    return encode_record(dict(schema='unix-protection-f0', version=2,
        provenance=dict(source_sha=env['ARX_REVIEWED_SHA'], workflow_sha=env['ARX_WORKFLOW_SHA'],
                        run=env['GITHUB_RUN_ID'], attempt=env['GITHUB_RUN_ATTEMPT']),
        first=first, final=final, mutation_lower_bound=feedback_mutation, cleanup=feedback_cleanup, internal=feedback_internal))


def entry():
    error = None
    try:
        main()
    except Exception as caught:
        error = caught
    # Only the outer run owns public feedback. Internal modes remain suppressed
    # by the existing parent subprocess collectors; no extra public lines.
    if sys.argv[1:] == ['run']:
        try:
            gate(os.environ, sys.platform)
            final = exception_record(error) if error is not None else None
            print(feedback_line(os.environ, feedback_first, final), end='')
        except Exception as record_error:
            try:
                gate(os.environ, sys.platform)
            except Exception as gate_error:
                code = exception_record(gate_error)['code']
            else:
                code = 'E_RECORD'
            print('unix experiment refused/failed: ' + code, file=sys.stderr)
            return 1
    elif error is not None:
        write_internal(error)
        print('unix experiment refused/failed: ' + exception_record(error)['code'], file=sys.stderr)
    return 1 if error is not None else 0


if __name__ == '__main__':
    sys.exit(entry())
