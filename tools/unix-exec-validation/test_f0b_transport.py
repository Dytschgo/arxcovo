"""F0b local mechanism tests. All transport and run effects are mocked."""
import ast
import copy
import hashlib
import io
import itertools
import json
from pathlib import Path
import stat
import sys
import types
import unittest
from contextlib import ExitStack
from unittest.mock import MagicMock, patch
import driver
from test_driver import valid_env


def refusal(mode='service', site=None, phase='initial', triples=None):
    return driver.Refusal(site or driver.INTERNAL_SITES[mode][0], phase,
                          triples or [['link', True, 0]])


def payload(mode='service', **changes):
    value = dict(schema='unix-protection-f0-internal', version=1, mode=mode, refusal=refusal(mode).record)
    value.update(changes)
    return driver.canonical(value)


class FakeOS:
    O_RDONLY=1; O_WRONLY=2; O_CREAT=4; O_EXCL=8; O_NOFOLLOW=16
    O_CLOEXEC=32; O_DIRECTORY=64; O_NONBLOCK=128

    def __init__(self):
        self.files = {}
        self.fds = {}
        self.closed = []
        self.opens = []
        self.nextfd = 10
        self.bad_component = None
        self.component_info = {}
        self.file_info = {}
        self.short = False
        self.read_sizes = []
        self.absent = True
        self.lstat_error = None
        self.events = []

    def open(self, name, flags, mode=0o777, dir_fd=None):
        self.opens.append((name, flags, mode, dir_fd))
        self.events.append('open:' + str(name))
        if name in ('/', 'opt', 'arx-ci-123-1'):
            if name == self.bad_component:
                raise OSError('synthetic component')
            assert flags & self.O_DIRECTORY and flags & self.O_NOFOLLOW and flags & self.O_CLOEXEC
            assert (dir_fd is None) == (name == '/')
            if dir_fd is not None:
                assert self.fds[dir_fd][0] == ('/' if name == 'opt' else 'opt')
        else:
            assert dir_fd in self.fds and self.fds[dir_fd][0] == 'arx-ci-123-1'
            assert name in (*driver.CHANNELS.values(), 'progress-cgroup')
            if flags & self.O_CREAT:
                if name in self.files and flags & self.O_EXCL:
                    raise FileExistsError()
                self.files[name] = b''
            else:
                assert flags & self.O_NONBLOCK, 'FIFO opens must be nonblocking'
                assert flags & self.O_NOFOLLOW, 'reader must refuse links'
                if name not in self.files:
                    raise FileNotFoundError()
        fd = self.nextfd; self.nextfd += 1
        self.fds[fd] = [name, 0]
        return fd

    def close(self, fd):
        assert fd in self.fds, 'descriptor closed twice'
        self.closed.append(fd)
        del self.fds[fd]

    def fstat(self, fd):
        name = self.fds[fd][0]
        directory = name in ('/', 'opt', 'arx-ci-123-1')
        info = dict(st_mode=(stat.S_IFDIR | (0o700 if name.startswith('arx-') else 0o755)) if directory else stat.S_IFREG | 0o600,
                    st_uid=0, st_nlink=1, st_size=len(self.files.get(name, b'')))
        info.update((self.component_info if directory else self.file_info).get(name, {}))
        return types.SimpleNamespace(**info)

    def fchmod(self, fd, mode):
        assert mode == 0o600 and fd in self.fds

    def write(self, fd, data):
        name = self.fds[fd][0]
        data = data[:-1] if self.short else data
        self.files[name] = data
        return len(data)

    def read(self, fd, size):
        self.read_sizes.append(size)
        assert 0 < size <= 1025
        name, pos = self.fds[fd]
        chunk = self.files[name][pos:pos + min(size, 113)]
        self.fds[fd][1] += len(chunk)
        return chunk

    def lstat(self, path):
        assert str(path).replace('\\', '/') == '/sys/fs/cgroup/system.slice/unit'
        self.events.append('snapshot-lstat')
        if self.lstat_error:
            raise self.lstat_error
        if self.absent:
            raise FileNotFoundError()
        return types.SimpleNamespace(st_mode=stat.S_IFDIR)

    def stat(self, name, *, dir_fd, follow_symlinks):
        assert name in ('progress-cgroup', 'fixture-identity') and not follow_symlinks
        assert self.fds[dir_fd][0] == 'arx-ci-123-1'
        if name not in self.files:
            raise FileNotFoundError()
        info = dict(st_mode=stat.S_IFREG | 0o600, st_uid=0)
        info.update(self.file_info.get(name, {}))
        return types.SimpleNamespace(**info)


class Transport(unittest.TestCase):
    def setUp(self):
        self.fs = FakeOS()
        self.enterContext(patch.object(driver, 'os', self.fs))
        self.enterContext(patch.object(driver.sys, 'argv', ['driver.py', 'service', '123-1']))
        for name, value in [('internal_trusted', True), ('feedback_mutation', 'service'), ('feedback_internal', None)]:
            self.enterContext(patch.object(driver, name, value))
        self.enterContext(patch.object(driver.sys, 'platform', 'linux'))

    def tearDown(self):
        self.assertEqual(self.fs.fds, {}, 'all descriptors must close')

    def write(self, error):
        try:
            driver.write_internal(error)
        except Exception:
            self.fail('writer must swallow Exception')

    def snapshot(self, stopped=True):
        driver.snapshot_internal('123-1', 'unit', object() if stopped else None)
        return driver.feedback_internal

    def test_writer_and_first_creator(self):
        for mode in driver.PRECEDENCE:
            driver.sys.argv[1] = mode
            self.write(refusal(mode))
            self.assertIn(driver.CHANNELS[mode], self.fs.files)
            first = self.fs.files[driver.CHANNELS[mode]]
            self.write(refusal(mode, triples=[['dir', False, 0]]))
            self.assertEqual(self.fs.files[driver.CHANNELS[mode]], first)
            self.assertEqual(first, payload(mode))
        for name, flags, mode, parent in self.fs.opens:
            self.assertTrue(flags & self.fs.O_CLOEXEC)
            if name in driver.CHANNELS.values():
                self.assertTrue(flags & self.fs.O_EXCL)
                self.assertEqual(mode, 0o600)

    def test_writer_guards(self):
        class Sub(driver.Refusal): pass
        cases = [(False, refusal()), (True, Sub('service_stage','initial',[['link',True,0]])),
                 (True, refusal(site='installed')), (True, refusal(site='harness_binary')),
                 (True, refusal(phase='final')), (True, RuntimeError('HOSTILE_writer'))]
        for trusted, error in cases:
            self.fs.opens.clear(); driver.internal_trusted = trusted
            self.write(error)
            self.assertEqual(self.fs.opens, [])
        driver.internal_trusted = True
        with patch.object(driver, 'MAX_INTERNAL', 10):
            self.write(refusal())
        self.assertEqual(self.fs.files, {})

    def test_stage_walk_guards(self):
        for name in ('/', 'opt', 'arx-ci-123-1'):
            for info in (dict(st_uid=1), dict(st_mode=stat.S_IFLNK | 0o700),
                         dict(st_mode=stat.S_IFDIR | 0o722)):
                self.fs.component_info = {name: info}
                self.assertIsNone(driver.open_stage('123-1'))
                self.assertEqual(self.fs.fds, {})
        self.fs.component_info = {'arx-ci-123-1':dict(st_mode=stat.S_IFDIR | 0o710)}
        self.assertIsNone(driver.open_stage('123-1'))
        self.fs.component_info = {}
        self.assertIsNone(driver.open_stage('../1'))
        for name in ('/', 'opt', 'arx-ci-123-1'):
            self.fs.bad_component = name
            self.assertIsNone(driver.open_stage('123-1'))

    def test_writer_short_and_invalid_created_file(self):
        self.fs.short = True
        self.write(refusal())
        self.assertEqual(self.snapshot()['service'], 'malformed')
        self.fs.files.clear(); self.fs.file_info['feedback-service'] = dict(st_nlink=2)
        self.write(refusal())
        self.assertEqual(self.fs.files['feedback-service'], b'')

    def test_reader_metadata_guards(self):
        name = 'feedback-service'
        self.fs.files[name] = payload()
        for info in (dict(st_mode=stat.S_IFLNK | 0o600), dict(st_mode=stat.S_IFIFO | 0o600),
                     dict(st_mode=stat.S_IFDIR | 0o600), dict(st_uid=1),
                     dict(st_mode=stat.S_IFREG | 0o640), dict(st_nlink=2),
                     dict(st_size=0), dict(st_size=1025), dict(st_size=len(payload())+1)):
            with self.subTest(info=info):
                self.fs.file_info[name] = info
                self.assertEqual(self.snapshot()['service'], 'malformed')
        self.fs.file_info.clear()
        self.assertEqual(self.snapshot()['service'], 'captured')

    def test_reader_bytes_guards(self):
        good = payload()
        bads = [good[:-1], good+b'\n', b'\xff'+good, b' '+good,
                good.replace(b'"version":1', b'"version":1,"version":1'),
                good.replace(b'"schema":', b'"schema" :'),
                payload(schema='HOSTILE_schema'), payload(version=True), payload(mode='harness')]
        value = json.loads(good); value = dict(reversed(list(value.items())))
        bads.append(driver.canonical(value))
        for key, value in [('site','installed'), ('phase','final'), ('ordinal',1), ('owner_zero',False)]:
            r = refusal().record; r[key] = value
            # owner_zero=False still agrees with link; use a passing kind to isolate disagreement.
            if key == 'owner_zero': r.update(kind='dir', owner_zero=True)
            bads.append(payload(refusal=r))
        for index,bad in enumerate(bads):
            with self.subTest(case=index):
                self.fs.files['feedback-service'] = bad
                self.assertEqual(self.snapshot()['service'], 'malformed')

    def test_completion_condition_and_progress(self):
        self.fs.files['feedback-service'] = payload()
        for stopped, absent, error in [(False, True, None), (True,False,None), (True,True,PermissionError())]:
            self.fs.absent=absent; self.fs.lstat_error=error; self.fs.opens.clear()
            r = self.snapshot(stopped)
            self.assertEqual([r[m] for m in driver.PRECEDENCE], ['unknown']*3)
            self.assertFalse(any(n in driver.CHANNELS.values() for n,*_ in self.fs.opens))
        self.fs.lstat_error = None
        for name, bound in [('progress-cgroup','cgroup'),('fixture-identity','account')]:
            self.fs.files[name] = b'HOSTILE_never_open'
            for info in [dict(st_mode=stat.S_IFLNK | 0o600), dict(st_mode=stat.S_IFDIR | 0o700),dict(st_uid=1)]:
                self.fs.file_info[name]=info
                driver.feedback_mutation='service'; self.snapshot(False)
                self.assertEqual(driver.feedback_mutation,'service')
            self.fs.file_info.clear()
            self.snapshot(False)
            self.assertEqual(driver.feedback_mutation,bound)
            self.fs.files.pop(name)
        self.assertFalse(any(n == 'fixture-identity' for n,*_ in self.fs.opens))

    def test_precedence(self):
        for bits in itertools.product((False,True), repeat=3):
            self.fs.files = {driver.CHANNELS[m]:payload(m) for m,on in zip(driver.PRECEDENCE,bits) if on}
            r = self.snapshot()
            selected = next((m for m in ('service','fixture-check','harness') if self.fs.files.get(driver.CHANNELS[m])),None)
            self.assertEqual(driver.feedback_mutation,'service')
            self.assertEqual(r['carried'], selected)
            self.assertEqual(r['refusal'], refusal(selected).record if selected else None)
            driver.validate_internal_summary(r)

    def test_marker_failure_swallowed(self):
        self.fs.bad_component='opt'
        driver.mark_cgroup('123-1')
        self.assertEqual(self.fs.files,{})

    def test_service_marker_does_not_change_effect_order(self):
        stage=MagicMock();cg=MagicMock();cg.__truediv__.return_value=cg
        cg.read_text.return_value='memory pids'
        cg.mkdir.side_effect=RuntimeError('E_TEST')
        proc=MagicMock();proc.read_text.return_value='0::/system.slice/arx-ci-123-1.service'
        with patch.object(driver,'protected'),patch.object(driver,'Path',side_effect=lambda value: proc if value=='/proc/self/cgroup' else cg), \
             patch.object(driver,'mark_cgroup',wraps=driver.mark_cgroup) as marker:
            self.fs.bad_component='opt'
            with self.assertRaisesRegex(RuntimeError,'E_TEST'):
                driver.service(stage,'123-1')
            marker.assert_called_once_with('123-1')
            cg.mkdir.assert_called_once_with(mode=0o700)



class Records(unittest.TestCase):
    def valid(self):
        return dict(schema='unix-protection-f0',version=2,
                    provenance=dict(source_sha='a'*40,workflow_sha='a'*40,run='1',attempt='1'),
                    first=None,final=None,mutation_lower_bound='service',cleanup='confirmed',
                    internal=dict(service='absent',harness='absent',**{'fixture-check':'absent'},carried=None,refusal=None))

    def test_existing_fences_unchanged(self):
        source=Path(__file__).with_name('driver.py').read_text()
        tree=ast.parse(source)
        run=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='run_body')
        calls=[n for n in ast.walk(run) if isinstance(n,ast.Call)]
        stop=next(n for n in calls if isinstance(n.func,ast.Attribute) and n.func.attr=='run' and
                  isinstance(n.args[0],ast.List) and isinstance(n.args[0].elts[0],ast.Constant) and
                  n.args[0].elts[0].value=='/usr/bin/systemctl')
        expected=ast.parse("subprocess.run(['/usr/bin/systemctl', 'stop', unit], env=SAFE_ENV, timeout=20, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)").body[0].value
        self.assertEqual(ast.dump(stop),ast.dump(expected))
        for fence in ("require(not cg.exists(), 'E_QUARANTINE_CGROUP')",'fixture_cleanup(stage)',
                      "protected(stage, 'run_cleanup')",'shutil.rmtree(stage)'):
            self.assertEqual(ast.get_source_segment(source,run).count(fence),1)

    def test_cross_fields(self):
        for change in [dict(internal=None),dict(version=1),dict(mutation_lower_bound='staged')]:
            r=self.valid();r.update(change)
            with self.assertRaises(RuntimeError): driver.encode_record(r)
        for status in ('unknown','malformed','captured'):
            r=self.valid()
            if status=='unknown':
                r['internal'].update(dict.fromkeys(driver.PRECEDENCE,status))
            else:
                r['internal']['service']=status
            if status=='captured':r['internal'].update(carried='service',refusal=refusal().record)
            with self.assertRaises(RuntimeError):driver.encode_record(r)
        r=self.valid();r.update(first={'code':'E_BUILD'},final={'code':'E_BUILD'})
        for change in [dict(service='unknown'),dict(carried='harness'),dict(refusal=refusal().record),dict(service='other')]:
            bad=copy.deepcopy(r);bad['internal'].update(change)
            with self.assertRaises(RuntimeError):driver.encode_record(bad)

    def test_measured_maxima(self):
        outer=[];inner=[]
        # Longest passing predecessor spelling, longest false owner refusal.
        triples=[['other',True,0]]*31+[['other',False,18]]
        for mode in driver.PRECEDENCE:
            site=max(driver.INTERNAL_SITES[mode],key=len)
            carried=refusal(mode,site=site,triples=triples).record
            inner.append(len(payload(mode,refusal=carried)))
            r=self.valid(); r['provenance'].update(run='9'*20,attempt='9'*20)
            r.update(first=refusal(site='cleanup_capture',triples=triples).record,
                     final=refusal(site='cleanup_capture',phase='final',triples=triples).record,
                     cleanup='unconfirmed',mutation_lower_bound='account')
            r['internal'].update(dict.fromkeys(driver.PRECEDENCE,'malformed'))
            r['internal'].update({mode:'captured','carried':mode,'refusal':carried})
            outer.append(len(driver.encode_record(r).encode('ascii')))
        self.assertEqual(max(inner),757)
        self.assertEqual(max(outer),2445)
        self.assertLessEqual(max(inner),1024)
        self.assertLessEqual(max(outer),4096)
        print('MEASURED internal='+str(max(inner))+' outer='+str(max(outer)))
        with patch.object(driver,'MAX_LINE',10),self.assertRaisesRegex(RuntimeError,'E_RECORD_BOUND'):
            driver.encode_record(self.valid())


class EntryRun(unittest.TestCase):
    def scenario(self, *, body_failure=False, stop_error=None, present=False, rc=0,
                 anomaly=None, cleanup_error=None, reader_error=False):
        fs=FakeOS();fs.absent=not present
        fs.environ=valid_env();fs.getuid=fs.geteuid=lambda:0
        stage=MagicMock();stage.name='arx-ci-123-1'
        stage.__truediv__.return_value=stage
        stage.stat.return_value.st_size=10
        stage.read_text.return_value=json.dumps(dict(reason='compiler-artifact',executable='/artifact',profile={'test':True}))
        stage.is_relative_to.return_value=True
        stage.exists.side_effect=[not body_failure,present] if not body_failure else [present]
        calls=fs.events
        def process(argv, **kw):
            if argv[0]=='/usr/bin/systemctl':
                calls.append('stop')
                if stop_error:raise stop_error
                return types.SimpleNamespace(returncode=rc)
            return types.SimpleNamespace(returncode=1 if body_failure and argv[0]=='/usr/bin/systemd-run' else 0)
        def clean(_):
            calls.append('cleanup')
            if cleanup_error:raise cleanup_error
        def protect(_,site):calls.append(site)
        def remove(_):calls.append('remove')
        from pathlib import PurePosixPath
        class CgroupPath(PurePosixPath):
            def exists(self):
                calls.append('quarantine')
                return present
        def path(value):
            return CgroupPath(value) if str(value).startswith('/sys/fs/cgroup') else stage
        if anomaly=='malformed':fs.files['feedback-service']=b'HOSTILE_incomplete'
        if anomaly=='captured':fs.files['feedback-service']=payload()
        if anomaly=='unknown':fs.bad_component='opt'
        with ExitStack() as stack:
            for name,value in [('phase','initial'),('mutation','none'),('cleanup','not_reached'),('first',None),('internal',None)]:
                stack.enter_context(patch.object(driver,'feedback_'+name,value))
            stack.enter_context(patch.object(driver,'os',fs))
            stack.enter_context(patch.object(driver.sys,'platform','linux'))
            stack.enter_context(patch.object(driver.sys,'argv',['driver.py','run']))
            stack.enter_context(patch.object(driver,'plan',return_value=(stage,'unit')))
            paths=stack.enter_context(patch.object(driver,'Path',side_effect=path));paths.cwd.return_value=stage
            for name in ('cg_preflight','copy_tree'):
                stack.enter_context(patch.object(driver,name))
            stack.enter_context(patch.object(driver,'protected',side_effect=protect))
            stack.enter_context(patch.object(driver,'fixture_cleanup',side_effect=clean))
            rm=stack.enter_context(patch.object(driver.shutil,'rmtree',side_effect=remove));rm.avoids_symlink_attacks=True
            stack.enter_context(patch.object(driver.subprocess,'check_output',side_effect=[b'not-found',valid_env()['GITHUB_SHA'].encode()]))
            stack.enter_context(patch.object(driver.subprocess,'run',side_effect=process))
            if reader_error:stack.enter_context(patch.object(driver,'read_internal',side_effect=ValueError('HOSTILE_reader')))
            out=stack.enter_context(patch.object(driver.sys,'stdout',io.StringIO()))
            err=stack.enter_context(patch.object(driver.sys,'stderr',io.StringIO()))
            result=driver.entry()
            state=(driver.feedback_first,driver.feedback_cleanup,driver.feedback_internal)
        self.assertEqual(fs.fds,{})
        self.assertNotIn('HOSTILE',out.getvalue()+err.getvalue())
        return result,out.getvalue(),err.getvalue(),state,calls

    def test_real_run_stop_order(self):
        for rc in (0,5):
            result,line,err,state,calls=self.scenario(rc=rc)
            self.assertEqual((result,err),(0,''))
            r=json.loads(line);driver.validate_record(r)
            self.assertEqual(r['internal']['service'],'absent')
            self.assertLess(calls.index('stop'),calls.index('snapshot-lstat'))
            self.assertLess(calls.index('snapshot-lstat'),calls.index('cleanup'))
            self.assertLess(calls.index('cleanup'),calls.index('run_cleanup'))
            self.assertLess(calls.index('run_cleanup'),calls.index('remove'))

    def test_real_run_existing_failures_preserved(self):
        for kind in ('body','stop','cleanup','quarantine'):
            kw=dict(body_failure=True)
            expected='E_SERVICE_OR_TEST'
            if kind=='stop':kw['stop_error']=driver.subprocess.TimeoutExpired('HOSTILE_stop',20);expected='E_COMMAND_TIMEOUT'
            if kind=='cleanup':kw['cleanup_error']=RuntimeError('E_UID_LIVE');expected='E_UID_LIVE'
            if kind=='quarantine':kw['present']=True;expected='E_QUARANTINE_CGROUP'
            for reader_error in (False,True):
                result,line,err,state,calls=self.scenario(reader_error=reader_error,**kw)
                self.assertEqual((result,err),(1,''))
                r=json.loads(line);driver.validate_record(r)
                self.assertEqual(r['first'],{'code':'E_SERVICE_OR_TEST'})
                self.assertEqual(r['final'],{'code':expected})
                self.assertEqual(r['cleanup'],'confirmed' if kind=='body' else 'unconfirmed')
                if kind in ('stop','quarantine'):
                    self.assertNotIn('cleanup',calls);self.assertNotIn('remove',calls)
                    self.assertFalse(any(v.startswith('open:feedback-') for v in calls))

    def test_real_success_anomalies_only_fallback(self):
        for anomaly in ('unknown','malformed','captured','reader-exception'):
            result,line,err,state,calls=self.scenario(anomaly=anomaly,reader_error=anomaly=='reader-exception')
            self.assertEqual((result,line,err),(1,'','unix experiment refused/failed: E_RECORD\n'))
            self.assertIsNone(state[0]);self.assertEqual(state[1],'confirmed')
            self.assertIn('remove',calls)

    def test_internal_entry_installed_gate_and_escaping_error(self):
        from pathlib import PurePosixPath
        for mode in driver.PRECEDENCE:
            for site in ('installed',*driver.INTERNAL_SITES[mode]):
                error=refusal(mode,site=site)
                with ExitStack() as stack:
                    stack.enter_context(patch.object(driver.sys,'platform','linux'))
                    stack.enter_context(patch.object(driver.sys,'argv',['driver.py',mode,'123-1']))
                    stack.enter_context(patch.object(driver,'Path',PurePosixPath))
                    stack.enter_context(patch.object(driver,'__file__','/opt/arx-ci-123-1/source/tools/unix-exec-validation/driver.py'))
                    stack.enter_context(patch.object(driver.os,'getuid',return_value=0,create=True))
                    stack.enter_context(patch.object(driver.os,'geteuid',return_value=0,create=True))
                    stack.enter_context(patch.object(driver,'internal_trusted',False))
                    def protect(*_):
                        if site=='installed':raise error
                    stack.enter_context(patch.object(driver,'protected',side_effect=protect))
                    for name in ('service','harness','capture_fixture'):
                        stack.enter_context(patch.object(driver,name,side_effect=error))
                    original=driver.create_channel
                    writer=stack.enter_context(patch.object(driver,'create_channel',side_effect=OSError('HOSTILE_writer')))
                    out=stack.enter_context(patch.object(driver.sys,'stdout',io.StringIO()))
                    err=stack.enter_context(patch.object(driver.sys,'stderr',io.StringIO()))
                    try:
                        result = driver.entry()
                    except Exception:
                        self.fail('internal writer changed escaping failure')
                    self.assertEqual(result,1)
                    self.assertEqual(err.getvalue(),'unix experiment refused/failed: E_PROTECTION\n')
                    self.assertEqual(out.getvalue(),'')
                    self.assertEqual(writer.call_count,int(site!='installed'))


SOURCE = Path(__file__).with_name('driver.py').read_text()
# Every edit is compiled in memory; absence of an assertion failure earns no kill.
MUTANTS = {
 'writer_ignores_trust':("if not internal_trusted or mode not in CHANNELS", "if False or mode not in CHANNELS"),
 'writer_ignores_installed_flag':("    internal_trusted = True", "    internal_trusted = False"),
 'writer_isinstance_refusal':("or type(error) is not Refusal:", "or not isinstance(error, Refusal):"),
 'writer_site_unchecked':("        internal_refusal(mode, error.record)", "        pass # omitted early validation"),
 'writer_drops_excl':("os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC", "os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC"),
 'writer_drops_nofollow':("os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC", "os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC"),
 'writer_walk_skips_ancestor':("('/', 'opt', stage.name)", "('/', stage.name)"),
 'ancestor_nofollow_dropped':("os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC", "os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC"),
 'writer_ignores_stage_mode':("require(info.st_mode & 0o077 == 0, 'E_RECORD')", "require(True, 'E_RECORD')"),
 'writer_truncates':("require(len(data) <= MAX_INTERNAL, 'E_RECORD')", "data = data[:MAX_INTERNAL]"),
 'writer_raises':("    except Exception:\n        pass\n\n\ndef mark_cgroup", "    except Exception:\n        raise\n\n\ndef mark_cgroup"),
 'reader_follows_link':("os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC", "os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC"),
 'reader_drops_nonblock':("os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC", "os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC"),
 'reader_drops_typecheck':("require(stat.S_ISREG(info.st_mode), 'E_RECORD')", "require(True, 'E_RECORD')"),
 'reader_ignores_owner':("require(info.st_uid == 0, 'E_RECORD')", "require(True, 'E_RECORD')"),
 'reader_ignores_mode':("require(stat.S_IMODE(info.st_mode) == 0o600, 'E_RECORD')", "require(True, 'E_RECORD')"),
 'reader_ignores_nlink':("require(info.st_nlink == 1, 'E_RECORD')", "require(True, 'E_RECORD')"),
 'reader_unbounded':("os.read(fd, MAX_INTERNAL + 1 - len(data))", "os.read(fd, 1000000)"),
 'reader_ascii_dropped':("data.decode('ascii')", "data.decode('utf-8')"),
 'reader_noncanonical_accepted':("require(canonical(value) == data, 'E_RECORD')", "require(True, 'E_RECORD')"),
 'internal_site_unchecked':("refusal['site'] in INTERNAL_SITES[mode] and", "True and"),
 'reader_accepts_final_phase':("refusal['phase'] == 'initial'", "True"),
 'reader_mode_mismatch':("value['mode'] == mode", "True"),
 'reader_reads_unquiesced':("if snapshot_available:", "if True:"),
 'reader_requires_rc_zero':("snapshot_available = stopped is not None and cgroup_absent", "snapshot_available = stopped is not None and stopped.returncode == 0 and cgroup_absent"),
 'reader_replaces_cg_fence':("require(not cg.exists(), 'E_QUARANTINE_CGROUP')", "require(True, 'E_QUARANTINE_CGROUP')"),
 'reader_after_cleanup':("                snapshot_internal(run_id, unit, stopped)", "                pass # delayed snapshot"),
 'reader_raises':("    except Exception:\n        pass\n    finally:\n        if stage is not None:", "    except Exception:\n        raise\n    finally:\n        if stage is not None:"),
 'fd_close_dropped':("            if stage is not None:\n                os.close(stage)", "            if stage is not None:\n                pass # leaked stage"),
 'carried_precedence_changed':("PRECEDENCE = ('service', 'fixture-check', 'harness')", "PRECEDENCE = ('harness', 'fixture-check', 'service')"),
 'success_accepts_unknown':("all(internal[m] == 'absent' for m in PRECEDENCE)", "all(internal[m] in ('absent','unknown') for m in PRECEDENCE)"),
 'success_accepts_malformed':("all(internal[m] == 'absent' for m in PRECEDENCE)", "all(internal[m] in ('absent','malformed') for m in PRECEDENCE)"),
 'success_accepts_captured':("all(internal[m] == 'absent' for m in PRECEDENCE)", "all(internal[m] in ('absent','captured') for m in PRECEDENCE)"),
 'progress_counts_link':("if stat.S_ISREG(info.st_mode) and info.st_uid == 0:", "if (stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode)) and info.st_uid == 0:"),
 'progress_from_status':("                value[mode] = status", "                value[mode] = status\n                if status == 'captured': feedback_mutation = 'account'"),
 'v1_accepted':("record['version'] == 2", "record['version'] in (1,2)"),
 'outer_overflow_truncates':("require(len(line.encode('ascii')) <= MAX_LINE, 'E_RECORD_BOUND')", "line = line[:MAX_LINE]"),
}
EQUIVALENT = {
 'writer_site_unchecked':'shared validate_internal still enforces the same site and phase before create',
 'writer_drops_nofollow':'O_EXCL independently rejects every existing entry, including a symlink',
 'reader_ascii_dropped':'canonical ensure_ascii byte equality still rejects non-ASCII UTF-8',
 'reader_mode_mismatch':'canonical rebuild pins the expected mode, independently rejecting mismatch',
}


def suite():
    return unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromTestCase(c) for c in (Transport,Records,EntryRun))


def mutations():
    baseline=unittest.TestResult();suite().run(baseline)
    if not baseline.wasSuccessful():raise AssertionError('baseline must be green')
    original=driver
    digest=hashlib.sha256(Path(__file__).with_name('driver.py').read_bytes()).hexdigest()
    for name,(before,after) in MUTANTS.items():
        if SOURCE.count(before)!=1:raise AssertionError('anchor '+name+' count='+str(SOURCE.count(before)))
        module=types.ModuleType('f0b_mutant');module.__file__='driver.py'
        changed = SOURCE.replace(before,after)
        if name == 'reader_after_cleanup':
            changed = changed.replace('        fixture_cleanup(stage)\n', '        fixture_cleanup(stage)\n        if unit_created: snapshot_internal(run_id, unit, stopped)\n')
        exec(compile(changed,'<in-memory-f0b-mutant>','exec'),module.__dict__)
        globals()['driver']=module
        try:
            result=unittest.TestResult();suite().run(result)
            if result.errors:raise AssertionError('errored run earns no credit: '+name+' '+str(result.errors))
            if name in EQUIVALENT:
                # An equivalent edit cannot be credited even if a flag assertion detects it.
                print('EQUIVALENT '+name+' '+EQUIVALENT[name])
            elif not result.failures:
                raise AssertionError('SURVIVED '+name)
            else:
                print('KILLED '+name+' assertion='+','.join(sorted(set(t.id().split(' (')[0] for t,_ in result.failures))))
        finally:globals()['driver']=original
    if hashlib.sha256(Path(__file__).with_name('driver.py').read_bytes()).hexdigest()!=digest:
        raise AssertionError('source changed')
    print('source unchanged sha256='+digest)


if __name__=='__main__':
    if sys.argv[1:]==['--mutants']:mutations()
    else:unittest.main()
