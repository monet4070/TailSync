#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import math
import os
import sys
from decimal import Decimal, localcontext
from pathlib import Path

SCHEMA = "tailsync-s2-performance/v1"
TOOL_NAME = "s2-performance-report"
EXIT_COMPLETE = 0
EXIT_INVALID = 2

MAX_INPUT_BYTES = 64 * 1024
MAX_TRIALS_PER_SCENARIO = 512
MIN_SAMPLES = 20
REGRESSION_PERCENT = 10.0
MAX_IDENTIFIER_LENGTH = 128
DECIMAL_PRECISION = 1024
HEX_DIGITS = frozenset("0123456789abcdef")

GUI_SCENARIOS = (
    "idle",
    "continuous_copy",
    "list_scroll",
    "first_image",
    "fast_switch",
    "repeated_document",
    "cancellation",
    "close",
)
HELPER_SCENARIO = "helper_analyze_rtfd"
SCENARIO_NAMES = GUI_SCENARIOS + (HELPER_SCENARIO,)
MEASUREMENT_KINDS = ("headless_saved_bytes", "native_gui")
BUILD_PROFILES = ("debug", "release", "profiling", "benchmark")
SCENARIO_STATUSES = ("measured", "unknown")

TOP_FIELDS = ("schema", "report_id", "measurement_kind", "environment", "bindings", "scenarios")
ENVIRONMENT_FIELDS = ("os", "arch", "build_profile", "hardware_id")
BINDING_FIELDS = ("source_sha256", "helper_sha256", "fixture_sha256", "workload_sha256")
SCENARIO_FIELDS = ("scenario", "status", "trials")
TRIAL_FIELDS = (
    "wall_ms",
    "cpu_user_ms",
    "cpu_system_ms",
    "rss_peak_bytes",
    "decode_count",
    "convert_count",
    "cache_hits",
    "cache_misses",
    "final_lease_released",
)
COUNTER_FIELDS = ("decode_count", "convert_count", "cache_hits", "cache_misses")

READ_CODE_MAP = {
    "E_RECEIPT_PATH": "E_INPUT_PATH",
    "E_RECEIPT_SYMLINK": "E_INPUT_SYMLINK",
    "E_RECEIPT_NOT_FILE": "E_INPUT_NOT_FILE",
    "E_RECEIPT_MISSING": "E_INPUT_MISSING",
    "E_RECEIPT_CHANGED": "E_INPUT_CHANGED",
    "E_RECEIPT_SIZE": "E_INPUT_CHANGED",
    "E_BUDGET_FILE": "E_INPUT_TOO_LARGE",
    "E_SECURE_IO_UNAVAILABLE": "E_TOOL_DEPENDENCY",
}

TEMPLATE = {
    "schema": SCHEMA,
    "report_id": "",
    "measurement_kind": "headless_saved_bytes",
    "environment": {"os": "", "arch": "", "build_profile": "release", "hardware_id": ""},
    "bindings": {
        "source_sha256": "",
        "helper_sha256": "",
        "fixture_sha256": "",
        "workload_sha256": "",
    },
    "scenarios": [
        {"scenario": scenario_name, "status": "unknown", "trials": []}
        for scenario_name in SCENARIO_NAMES
    ],
}


class DuplicateKeyError(ValueError):
    pass


class JsonConstantError(ValueError):
    pass


class Report:
    def __init__(self):
        self.invalid = set()
        self.evidence_module = None

    def bad(self, code):
        self.invalid.add("E_INPUT_ROOT" if code == "E_EVIDENCE_ROOT" else code)


class SafeArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        raise ValueError("E_ARGUMENTS")


def load_evidence_module():
    module_path = Path(__file__).resolve().parent / "validate-s2-06-evidence.py"
    try:
        spec = importlib.util.spec_from_file_location("tailsync_s206_evidence", module_path)
        if spec is None or spec.loader is None:
            return None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    except (OSError, ImportError, AttributeError, SyntaxError):
        return None


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise DuplicateKeyError("E_INPUT_DUPLICATE_KEY")
        value[key] = item
    return value


def reject_constant(value):
    raise JsonConstantError("E_INPUT_CONSTANT")


def load_document(path_text, root, report):
    evidence = report.evidence_module
    absolute = Path(os.path.abspath(path_text))
    try:
        relative = str(absolute.relative_to(Path(os.path.abspath(root))))
    except ValueError:
        report.bad("E_INPUT_PATH")
        return None
    try:
        descriptor = evidence.open_regular(root, relative)
    except evidence.EvidenceReadError as error:
        report.bad(READ_CODE_MAP.get(error.args[0], "E_INPUT_READ"))
        return None
    except (OSError, ValueError):
        report.bad("E_INPUT_READ")
        return None
    try:
        payload = evidence.read_bounded(descriptor, MAX_INPUT_BYTES, collect=True)
    except evidence.EvidenceReadError as error:
        report.bad(READ_CODE_MAP.get(error.args[0], "E_INPUT_READ"))
        return None
    except OSError:
        report.bad("E_INPUT_READ")
        return None
    finally:
        os.close(descriptor)
    try:
        data = json.loads(
            payload.decode("utf-8"),
            object_pairs_hook=unique_object,
            parse_constant=reject_constant,
        )
    except DuplicateKeyError:
        report.bad("E_INPUT_DUPLICATE_KEY")
        return None
    except JsonConstantError:
        report.bad("E_INPUT_CONSTANT")
        return None
    except (UnicodeDecodeError, ValueError, RecursionError):
        report.bad("E_INPUT_JSON")
        return None
    if not isinstance(data, dict):
        report.bad("E_INPUT_JSON")
        return None
    return data


def is_hex64(value):
    return isinstance(value, str) and len(value) == 64 and all(character in HEX_DIGITS for character in value)


def is_bounded_identifier(value):
    if not isinstance(value, str) or not value or len(value) > MAX_IDENTIFIER_LENGTH:
        return False
    return all(character.isprintable() for character in value)


def is_finite_number(value):
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        return False
    try:
        return math.isfinite(value)
    except OverflowError:
        return False


def is_plain_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def nearest_rank(ordered_values, percentile):
    if not ordered_values:
        raise ValueError("E_EMPTY_SAMPLES")
    rank = math.ceil(percentile * len(ordered_values))
    index = min(max(rank - 1, 0), len(ordered_values) - 1)
    return ordered_values[index]


def validate_environment(environment, report):
    if not isinstance(environment, dict):
        report.bad("E_TYPE")
        return None
    if set(environment) - set(ENVIRONMENT_FIELDS):
        report.bad("E_ENVIRONMENT_FIELDS")
    for field in ("os", "arch", "hardware_id"):
        if not is_bounded_identifier(environment.get(field)):
            report.bad("E_ENVIRONMENT_VALUE")
    if environment.get("build_profile") not in BUILD_PROFILES:
        report.bad("E_ENVIRONMENT_VALUE")
    if report.invalid:
        return None
    return {field: environment[field] for field in ENVIRONMENT_FIELDS}


def validate_bindings(bindings, report):
    if not isinstance(bindings, dict):
        report.bad("E_TYPE")
        return None
    if set(bindings) - set(BINDING_FIELDS):
        report.bad("E_BINDING_FIELDS")
    for field in BINDING_FIELDS:
        if not is_hex64(bindings.get(field)):
            report.bad("E_HASH")
    if report.invalid:
        return None
    return {field: bindings[field] for field in BINDING_FIELDS}


def validate_trial(trial, report):
    if not isinstance(trial, dict):
        report.bad("E_TRIAL_TYPE")
        return None
    if set(trial) - set(TRIAL_FIELDS) or set(TRIAL_FIELDS) - set(trial):
        report.bad("E_TRIAL_FIELDS")
    wall = trial.get("wall_ms")
    if not is_finite_number(wall):
        report.bad("E_TRIAL_TYPE")
    elif wall <= 0:
        report.bad("E_TRIAL_VALUE")
    for field in ("cpu_user_ms", "cpu_system_ms"):
        value = trial.get(field)
        if value is None:
            continue
        if not is_finite_number(value):
            report.bad("E_TRIAL_TYPE")
        elif value < 0:
            report.bad("E_TRIAL_VALUE")
    rss = trial.get("rss_peak_bytes")
    if rss is not None:
        if not is_plain_int(rss):
            report.bad("E_TRIAL_TYPE")
        elif rss <= 0:
            report.bad("E_TRIAL_VALUE")
    for field in COUNTER_FIELDS:
        value = trial.get(field)
        if value is None:
            continue
        if not is_plain_int(value):
            report.bad("E_TRIAL_TYPE")
        elif value < 0:
            report.bad("E_TRIAL_VALUE")
    released = trial.get("final_lease_released")
    if released is not None and not isinstance(released, bool):
        report.bad("E_TRIAL_TYPE")
    return trial


def validate_scenario(row, measurement_kind, report):
    if not isinstance(row, dict):
        report.bad("E_TYPE")
        return None
    if set(row) - set(SCENARIO_FIELDS):
        report.bad("E_SCENARIO_FIELDS")
    name = row.get("scenario")
    if name not in SCENARIO_NAMES:
        report.bad("E_SCENARIO_UNKNOWN")
        return None
    status = row.get("status")
    if status not in SCENARIO_STATUSES:
        report.bad("E_STATUS")
        return None
    trials = row.get("trials")
    if not isinstance(trials, list):
        report.bad("E_TYPE")
        return None
    if len(trials) > MAX_TRIALS_PER_SCENARIO:
        report.bad("E_TRIAL_COUNT")
    if status == "unknown":
        if trials:
            report.bad("E_STATUS_CONTRADICTION")
        return {"scenario": name, "status": "unknown", "trials": []}
    if len(trials) < MIN_SAMPLES:
        report.bad("E_CARDINALITY")
    if measurement_kind == "headless_saved_bytes" and name in GUI_SCENARIOS:
        report.bad("E_KIND_MISMATCH")
    if measurement_kind == "native_gui" and name == HELPER_SCENARIO:
        report.bad("E_KIND_MISMATCH")
    validated_trials = []
    for trial in trials:
        parsed_trial = validate_trial(trial, report)
        if parsed_trial is not None:
            validated_trials.append(parsed_trial)
    return {"scenario": name, "status": "measured", "trials": validated_trials}


def validate_document(document, report):
    if set(document) - set(TOP_FIELDS) or set(TOP_FIELDS) - set(document):
        report.bad("E_FIELDS")
    if document.get("schema") != SCHEMA:
        report.bad("E_SCHEMA")
    if not is_bounded_identifier(document.get("report_id")):
        report.bad("E_REPORT_ID")
    measurement_kind = document.get("measurement_kind")
    if measurement_kind not in MEASUREMENT_KINDS:
        report.bad("E_ENUM")
        measurement_kind = None
    environment = validate_environment(document.get("environment"), report)
    bindings = validate_bindings(document.get("bindings"), report)
    rows = document.get("scenarios")
    scenarios = {}
    if not isinstance(rows, list) or not rows:
        report.bad("E_TYPE")
    else:
        for row in rows:
            parsed_row = validate_scenario(row, measurement_kind, report)
            if parsed_row is None:
                continue
            name = parsed_row["scenario"]
            if name in scenarios:
                report.bad("E_SCENARIO_DUPLICATE")
                continue
            scenarios[name] = parsed_row
    return {
        "measurement_kind": measurement_kind,
        "environment": environment,
        "bindings": bindings,
        "scenarios": scenarios,
    }


def metric_from_trials(trials, field, unit):
    values = []
    for trial in trials:
        value = trial.get(field)
        if value is None:
            return {"status": "UNKNOWN", "unit": unit}
        values.append(value)
    ordered = sorted(values)
    return {
        "status": "MEASURED",
        "unit": unit,
        "count": len(ordered),
        "min": ordered[0],
        "max": ordered[-1],
        "p50": nearest_rank(ordered, 0.5),
        "p95": nearest_rank(ordered, 0.95),
    }


def counter_from_trials(trials, field, unit):
    total = 0
    for trial in trials:
        value = trial.get(field)
        if value is None:
            return {"status": "UNKNOWN", "unit": unit}
        total += value
    return {"status": "MEASURED", "unit": unit, "sum": total}


def boolean_from_trials(trials, field, unit):
    for trial in trials:
        if trial.get(field) is None:
            return {"status": "UNKNOWN", "unit": unit}
    return {
        "status": "MEASURED",
        "unit": unit,
        "value": all(trial.get(field) for trial in trials),
    }


def summarize_scenario(row):
    if row["status"] == "unknown":
        return {"status": "UNKNOWN"}
    trials = row["trials"]
    return {
        "status": "MEASURED",
        "sample_count": len(trials),
        "wall_ms": metric_from_trials(trials, "wall_ms", "ms"),
        "cpu_user_ms": metric_from_trials(trials, "cpu_user_ms", "ms"),
        "cpu_system_ms": metric_from_trials(trials, "cpu_system_ms", "ms"),
        "rss_peak_bytes": metric_from_trials(trials, "rss_peak_bytes", "bytes"),
        "decode_count": counter_from_trials(trials, "decode_count", "count"),
        "convert_count": counter_from_trials(trials, "convert_count", "count"),
        "cache_hits": counter_from_trials(trials, "cache_hits", "count"),
        "cache_misses": counter_from_trials(trials, "cache_misses", "count"),
        "final_lease_released": boolean_from_trials(trials, "final_lease_released", "bool"),
    }


def build_coverage(scenarios):
    measured = [
        name for name in SCENARIO_NAMES if scenarios.get(name, {}).get("status") == "measured"
    ]
    unknown = [
        name for name in SCENARIO_NAMES if scenarios.get(name, {}).get("status") == "unknown"
    ]
    present = set(scenarios)
    required_missing = [name for name in GUI_SCENARIOS if name not in present]
    helper_row = scenarios.get(HELPER_SCENARIO)
    helper_status = helper_row["status"].upper() if helper_row else "ABSENT"
    return {
        "measured": measured,
        "unknown": unknown,
        "required_missing": required_missing,
        "helper_analyze_rtfd": helper_status,
    }


def limits():
    return {
        "min_samples": MIN_SAMPLES,
        "max_trials_per_scenario": MAX_TRIALS_PER_SCENARIO,
        "max_input_bytes": MAX_INPUT_BYTES,
        "regression_percent": REGRESSION_PERCENT,
    }


def default_comparison():
    return {
        "status": "SKIPPED",
        "reasons": [],
        "threshold_percent": REGRESSION_PERCENT,
        "scenarios": {},
    }


def scenario_p95(row):
    ordered = sorted(trial["wall_ms"] for trial in row["trials"])
    return nearest_rank(ordered, 0.95)


def compare_documents(current, baseline):
    if baseline is None:
        return {
            "status": "NO_BASELINE",
            "reasons": [],
            "threshold_percent": REGRESSION_PERCENT,
            "scenarios": {},
        }
    reasons = []
    if current["measurement_kind"] != baseline["measurement_kind"]:
        reasons.append("MEASUREMENT_KIND_DIFFERS")
    for field in ENVIRONMENT_FIELDS:
        if current["environment"][field] != baseline["environment"][field]:
            reasons.append("ENVIRONMENT_DIFFERS")
            break
    binding_reasons = {
        "helper_sha256": "HELPER_DIFFERS",
        "fixture_sha256": "FIXTURE_DIFFERS",
        "workload_sha256": "WORKLOAD_DIFFERS",
    }
    for field, reason in binding_reasons.items():
        if current["bindings"][field] != baseline["bindings"][field]:
            reasons.append(reason)
    if reasons:
        return {
            "status": "NOT_COMPARABLE",
            "reasons": sorted(set(reasons)),
            "threshold_percent": REGRESSION_PERCENT,
            "scenarios": {},
        }
    scenario_results = {}
    compared_pairs = 0
    for name in SCENARIO_NAMES:
        current_row = current["scenarios"].get(name)
        baseline_row = baseline["scenarios"].get(name)
        if current_row is None or current_row["status"] != "measured":
            continue
        if baseline_row is None or baseline_row["status"] != "measured":
            scenario_results[name] = {"status": "UNKNOWN", "reason": "BASELINE_SCENARIO_MISSING"}
            continue
        if len(baseline_row["trials"]) < MIN_SAMPLES:
            scenario_results[name] = {"status": "UNKNOWN", "reason": "BASELINE_CARDINALITY_LOW"}
            continue
        baseline_p95 = scenario_p95(baseline_row)
        current_p95 = scenario_p95(current_row)
        if baseline_p95 <= 0:
            scenario_results[name] = {"status": "UNKNOWN", "reason": "BASELINE_NOT_POSITIVE"}
            continue
        precise_baseline = Decimal(str(baseline_p95))
        precise_current = Decimal(str(current_p95))
        with localcontext() as context:
            context.prec = DECIMAL_PRECISION
            precise_delta = (precise_current - precise_baseline) / precise_baseline * 100
            delta_percent = float(precise_delta)
            verdict = "REVIEW_REQUIRED" if precise_delta > Decimal(str(REGRESSION_PERCENT)) else "OK"
        scenario_results[name] = {
            "status": verdict,
            "baseline_p95_ms": baseline_p95,
            "current_p95_ms": current_p95,
            "delta_percent": delta_percent if math.isfinite(delta_percent) else None,
        }
        if not math.isfinite(delta_percent):
            scenario_results[name]["reason"] = "DELTA_OUT_OF_RANGE"
        compared_pairs += 1
    return {
        "status": "COMPARED" if compared_pairs else "NOT_COMPARABLE",
        "reasons": [] if compared_pairs else ["NO_MEASURED_PAIRS"],
        "threshold_percent": REGRESSION_PERCENT,
        "scenarios": scenario_results,
    }


def build_summary(report, model, comparison):
    resolved_comparison = comparison if comparison is not None else default_comparison()
    if report.invalid or model is None:
        return {
            "schema": SCHEMA,
            "tool": TOOL_NAME,
            "class": "INVALID",
            "invalid_codes": sorted(report.invalid),
            "measurement_kind": None,
            "declared_bindings": None,
            "environment_build_profile": None,
            "coverage": {
                "measured": [],
                "unknown": [],
                "required_missing": list(GUI_SCENARIOS),
                "helper_analyze_rtfd": "ABSENT",
            },
            "scenarios": {},
            "gui_acceptance": "UNKNOWN",
            "comparison": resolved_comparison,
            "limits": limits(),
            "release": "NO-GO",
            "real_a07": "UNKNOWN",
            "real_a02": "UNKNOWN",
            "full_s2": False,
        }
    scenario_summary = {}
    for name in SCENARIO_NAMES:
        row = model["scenarios"].get(name)
        if row is None:
            if name in GUI_SCENARIOS:
                scenario_summary[name] = {"status": "UNKNOWN", "reason": "ROW_MISSING"}
            continue
        scenario_summary[name] = summarize_scenario(row)
    return {
        "schema": SCHEMA,
        "tool": TOOL_NAME,
        "class": "REPORT_COMPLETE",
        "invalid_codes": [],
        "measurement_kind": model["measurement_kind"],
        "declared_bindings": model["bindings"],
        "environment_build_profile": model["environment"]["build_profile"],
        "coverage": build_coverage(model["scenarios"]),
        "scenarios": scenario_summary,
        "gui_acceptance": "UNKNOWN",
        "comparison": resolved_comparison,
        "limits": limits(),
        "release": "NO-GO",
        "real_a07": "UNKNOWN",
        "real_a02": "UNKNOWN",
        "full_s2": False,
    }


def emit_summary(report, model, comparison):
    summary = build_summary(report, model, comparison)
    try:
        payload = json.dumps(summary, indent=2, sort_keys=True, allow_nan=False)
    except (ValueError, OverflowError):
        report.bad("E_OUTPUT_NUMBER")
        payload = json.dumps(build_summary(report, None, None), indent=2, sort_keys=True, allow_nan=False)
    sys.stdout.write(payload + "\n")
    return EXIT_INVALID if report.invalid else EXIT_COMPLETE


def main(argv=None):
    parser = SafeArgumentParser(
        prog=TOOL_NAME,
        description="Offline TailSync S2-07 performance report tool.",
    )
    parser.add_argument("--input", type=Path)
    parser.add_argument("--evidence-root", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--template", action="store_true")
    report = Report()
    try:
        arguments = parser.parse_args(argv)
    except ValueError:
        report.bad("E_ARGUMENTS")
        return emit_summary(report, None, None)
    if arguments.template:
        if arguments.input is not None or arguments.evidence_root is not None or arguments.baseline is not None:
            report.bad("E_ARGUMENTS")
            return emit_summary(report, None, None)
        sys.stdout.write(json.dumps(TEMPLATE, indent=2, sort_keys=True) + "\n")
        return EXIT_COMPLETE
    if arguments.input is None or arguments.evidence_root is None:
        report.bad("E_ARGUMENTS")
        return emit_summary(report, None, None)
    evidence = load_evidence_module()
    if evidence is None:
        report.bad("E_TOOL_DEPENDENCY")
        return emit_summary(report, None, None)
    report.evidence_module = evidence
    root = evidence.load_root(arguments.evidence_root, report)
    if root is None:
        return emit_summary(report, None, None)
    document = load_document(arguments.input, arguments.evidence_root, report)
    model = validate_document(document, report) if document is not None else None
    baseline_model = None
    baseline_invalid = False
    if arguments.baseline is not None:
        baseline_report = Report()
        baseline_report.evidence_module = evidence
        baseline_document = load_document(arguments.baseline, arguments.evidence_root, baseline_report)
        if baseline_document is None or baseline_report.invalid:
            baseline_invalid = True
        else:
            baseline_model = validate_document(baseline_document, baseline_report)
            if baseline_report.invalid:
                baseline_invalid = True
                baseline_model = None
    if report.invalid or model is None:
        comparison = {
            "status": "BASELINE_INVALID" if baseline_invalid else "SKIPPED",
            "reasons": [],
            "threshold_percent": REGRESSION_PERCENT,
            "scenarios": {},
        }
        return emit_summary(report, None, comparison)
    if baseline_invalid:
        comparison = {
            "status": "BASELINE_INVALID",
            "reasons": [],
            "threshold_percent": REGRESSION_PERCENT,
            "scenarios": {},
        }
    else:
        comparison = compare_documents(model, baseline_model)
    return emit_summary(report, model, comparison)


if __name__ == "__main__":
    raise SystemExit(main())
