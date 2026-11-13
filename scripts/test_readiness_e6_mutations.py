"""Compile and kill E6 regressions; restore every byte even on failure.
Run from the isolated repository root. Only fixed status labels are printed.
"""
from pathlib import Path
import subprocess
B = Path("crates/arxd/src/service/unix_bridge.rs")
R = B.parent / "unix_bridge/readiness.rs"
original = {p: p.read_bytes() for p in (B, R)}
b = original[B].decode().replace("\r\n", "\n")
r = original[R].decode().replace("\r\n", "\n")
mutants = [
    ("gate_removed", B, "if let Err(token) = self.readiness.check() {", "if let Err(token) = Ok::<(), Unavailable>(()) {", "t18_stale_preflight_gate_limit_one"),
    ("gate_residue_ignored", B, "if runner.cleanup_residue() {", "if false {", "t13c_existing_residue_gate"),
    ("post_residue_ignored", B, "if runner.cleanup_residue()\n", "if false\n", "t11_t12_t13_t13b_reports_and_latches"),
    ("containment_latch_removed", B, "self.readiness.latch(Cause::Containment);", "let _ = Cause::Containment;", "t11_t12_t13_t13b_reports_and_latches"),
    ("cancelled_latched", B, "exec::NotStarted::Containment | exec::NotStarted::Resource", "exec::NotStarted::Containment | exec::NotStarted::Resource | exec::NotStarted::Cancelled", "t11_t12_t13_t13b_reports_and_latches"),
    ("rights_tools_withheld", B, "self.readiness.latch(Cause::Rights);", "self.readiness.latch(Cause::Rights); return Report::Discovered { evidence, tools: Vec::new() };", "t14_t17_rights_tools_and_private_bytes"),
    ("rights_untrusted_ignored", B, "if classify_rights(Rights::Standard, Hardening::Standard, fact).is_err()", "if fact.sudo != crate::exec::hierarchy::parse::Sudo::Untrusted && classify_rights(Rights::Standard, Hardening::Standard, fact).is_err()", "t14_t17_rights_tools_and_private_bytes"),
    ("janitor_wrong_token", R, "4 => Err(Unavailable::Quarantined)", "4 => Err(Unavailable::MechanismUnavailable)", "t15_exhaustive_setup_failure_dispatch"),
    ("nonmonotone_store", R, ".compare_exchange(1, cause as u8, Ordering::AcqRel, Ordering::Acquire)", ".store(cause as u8, Ordering::Release)", "t16_monotone_first_cause"),
    ("rights_released", B, "tools.push(ToolVersion {", "tools.push(ToolVersion { name: \"no_new_privs sudo\".into(), found: true, version: None }); tools.push(ToolVersion {", "t14_t17_rights_tools_and_private_bytes"),
    ("domain_rebuilt", B, "self.execution.clone()", "super::executor::Execution::unavailable()", "t20_shared_domain_handles_never_replace_runner"),
]
try:
    for name, path, before, after, test in mutants:
        source = b if path == B else r
        assert before in source, "mutation anchor missing"
        # Remove both containment latch sites for the latch-removed mutant.
        changed = source.replace(before, after, -1 if name == "containment_latch_removed" else 1)
        path.write_text(changed)
        cmd = ["cargo", "test", "-p", "arxd", "--lib", "--locked", test]
        completed = subprocess.run(cmd, capture_output=True, timeout=120)
        output = completed.stdout + completed.stderr
        compiled = b"error[E" not in output and b"could not compile" not in output
        killed = completed.returncode != 0 and (test + " ... FAILED").encode() in output
        print(name + (": COMPILED KILLED" if compiled and killed else ": NOT PROVEN"), flush=True)
        assert compiled and killed, "compiled mutation kill required"
        path.write_bytes(original[path])
finally:
    for path, content in original.items():
        path.write_bytes(content)
    assert all(p.read_bytes() == content for p, content in original.items()), "exact restoration"
    print("source: EXACT RESTORE", flush=True)
