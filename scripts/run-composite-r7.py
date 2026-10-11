#!/usr/bin/env python3
"""R7 acceptance harness (repair plan §4 R7).

R7 is real-device work: a Windows machine, a second device, real source and target
applications, VoiceOver and IME. None of it can be executed by the local gate, and the
plan is explicit that host results must not stand in for Windows-native evidence.

What this harness does is make that phase **runnable and non-skippable**:

* every gate in the plan's R7 table is enumerated, so nothing is quietly dropped;
* PASS requires a structured observation for every scenario, candidate/package hashes,
  a complete sealed R6 run, and non-empty raw evidence;
* gates this tool must not automate (starting the full App, touching the user's general
  clipboard, driving a second device) are marked as such rather than silently skipped;
* `--emit` writes `native-acceptance.md` in the §8.1 shape, listing every gate with its
  independent PASS / FAIL / UNKNOWN and the scope the operator accepted.

Usage:
  python3 scripts/run-composite-r7.py <run_id> --status
  python3 scripts/run-composite-r7.py <run_id> --record mac-app --result PASS \
      --r6-run <sealed-r6-run> --evidence /path/to/observation.json
  python3 scripts/run-composite-r7.py <run_id> --emit
"""
from __future__ import annotations

import argparse
import datetime
import json
import importlib.util
import re
import time
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("evidence", ROOT / "scripts/composite-evidence.py")
ev = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ev)

# `mode` says who can produce the evidence:
#   auto   — this harness can run it on the current host
#   device — needs a real target machine or a second device
#   human  — needs a person to judge what an application actually pasted
GATES = [
    dict(id="mac-app", group="Mac完整App", mode="device",
         scenario="新包启动、采集、历史恢复、通知与故障",
         criteria="生产 daemon/GUI/helper 全部接通；不能只用包内 helper 替代",
         blocked_by="启动完整 App 会与用户正在运行的 daemon 竞争，且会触碰 general clipboard；"
                    "本工具不自动执行"),
    dict(id="windows-native", group="Windows原生", mode="device",
         scenario="MSVC、tauri dev、portable、NSIS 安装",
         criteria="helper 定位正确；真实剪贴板读写与错误提示通过",
         blocked_by="需要 Windows 主机"),
    dict(id="source-apps", group="来源应用", mode="human",
         scenario="Chrome、Word、Notes/可用富文本编辑器",
         criteria="实际源版本、format 集合和 resource 完整性有记录",
         blocked_by="需要真实应用产生剪贴板内容并由人确认 format 集合"),
    dict(id="target-apps", group="目标应用", mode="human",
         scenario="微信、Word、富文本编辑器、终端/记事本、图片消费者",
         criteria="依据目标接受格式核验文字/所有图片；不把纯文本目标不显示图判为失败",
         blocked_by="需要人判断目标应用实际粘贴出什么"),
    dict(id="two-way-sync", group="双向同步", mode="device",
         scenario="Mac→Win、Win→Mac，单图/两图/RTF/中文 emoji",
         criteria="一逻辑条目；无拆分覆盖、无重复回音；内容和终态一致",
         blocked_by="需要两台设备"),
    dict(id="intent-race", group="意图竞争", mode="device",
         scenario="A 复制后 B 同内容；慢传输中接收端再复制；双端交替 100 轮",
         criteria="新 revision 不吞；旧事件不覆盖新内容；双方不自激循环",
         blocked_by="需要两台设备"),
    dict(id="network", group="网络", mode="device",
         scenario="LAN、Tailscale、Iroh-only direct/可用 relay、重连/睡眠",
         criteria="无旧事件重放、ACK 丢失不再写；路由与耗时明确",
         blocked_by="需要真实网络路径与第二台设备"),
    dict(id="faults", group="故障", mode="device",
         scenario="helper 缺失/挂起/写后断回执、storage 满/不可用、撤销授权",
         criteria="结果诚实、历史可恢复、不阻塞其他操作、不晚写",
         blocked_by="本机支持证据仅覆盖部分 seam；完整 App 与真实设备仍须人工验证"),
    dict(id="rollback", group="回退", mode="device",
         scenario="v11 完整快照恢复、旧版本读取；升级后资料归档",
         criteria="外置 payload 真实可读，identity/trust/receipts 不倒退",
         blocked_by="本机支持证据仅覆盖部分 seam；完整 App 与真实设备仍须人工验证"),
    dict(id="accessibility", group="可访问性", mode="human",
         scenario="VoiceOver、仅键盘、IME、深浅色、高对比、缩放",
         criteria="菜单有名称，格式可选，焦点/反馈明确，两端无裁切",
         blocked_by="需要人使用辅助技术实际操作"),
]


# Local support is useful but cannot close a complete real-device gate.
AUTO_COMMANDS = {"faults": [["python3", "tools/repair-probes/native_fault_probe.py", "--json"]]}
CASES = {
    "mac-app": ["candidate-start", "capture", "history-restore", "notification", "fault-feedback"],
    "windows-native": ["msvc", "tauri-dev", "portable", "nsis-install", "clipboard-read-write", "error-feedback"],
    "source-apps": ["chrome", "word", "notes-or-rich-editor"],
    "target-apps": ["wechat", "word", "rich-editor", "plain-text", "image-consumer"],
    "two-way-sync": [f"{direction}-{content}" for direction in ("mac-to-win", "win-to-mac")
                     for content in ("one-image", "two-images", "rtf", "chinese-emoji")],
    "intent-race": ["same-content-new-revision", "copy-during-transfer", "alternating-100-rounds"],
    "network": ["lan", "tailscale", "iroh-direct", "iroh-relay", "reconnect", "sleep-wake", "lost-ack"],
    "faults": ["helper-missing", "helper-hang", "receipt-lost-after-write", "storage-full",
               "storage-unavailable", "revoke-authorization", "responsive-no-late-write"],
    "rollback": ["full-v11-snapshot", "installed-old-reader", "external-payloads",
                 "archive-v12", "retain-identity-trust-receipts"],
    "accessibility": [f"{platform}-{case}" for platform in ("mac", "windows")
                      for case in ("screen-reader", "keyboard", "ime", "light-dark", "high-contrast", "zoom")],
}
RESULTS = {"PASS", "FAIL", "UNKNOWN"}


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def run_path(run_id):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", run_id):
        raise ValueError("run id must be one directory name, without path separators")
    return ROOT / "artifacts/composite-repair" / run_id


def state_path(run):
    return run / "r7-state.json"


def load(run):
    if state_path(run).exists():
        state = {k: v for k, v in json.loads(state_path(run).read_text()).items() if not k.startswith("_")}
        if not isinstance(state.get("gates"), dict):
            raise ValueError("invalid R7 gate state")
        return state
    return dict(schema=2, run_id=run.name, created=now(), gates={})


def save(run, state):
    run.mkdir(parents=True, exist_ok=True)
    state["updated"] = now()
    clean = {k: v for k, v in state.items() if not k.startswith("_")}
    tmp = run / "r7-state.tmp"
    tmp.write_text(json.dumps(clean, indent=2, ensure_ascii=False) + "\n")
    tmp.replace(state_path(run))


def _sha256(path):
    return ev.digest(path)


def evidence_item(run, path):
    path = Path(path).resolve()
    if not path.is_file() or not path.read_bytes().strip():
        raise ValueError(f"evidence must be a nonempty regular file: {path}")
    return dict(path=str(path.relative_to(run.resolve())) if path.is_relative_to(run.resolve()) else str(path),
                sha256=_sha256(path), bytes=path.stat().st_size)


def binding(r6_run):
    path = run_path(r6_run)
    source = ev.fingerprint()
    ev.verify_r6(path, source=source)
    return dict(source_sha256=source["source_sha256"], r6_run=r6_run,
                r6_seal_sha256=_sha256(path / "evidence-index.json"))


def manifest_items(run, gate, path, bound):
    """Validate declared coverage and hash the raw observations, not just the manifest.

    Human observations remain attestations; this checks completeness and provenance,
    not whether a person truthfully described a screenshot or a paste operation.
    """
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    ev.require(data.get("schema") == 1 and data.get("gate") == gate["id"], "wrong observation schema/gate")
    for key in ("source_sha256", "r6_run"):
        ev.require(data.get(key) == bound[key], f"observation {key} differs from candidate")
    ev.require(bool(str(data.get("observer", "")).strip()), "observer is required")
    stamp = datetime.datetime.fromisoformat(data.get("recorded_at", "").replace("Z", "+00:00"))
    ev.require(stamp.tzinfo is not None, "recorded_at must include timezone")
    environment = data.get("environment", {})
    devices = environment.get("devices", [])
    ev.require(bool(devices) and all(all(str(d.get(k, "")).strip() for k in ("id", "os", "version")) for d in devices),
               "device identity and OS version are required")
    if gate["id"] in ("two-way-sync", "intent-race", "network"):
        ev.require(len({d["id"] for d in devices}) >= 2, "two distinct devices are required")
        ev.require({d["os"].lower() for d in devices} >= {"macos", "windows"}, "Mac and Windows devices are required")
    if gate["id"] == "windows-native":
        ev.require(any(d["os"].lower() == "windows" for d in devices), "Windows-native requires a Windows device")
    if gate["id"] in ("source-apps", "target-apps", "accessibility"):
        apps = environment.get("apps", [])
        ev.require(bool(apps) and all(a.get("name") and a.get("version") for a in apps), "app/assistive technology versions required")
    pieces = [evidence_item(run, path)]
    packages = data.get("packages", [])
    ev.require(bool(packages), "tested package hashes are required")
    for package in packages:
        artifact = path.parent / package["path"]
        ev.require(artifact.is_file() and _sha256(artifact) == package["sha256"], "tested package hash mismatch")
        pieces.append(evidence_item(run, artifact))
    cases = data.get("cases", [])
    ev.require(len(cases) == len(CASES[gate["id"]]) and {c["id"] for c in cases} == set(CASES[gate["id"]]),
               "missing, duplicate or unknown acceptance scenarios")
    for case in cases:
        ev.require(case.get("result") == "PASS", f"scenario {case['id']} is not PASS")
        ev.require(bool(str(case.get("expected", "")).strip()) and bool(str(case.get("observed", "")).strip()),
                   "expected and observed behavior are required per scenario")
        ev.require(bool(case.get("evidence")), "raw evidence required per scenario")
        pieces.extend(evidence_item(run, path.parent / name) for name in case["evidence"])
    return list({item["path"]: item for item in pieces}.values())


def attach(run, gate, result, evidence, note, *, scope="device", r6_run=None, bound=None):
    try:
        ev.require(result in RESULTS, "invalid result")
        usable = [evidence_item(run, p) for p in evidence]
        if result == "PASS":
            ev.require(bool(usable), f"{gate['id']} cannot be PASS without evidence")
            bound = binding(r6_run) if bound is None else bound
            if scope == "device":
                ev.require(len(evidence) == 1, "device PASS requires one observation JSON manifest")
                usable = manifest_items(run, gate, evidence[0], bound)
            else:
                ev.require(scope == "local-support", "invalid evidence scope")
        state = load(run)
        old = state["gates"].get(gate["id"])
        history = old.get("history", []) + [{k: v for k, v in old.items() if k != "history"}] if old else []
        # Local support updates never erase an existing human/device observation.
        record = dict(id=gate["id"], result=result, scope=scope, evidence=usable,
                      binding=bound, note=note, recorded=now(), history=history)
        if scope == "device" and old and old.get("scope") == "local-support":
            record["local_support"] = old
        if scope == "local-support" and old and old.get("scope") == "device":
            old["local_support"] = record
        else:
            state["gates"][gate["id"]] = record
        save(run, state)
        print(f"recorded {gate['id']} = {result} ({scope})")
        return True
    except (ValueError, KeyError, TypeError, OSError) as error:
        print(f"REFUSED: {error}")
        return False


def faults_passed(data):
    return all(data.get(k, {}).get("satisfied") is True for k in
               ("RR-03-contract", "RR-04-contract", "R6-before-spawn", "R6-output-exceeded")) \
        and data.get("RR-07", {}).get("classified_as_file_selection") is True


def run_auto(run, gate, r6_run):
    evidence_dir = run / "evidence"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    tag = str(time.time_ns())
    if gate["id"] == "rollback":
        try:
            bound = binding(r6_run)
            proof = run_path(r6_run) / "rollback-proof.json"
            return attach(run, gate, "PASS", [proof],
                          "R6 current writer/baseline reader with synthetic authority; installed switching remains UNKNOWN",
                          scope="local-support", bound=bound)
        except (ValueError, KeyError, TypeError, OSError) as error:
            return attach(run, gate, "UNKNOWN", [], str(error), scope="local-support")
    pieces, ok = [], True
    source = ev.fingerprint()["source_sha256"]
    for index, command in enumerate(AUTO_COMMANDS.get(gate["id"], []), 1):
        completed = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
        path = evidence_dir / f"{gate['id']}.{tag}.{index}.json"
        path.write_text(json.dumps(dict(command=command, exit_code=completed.returncode,
                                        source_sha256=source, stdout=completed.stdout,
                                        stderr=completed.stderr), indent=2) + "\n")
        pieces.append(path)
        try:
            ok = ok and completed.returncode == 0 and faults_passed(json.loads(completed.stdout))
        except (ValueError, TypeError, AttributeError):
            ok = False
    ok = ok and bool(pieces) and ev.fingerprint()["source_sha256"] == source
    # Preserve real failures even if R6 is absent. Successful support requires a valid R6.
    if not ok:
        attach(run, gate, "FAIL", pieces, "probe exit/assertion failure", scope="local-support")
        return False
    try:
        bound = binding(r6_run)
    except (ValueError, KeyError, TypeError, OSError) as error:
        attach(run, gate, "UNKNOWN", pieces, str(error), scope="local-support")
        return False
    return attach(run, gate, "PASS", pieces, "private board only; storage, revoke and full App behavior remain UNKNOWN",
                  scope="local-support", bound=bound)


def record_state(state, recorded):
    if not isinstance(recorded, dict) or recorded.get("result") not in RESULTS:
        return "INVALID"
    result = recorded["result"]
    if result == "PASS":
        bound = recorded.get("binding")
        if not isinstance(bound, dict) or not recorded.get("evidence"):
            return "STALE"
        try:
            if "_source" not in state:
                state["_source"] = ev.fingerprint()["source_sha256"]
            if bound.get("source_sha256") != state["_source"]:
                return "STALE"
            key = json.dumps(bound, sort_keys=True)
            cache = state.setdefault("_verified_bindings", {})
            if key not in cache:
                cache[key] = binding(bound["r6_run"]) == bound
            if not cache[key]:
                return "STALE"
        except (ValueError, KeyError, TypeError, OSError):
            return "STALE"
    for item in recorded.get("evidence", []):
        try:
            path = Path(item["path"])
            if not path.is_absolute():
                path = Path(state["_run"]) / path
            if not path.is_file() or _sha256(path) != item["sha256"]:
                return "STALE"
        except (ValueError, KeyError, TypeError, OSError):
            return "STALE"
    return result


def gate_state(state, gate):
    recorded = state["gates"].get(gate["id"])
    if not recorded:
        return "UNKNOWN"
    result = record_state(state, recorded)
    if result == "PASS" and recorded.get("scope") != "device":
        return "UNKNOWN"
    if result == "PASS":
        try:
            if recorded.get("scope") != "device":
                return "INVALID"
            path = Path(recorded["evidence"][0]["path"])
            if not path.is_absolute():
                path = Path(state["_run"]) / path
            items = manifest_items(Path(state["_run"]), gate, path, recorded["binding"])
            if items != recorded["evidence"]:
                return "INVALID"
        except (ValueError, KeyError, TypeError, OSError):
            return "INVALID"
    return result


def emit(run, state):
    statuses = {g["id"]: gate_state(state, g) for g in GATES}
    counts = {name: list(statuses.values()).count(name) for name in ("PASS", "FAIL", "UNKNOWN", "STALE", "INVALID")}
    lines = ["# Composite clipboard R7 native acceptance", "", f"Run: `{state['run_id']}` ｜ generated {now()}", "",
             "Each complete device gate is independent. Local support never closes a device gate. "
             "Human evidence is an operator attestation; hashes verify provenance and later changes, not truthfulness.", "",
             "| 验收组 | 场景 | 通过标准 | 状态 | 本机支持证据 |", "|---|---|---|---|---|"]
    for gate in GATES:
        record = state["gates"].get(gate["id"], {})
        support = record if record.get("scope") == "local-support" else record.get("local_support")
        local = record_state(state, support) if support else "UNKNOWN"
        lines.append(f"| {gate['group']} | {gate['scenario']} | {gate['criteria']} | **{statuses[gate['id']]}** | {local} |")
    lines += ["", "## Evidence and remaining scenarios", ""]
    for gate in GATES:
        record = state["gates"].get(gate["id"], {})
        lines += [f"- **{gate['id']}**: {', '.join(CASES[gate['id']])}", f"  {gate['blocked_by']}"]
        if record.get("note"):
            lines.append(f"  Note: {record['note']}")
        for item in record.get("evidence", []):
            lines.append(f"  Evidence: `{item['path']}` SHA256 `{item['sha256']}`")
    lines += ["", "## Verdict", "", " ｜ ".join(f"{k} {v}" for k, v in counts.items()), ""]
    if counts["PASS"] != len(GATES):
        lines.append("**Release NO-GO.** Every promised device scenario must pass. UNKNOWN, STALE, INVALID and FAIL block acceptance.")
    else:
        lines.append("All R7 scenarios have current operator evidence. Candidate acceptance can proceed to independent release review; this does not publish a release.")
    run.mkdir(parents=True, exist_ok=True)
    out = run / "native-acceptance.md"
    out.write_text("\n".join(lines) + "\n")
    return out


def template(gate, r6_run):
    return dict(schema=1, gate=gate["id"], source_sha256=ev.fingerprint()["source_sha256"],
                r6_run=r6_run, observer="", recorded_at=now(),
                environment=dict(devices=[dict(id="", os="", version="")], apps=[]),
                packages=[dict(path="", sha256="")],
                cases=[dict(id=c, result="UNKNOWN", expected=gate["criteria"], observed="", evidence=[])
                       for c in CASES[gate["id"]]])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("run_id")
    parser.add_argument("--status", action="store_true")
    parser.add_argument("--run-auto", action="store_true")
    parser.add_argument("--record", choices=[g["id"] for g in GATES])
    parser.add_argument("--template", choices=[g["id"] for g in GATES])
    parser.add_argument("--result", choices=sorted(RESULTS))
    parser.add_argument("--evidence", action="append", default=[])
    parser.add_argument("--note", default="")
    parser.add_argument("--emit", action="store_true")
    parser.add_argument("--r6-run", help="explicit complete, sealed candidate R6 run")
    args = parser.parse_args()
    try:
        run = run_path(args.run_id)
        ev.require(sum(bool(v) for v in (args.record, args.template, args.run_auto)) <= 1, "choose one record/template/auto action")
        ev.require(not (args.result or args.evidence) or bool(args.record), "--result/--evidence require --record")
        ev.require(not (args.run_auto or args.template or (args.record and args.result == "PASS")) or bool(args.r6_run), "--r6-run is required")
        if args.template:
            gate = next(g for g in GATES if g["id"] == args.template)
            print(json.dumps(template(gate, args.r6_run), indent=2, ensure_ascii=False))
            return 0
        ok = True
        if args.record:
            ev.require(args.result is not None, "--record requires --result")
            gate = next(g for g in GATES if g["id"] == args.record)
            ok = attach(run, gate, args.result, [Path(p) for p in args.evidence], args.note, r6_run=args.r6_run)
        if args.run_auto:
            for gate_id in ("faults", "rollback"):
                ok = run_auto(run, next(g for g in GATES if g["id"] == gate_id), args.r6_run) and ok
        state = load(run)
        state["_run"] = str(run)
        if args.status or not (args.emit or args.record or args.run_auto):
            for gate in GATES:
                print(f"{gate_state(state, gate):8} {gate['id']:16} {gate['group']}")
        if args.emit:
            print(f"wrote {emit(run, state)}")
            ok = all(gate_state(state, g) == "PASS" for g in GATES) and ok
        return 0 if ok else 1
    except (ValueError, KeyError, TypeError, OSError) as error:
        print(f"REFUSED: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
