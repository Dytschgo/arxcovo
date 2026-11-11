"""Read-only CI diagnostics: no addresses, error text, retries or gate override."""
import concurrent.futures
import json
import subprocess
import sys

TARGETS = (
    ("ubuntu_azure", "azure.archive.ubuntu.com"),
    ("ubuntu_archive", "archive.ubuntu.com"),
    ("ubuntu_security", "security.ubuntu.com"),
    ("github_control", "github.com"),
    ("microsoft_control", "packages.microsoft.com"),
)


def probe(target, run=subprocess.run):
    label, hostname = target
    try:
        result = run(
            ["/usr/bin/getent", "ahosts", hostname],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=5,
            check=False,
            shell=False,
        )
        status = "resolved" if result.returncode == 0 else "not_resolved"
    except subprocess.TimeoutExpired:
        status = "timeout"
    except OSError:
        status = "unavailable"
    return {"target": label, "status": status}


def observe(run=subprocess.run):
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(TARGETS)) as pool:
        rows = list(pool.map(lambda target: probe(target, run), TARGETS))
    encoded = json.dumps({"schemaVersion": 1, "observations": rows}, separators=(",", ":"))
    if len(encoded) > 1024:
        raise RuntimeError("dns-observation-bound")
    return encoded


def main():
    if len(sys.argv) != 1:
        print("dns-observation-arguments", file=sys.stderr)
        return 2
    print(observe())
    # Resolution failures are observations only. The existing install and smoke
    # checks remain authoritative and run afterwards without an exception.
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
