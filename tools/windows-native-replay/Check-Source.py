"""Gate 2 structural inventory. Reads fixed source only; never discovers/runs tests."""
import json
import re
from pathlib import Path

root = Path(__file__).absolute().parents[2]
s = (root / "crates/arxd/src/development/windows.rs").read_text(encoding="utf-8")
native = s.split("mod native_replay {", 1)[1].split("pub(crate) mod tests", 1)[0]
native, pure = native.split("    mod pure {", 1)
# Strip comments so explanatory mentions of registration do not count as code.
code = re.sub(r"//[^\n]*", "", native)
assert not re.search(r"#\s*\[\s*(test|ignore)", code)
assert len(re.findall(r"\bfn bounded_matrix\(", code)) == 1
assert len(re.findall(r"\bfn kernel_peek_capture\(", code)) == 1
assert len(re.findall(r"\bbounded_matrix\(", s)) == 1
assert len(re.findall(r"\bkernel_peek_capture\(", s)) == 1
# Pure tests may call only in-memory fixture bookkeeping and assertion helpers.
for forbidden in ("run_inner(", "run_ok(", "one_row(", "prepared_execution(", "drain(",
                  "prefill(", "retain_if_active(", "kernel_peek_capture(", "bounded_matrix(",
                  "unsafe", "Instant::", "std::process", "std::fs"):
    assert forbidden not in pure
assert len(re.findall(r"#\[test\]", pure)) == 5
assert code.count('row!(\n') + code.count('row!("') == 28
selectors = json.loads((root / "tools/windows-native-replay/selectors.json").read_text())
assert selectors["registration_enabled"] is False
assert sum(f["cases"] for f in selectors["filters"]) == 35
assert sum(f["children"] for f in selectors["filters"]) == 20
assert sum(f["resumes"] for f in selectors["filters"]) == 16
helper = (root / "tools/windows-native-replay/child/src/main.rs").read_text()
assert re.sub(r"//[^\n]*", "", helper).strip() == "fn main() {}"
print("gate2 source inventory: 28 oracle rows; 5 pure tests; 0 native registrations/callers; selectors disabled")

# selectors.json is the canonical global schedule. C# is a checked projection;
# Rust supplies the same indices without deriving the independent label oracle.
rows = selectors["row_map"]
assert len(rows) == 35
for i, row in enumerate(rows):
    assert row == dict(index=i, kind="kernel" if i == 0 else "failure" if i <= 28 else "peek109" if i <= 32 else "control",
                       consumed=int(1 <= i <= 32), creates=int(i >= 15), resumes=int(i >= 19))
controller = (root / "tools/windows-native-replay/Controller.cs").read_text()
projection = re.findall(r"new Row\((\d+),(\d+),(\d+)\), // (\d+)", controller)
assert projection == [(str(r["consumed"]), str(r["creates"]), str(r["resumes"]), str(r["index"])) for r in rows]
assert "local + 1" in code and "matrix_index(i as u32)" in code
assert "matrix_index(28 + i as u32)" in code
assert "matrix_index(32)" in code and "matrix_index(33)" in code
assert "Func<" not in controller
assert "return Poll() && state.Success" in controller
assert "readonly NativeJob job" in controller and "new FrameDecoder(channel)" in controller
assert "v[1]!=(uint)(frames/2)" in controller and "frames!=expected" in controller
held = (root / "tools/windows-native-replay/HeldFiles.cs").read_text()
assert "links==1" in held and "NtCreateFile" in held and "parent=parent.DangerousGetHandle()" in held
assert "GetFileInformationByHandleEx(h,18" in held and "Info after=Inspect(stream,limit)" in held
assert "CopyPinned" not in held
job = (root / "tools/windows-native-replay/NativeJob.cs").read_text()
assert "0x80404" in job and "flags=0x2000" in job
assert job.index("AssignProcessToJobObject(job,process)") < job.index("ResumeThread(thread)!=1")
assert "TerminateJobObject(job,1)" in job and "WaitForSingleObject(process,0)" in job
assert "emit_frame(index, true" in code and "emit_frame(0, true" in code
assert "helper directory exclusion unproved" in (root / "tools/windows-native-replay/Admission.cs").read_text()
entry = (root / "tools/windows-native-replay/Invoke-BoundedReplay.ps1").read_text()
assert "throw 'GATE2_ONLY:" in entry
print("gate2 map: kernel 0; failures 1..28; peek109 29..32; controls 33..34; dormant adapter admission blocked; native imports source only")

assert selectors["filters"][0]["row_indices"] == [0]
assert selectors["filters"][1]["row_indices"] == list(range(1, 35))
for f in selectors["filters"]:
    subset = [rows[i] for i in f["row_indices"]]
    assert len(subset) == f["cases"]
    assert sum(r["creates"] for r in subset) == f["children"]
    assert sum(r["resumes"] for r in subset) == f["resumes"]
