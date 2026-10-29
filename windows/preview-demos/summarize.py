#!/usr/bin/env python3
"""Summarize complete demo runs; never substitute zero for unsupported memory counters."""
import argparse
import csv
import json
import math
from pathlib import Path


def percentile(values, fraction):
    if not values:
        return None
    data = sorted(values)
    # Nearest-rank estimator. Small sample counts are explicitly included in the report.
    return data[max(0, math.ceil(len(data) * fraction) - 1)]


def summarize(path):
    events = []
    malformed = False
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            malformed = True
    env = next((e["data"] for e in events if e["kind"] == "environment"), {})
    resources = [e for e in events if e["kind"] == "resource"]
    first = [e["data"]["first_from_open_request_ms"] for e in events if e["kind"] == "frame_assigned" and e["data"]["reason"] == "first_page"]
    render = [e["data"]["duration_ms"] for e in events if e["kind"] == "engine_render"]
    page = [e["data"]["duration_ms"] for e in events if e["kind"] == "frame_assigned" and e["data"]["reason"] == "benchmark_page"]
    def peak(key):
        values = [e["data"][key] for e in resources if e["data"].get(key) is not None]
        return max(values) / 1048576 if values else None
    idle = []
    baseline = []
    phase = None
    for e in events:
        if e["kind"] in ("baseline_begin", "idle_after_close_begin"):
            phase = e["kind"]
        elif e["kind"] == "resource" and phase:
            (baseline if phase == "baseline_begin" else idle).append(e["data"]["private_bytes"])
        elif e["kind"] in ("baseline_end", "idle_after_close_end"):
            phase = None
    document = next((e["data"] for e in events if e["kind"] == "document_open_begin"), {})
    complete = any(e["kind"] == "benchmark_complete" for e in events)
    errors = [e for e in events if e["kind"] in ("error", "sample_error", "shutdown_error")]
    shutdown = next((e["data"] for e in reversed(events) if e["kind"] == "shutdown"), {})
    status = "complete" if complete and not errors and not malformed and shutdown.get("exit_code") == 0 else "incomplete_or_failed"
    return {
        "run": str(path.parent.name), "status": status, "engine": env.get("engine"), "version": env.get("engine_version"),
        "file": document.get("name"), "sha256": document.get("sha256"), "cycles_measured": len(first),
        "first_open_frame_assigned_ms": first[0] if first else None,
        "warm_reopen_frame_assigned_p95_ms": percentile(first[1:], .95),
        "page_frame_assigned_p95_ms": percentile(page, .95), "page_samples": len(page),
        "engine_render_p50_ms": percentile(render, .5), "engine_render_p95_ms": percentile(render, .95), "render_samples": len(render),
        "peak_private_mib": peak("private_bytes"), "peak_working_set_mib": peak("working_set_bytes"),
        "peak_private_working_set_mib": peak("private_working_set_bytes"),
        "baseline_last_private_mib": baseline[-1] / 1048576 if baseline else None,
        "closed_last_private_mib": idle[-1] / 1048576 if idle else None,
        "errors": len(errors), "malformed_log": malformed,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    rows = [summarize(path) for path in sorted(args.directory.rglob("events.jsonl"))]
    if not rows:
        parser.error("No events.jsonl files found")
    manifest = args.directory / "runs.json"
    missing = []
    if manifest.exists():
        runs = json.loads(manifest.read_text(encoding="utf-8-sig"))
        runs = runs if isinstance(runs, list) else [runs]
        by_run = {r["run"]: r for r in rows}
        for run in runs:
            if run["output"] not in by_run:
                missing.append(run)
            elif run["exit_code"] != 0:
                by_run[run["output"]]["status"] = "incomplete_or_failed"
    report = {"schema": 1, "note": "250 ms samples may miss short peaks. First open is process-cold, not guaranteed disk-cold. Assignment/render tick is not input-to-photon. Only compare identical files, dimensions, and operations. Small-sample p95 is not a release gate.", "runs": rows, "missing_logs": missing}
    (args.directory / "summary.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    with (args.directory / "summary.csv").open("w", encoding="utf-8-sig", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0])); writer.writeheader(); writer.writerows(rows)
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
