"""Exercise the real named-pipe backend with isolated data and no OS integrations.

Usage: python check-wpf-backend.py <tailsync.exe> <absolute-result-directory>
No production data, clipboard, network, startup settings, or theme store is used.
"""
import json
import os
from pathlib import Path
import secrets
import struct
import subprocess
import sys
import time


def run(executable, directory):
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    data = directory / ("data-" + secrets.token_hex(6))
    data.mkdir()
    pipe = "TailSync.Wpf." + secrets.token_hex(16)
    token = secrets.token_hex(32)
    env = os.environ.copy()
    env["TAILSYNC_WPF_SESSION_TOKEN"] = token
    env["TAILSYNC_DATA_DIR"] = str(data)
    process = subprocess.Popen([str(Path(executable).resolve()), "--wpf-backend", "--wpf-isolated", "--pipe=" + pipe, "--parent-pid=" + str(os.getpid())], env=env)
    checks = []
    counter = 0

    def check(value, label):
        if not value:
            raise AssertionError(label)
        checks.append(label)

    def connect():
        deadline = time.monotonic() + 30
        while True:
            try:
                return open("\\\\.\\pipe\\" + pipe, "r+b", buffering=0)
            except OSError:
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("Backend did not open its named pipe")
                time.sleep(.05)

    def exact(stream, count):
        chunks = bytearray()
        while len(chunks) < count:
            part = stream.read(count - len(chunks))
            if not part:
                raise EOFError("Truncated backend frame")
            chunks.extend(part)
        return bytes(chunks)

    def invoke(command, args=None, auth=token, expect_success=True, raw=False):
        nonlocal counter
        counter += 1
        request = json.dumps({"id": counter, "token": auth, "command": command, "args": args or {}}).encode()
        with connect() as stream:
            stream.write(struct.pack("<I", len(request)) + request)
            size, = struct.unpack("<I", exact(stream, 4))
            check(0 < size <= 1024 * 1024, command + ": bounded JSON response")
            result = json.loads(exact(stream, size))
            check(result["id"] == counter, command + ": correlates request id")
            binary_size = result.get("binaryLength", 0)
            if binary_size:
                result["binary"] = exact(stream, binary_size)
            if expect_success:
                check(result["ok"], command + ": succeeds")
                return result if raw else result.get("result")
            check(not result["ok"], command + ": rejects invalid request")
            return result["error"]

    failure = None
    try:
        handshake = invoke("handshake")
        check(handshake["protocolVersion"] == 1 and handshake["frontend"] == "wpf" and handshake["isolated"], "real isolated backend handshake")
        for command, args in [("get_preview", {"id": 8000000001}), ("get_image_data", {"id": 8000000008}), ("delete_entry", {"id": 8000000001}), ("restore_file_batch", {"batchId": "tailsync-demo-batch"})]:
            error = invoke(command, args, expect_success=False)
            check(error["code"] == "invalid_argument", command + ": virtual guide IDs rejected before database/clipboard operations")
        check(invoke("onboarding_should_show"), "new installation offers native onboarding")
        invoke("dismiss_onboarding")
        check(not invoke("onboarding_should_show") and not invoke("onboarding_suppressed"), "ordinary dismissal retains the restart entry")
        invoke("suppress_onboarding")
        check(invoke("onboarding_suppressed"), "permanent suppression is served over the WPF transport")
        error = invoke("get_settings", auth="0" * 64, expect_success=False)
        check(error["code"] == "unauthorized", "wrong secret never executes a command")
        invoke("does_not_exist", expect_success=False)
        settings = invoke("get_settings")
        check(settings["history_limit"] > 0, "settings load from real core")
        invoke("update_settings", {"settingsJson": json.dumps({"history_limit": 130})})
        check(invoke("get_settings")["history_limit"] == 130, "editable settings patch commits")
        invoke("update_settings", {"settingsJson": json.dumps({"trusted_peer_keys": {"injected": "key"}})}, expect_success=False)
        history = invoke("get_history_page", {"limit": 30, "offset": 0, "collection": "all"})
        check(history["entries"] == [] and history["total"] == 0, "fresh isolated database is empty")
        seeded = invoke("seed_wpf_test_history", {"count": 65})
        check(seeded["count"] == 65, "synthetic records pass through encrypted production database")
        history = invoke("get_history_page", {"limit": 30, "offset": 0, "collection": "all"})
        check(len(history["entries"]) == 30 and history["total"] == 65 and history["has_more"], "server paginates the first 30 records")
        last_page = invoke("get_history_page", {"limit": 30, "offset": 60, "collection": "all"})
        check(len(last_page["entries"]) == 5 and not last_page["has_more"], "server returns the final partial page")
        entry = history["entries"][0]
        preview = invoke("get_preview", {"id": entry["id"]}, raw=True)
        check(preview.get("binary", b"").startswith(b"TSPV"), "text preview uses raw binary transport")
        image = next(entry for entry in history["entries"] if entry["type"] == "image")
        thumbnail = invoke("get_image_data", {"id": image["id"]})
        check(thumbnail["thumbnail_width"] == 2 and thumbnail["thumbnail_height"] == 2, "real packed image is decoded as a bounded thumbnail")
        invoke("set_history_favorite", {"id": entry["id"], "favorite": True})
        favorites = invoke("get_history_page", {"limit": 30, "offset": 0, "collection": "favorites"})
        check(favorites["total"] == 1, "favorite mutation reaches its own collection")
        invoke("delete_entry", {"id": entry["id"]}, expect_success=False)
        invoke("get_preview", {"id": 999999}, expect_success=False)
        invoke("restore_entry", {"id": 999999}, expect_success=False)
        themes = invoke("list_themes_v2")
        check(len(themes) >= 5, "built-in Theme V2 descriptors available")
        for theme in themes:
            if theme["source"] == "builtin":
                resolved = invoke("resolve_theme", {"themeId": theme["id"], "mode": "light", "platform": "windows", "highContrast": False})
                check(bool(resolved["tokens"]), "resolved Windows tokens: " + theme["id"])
        theme_settings = invoke("get_local_theme_settings")
        selected_theme = next(theme["id"] for theme in themes if ":flux@" in theme["id"])
        theme_settings["activeThemeId"] = selected_theme
        invoke("set_local_theme_settings", {"settings": theme_settings})
        check(invoke("get_local_theme_settings")["activeThemeId"] == selected_theme, "local theme selection persists in isolated store")
        events = invoke("wait_wpf_events", {"sinceRevision": 0, "waitMs": 50})
        check(any(event["action"] == "refresh" for event in events["events"]), "backend broadcasts native settings/theme invalidation")
        invoke("get_storage_status")
        invoke("get_pairing_status")
        invoke("get_peers")
        invoke("get_update_status")
        invoke("install_update", expect_success=False)
        invoke("open_settings_window")
        events = invoke("wait_wpf_events", {"sinceRevision": events["revision"], "waitMs": 50})
        check(any(event["action"] == "open" and event["window"] == "settings" for event in events["events"]), "native window action routes to WPF without opening a WebView")
        invoke("clear_history")
        remaining = invoke("get_history_page", {"limit": 30, "offset": 0, "collection": "all"})
        check(remaining["total"] == 1, "clear history preserves the protected favorite")
        invoke("delete_favorite_entry", {"id": entry["id"]})
        check(invoke("get_history_page", {"limit": 30, "offset": 0, "collection": "all"})["total"] == 0, "explicit favorite deletion removes the protected record")
        invoke("shutdown_wpf_backend")
        check(process.wait(timeout=10) == 0, "authenticated shutdown exits cleanly")
    except Exception as error:
        failure = repr(error)
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        result = {"passed": failure is None, "checks": checks, "failure": failure, "backendExitCode": process.returncode, "isolatedDataDirectory": str(data)}
        (directory / "backend-checks.json").write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps({"passed": failure is None, "checkCount": len(checks), "failure": failure}, ensure_ascii=False))
    return 0 if failure is None else 1


if __name__ == "__main__":
    raise SystemExit(run(sys.argv[1], sys.argv[2]))
