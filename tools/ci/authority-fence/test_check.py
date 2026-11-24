"""Pure tests for the authority fence gate's parsing and verdicts."""
import importlib.util
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
                "plain text",
            ]
        )
        self.assertEqual(
            CHECK.errors(stderr),
            [("E0451", "field `origin` of struct `Baseline` is private")],
        )

    def test_a_refusal_must_carry_its_declared_code_and_nothing_else(self):
        tag = "compile_fail,E0451"
        self.assertIsNone(CHECK.verdict(tag, [("E0451", "fields are private"), PRIVATE]))
        for name, found in [
            ("compiled", []),
            ("only private-type noise", [PRIVATE]),
            ("another code", [("E0433", "failed to resolve")]),
            ("declared code plus a stray code", [("E0451", "private"), ("E0308", "mismatched types")]),
            ("declared code plus a stray code-less error", [("E0451", "private"), (None, "cannot find value")]),
        ]:
            self.assertIsNotNone(CHECK.verdict(tag, found), name)

    def test_the_positive_control_must_compile_cleanly(self):
        self.assertIsNone(CHECK.verdict("no_run", []))
        self.assertIsNotNone(CHECK.verdict("no_run", [PRIVATE]))
        self.assertIsNotNone(CHECK.verdict("no_run", [("E0106", "missing lifetime specifier")]))


if __name__ == "__main__":
    unittest.main()
