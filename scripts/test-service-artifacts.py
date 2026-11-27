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


def rust_str_consts(path):
    """Plain `const NAME: &str = "...";` literals only; no evaluation."""
    text = path.read_text(encoding="utf-8")
    return dict(re.findall(r'^(?:pub )?const ([A-Z_]+): &str = "([^"\\]*)";$', text, re.M))


class Artifacts(unittest.TestCase):
    def test_artifacts_match_owner_query_constants(self):
        # The installer embeds these bytes at the owner_query paths, and the
        # launchd parser accepts only this label, plist path and argv.
        consts = rust_str_consts(OWNER_QUERY)
        self.assertEqual(consts["UNIT_PATH"], "/etc/systemd/system/" + LINUX.name)
        self.assertEqual(consts["DROPIN_PATH"],
                         "/etc/systemd/system/arxd.service.d/50-arxcovo-standard-rights.conf")
        self.assertTrue((LINUX.parent / pathlib.PurePosixPath(consts["DROPIN_PATH"]).name).is_file())
        self.assertEqual(consts["PLIST_PATH"], "/Library/LaunchDaemons/" + PLIST.name)
        plist = plistlib.loads(PLIST.read_bytes())
        self.assertEqual(consts["LAUNCHD_LABEL"], plist["Label"])
        self.assertEqual(consts["LAUNCHD_TARGET"], "system/" + plist["Label"])
        self.assertEqual(consts["LAUNCHD_PROGRAM"], plist["ProgramArguments"][0])
        self.assertIn('LAUNCHD_ARGUMENTS: &[&str] = &[LAUNCHD_PROGRAM, "service", "run"];',
                      OWNER_QUERY.read_text(encoding="utf-8"))
        self.assertEqual(plist["ProgramArguments"][1:], ["service", "run"])
        # Fixed binary paths in the service layout match both manager files.
        layout = LAYOUT.read_text(encoding="utf-8")
        self.assertIn('"/usr/local/libexec/arxcovo/arxd"', layout)
        self.assertIn('"%s"' % plist["ProgramArguments"][0], layout)


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
