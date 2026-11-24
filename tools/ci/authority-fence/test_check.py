"""Pure tests for the authority fence gate's parsing and verdicts."""
from contextlib import redirect_stdout
import importlib.util
from io import StringIO
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("fence_check", Path(__file__).with_name("check.py"))
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)

PRIVATE = (None, "type `qualification::Held` is private")


def diagnostic(code, message, level="error"):
    return json.dumps(
        {
            "$message_type": "diagnostic",
            "level": level,
            "code": {"code": code} if code else None,
            "message": message,
        }
    )


class DoctestLibraryTests(unittest.TestCase):
    def artifact(self, path, manifest=CHECK.MANIFEST, test=False):
        return json.dumps({
            "reason": "compiler-artifact",
            "manifest_path": str(manifest),
            "target": {"name": "arxd", "kind": ["lib"]},
            "profile": {"test": test},
            "filenames": [str(path), str(path.with_suffix(".rmeta"))],
            "fresh": True,
        })

    def test_reuses_the_doctest_library_and_prints_the_doctest_output(self):
        rlib = Path("target/debug/deps/libarxd-abc.rlib")
        output = "\n".join([
            self.artifact(rlib, manifest=Path("other/Cargo.toml")),
            self.artifact(rlib, test=True),
            self.artifact(rlib),
            '{"reason":"build-finished","success":true}',
            '{"doctests":"passed"}',
        ])
        printed = StringIO()
        with redirect_stdout(printed):
            self.assertEqual(CHECK.doctest_library(output), rlib)
        self.assertEqual(printed.getvalue(), '{"doctests":"passed"}\n')

    def test_missing_or_ambiguous_dependency_artifacts_fail(self):
        for artifacts in [
            [Path("target/debug/libarxd.rlib")],
            [Path("target/debug/deps/libarxd-a.rlib"), Path("target/debug/deps/libarxd-b.rlib")],
        ]:
            with self.assertRaisesRegex(RuntimeError, "expected one arxd doctest rlib"):
                CHECK.doctest_library("\n".join(self.artifact(path) for path in artifacts))


class BlockTests(unittest.TestCase):
    def test_tagged_doc_blocks_are_extracted_without_comment_markers(self):
        text = "\n".join(
            [
                "/// Prose.",
                "///",
                "/// ```compile_fail,E0451",
                "/// let x = 1;",
                "///",
                "///     indented();",
                "/// ```",
                "/// ```text",
                "/// not rust",
                "/// ```",
                "pub struct S;",
            ]
        )
        self.assertEqual(
            CHECK.blocks(text),
            [
                ("compile_fail,E0451", "let x = 1;\n\n    indented();"),
                ("text", "not rust"),
            ],
        )
        with self.assertRaises(ValueError):
            CHECK.blocks("/// ```no_run\n/// let x = 1;\n")

    def test_the_source_holds_exactly_the_gated_blocks(self):
        tags = [tag for tag, _ in CHECK.blocks(CHECK.SOURCE.read_text(encoding="utf-8"))]
        self.assertEqual(tags, CHECK.EXPECTED_TAGS)

    def test_blocks_without_main_are_wrapped_like_rustdoc(self):
        self.assertEqual(CHECK.program("let _ = 1;"), "fn main() {\nlet _ = 1;\n}\n")
        self.assertEqual(CHECK.program("fn main() {}"), "fn main() {}\n")

    def test_only_declared_tags_are_supported(self):
        self.assertEqual(CHECK.expected_codes("compile_fail,E0277"), ["E0277"])
        self.assertEqual(CHECK.expected_codes("no_run"), [])
        for tag in ["compile_fail", "compile_fail,E277", "ignore", "should_panic"]:
            with self.assertRaises(ValueError, msg=tag):
                CHECK.expected_codes(tag)


class VerdictTests(unittest.TestCase):
    def test_json_errors_exclude_warnings_notes_and_the_abort_summary(self):
        stderr = "\n".join(
            [
                diagnostic("E0451", "field `origin` of struct `Baseline` is private"),
                diagnostic(None, "unused variable", level="warning"),
                diagnostic(None, "aborting due to 1 previous error"),
                json.dumps({"$message_type": "artifact", "artifact": "x"}),
                "",
            ]
        )
        self.assertEqual(
            CHECK.errors(stderr),
            [("E0451", "field `origin` of struct `Baseline` is private")],
        )

    def test_a_refusal_must_carry_its_declared_code_and_nothing_else(self):
        tag = "compile_fail,E0451"
        self.assertIsNone(CHECK.verdict(tag, 1, [("E0451", "fields are private"), PRIVATE]))
        for name, found in [
            ("compiled", []),
            ("only private-type noise", [PRIVATE]),
            ("another code", [("E0433", "failed to resolve")]),
            ("declared code plus a stray code", [("E0451", "private"), ("E0308", "mismatched types")]),
            ("declared code plus a stray code-less error", [("E0451", "private"), (None, "cannot find value")]),
        ]:
            self.assertIsNotNone(CHECK.verdict(tag, 1, found), name)

    def test_a_refusal_needs_a_failing_compiler_exit(self):
        tag = "compile_fail,E0451"
        # Exit 0 with the expected diagnostic is not a refusal.
        self.assertIsNotNone(CHECK.verdict(tag, 0, [("E0451", "fields are private")]))
        # A failing exit without the structured code is not the fence.
        self.assertIsNotNone(CHECK.verdict(tag, 1, []))
        self.assertIsNotNone(CHECK.judge(tag, 101, "error: rustc crashed\nplain text"))
        self.assertIsNotNone(CHECK.verdict(tag, 1, [("E0433", "failed to resolve")]))

    def test_the_positive_control_must_compile_cleanly(self):
        self.assertIsNone(CHECK.verdict("no_run", 0, []))
        self.assertIsNotNone(CHECK.verdict("no_run", 0, [PRIVATE]))
        self.assertIsNotNone(CHECK.verdict("no_run", 0, [("E0106", "missing lifetime specifier")]))

    def test_a_positive_control_whose_compiler_fails_never_passes(self):
        # Exit 1 with plain text only and no structured diagnostic.
        self.assertIsNotNone(
            CHECK.judge("no_run", 1, "error: could not compile\nterminated before JSON output")
        )
        self.assertIsNotNone(CHECK.verdict("no_run", 1, []))

    def test_an_expected_code_followed_by_a_plain_fatal_line_fails(self):
        tag = "compile_fail,E0451"
        expected = diagnostic("E0451", "fields of struct `Baseline` are private")
        fatal = "error: internal compiler error: unexpected panic"
        self.assertIsNotNone(CHECK.judge(tag, 101, expected + "\n" + fatal))
        # Control: the structured diagnostic alone, with blank lines, passes.
        self.assertIsNone(CHECK.judge(tag, 101, "\n" + expected + "\n\n"))

    def test_a_successful_positive_control_with_a_plain_error_line_fails(self):
        self.assertIsNotNone(CHECK.judge("no_run", 0, "error: something went wrong"))
        # Control: no output at all passes.
        self.assertIsNone(CHECK.judge("no_run", 0, ""))

    def test_unparseable_json_output_is_an_error(self):
        for stderr in ["{not json", '{"level": "error"', "plain text\n{1}", "{}x", "plain text"]:
            with self.assertRaises(ValueError, msg=stderr):
                CHECK.errors(stderr)


if __name__ == "__main__":
    unittest.main()
