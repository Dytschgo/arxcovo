"""Bounded compiled PA regression controls. No raw child output; exact restore."""
from pathlib import Path
import hashlib
import subprocess

A = Path("crates/arxd/src/service/application.rs")
M = Path("crates/arxd/src/mcp.rs")
B = Path("crates/arxd/src/service/unix_bridge.rs")
original = {p: p.read_bytes() for p in (A, M, B)}


def replace(source, before, after):
    assert source.count(before) == 1, "unique mutation anchor"
    return source.replace(before, after, 1)


def mcp_completion(s):
    start = s.index('        let observation = if self.activity.dedicated')
    end = s.index('        let id = if', start)
    allocation = s[start:end]
    s = s[:start] + s[end:]
    return replace(s, '            if let Some(observation) = observation {',
                   allocation + '            if let Some(observation) = observation {')


def app_completion(s):
    s = replace(s, 'let observation = a.discovery_observation().map_err(|_| ExitCode::Store)?;',
                'let observation = (0, 0);')
    return replace(s, 'activity.cache_discovery(observation, &response);',
                   'let _ = observation; let observation = activity.discovery_observation().map_err(|_| ExitCode::Store)?; activity.cache_discovery(observation, &response);')


def sequence_first(s):
    start = s.index('        if let Some(e) = self.activity.as_ref()', s.index('    async fn drain(&mut self)'))
    mid = s.index('        #[cfg(test)]\n        if let Some(task) = self.discovery.as_mut()', start)
    end = s.index('        #[cfg(test)]\n        self.discovery.take();', mid)
    return s[:start] + s[mid:end] + s[start:mid] + s[end:]


mutants = [
    ('mcp_ticket_at_completion', M, mcp_completion, 'pa1_actual_callers_reverse_finish'),
    ('app_ticket_at_completion', A, app_completion, 'pa1_actual_callers_reverse_finish'),
    ('ready_despite_stop', B, lambda s: replace(s, 'SetupOutcome::Ready(_) if !stop.is_cancelled() =>', 'SetupOutcome::Ready(_) if true =>'), 'w2_w3_shutdown_boundaries'),
    ('drain_sequence_before_execution', A, sequence_first, 'w1_w4_t20b_order_single_domain'),
    ('bridge_parent_not_location', B, lambda s: replace(s, '            location.runs(),', '            "/unrelated/runs".into(),'), 'w1_w4_t20b_order_single_domain'),
    ('panic_claims_no_creation', B, lambda s: replace(s, 'Err(_payload) => Report::Failed {\n                creation: Creation::Unknown,', 'Err(_payload) => Report::Failed {\n                creation: Creation::ProvenNone,'), 't10_reexec_diagnostics'),
    ('residue_gate_omitted', B, lambda s: replace(s, '        if runner.cleanup_residue() {', '        if false {'), 't10_reexec_diagnostics'),
]
try:
    for name, path, mutate, test in mutants:
        path.write_bytes(mutate(original[path].decode().replace('\r\n', '\n')).encode())
        result = subprocess.run(['cargo', 'test', '-p', 'arxd', '--lib', '--locked', test], capture_output=True, timeout=120)
        output = result.stdout + result.stderr
        compiled = b'error[E' not in output and b'could not compile' not in output
        killed = result.returncode != 0 and (test + ' ... FAILED').encode() in output
        print(name + (': COMPILED KILLED' if compiled and killed else ': NOT PROVEN'), flush=True)
        assert compiled and killed, 'compiled named assertion kill required'
        path.write_bytes(original[path])
finally:
    for path, content in original.items():
        path.write_bytes(content)
        assert hashlib.sha256(path.read_bytes()).digest() == hashlib.sha256(content).digest(), 'exact SHA-256 restore'
    print('source: EXACT RESTORE', flush=True)
