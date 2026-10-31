"""Check native backend ownership/early validation with fresh isolated storage."""
import ctypes
import json
import os
from pathlib import Path
import secrets
import struct
import subprocess
import sys
import time


def launch_then_abandon(executable, directory):
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    token = secrets.token_hex(32)
    pipe = "TailSync.Wpf." + secrets.token_hex(16)
    env["TAILSYNC_DATA_DIR"] = str(directory)
    env["TAILSYNC_WPF_SESSION_TOKEN"] = token
    process = subprocess.Popen([executable, "--wpf-backend", "--wpf-isolated", "--pipe=" + pipe, "--parent-pid=" + str(os.getpid())], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = time.monotonic() + 30
    while True:
        try:
            stream = open("\\\\.\\pipe\\" + pipe, "r+b", buffering=0)
            break
        except OSError:
            if process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("Backend did not become ready")
            time.sleep(.05)
    with stream:
        request = json.dumps({"id": 1, "token": token, "command": "handshake", "args": {}}).encode()
        stream.write(struct.pack("<I", len(request)) + request)
        size, = struct.unpack("<I", stream.read(4))
        response = json.loads(stream.read(size))
        if not response["ok"]:
            raise RuntimeError("Backend handshake failed")
    # The owner deliberately exits without sending a shutdown request.
    (directory / "owned-process.json").write_text(json.dumps({"pid": process.pid}), encoding="utf-8")


def check(executable, result_directory):
    executable = str(Path(executable).resolve())
    directory = Path(result_directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    run_id = secrets.token_hex(6)
    orphan_dir = directory / ("owner-exit-" + run_id)
    completed = subprocess.run([sys.executable, __file__, "--owner", executable, str(orphan_dir)], capture_output=True, timeout=40)
    checks = []
    failure = None
    try:
        if completed.returncode:
            raise AssertionError("Isolated owner process failed to prepare its backend")
        pid = json.loads((orphan_dir / "owned-process.json").read_text())["pid"]
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        kernel.WaitForSingleObject.restype = ctypes.c_uint32
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = kernel.OpenProcess(0x00100000, False, pid)
        if handle:
            try:
                if kernel.WaitForSingleObject(handle, 10000) != 0:
                    raise AssertionError("Backend survived its actual owner process")
            finally:
                kernel.CloseHandle(handle)
        checks.append("actual parent exit reaps authenticated backend without a shutdown request")

        for case in ["missing-secret", "wrong-parent"]:
            data = directory / (case + "-" + run_id)
            data.mkdir()
            env = os.environ.copy()
            env["TAILSYNC_DATA_DIR"] = str(data)
            env.pop("TAILSYNC_WPF_SESSION_TOKEN", None)
            if case != "missing-secret":
                env["TAILSYNC_WPF_SESSION_TOKEN"] = secrets.token_hex(32)
            arguments = [executable, "--wpf-backend", "--wpf-isolated", "--pipe=TailSync.Wpf." + secrets.token_hex(16)]
            if case == "wrong-parent":
                arguments.append("--parent-pid=0")
            rejected = subprocess.run(arguments, env=env, capture_output=True, timeout=10)
            if rejected.returncode != 1 or any(data.iterdir()):
                raise AssertionError(case + " accessed storage or unexpectedly started")
            checks.append(case + " rejects before database/key/identity initialization")
    except Exception as error:
        failure = repr(error)
    report = {"passed": failure is None, "checks": checks, "failure": failure}
    (directory / "backend-lifecycle.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report))
    return 0 if failure is None else 1


if __name__ == "__main__":
    if sys.argv[1] == "--owner":
        launch_then_abandon(sys.argv[2], sys.argv[3])
    else:
        raise SystemExit(check(sys.argv[1], sys.argv[2]))
