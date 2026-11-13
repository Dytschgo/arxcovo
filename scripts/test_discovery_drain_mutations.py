"""SP1/SP2 compiled assertion controls; fixed output and exact byte restore."""
from pathlib import Path
import hashlib
import subprocess

A = Path('crates/arxd/src/service/application.rs')
E = Path('crates/arxd/src/service/executor.rs')
TEST = 'sp1_sp2_actual_drain_precedes_sequence_and_empties_workers'
original = {p: p.read_bytes() for p in (A, E)}


def replace(source, before, after):
    assert source.count(before) == 1, 'unique mutation anchor'
    return source.replace(before, after, 1)


def move_real_drain(source):
    # Keep every caller label in its original position. Move only the real call.
    call = '''            let drained = e.drain().await.map_err(|_| ExitCode::Store);
            if result.is_ok() {
                result = drained;
            }
'''
    source = replace(source, call, '')
    anchor = '        #[cfg(test)]\n        self.discovery.take();'
    return replace(source, anchor, '''        if let Some(e) = self.activity.as_ref().and_then(|a| a.dedicated.as_ref()) {
''' + call + '        }\n' + anchor).replace(
        '            self.hooks.discovery_barrier("drain_execution");',
        '            self.hooks.discovery_barrier("drain_execution");\n            let _ = e;', 1)


mutants = [
    ('exec_drain_after_sequence_hook_kept', A, move_real_drain,
     ('actual Execution::drain before sequence join',)),
    ('drain_leaves_tracked_worker', E,
     lambda s: replace(s, 'while let Some(result) = workers.join_next().await {',
                      'while let Some(result) = None::<Result<(), tokio::task::JoinError>> {'),
     ('actual drained JoinSet empty', 'actual drain must wait for held worker')),
]
try:
    for name, path, mutate, assertion in mutants:
        path.write_bytes(mutate(original[path].decode().replace('\r\n', '\n')).encode())
        result = subprocess.run(
            ['cargo', 'test', '-p', 'arxd', '--lib', '--locked', TEST],
            capture_output=True, timeout=120)
        output = result.stdout + result.stderr
        compiled = b'error[E' not in output and b'could not compile' not in output
        killed = result.returncode != 0 and (TEST + ' ... FAILED').encode() in output
        assertion_hit = any(label.encode() in output for label in assertion)
        print(name + (': COMPILED ASSERTION KILLED' if compiled and killed and assertion_hit
                      else ': NOT PROVEN'), flush=True)
        assert compiled and killed and assertion_hit, 'named compiled assertion kill required'
        path.write_bytes(original[path])
finally:
    for path, content in original.items():
        path.write_bytes(content)
        assert hashlib.sha256(path.read_bytes()).digest() == hashlib.sha256(content).digest()
    print('source: EXACT SHA-256 RESTORE', flush=True)
