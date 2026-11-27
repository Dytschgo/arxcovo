"""Read-only package syntax/contract checks; no service installation or start."""
import configparser
import pathlib
import plistlib
import re
import shutil
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
LINUX = ROOT / "packaging/linux/arxd.service"
PLIST = ROOT / "packaging/macos/com.arxcovo.arxd.plist"
OWNER_QUERY = ROOT / "crates/arxd/src/service/owner_query.rs"
LAYOUT = ROOT / "crates/arxd/src/service/layout.rs"


# Rust char literal such as '"', '\\', '\n' or '\u{7f}'; lifetimes never match.
CHAR_LITERAL = re.compile(r"'(?:\\(?:u\{[0-9a-fA-F]{1,6}\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'")
RAW_STRING = re.compile(r'r(#*)"')


def strip_rust_comments(text):
    """Remove // and /* */ comments outside string and char literals."""
    out, i, n = [], 0, len(text)
    while i < n:
        boundary = i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")
        raw = RAW_STRING.match(text, i) if boundary else None
        char = CHAR_LITERAL.match(text, i)
        if raw:
            end = text.index('"' + raw.group(1), raw.end())
            stop = end + 1 + len(raw.group(1))
            out.append(text[i:stop])
            i = stop
        elif char:
            out.append(char.group(0))
            i = char.end()
        elif text[i] == '"':
            j = i + 1
            while text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1])
            i = j + 1
        elif text.startswith("//", i):
            i = text.find("\n", i)
            i = n if i < 0 else i
        elif text.startswith("/*", i):
            i = text.index("*/", i) + 2
            out.append(" ")
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def rust_str_consts(text):
    """Plain `const NAME: &str = "...";` literals only, each defined once."""
    pairs = re.findall(r'^\s*(?:pub )?const ([A-Z_]+): &str = "([^"\\]*)";$',
                       strip_rust_comments(text), re.M)
    names = [name for name, _ in pairs]
    if len(names) != len(set(names)):
        raise AssertionError("duplicate string constant")
    return dict(pairs)


def split_top_level(text, separator):
    """Split on `separator` outside (), [], {} and string literals."""
    parts, depth, i, last = [], 0, 0, 0
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth < 0:
                raise AssertionError("unbalanced Layout::unix source")
        elif depth == 0 and text.startswith(separator, i):
            parts.append(text[last:i])
            last = i + len(separator)
            i = last
            continue
        i += 1
    if depth != 0:
        raise AssertionError("unbalanced Layout::unix source")
    parts.append(text[last:])
    return parts


def unix_layout_binaries(text):
    """Binary (first tuple element) and artifact constant of every arm in the
    active `Layout::unix` match, read from comment-free source. The match must
    have exactly the arms `Platform::Linux` and `_`, in that order."""
    code = strip_rust_comments(text)
    body = re.search(r"fn unix\(platform: Platform\) -> Self \{(.*?)\n    \}\n", code, re.S)
    if body is None:
        raise AssertionError("Layout::unix not found")
    head = re.search(r"= match platform \{", body.group(1))
    if head is None or body.group(1).count("match ") != 1:
        raise AssertionError("Layout::unix must contain exactly one match")
    rest = body.group(1)[head.end():]
    # The match body ends at the first unbalanced closing brace.
    depth, i = 0, 0
    while True:
        c = rest[i]
        if c == '"':
            i += 1
            while rest[i] != '"':
                i += 2 if rest[i] == "\\" else 1
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                break
            depth -= 1
        i += 1
    arms = [arm.strip() for arm in split_top_level(rest[:i], ",") if arm.strip()]
    parsed = []
    for arm in arms:
        pattern_expr = split_top_level(arm, "=>")
        if len(pattern_expr) != 2:
            raise AssertionError("malformed Layout::unix arm")
        pattern, expr = (part.strip() for part in pattern_expr)
        found = re.fullmatch(r'\(\s*"([^"]+)",(?:\s*"[^"]*",){3}\s*super::owner_query::([A-Z_]+),.*\)',
                             expr, re.S)
        if found is None:
            raise AssertionError("unexpected Layout::unix arm value")
        parsed.append((pattern, found.group(1), found.group(2)))
    if [pattern for pattern, _, _ in parsed] != ["Platform::Linux", "_"]:
        raise AssertionError("unexpected Layout::unix arms")
    return {pattern: (binary, artifact) for pattern, binary, artifact in parsed}


def require(condition):
    """Explicit check that survives `python -O`."""
    if not condition:
        raise AssertionError("service artifact contract mismatch")


def check_contract(owner_query, layout, plist, unit):
    """Raise AssertionError when manager files and Rust constants disagree."""
    consts = rust_str_consts(owner_query)
    require(consts["UNIT_PATH"] == "/etc/systemd/system/" + LINUX.name)
    require(consts["DROPIN_PATH"] == "/etc/systemd/system/arxd.service.d/50-arxcovo-standard-rights.conf")
    require(consts["PLIST_PATH"] == "/Library/LaunchDaemons/" + PLIST.name)
    require(consts["LAUNCHD_LABEL"] == plist["Label"])
    require(consts["LAUNCHD_TARGET"] == "system/" + plist["Label"])
    # launchd prefers `Program` over ProgramArguments[0]; the parser expects argv[0].
    require("Program" not in plist)
    require(consts["LAUNCHD_PROGRAM"] == plist["ProgramArguments"][0])
    require(('const LAUNCHD_ARGUMENTS: &[&str] = &[LAUNCHD_PROGRAM, "service", "run"];')
            in strip_rust_comments(owner_query))
    require(plist["ProgramArguments"][1:] == ["service", "run"])
    arms = unix_layout_binaries(layout)
    exec_start = unit["Service"]["ExecStart"].split(" ")
    require(arms["Platform::Linux"] == (exec_start[0], "UNIT_PATH"))
    require(exec_start[1:] == ["service", "run"])
    require(arms["_"] == (plist["ProgramArguments"][0], "PLIST_PATH"))


def read_unit():
    parser = configparser.ConfigParser(interpolation=None, strict=True)
    parser.optionxform = str
    parser.read_string(LINUX.read_text(encoding="utf-8"))
    return parser


class Artifacts(unittest.TestCase):
    def inputs(self):
        return (OWNER_QUERY.read_text(encoding="utf-8"), LAYOUT.read_text(encoding="utf-8"),
                plistlib.loads(PLIST.read_bytes()), read_unit())

    def test_artifacts_match_owner_query_constants(self):
        # The installer embeds these bytes at the owner_query paths, and the
        # launchd parser accepts only this label, plist path and argv.
        check_contract(*self.inputs())
        self.assertTrue((LINUX.parent / "50-arxcovo-standard-rights.conf").is_file())

    def test_contract_rejects_effective_drift(self):
        owner_query, layout, plist, unit = self.inputs()
        linux_bin = '"/usr/local/libexec/arxcovo/arxd",'
        mac_bin = '"/Library/Application Support/Arxcovo/libexec/arxd",\n                "/Library/Application Support/Arxcovo/config'
        self.assertEqual(layout.count(linux_bin), 1)
        self.assertEqual(layout.count(mac_bin), 1)
        self.assertEqual(layout.count("            _ => ("), 1)
        mac_arm = "".join(line + "\n" for line in [
            "Platform::Macos => (",
            '                "/opt/arxd",',
            '                "/opt/arxd.json",',
            '                "/opt/state",',
            '                "/opt/arxd.sock",',
            "                super::owner_query::PLIST_PATH,",
            "                vec![],",
            "            ),",
        ])
        linux_arm = mac_arm.replace("Platform::Macos", "Platform::Linux")
        mutations = [
            # Active binary changed while the old literal survives in a comment.
            (owner_query, layout.replace(linux_bin, '// "/usr/local/libexec/arxcovo/arxd",\n                "/opt/arxd",'), plist, unit),
            (owner_query, layout.replace(mac_bin, '/* "/Library/Application Support/Arxcovo/libexec/arxd", */ "/opt/arxd",\n                "/Library/Application Support/Arxcovo/config'), plist, unit),
            # A conflicting or even identical Program key in the plist.
            (owner_query, layout, {**plist, "Program": "/opt/arxd"}, unit),
            (owner_query, layout, {**plist, "Program": plist["ProgramArguments"][0]}, unit),
            # A new arm before `_` would change the effective macOS layout.
            (owner_query, layout.replace("            _ => (", mac_arm + "            _ => (", 1), plist, unit),
            # A duplicate Linux arm or a guard also changes the arm set.
            (owner_query, layout.replace("            _ => (", linux_arm + "            _ => (", 1), plist, unit),
            # owner_query constant shadowed only by a comment.
            (owner_query.replace('pub const LAUNCHD_LABEL: &str = "com.arxcovo.arxd";',
                                 '// pub const LAUNCHD_LABEL: &str = "com.arxcovo.arxd";\npub const LAUNCHD_LABEL: &str = "com.other";'),
             layout, plist, unit),
        ]
        for index, args in enumerate(mutations):
            self.assertNotEqual(args[:3], (owner_query, layout, plist), index)
            with self.assertRaises(AssertionError, msg=index):
                check_contract(*args)
        changed = read_unit()
        changed["Service"]["ExecStart"] = "/opt/arxd service run"
        with self.assertRaises(AssertionError):
            check_contract(owner_query, layout, plist, changed)


    def test_systemd(self):
        parser = configparser.ConfigParser(interpolation=None, strict=True)
        parser.optionxform = str
        parser.read_string(LINUX.read_text(encoding="utf-8"))
        self.assertEqual(parser["Service"]["ExecStart"], "/usr/local/libexec/arxcovo/arxd service run")
        self.assertEqual(parser["Service"]["RestartPreventExitStatus"], "2 3 4 5 7 10")
        dropin = configparser.ConfigParser(interpolation=None, strict=True)
        dropin.optionxform = str
        dropin.read_string((LINUX.parent / "50-arxcovo-standard-rights.conf").read_text(encoding="utf-8"))
        self.assertEqual(dict(dropin["Service"]), {
            "NoNewPrivileges": "yes", "CapabilityBoundingSet": "CAP_SETUID CAP_SETGID CAP_CHOWN",
            "ProtectSystem": "full", "ProtectHome": "yes", "PrivateTmp": "yes"})

    def test_plist(self):
        value = plistlib.loads(PLIST.read_bytes())
        self.assertEqual(value["Label"], "com.arxcovo.arxd")
        self.assertTrue(value["Disabled"])
        self.assertEqual(value["ProgramArguments"], ["/Library/Application Support/Arxcovo/libexec/arxd", "service", "run"])
        self.assertNotIn("UserName", value)
        self.assertEqual(value["KeepAlive"], {"SuccessfulExit": False})
        self.assertEqual(value["Umask"], 63)

    def test_no_interpolation(self):
        for path in [LINUX, LINUX.parent / "50-arxcovo-standard-rights.conf", PLIST]:
            text = path.read_text(encoding="utf-8")
            for forbidden in ["$", "%", "/bin/sh", "cmd.exe", "powershell"]:
                self.assertNotIn(forbidden, text)

    @unittest.skipUnless(sys.platform == "linux" and shutil.which("systemd-analyze") and pathlib.Path("/usr/local/libexec/arxcovo/arxd").is_file(), "Linux systemd-analyze or fixed binary unavailable")
    def test_native_systemd_verify(self):
        # Read-only syntax validation. No systemctl or manager mutation.
        subprocess.run(["systemd-analyze", "verify", str(LINUX)], check=True, timeout=20, capture_output=True)

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("plutil"), "macOS plutil unavailable")
    def test_native_plutil(self):
        subprocess.run(["plutil", "-lint", str(PLIST)], check=True, timeout=20, capture_output=True)

if __name__ == "__main__":
    unittest.main()
