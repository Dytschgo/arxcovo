"""Focused source-wiring audit; no Rust or engine execution.

This checks the selected live arms and refusal cleanup, not expanded MIR.
Synthetic source mutants prove the checks reject their designated regressions.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / "crates/arxd/src/cluster"
PATHS = {
    "found": "consensus/found.rs",
    "guard": "consensus.rs",
    "f10": "consensus/found_tests.rs",
    "query": "consensus/anchor_query_tests.rs",
    "proof": "consensus/anchored_join_tests.rs",
}


def block(text, marker):
    # Mask strings/comments without changing offsets before balancing braces.
    masked = re.sub(r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"',
                    lambda m: " " * len(m[0]), text)
    start = masked.index("{", masked.index(marker))
    depth = 0
    for end in range(start, len(masked)):
        depth += (masked[end] == "{") - (masked[end] == "}")
        if depth == 0:
            return text[start + 1:end]
    raise AssertionError("unterminated selected block")


def flat(text):
    return "".join(text.split())


def check(s):
    final = block(s["found"], "match timeout_at(deadline, work).await")
    for marker, tag, result in [
        ("Ok(r) if Instant::now() < deadline =>", "InnerOnTime", "r"),
        ("Ok(_) =>", "ReadyLate", "Err(FoundError::Unavailable)"),
        ("_ =>", "WaitElapsed", "Err(FoundError::Unavailable)"),
    ]:
        arm = flat(block(final, marker))
        assert f"observation::decision(observation::Decision::{tag});" in arm
        assert arm.endswith(result), "founder arm changed its returned value"
    guard = block(s["guard"], "match timeout_at(deadline, rx).await")
    for marker, edge, reason, result in [
        ("Ok(Ok(result)) if Instant::now() < deadline =>", "CallerReply", "Ok", "result"),
        ("Ok(Ok(_)) =>", "CallerReply", "Expired", "Err(ReadError::Unavailable)"),
        ("Ok(Err(_)) =>", "CallerExpiry", "ReceiverClosed", "Err(ReadError::Unavailable)"),
        ("_ =>", "CallerExpiry", "Expired", "Err(ReadError::Unavailable)"),
    ]:
        arm = flat(block(guard, marker))
        assert f"observation::Edge::{edge},observation::Reason::{reason}," in arm
        assert arm.endswith(result), "guard arm changed its returned value"
    wait = s["found"][s["found"].index('"leaderwait",'):]
    wait = block(wait, ".map_err(|_error|")
    assert "leader_wait_reason(&_error)" in wait
    assert flat(wait).endswith("FoundError::Unavailable")
    classify = flat(block(s["found"], "fn leader_wait_reason"))
    assert "WaitError::Timeout(_,_)=>Reason::WaitTimeout" in classify
    assert "WaitError::ShuttingDown=>Reason::ShuttingDown" in classify
    for key in ("query", "proof"):
        refusal = block(s[key], "if let Some(error) = refusal")
        assert refusal.index("n.shutdown()") < refusal.index("observation::emit(")
        assert refusal.index("observation::emit(") < refusal.index('panic!("original founder refusal:')
        remainder = refusal.replace("captured.as_ref().unwrap()", "CAPTURED")
        assert ".unwrap(" not in remainder and ".expect(" not in remainder
        assert "assert!" not in remainder and "assert_eq!" not in remainder
    f10 = block(s["f10"], "bounded!(f10_drop_bounded, async")
    assert "Ok(leaf)=>drop(leaf)," in flat(f10)
    refusal = block(f10, "Err(error) =>")
    assert refusal.index("n.shutdown()") < refusal.index("observation::emit(")
    assert refusal.index("observation::emit(") < refusal.index('panic!("original founder refusal:')
    assert ".unwrap(" not in refusal and ".expect(" not in refusal
    assert "if self.raft.is_none()" not in s["found"] + s["guard"]
    assert "if self.sm.is_none()" not in s["found"] + s["guard"]


def main():
    sources = {k: (BASE / p).read_text(encoding="utf-8") for k, p in PATHS.items()}
    check(sources)
    mutants = [
        ("found", "observation::decision(observation::Decision::InnerOnTime);",
         "observation::decision(observation::Decision::ReadyLate);"),
        ("guard", "observation::Edge::CallerReply,\n                    observation::Reason::Expired,",
         "observation::Edge::CallerExpiry,\n                    observation::Reason::Expired,"),
        ("found", "WaitError::Timeout(_, _) => Reason::WaitTimeout",
         "WaitError::Timeout(_, _) => Reason::ReplyError"),
        ("f10", "Ok(leaf) => drop(leaf),", "Ok(_) => (),"),
        ("query", "cleanup.shutdown = observation::shutdown_status(&shutdown);",
         "shutdown.unwrap();\n        cleanup.shutdown = observation::Status::Ok;"),
        ("proof", "cleanup.shutdown = observation::shutdown_status(&shutdown);",
         "shutdown.unwrap();\n        cleanup.shutdown = observation::Status::Ok;"),
    ]
    for key, original, replacement in mutants:
        assert sources[key].count(original) == 1, "mutation seam drift"
        changed = dict(sources)
        changed[key] = changed[key].replace(original, replacement, 1)
        try:
            check(changed)
        except AssertionError:
            continue
        raise AssertionError("designated source mutant survived")
    print("Live wiring audit passed; six designated source mutants rejected.")


if __name__ == "__main__":
    main()
