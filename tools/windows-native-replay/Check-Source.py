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
assert len(re.findall(r"#\[test\]", pure)) == 4
assert code.count('row!(\n') + code.count('row!("') == 28
selectors = json.loads((root / "tools/windows-native-replay/selectors.json").read_text())
assert selectors["registration_enabled"] is False
assert sum(f["cases"] for f in selectors["filters"]) == 35
assert sum(f["children"] for f in selectors["filters"]) == 20
assert sum(f["resumes"] for f in selectors["filters"]) == 16
helper = (root / "tools/windows-native-replay/child/src/main.rs").read_text()
assert re.sub(r"//[^\n]*", "", helper).strip() == "fn main() {}"
print("gate2 source inventory: 28 oracle rows; 4 pure tests; 0 native registrations/callers; selectors disabled")
