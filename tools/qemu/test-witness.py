"""Windows-only synthetic child-protocol checks. No VM or real media."""

import contextlib
import ctypes
from ctypes import wintypes
import json
import msvcrt
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time

FRAME = struct.Struct("<4sBBBB16sIBBBxQ8x")
NONCE = bytes(range(1, 17))
INIT, REQUEST, GRANT, ADOPT, ACK, STARTUP, TERMINAL = range(1, 8)
PAUSED, INFO, CREATE = range(1, 4)
IMAGE, GOLDEN, OUTPUT = range(3, 6)
READ_RIGHTS = 0x00100081  # SYNCHRONIZE | FILE_READ_ATTRIBUTES | FILE_READ_DATA
WRITE_RIGHTS = READ_RIGHTS | 2  # FILE_WRITE_DATA
FILE_SHARE_READ = 1
CREATE_NEW, OPEN_EXISTING = 1, 3
FILE_ATTRIBUTE_NORMAL = 0x80
INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
TIMEOUT = 10
K32 = ctypes.WinDLL("kernel32", use_last_error=True)


def bind(name, result, *args):
    fn = getattr(K32, name)
    fn.restype = result
    fn.argtypes = args
    return fn


H = wintypes.HANDLE
D = wintypes.DWORD
B = wintypes.BOOL
create_file = bind("CreateFileW", H, wintypes.LPCWSTR, D, D, H, D, D, H)
dup_handle = bind("DuplicateHandle", B, H, H, H, ctypes.POINTER(H), D, B, D)
current_process = bind("GetCurrentProcess", H)
close_handle = bind("CloseHandle", B, H)
peek = bind("PeekNamedPipe", B, H, H, D, H, ctypes.POINTER(D), H)
info = bind("GetFileInformationByHandle", B, H, H)


def frame(kind, stage, sequence=0, role=0, site=0, write=0, handle=0, nonce=NONCE):
    return FRAME.pack(b"ARXW", 1, 1, kind, stage, nonce, sequence, role, site,
                      write, handle)


def duplicate(source_process, handle, target_process, rights):
    result = H()
    if not dup_handle(source_process, handle, target_process, ctypes.byref(result),
                      rights, False, 0):
        raise ctypes.WinError(ctypes.get_last_error())
    return result.value


def file_open(path, rights, disposition):
    result = create_file(str(path), rights, FILE_SHARE_READ, None, disposition,
                         FILE_ATTRIBUTE_NORMAL, None)
    if result == INVALID_HANDLE_VALUE:
        raise ctypes.WinError(ctypes.get_last_error())
    return result


def identity(handle):
    # BY_HANDLE_FILE_INFORMATION contains thirteen consecutive DWORD values.
    fields = (D * 13)()
    if not info(handle, ctypes.byref(fields)):
        raise ctypes.WinError(ctypes.get_last_error())
    return fields[7], fields[11], fields[12]


class Child:
    def __init__(self, executable, argv):
        self.rx, child_tx = os.pipe()
        child_rx, self.tx = os.pipe()
        handles = [msvcrt.get_osfhandle(fd) for fd in (child_rx, child_tx)]
        for handle in handles:
            os.set_handle_inheritable(handle, True)
        startup = subprocess.STARTUPINFO(lpAttributeList={"handle_list": handles})
        environment = {
            "SystemRoot": os.environ["SystemRoot"],
            "ARX_WITNESS_RX": f"{handles[0]:x}",
            "ARX_WITNESS_TX": f"{handles[1]:x}",
        }
        try:
            self.proc = subprocess.Popen(
                [executable, *argv], stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                startupinfo=startup, close_fds=True, env=environment,
            )
        except BaseException:
            os.close(self.rx)
            os.close(self.tx)
            raise
        finally:
            os.close(child_rx)
            os.close(child_tx)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        if self.proc.poll() is None:
            self.proc.kill()
        self.proc.communicate(timeout=TIMEOUT)
        os.close(self.rx)
        os.close(self.tx)

    def send(self, message):
        if os.write(self.tx, message) != len(message):
            raise RuntimeError("short fixture write")

    def receive(self):
        available = D()
        pipe = msvcrt.get_osfhandle(self.rx)
        deadline = time.monotonic() + TIMEOUT
        while time.monotonic() < deadline:
            if not peek(pipe, None, 0, None, ctypes.byref(available), None):
                raise RuntimeError("witness channel closed before a complete frame")
            if available.value >= FRAME.size:
                return os.read(self.rx, FRAME.size)
            time.sleep(0.01)
        raise RuntimeError("witness frame deadline")

    def expect(self, message):
        if self.receive() != message:
            raise AssertionError("unexpected witness frame")

    def complete(self, status):
        stdout, _ = self.proc.communicate(timeout=TIMEOUT)
        if self.proc.returncode != status:
            raise AssertionError(f"child exit {self.proc.returncode}, expected {status}")
        return stdout

    def grant(self, stage, sequence, role, handle, *, write=False):
        flags = WRITE_RIGHTS if write else READ_RIGHTS
        self.expect(frame(REQUEST, stage, sequence, role, 3, write))
        child_handle = duplicate(current_process(), handle, int(self.proc._handle), flags)
        self.send(frame(GRANT, stage, sequence, role, 3, write, child_handle))
        self.expect(frame(ADOPT, stage, sequence, role, 3, write, child_handle))
        back = duplicate(int(self.proc._handle), child_handle, current_process(), READ_RIGHTS)
        try:
            if identity(back) != identity(handle):
                raise AssertionError("adopted a different file")
        finally:
            close_handle(back)


def assert_writer_refused(path):
    try:
        writer = file_open(path, WRITE_RIGHTS, OPEN_EXISTING)
    except OSError as error:
        if error.winerror != 32:  # sharing violation
            raise
    else:
        close_handle(writer)
        raise AssertionError("independent writer obtained access")


def create(image_tool, output, backing=None):
    args = ["create", "-f", "qcow2", "-o",
            "compat=1.1,cluster_size=65536,lazy_refcounts=off"]
    if backing is not None:
        args += ["-F", "qcow2", "-b", backing]
    with contextlib.ExitStack() as stack:
        # Keep the same create object, with its sharing guard, under a
        # read-only duplicate after the writer is handed off.
        writer = file_open(output, WRITE_RIGHTS, CREATE_NEW)
        writer_cleanup = stack.enter_context(contextlib.ExitStack())
        writer_cleanup.callback(close_handle, writer)
        reader = duplicate(current_process(), writer, current_process(), READ_RIGHTS)
        stack.callback(close_handle, reader)
        assert_writer_refused(output)
        child = stack.enter_context(Child(image_tool, [*args, output, "1048576"]))
        child.send(frame(INIT, CREATE))
        seq = 1
        if backing is not None:
            golden = file_open(backing, READ_RIGHTS, OPEN_EXISTING)
            stack.callback(close_handle, golden)
            child.grant(CREATE, seq, GOLDEN, golden)
            child.send(frame(ACK, CREATE, seq, GOLDEN, 3))
            seq += 1
        child.grant(CREATE, seq, OUTPUT, writer, write=True)
        writer_cleanup.close()
        assert_writer_refused(output)
        child.send(frame(ACK, CREATE, seq, OUTPUT, 3, 1))
        child.expect(frame(TERMINAL, CREATE, seq + 1))
        assert_writer_refused(output)
        child.send(frame(ACK, CREATE, seq + 1))
        child.complete(0)
        assert_writer_refused(output)
        duplicate_reader = duplicate(current_process(), reader, current_process(), READ_RIGHTS)
        with os.fdopen(msvcrt.open_osfhandle(duplicate_reader, os.O_RDONLY | os.O_BINARY), "rb") as file:
            file.seek(0)
            if file.read(4) != b"QFI\xfb":
                raise AssertionError("output header unavailable through retained object")


def inspect(image_tool, image):
    args = ["info", "--output=json", "-f", "qcow2", image]
    handle = file_open(image, READ_RIGHTS, OPEN_EXISTING)
    try:
        with Child(image_tool, args) as child:
            child.send(frame(INIT, INFO))
            child.grant(INFO, 1, IMAGE, handle)
            child.send(frame(ACK, INFO, 1, IMAGE, 3))
            child.expect(frame(TERMINAL, INFO, 2))
            child.send(frame(ACK, INFO, 2))
            result = json.loads(child.complete(0))
            if result["format"] != "qcow2" or result["virtual-size"] != 1048576:
                raise AssertionError("unexpected inspection result")
        for invalid in (
            frame(ACK, INFO, 0, IMAGE, 3),
            frame(ACK, INFO, 1, GOLDEN, 3),
            frame(ACK, INFO, 1, IMAGE, 3, nonce=b"X" * 16),
        ):
            with Child(image_tool, args) as child:
                child.send(frame(INIT, INFO))
                child.grant(INFO, 1, IMAGE, handle)
                child.send(invalid)
                child.complete(125)
        with Child(image_tool, args) as child:
            child.send(frame(INIT, INFO))
            child.expect(frame(REQUEST, INFO, 1, IMAGE, 3))
            child.send(frame(GRANT, INFO, 1, IMAGE, 3))  # no grant
            child.complete(125)
    finally:
        close_handle(handle)


def main():
    root = Path(sys.argv[1]).resolve(strict=True)
    image_tool = root / "qemu-img.exe"
    system = root / "qemu-system-x86_64.exe"
    # Refusal happens before WHPX or machine initialization.
    with Child(system, ["-version"]) as child:
        child.complete(125)
    with tempfile.TemporaryDirectory(prefix="arx-witness-fixtures-") as tmp:
        tmp = Path(tmp)
        golden = tmp / "golden.qcow2"
        overlay = tmp / "overlay.qcow2"
        create(image_tool, golden)
        inspect(image_tool, golden)
        create(image_tool, overlay, golden)
        inspect(image_tool, overlay)
    print("Witness framing and synthetic qcow2 create/info checks passed.")


if __name__ == "__main__":
    main()
