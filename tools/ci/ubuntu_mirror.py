"""Prepare URI-only APT source edits for disposable GitHub Ubuntu runners."""
import os
from pathlib import Path
import re
import stat
import sys
import tempfile
from urllib.parse import urlsplit

MIRROR = "https://mirrors.edge.kernel.org/ubuntu"
HOSTS = {"azure.archive.ubuntu.com", "archive.ubuntu.com", "security.ubuntu.com"}
KEYRING = "/usr/share/keyrings/ubuntu-archive-keyring.gpg"
MIRROR_LIST = "mirror+file:/etc/apt/apt-mirrors.txt"
MAX_BYTES = 65536
MAX_FILES = 32
MAX_MIRROR_LIST = 4096


class Refusal(Exception):
    pass


def replacement(uri, indirect=False):
    if uri == MIRROR_LIST:
        if not indirect:
            raise Refusal()
        # Keep the indirect URI; the validated list gains the Kernel entry and
        # APT's mirror method falls back to the original Ubuntu entries.
        return MIRROR_LIST
    if uri.rstrip("/") == MIRROR:
        return MIRROR
    parts = urlsplit(uri)
    if parts.hostname not in HOSTS:
        if parts.hostname and parts.hostname.endswith(".ubuntu.com"):
            raise Refusal()
        return None
    if (parts.scheme not in ("http", "https") or parts.netloc != parts.hostname
            or parts.path not in ("/ubuntu", "/ubuntu/") or parts.query or parts.fragment):
        raise Refusal()
    return MIRROR


def trust(options):
    for key, value in re.findall(r"([a-z-]+)\s*=\s*([^\s\]]+)", options, re.I):
        if key.lower() in ("trusted", "allow-insecure", "allow-weak", "allow-downgrade-to-insecure"):
            if value.lower() not in ("no", "false", "0"):
                raise Refusal()
        if key.lower() == "check-valid-until" and value.lower() not in ("yes", "true", "1"):
            raise Refusal()
    keys = re.findall(r"signed-by\s*=\s*([^\s\]]+)", options, re.I)
    if keys and keys != [KEYRING]:
        raise Refusal()


def rewrite_list(text, indirect=False):
    lines = []
    for line in text.splitlines(keepends=True):
        if not line.strip() or line.lstrip().startswith("#"):
            lines.append(line)
            continue
        match = re.match(r"[ \t]*deb(?:-src)?[ \t]+(?:\[[^\]\r\n]*\][ \t]+)?(?P<uri>[^ \t\r\n]+)", line)
        if not match:
            raise Refusal()
        new = replacement(match["uri"], indirect)
        if new is not None:
            trust(line[:match.start("uri")])
            if len(line[match.end("uri"):].split("#", 1)[0].split()) < 2:
                raise Refusal()
            line = line[:match.start("uri")] + new + line[match.end("uri"):]
        lines.append(line)
    return "".join(lines)


def rewrite_sources(text, indirect=False):
    blocks = re.split(r"(\n[ \t]*\n)", text)
    for index in range(0, len(blocks), 2):
        block = blocks[index]
        matches = list(re.finditer(r"(?mi)^URIs:[ \t]*(?P<uris>[^\r\n]*)", block))
        if not matches:
            continue
        uris = matches[0]["uris"].split()
        replacements = [replacement(uri, indirect) for uri in uris]
        if not any(value is not None for value in replacements):
            continue
        if len(matches) != 1 or not uris or any(value is None for value in replacements):
            raise Refusal()
        fields = {}
        for line in block.splitlines():
            if not line or line.startswith("#"):
                continue
            field = re.fullmatch(r"([A-Za-z][A-Za-z0-9-]*):[ \t]*(.*)", line)
            if not field or field[1].lower() in fields:
                raise Refusal()
            fields[field[1].lower()] = field[2]
        if (not fields.get("types") or not fields.get("suites") or not fields.get("components")
                or fields.get("signed-by", KEYRING) != KEYRING):
            raise Refusal()
        for key in ("trusted", "allow-insecure", "allow-weak", "allow-downgrade-to-insecure"):
            if fields.get(key, "no").lower() != "no":
                raise Refusal()
        if fields.get("check-valid-until", "yes").lower() != "yes":
            raise Refusal()
        match = matches[0]
        blocks[index] = block[:match.start("uris")] + " ".join(replacements) + block[match.end("uris"):]
    return "".join(blocks)


def rewrite(data, suffix, indirect=False):
    if len(data) > MAX_BYTES or b"\x00" in data:
        raise Refusal()
    try:
        text = data.decode("utf-8")
        if suffix == ".list":
            result = rewrite_list(text, indirect)
        elif suffix == ".sources":
            result = rewrite_sources(text, indirect)
        else:
            raise Refusal()
    except (UnicodeError, ValueError):
        raise Refusal() from None
    encoded = result.encode("utf-8")
    if len(encoded) > MAX_BYTES:
        raise Refusal()
    return encoded


def read_source(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                or info.st_mode & 0o022 or info.st_size > MAX_BYTES):
            raise Refusal()
        data = stream.read(MAX_BYTES + 1)
        if len(data) > MAX_BYTES:
            raise Refusal()
    return data, info


def runner_context():
    return (sys.platform == "linux" and os.geteuid() == 0
            and os.environ.get("GITHUB_ACTIONS") == "true"
            and os.environ.get("RUNNER_OS") == "Linux"
            and os.environ.get("CI") == "true"
            and os.environ.get("GITHUB_REPOSITORY") == "Dytschgo/arxcovo")


def rewrite_mirror_list(data):
    # Prefer the Kernel mirror (lowest priority is tried first) and keep every
    # validated Ubuntu entry, shifted by one, as an automatic APT fallback.
    if len(data) > MAX_MIRROR_LIST or b"\x00" in data:
        raise Refusal()
    try:
        lines = data.decode("utf-8").splitlines()
    except UnicodeError:
        raise Refusal() from None
    result = [f"{MIRROR}/\tpriority:1"]
    count = 0
    for line in lines:
        if not line.strip() or line.lstrip().startswith("#"):
            result.append(line)
            continue
        tokens = line.split()
        if (len(tokens) not in (1, 2)
                or (len(tokens) == 2 and not re.fullmatch(r"priority:[0-9]{1,6}", tokens[1]))):
            raise Refusal()
        try:
            if urlsplit(tokens[0]).hostname not in HOSTS or replacement(tokens[0]) != MIRROR:
                raise Refusal()
        except ValueError:
            raise Refusal() from None
        if len(tokens) == 2:
            line = f"{tokens[0]}\tpriority:{int(tokens[1][9:]) + 1}"
        result.append(line)
        count += 1
    if not 1 <= count <= 16:
        raise Refusal()
    encoded = ("\n".join(result) + "\n").encode("utf-8")
    if len(encoded) > MAX_MIRROR_LIST:
        raise Refusal()
    return encoded


def _apply(apt_root, read=read_source, inspect_dir=Path.lstat):
    # Private injection seam for owned-temp tests, never a CLI/config selector.
    roots = (apt_root, apt_root / "sources.list.d")
    for root in roots:
        info = inspect_dir(root)
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise Refusal()
    paths = sorted(p for p in roots[1].iterdir() if p.suffix in (".list", ".sources"))
    legacy = roots[0] / "sources.list"
    if legacy.exists() or legacy.is_symlink():
        paths.append(legacy)
    if not paths or len(paths) > MAX_FILES:
        raise Refusal()
    records = [(path, *read(path)) for path in paths]
    indirect = any(MIRROR_LIST.encode() in data for _, data, _ in records)
    plan = []
    for path, before, info in records:
        after = rewrite(before, path.suffix, indirect)
        if before != after:
            plan.append((path, before, after, info))
    if indirect:
        listing = apt_root / "apt-mirrors.txt"
        before, info = read(listing)
        plan.append((listing, before, rewrite_mirror_list(before), info))
    if not plan:
        raise Refusal()
    # Validate the complete plan before the first change. Only root can modify
    # these parents; noncooperating root writers remain outside this CI helper.
    for path, before, after, old in plan:
        current, info = read(path)
        if current != before or (info.st_dev, info.st_ino) != (old.st_dev, old.st_ino):
            raise Refusal()
        fd, staged = tempfile.mkstemp(prefix=".arxcovo-mirror-", dir=path.parent)
        try:
            with os.fdopen(fd, "wb") as stream:
                stream.write(after)
                os.fchmod(stream.fileno(), stat.S_IMODE(old.st_mode))
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(staged, path)
        finally:
            if os.path.exists(staged):
                os.unlink(staged)
    return len(plan)


def apply():
    # Intent guard only, not provenance proof. Never run this on an owner host.
    if not runner_context():
        raise Refusal()
    return _apply(Path("/etc/apt"))


def main():
    if len(sys.argv) != 1:
        print("ubuntu-mirror-arguments", file=sys.stderr)
        return 2
    try:
        count = apply()
    except (OSError, Refusal):
        print("ubuntu-mirror-refused", file=sys.stderr)
        return 1
    print(f"ubuntu-mirror-updated:{count}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
