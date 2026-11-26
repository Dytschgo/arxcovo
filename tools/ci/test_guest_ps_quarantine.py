"""Pin the hosted Windows PowerShell `Ok(TimedOut)` test quarantine.

ROADMAP known issue "Hosted Windows arx-guest PowerShell `TimedOut`". Every
function on the outcome path of the four affected tests is pinned to its exact
text, and every reference to a pinned item must sit inside an allowed function,
so an early return, a widened condition or a new caller (with any visibility)
fails here. The `0x80131623` exit-code route stays a separate predicate.

Kept apart from test_windows_cluster_quarantine.py, whose size is bounded by
the reader in windows_diag_sample.py.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/arx-guest/src/platform/windows.rs"


def _norm(text):
    return re.sub(r"\s+", "", text)


# Rust lexemes whose contents may hold braces, quotes or `fn`: comments, raw
# strings, strings, and char or byte literals. Lifetimes (`'a`) stay as code.
_LEXEME = re.compile(
    r"//[^\n]*"
    r"|/\*.*?\*/"
    r'|(?<!\w)b?r(?P<hashes>#*)".*?"(?P=hashes)'
    r'|(?<!\w)b?"(?:\\.|[^"\\])*"'
    r"|(?<!\w)b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^'\\\n])'",
    re.S,
)


def _mask_comments(source):
    """Blank comments and literal contents. Indexes and newlines stay put.

    Literal delimiters remain, so `"abc"` masks to `"   "` and `'"'` to `' '`.
    """
    def blank(match):
        text = match.group(0)
        if text.startswith(("//", "/*")):
            head, tail = 0, 0
        else:
            quote = '"' if text.endswith('"') or "#" in text[-1:] else "'"
            head = text.index(quote) + 1
            tail = 1 + len(match.group("hashes") or "")
        middle = text[head:len(text) - tail]
        return text[:head] + re.sub(r"[^\n]", " ", middle) + text[len(text) - tail:]

    return _LEXEME.sub(blank, source)


# Copied from test_windows_cluster_quarantine.py so this guard stays
# self-contained under `python -I`.
def _attribute_cut(masked):
    """Start of the trailing attribute, doc-comment, comment, and blank block."""
    i = len(masked)
    while i > 0:
        j = i - 1
        while j >= 0 and masked[j].isspace():
            j -= 1
        if j < 0:
            return 0
        if masked[j] != "]":
            return j + 1
        depth = 1
        k = j - 1
        while k >= 0 and depth:
            if masked[k] == "]":
                depth += 1
            elif masked[k] == "[":
                depth -= 1
            k -= 1
        if depth:
            return j + 1
        while k >= 0 and masked[k].isspace():
            k -= 1
        if k >= 0 and masked[k] == "!":
            k -= 1
            while k >= 0 and masked[k].isspace():
                k -= 1
        if k >= 0 and masked[k] == "#":
            i = k
            continue
        return j + 1
    return 0


POWERSHELL_TIMEOUT_TESTS = (
    "wrapper_evaluates_utf8_multiline_with_implicit_success",
    "wrapper_reports_implicit_error",
    "production_identity_runs_powershell_and_removes_its_request_directory",
    "self_test_admits_the_production_identity",
)
POWERSHELL_CRASH_CONST = "const HOSTED_POWERSHELL_CRASH: u32 = 0x8013_1623;"
# Exact source bodies (whitespace-insensitive), keyed by function name.
POWERSHELL_EXACT = {
    "is_hosted_win25": r'''
        ci.is_some_and(|value| value.eq_ignore_ascii_case("true"))
            && runner == Some("github-hosted")
            && image.is_some_and(|value| value.starts_with("win25"))
''',
    "is_known_hosted_crash": r'''
        is_hosted_win25(ci, runner, image)
            && outcome
                == Ok(ScriptOutcome::Exited {
                    exit_code: HOSTED_POWERSHELL_CRASH,
                })
''',
    "from_env": r'''
            Self {
                ci: std::env::var("CI").ok(),
                environment: std::env::var("RUNNER_ENVIRONMENT").ok(),
                image: std::env::var("ImageOS").ok(),
            }
''',
    "quarantine_hosted_crash": r'''
        if outcome == Ok(expected) {
            return false;
        }
        let ci = runner.ci.as_deref();
        let environment = runner.environment.as_deref();
        let image = runner.image.as_deref();
        let os = std::env::var("RUNNER_OS").ok();
        eprintln!(
            "restricted PowerShell runner: CI={ci:?} RUNNER_ENVIRONMENT={environment:?} \
             ImageOS={image:?} RUNNER_OS={os:?}"
        );
        let known = is_known_hosted_crash(outcome, ci, environment, image);
        if known {
            hosted_restricted_powershell_ladder(profile);
        }
        known
''',
    "assert_powershell": r'''
        if outcome != Ok(expected) {
            panic!(
                "PowerShell returned {outcome:?}, expected {expected:?}\ndiagnostics:\n{}",
                diagnose()
            );
        }
''',
    "is_known_hosted_timeout": r'''
        is_hosted_win25(
            runner.ci.as_deref(),
            runner.environment.as_deref(),
            runner.image.as_deref(),
        ) && outcome == Ok(ScriptOutcome::TimedOut)
''',
    "settle_wrapper_outcome": r'''
        if is_known_hosted_timeout(outcome, runner) {
            eprintln!("KNOWN-ISSUE hosted-windows-powershell-wrapper: Ok(TimedOut)");
            return true;
        }
        assert_powershell(outcome, expected, diagnose);
        false
''',
    "settle_restricted_outcome": r'''
        if quarantine_hosted_crash(outcome, expected, runner, profile) {
            return true;
        }
        if is_known_hosted_timeout(outcome, runner) {
            eprintln!("KNOWN-ISSUE hosted-windows-powershell-restricted-identity: Ok(TimedOut)");
            return true;
        }
        assert_powershell(outcome, expected, diagnose);
        false
''',
    "assert_powershell_wrapper": r'''
        let root = TestRoot::new("wrapper");
        let (windows, system) = system_paths().unwrap();
        let command = CommandLine::powershell(&windows, &system, &root.0).unwrap();
        let stop = Arc::new(event().unwrap());
        let worker_stop = Arc::clone(&stop);
        let worker_script = script.to_vec();
        let deadline = Instant::now() + POWERSHELL_GUARD;
        let outcome = supervise(&stop, deadline, CLEANUP_BUDGET, move || {
            prepare(
                &command,
                Identity::TestCurrent,
                &current_user_sid(),
                worker_script,
                &worker_stop,
                deadline,
                |_| Ok(()),
            )
        });
        settle_wrapper_outcome(
            outcome,
            ScriptOutcome::Exited {
                exit_code: expected,
            },
            &HostedRunner::from_env(),
            || {
                let rerun = TestRoot::new("wrapper-diagnostic");
                capture_powershell(
                    Identity::TestCurrent,
                    &current_user_sid(),
                    &rerun.0,
                    script,
                    None,
                )
            },
        );
''',
}
# Test bodies are pinned with comments and string contents masked, so fixture
# scripts and messages may change but no statement can be added or altered.
POWERSHELL_TEST_CODE = {
    "wrapper_evaluates_utf8_multiline_with_implicit_success":
        'assert_powershell_wrapper("".as_bytes(), 0);',
    "wrapper_reports_implicit_error": 'assert_powershell_wrapper(b"", 1);',
    "production_identity_runs_powershell_and_removes_its_request_directory": r'''
        let root = TestRoot::new("");
        let profile = Arc::new(test_profile(&root.0));
        let stop = Arc::new(event().unwrap());
        let script = "";
        let outcome = run_profile(
            &profile,
            &stop,
            script.as_bytes().to_vec(),
            Instant::now() + POWERSHELL_GUARD,
        );
        let expected = ScriptOutcome::Exited { exit_code: 7 };
        settle_restricted_outcome(
            outcome,
            expected,
            &HostedRunner::from_env(),
            &profile,
            || restricted_failure_report(&profile, script.as_bytes()),
        );
        assert_eq!(root.entries(), 0, "");
''',
    "self_test_admits_the_production_identity": r'''
        let root = TestRoot::new("");
        let stop = Arc::new(event().unwrap());
        let profile = Arc::new(test_profile(&root.0));
        let outcome = run_profile(
            &profile,
            &stop,
            SELF_TEST.as_bytes().to_vec(),
            Instant::now() + POWERSHELL_GUARD,
        );
        let sentinel = ScriptOutcome::Exited {
            exit_code: SELF_TEST_SENTINEL,
        };
        let tolerated = settle_restricted_outcome(
            outcome,
            sentinel,
            &HostedRunner::from_env(),
            &profile,
            || restricted_failure_report(&profile, SELF_TEST.as_bytes()),
        );
        if !tolerated {
            let admitted = admit(test_profile(&root.0), &clean_policy(), |_| outcome);
            assert!(matches!(admitted, Ok(Some(_))));
        }
        assert_eq!(
            root.entries(),
            0,
            ""
        );
''',
}
# Every function that may name each pinned item. The first entry is the
# definition itself; the rest are its only callers.
POWERSHELL_CALLERS = {
    "is_known_hosted_crash": (
        "is_known_hosted_crash",
        "quarantine_hosted_crash",
        "hosted_exception_requires_a_win25_image_and_the_exact_crash_exit",
        "hosted_timeout_and_crash_predicates_stay_distinct",
    ),
    "quarantine_hosted_crash": ("quarantine_hosted_crash", "settle_restricted_outcome"),
    "is_known_hosted_timeout": (
        "is_known_hosted_timeout",
        "settle_wrapper_outcome",
        "settle_restricted_outcome",
        "hosted_timeout_and_crash_predicates_stay_distinct",
    ),
    "settle_wrapper_outcome": (
        "settle_wrapper_outcome",
        "assert_powershell_wrapper",
        "wrapper_family_tolerates_only_the_hosted_timeout",
    ),
    "settle_restricted_outcome": (
        "settle_restricted_outcome",
        POWERSHELL_TIMEOUT_TESTS[2],
        POWERSHELL_TIMEOUT_TESTS[3],
        "restricted_identity_family_tolerates_only_the_hosted_timeout",
    ),
    "assert_powershell_wrapper": (
        "assert_powershell_wrapper",
        POWERSHELL_TIMEOUT_TESTS[0],
        POWERSHELL_TIMEOUT_TESTS[1],
    ),
    "from_env": ("from_env", "assert_powershell_wrapper") + POWERSHELL_TIMEOUT_TESTS[2:],
}


# A function declaration with any visibility and qualifiers, at any indent.
_FN_DECL = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?\s+)?"
    r"(?:(?:const|async|unsafe|extern)\s+)*fn\s+(\w+)",
    re.M,
)


def _functions(masked):
    """(name, decl_start, body_open, body_close) for every fn with a body.

    Braces are counted on the masked text, so comments and strings cannot
    unbalance them.
    """
    found = []
    for decl in _FN_DECL.finditer(masked):
        depth = 0
        i = decl.end()
        body_open = None
        while i < len(masked):
            c = masked[i]
            if c in "([":
                depth += 1
            elif c in ")]":
                depth -= 1
            elif depth == 0 and c == ";":
                break
            elif depth == 0 and c == "{":
                body_open = i
                break
            i += 1
        if body_open is None:
            continue
        depth = 0
        for j in range(body_open, len(masked)):
            if masked[j] == "{":
                depth += 1
            elif masked[j] == "}":
                depth -= 1
                if depth == 0:
                    found.append((decl.group(1), decl.start(), body_open, j))
                    break
        else:
            raise AssertionError(f"unbalanced fn {decl.group(1)}")
    return found


def _owner(functions, pos):
    """Innermost function whose declaration-to-close range contains pos."""
    owners = [f for f in functions if f[1] <= pos <= f[3]]
    return max(owners, key=lambda f: f[1])[0] if owners else None


def _one(functions, name):
    matches = [f for f in functions if f[0] == name]
    assert len(matches) == 1, name
    return matches[0]


def assert_powershell_timeout_quarantine(source):
    masked = _mask_comments(source)
    functions = _functions(masked)
    assert source.count(POWERSHELL_CRASH_CONST) == 1
    for name, expected in POWERSHELL_EXACT.items():
        _, _, body_open, body_close = _one(functions, name)
        assert _norm(source[body_open + 1:body_close]) == _norm(expected), name
    for name, expected in POWERSHELL_TEST_CODE.items():
        _, start, body_open, body_close = _one(functions, name)
        code = masked[body_open + 1:body_close]
        assert _norm(code) == _norm(_mask_comments(expected)), name
        assert _norm(masked[_attribute_cut(masked[:start]):start]) == "#[test]", name
        assert re.match(r"\s*fn\s", masked[start:]), name
    for token, allowed in POWERSHELL_CALLERS.items():
        for match in re.finditer(rf"\b{token}\b", masked):
            owner = _owner(functions, match.start())
            assert owner in allowed, (token, owner)


class PowerShellTimeoutQuarantineTests(unittest.TestCase):
    def test_exact_hosted_powershell_timeout_quarantine(self):
        assert_powershell_timeout_quarantine(SOURCE.read_text(encoding="utf-8"))

    def test_mutations_widening_the_quarantine_fail(self):
        source = SOURCE.read_text(encoding="utf-8")
        early = "        if outcome.is_err() {\n            return;\n        }\n"
        mutations = {
            "tolerate any error": (
                ") && outcome == Ok(ScriptOutcome::TimedOut)",
                ") && (outcome.is_err() || outcome == Ok(ScriptOutcome::TimedOut))",
            ),
            "timeout off CI": (
                ") && outcome == Ok(ScriptOutcome::TimedOut)",
                ") || outcome == Ok(ScriptOutcome::TimedOut)",
            ),
            "absent CI counts": (
                'ci.is_some_and(|value| value.eq_ignore_ascii_case("true"))',
                'ci.is_none_or(|value| value.eq_ignore_ascii_case("true"))',
            ),
            "self-hosted runner": ('Some("github-hosted")', 'Some("self-hosted")'),
            "any Windows image": ('value.starts_with("win25")', 'value.starts_with("win")'),
            "forced CI": ('ci: std::env::var("CI").ok(),', 'ci: Some("true".to_owned()),'),
            "harness early return": (
                "\n        settle_wrapper_outcome(\n",
                "\n" + early + "        settle_wrapper_outcome(\n",
            ),
            "wrapper settle early return": (
                "        assert_powershell(outcome, expected, diagnose);\n        false\n",
                "        if outcome.is_err() {\n            return true;\n        }\n"
                "        assert_powershell(outcome, expected, diagnose);\n        false\n",
            ),
            "restricted settle early return": (
                "restricted-identity: Ok(TimedOut)\");\n            return true;\n        }\n",
                "restricted-identity: Ok(TimedOut)\");\n            return true;\n        }\n"
                "        if outcome.is_err() {\n            return true;\n        }\n",
            ),
            "identity test early return": (
                "        let expected = ScriptOutcome::Exited { exit_code: 7 };\n",
                "        let expected = ScriptOutcome::Exited { exit_code: 7 };\n" + early,
            ),
            "self-test early return": (
                "        let tolerated = settle_restricted_outcome(\n",
                early + "        let tolerated = settle_restricted_outcome(\n",
            ),
            "self-test skips admission": ("        if !tolerated {", "        if !tolerated && outcome.is_ok() {"),
            "strict assertion widened": (
                "        if outcome != Ok(expected) {\n            panic!(",
                "        if outcome != Ok(expected) && outcome.is_ok() {\n            panic!(",
            ),
            "crash route widened": (
                "let known = is_known_hosted_crash(outcome, ci, environment, image);",
                "let known = outcome.is_err() || is_known_hosted_crash(outcome, ci, environment, image);",
            ),
            "crash code changed": (POWERSHELL_CRASH_CONST, "const HOSTED_POWERSHELL_CRASH: u32 = 0x8013_1624;"),
            "new timeout caller": (
                "    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {\n",
                "    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {\n"
                "        let _ = is_known_hosted_timeout;\n",
            ),
            "new restricted settle caller": (
                "    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {\n",
                "    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {\n"
                "        let _ = settle_restricted_outcome::<fn() -> String>;\n",
            ),
        }
        family = "    fn wrapper_family_tolerates_only_the_hosted_timeout() {\n"
        sneaky = (
            "settle_wrapper_outcome(Ok(ScriptOutcome::TimedOut), SETTLE_EXPECTED, "
            "&hosted_win25_runner(), String::new);"
        )
        for visibility in ("pub(crate) ", "pub ", "pub(super) ", "const ", ""):
            caller = "    #[test]\n    " + visibility + "fn sneaky() {\n        " + sneaky + "\n    }\n\n"
            mutations[f"new {visibility or 'private '}caller"] = (family, caller + family)
        mutations["nested caller"] = (
            family,
            family + "        fn nested() {\n            " + sneaky + "\n        }\n",
        )
        mutations["module-level caller"] = (
            "    const SETTLE_EXPECTED: ScriptOutcome",
            "    const SNEAKY: fn(Result<ScriptOutcome, &'static str>, ScriptOutcome, "
            "&HostedRunner, fn() -> String) -> bool = settle_wrapper_outcome;\n"
            "    const SETTLE_EXPECTED: ScriptOutcome",
        )
        mutations["pub test"] = (
            "    fn wrapper_reports_implicit_error()",
            "    pub(crate) fn wrapper_reports_implicit_error()",
        )
        for name in POWERSHELL_TIMEOUT_TESTS:
            mutations[f"ignored {name}"] = (f"    fn {name}()", f"    #[ignore]\n    fn {name}()")
        for label, (original, replacement) in mutations.items():
            with self.subTest(mutation=label):
                mutant = source.replace(original, replacement, 1)
                self.assertNotEqual(mutant, source)
                with self.assertRaises(AssertionError):
                    assert_powershell_timeout_quarantine(mutant)


if __name__ == "__main__":
    unittest.main()
