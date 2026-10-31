"""Two real Iroh endpoints, authenticated WPF IPC, isolated identities/storage.

No clipboard, registry, LAN broadcast, production profile or system shortcuts.
Usage: python check-wpf-network.py <backend.exe> <report-directory>
"""
import json
import os
from pathlib import Path
import secrets
import struct
import subprocess
import sys
import time


class Backend:
    def __init__(self, executable, directory):
        self.data = directory
        directory.mkdir(parents=True)
        settings = dict(notifications_enabled=False, progress_bar_enabled=True,
                        sync_enabled=False, sync_shortcut="", history_shortcut="",
                        history_limit=100, enabled_peers={}, language="zh-CN",
                        connection_mode="iroh_only")
        (directory / "config-v2.json").write_text(json.dumps(settings), encoding="utf-8")
        self.pipe = "TailSync.Wpf." + secrets.token_hex(16)
        self.secret = secrets.token_hex(32)
        self.counter = 0
        env = os.environ.copy()
        env.update(TAILSYNC_DATA_DIR=str(directory), TAILSYNC_V1_DATA_DIR=str(directory / "absent-legacy"),
                   TAILSYNC_WPF_SESSION_TOKEN=self.secret)
        self.process = subprocess.Popen([executable, "--wpf-backend", "--wpf-isolated", "--wpf-network-check",
                                         "--pipe=" + self.pipe, "--parent-pid=" + str(os.getpid())], env=env)

    def invoke(self, command, args=None):
        deadline = time.monotonic() + 35
        while True:
            try:
                stream = open("\\\\.\\pipe\\" + self.pipe, "r+b", buffering=0)
                break
            except OSError:
                if self.process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("Backend failed before named-pipe connection")
                time.sleep(.05)

        def exact(count):
            data = bytearray()
            while len(data) < count:
                part = stream.read(count - len(data))
                if not part:
                    raise EOFError("Truncated named-pipe frame")
                data.extend(part)
            return data

        self.counter += 1
        body = json.dumps(dict(id=self.counter, token=self.secret, command=command, args=args or {})).encode()
        with stream:
            stream.write(struct.pack("<I", len(body)) + body)
            length, = struct.unpack("<I", exact(4))
            assert 0 < length <= 1024 * 1024
            reply = json.loads(exact(length))
        assert reply["id"] == self.counter
        if not reply["ok"]:
            raise RuntimeError(command + ": " + json.dumps(reply.get("error"), ensure_ascii=False))
        return reply.get("result")

    def stop(self):
        if self.process.poll() is None:
            try:
                self.invoke("shutdown_wpf_backend")
                self.process.wait(timeout=10)
            except Exception:
                self.process.terminate()
                self.process.wait(timeout=10)


def run(executable, directory):
    directory.mkdir(parents=True, exist_ok=True)
    checks, nodes, failure, diagnostics = [], [], None, []

    def check(value, label):
        if not value:
            raise AssertionError(label)
        checks.append(label)
        print("PASS: " + label, flush=True)

    try:
        for name in ["a", "b"]:
            node = Backend(executable, directory / (name + "-" + secrets.token_hex(4)))
            nodes.append(node)
            hello = node.invoke("handshake")
            check(hello["isolated"] and hello["networkEnabled"] and hello["nativeUiContractVersion"] == 2,
                  name + ": explicit isolated network acceptance handshake")
            check(set(hello["connectionModes"]) == {"auto", "lan_only", "iroh_only", "tailscale_only"},
                  name + ": all four modes advertised by the real backend")
        a, b = nodes
        for mode in ["auto", "lan_only", "iroh_only", "tailscale_only", "iroh_only"]:
            committed = a.invoke("update_settings", {"settingsJson": json.dumps({"connection_mode": mode})})
            check(committed["connection_mode"] == mode and a.invoke("get_settings")["connection_mode"] == mode,
                  mode + ": setting commits and round-trips through WPF IPC")
        time.sleep(.5)
        invite = a.invoke("create_remote_pairing_invite")
        preview = b.invoke("inspect_remote_pairing_link", {"link": invite["link"]})
        check(bool(preview["endpoint_id"]) and 0 < preview["remaining_seconds"] <= 120,
              "real Iroh listener creates a valid short-lived invite")
        b.invoke("start_remote_pairing", {"link": invite["link"]})
        statuses = []
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            statuses = [node.invoke("get_pairing_status") for node in nodes]
            if all((status.get("peer") or {}).get("verification_code") for status in statuses):
                break
            time.sleep(.15)
        codes = [str((status.get("peer") or {}).get("verification_code", "")) for status in statuses]
        check(len(codes) == 2 and len(codes[0]) == 6 and codes[0].isascii() and codes[0].isdigit() and codes[0] == codes[1],
              "two real Iroh/Noise endpoints show the same six-digit verification code")
        a.invoke("confirm_pairing")
        first = a.invoke("get_pairing_status")
        check(first["phase"] in ["waiting_for_peer", "finalizing"], "one-sided confirmation never completes pairing")
        b.invoke("confirm_pairing")
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            statuses = [node.invoke("get_pairing_status") for node in nodes]
            if all(status["phase"] == "paired" for status in statuses):
                break
            time.sleep(.15)
        check(all(status["phase"] == "paired" for status in statuses), "both confirmations complete the real Iroh pairing")
        for label, node in zip(["a", "b"], nodes):
            settings = node.invoke("get_settings")
            check(bool(settings["trusted_peer_keys"]) and bool(settings["paired_peer_endpoints"]),
                  label + ": peer identity and Iroh endpoint persist through the real Core")
        # Keep one device's completed receipt while the other explicitly
        # forgets. An old receipt must not replace consent for the new SAS.
        # Swap creator/joiner to cover both inbound and outbound repair.
        for label, forgetter, creator, joiner in [("b", b, a, b), ("a", a, b, a)]:
            hostname = next(iter(forgetter.invoke("get_settings")["trusted_peer_keys"]))
            forgetter.invoke("forget_peer", {"hostname": hostname})
            check(not forgetter.invoke("get_settings")["trusted_peer_keys"],
                  label + ": explicit forget removes authoritative trust")
            invite = creator.invoke("create_remote_pairing_invite")
            joiner.invoke("start_remote_pairing", {"link": invite["link"]})
            deadline = time.monotonic() + 25
            while time.monotonic() < deadline:
                statuses = [node.invoke("get_pairing_status") for node in nodes]
                if all(status["phase"] == "verification" for status in statuses):
                    break
                if any(status.get("error") for status in statuses):
                    break
                time.sleep(.15)
            check(all(status["phase"] == "verification" for status in statuses),
                  label + ": one-sided forget permits a new code confirmation on both devices")
            codes = [(status.get("peer") or {}).get("verification_code") for status in statuses]
            check(len(codes[0]) == 6 and codes[0] == codes[1], label + ": repaired pairing codes match")
            check(all(0 < status["remaining_seconds"] <= 30 for status in statuses),
                  label + ": verification countdown agrees with the real session deadline")
            creator.invoke("confirm_pairing")
            joiner.invoke("confirm_pairing")
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                statuses = [node.invoke("get_pairing_status") for node in nodes]
                if all(status["phase"] == "paired" for status in statuses):
                    break
                if any(status.get("error") for status in statuses):
                    break
                time.sleep(.15)
            check(all(status["phase"] == "paired" for status in statuses),
                  label + ": fresh confirmations replace the old transaction and pair both devices")
        # Acceptance must still reject user clipboard and global-shortcut writes.
        try:
            a.invoke("restore_entry", {"id": 1})
        except RuntimeError:
            check(True, "network acceptance still rejects clipboard restoration")
        else:
            raise AssertionError("Network acceptance leaked clipboard integration")
    except Exception as error:
        failure = repr(error)
        print("FAIL: " + failure, flush=True)
        for node in nodes:
            try:
                status = node.invoke("get_pairing_status")
                detail = {key: value for key, value in status.items() if key != "peer"}
                diagnostics.append(detail)
                print("PAIRING STATUS: " + json.dumps(detail, ensure_ascii=False), flush=True)
            except Exception:
                pass
    finally:
        for node in nodes:
            node.stop()
    (directory / "network.json").write_text(json.dumps(dict(passed=failure is None, checks=checks, failure=failure, diagnostics=diagnostics), ensure_ascii=False, indent=2), encoding="utf-8")
    return 0 if failure is None else 1


if __name__ == "__main__":
    sys.exit(run(str(Path(sys.argv[1]).resolve()), Path(sys.argv[2]).resolve()))
