#!/usr/bin/env python3
from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import math
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).resolve().parents[1] / "s2-performance-report.py"
MODULE_SPEC = importlib.util.spec_from_file_location("s2_performance_report", MODULE_PATH)
perf = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(perf)

HEX_A = "a" * 64
HEX_B = "b" * 64
HEX_C = "c" * 64
HEX_D = "d" * 64
HEX_E = "e" * 64


def make_trial(wall_ms, cpu_user_ms=1.0, cpu_system_ms=0.5, rss_peak_bytes=4096, telemetry=True):
    return {
        "wall_ms": wall_ms,
        "cpu_user_ms": cpu_user_ms,
        "cpu_system_ms": cpu_system_ms,
        "rss_peak_bytes": rss_peak_bytes,
        "decode_count": 1 if telemetry else None,
        "convert_count": 1 if telemetry else None,
        "cache_hits": 0 if telemetry else None,
        "cache_misses": 0 if telemetry else None,
        "final_lease_released": True if telemetry else None,
    }


def trials_for(wall_ms, count=20, telemetry=True):
    return [make_trial(wall_ms, telemetry=telemetry) for repeat in range(count)]


def gui_rows_unknown():
    return [
        {"scenario": name, "status": "unknown", "trials": []}
        for name in perf.GUI_SCENARIOS
    ]


def make_document(
    wall_values=None,
    measurement_kind="headless_saved_bytes",
    gui_rows=None,
    helper_row=None,
    telemetry=True,
    environment=None,
    bindings=None,
):
    values = wall_values if wall_values is not None else [10.0 + index for index in range(20)]
    resolved_gui_rows = gui_rows_unknown() if gui_rows is None else gui_rows
    resolved_helper_row = (
        {
            "scenario": perf.HELPER_SCENARIO,
            "status": "measured",
            "trials": [make_trial(value, telemetry=telemetry) for value in values],
        }
        if helper_row is None
        else helper_row
    )
    base_environment = {
        "os": "macos",
        "arch": "arm64",
        "build_profile": "release",
        "hardware_id": "hw-local-1",
    }
    if environment is not None:
        base_environment.update(environment)
    base_bindings = {
        "source_sha256": HEX_A,
        "helper_sha256": HEX_B,
        "fixture_sha256": HEX_C,
        "workload_sha256": HEX_D,
    }
    if bindings is not None:
        base_bindings.update(bindings)
    return {
        "schema": perf.SCHEMA,
        "report_id": "s2-07-local",
        "measurement_kind": measurement_kind,
        "environment": base_environment,
        "bindings": base_bindings,
        "scenarios": list(resolved_gui_rows) + [resolved_helper_row],
    }


class PerformanceReportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def write_json(self, name, document):
        path = self.root / name
        path.write_text(json.dumps(document), encoding="utf-8")
        return path

    def write_raw(self, name, text):
        path = self.root / name
        path.write_text(text, encoding="utf-8")
        return path

    def run_tool(self, arguments):
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            exit_code = perf.main(arguments)
        return exit_code, json.loads(buffer.getvalue())

    def run_document(self, document, baseline=None, name="input.json"):
        path = self.write_json(name, document)
        arguments = ["--input", str(path), "--evidence-root", str(self.root)]
        if baseline is not None:
            baseline_path = self.write_json("baseline.json", baseline)
            arguments.extend(["--baseline", str(baseline_path)])
        return self.run_tool(arguments)

    def test_nearest_rank_indexing(self):
        values = [1.0, 2.0, 3.0, 4.0]
        self.assertEqual(perf.nearest_rank(values, 0.5), 2.0)
        self.assertEqual(perf.nearest_rank(values, 0.95), 4.0)
        ordered = [float(index) for index in range(1, 21)]
        self.assertEqual(perf.nearest_rank(ordered, 0.5), 10.0)
        self.assertEqual(perf.nearest_rank(ordered, 0.95), 19.0)

    def test_headless_report_complete(self):
        exit_code, summary = self.run_document(make_document())
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["class"], "REPORT_COMPLETE")
        self.assertEqual(summary["measurement_kind"], "headless_saved_bytes")
        self.assertEqual(summary["coverage"]["helper_analyze_rtfd"], "MEASURED")
        self.assertEqual(summary["coverage"]["required_missing"], [])
        self.assertEqual(summary["scenarios"][perf.HELPER_SCENARIO]["wall_ms"]["p50"], 19.0)
        self.assertEqual(summary["scenarios"][perf.HELPER_SCENARIO]["wall_ms"]["p95"], 28.0)
        self.assertEqual(summary["gui_acceptance"], "UNKNOWN")
        self.assertEqual(summary["release"], "NO-GO")
        self.assertEqual(summary["real_a07"], "UNKNOWN")
        self.assertEqual(summary["real_a02"], "UNKNOWN")
        self.assertFalse(summary["full_s2"])

    def test_headless_cannot_fill_gui(self):
        gui_rows = [
            {"scenario": "idle", "status": "measured", "trials": trials_for(5.0)}
        ]
        gui_rows.extend(
            {"scenario": name, "status": "unknown", "trials": []}
            for name in perf.GUI_SCENARIOS
            if name != "idle"
        )
        exit_code, summary = self.run_document(make_document(gui_rows=gui_rows))
        self.assertEqual(exit_code, 2)
        self.assertEqual(summary["class"], "INVALID")
        self.assertIn("E_KIND_MISMATCH", summary["invalid_codes"])
        self.assertEqual(summary["gui_acceptance"], "UNKNOWN")

    def test_native_gui_helper_mismatch(self):
        gui_rows = [
            {"scenario": name, "status": "measured", "trials": trials_for(5.0)}
            for name in perf.GUI_SCENARIOS
        ]
        helper_row = {"scenario": perf.HELPER_SCENARIO, "status": "measured", "trials": trials_for(5.0)}
        document = make_document(
            measurement_kind="native_gui",
            gui_rows=gui_rows,
            helper_row=helper_row,
        )
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_KIND_MISMATCH", summary["invalid_codes"])

    def test_baseline_regression_requires_review(self):
        baseline = make_document(wall_values=[100.0 for repeat in range(20)])
        current = make_document(wall_values=[115.0 for repeat in range(20)])
        exit_code, summary = self.run_document(current, baseline=baseline)
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["comparison"]["status"], "COMPARED")
        result = summary["comparison"]["scenarios"][perf.HELPER_SCENARIO]
        self.assertEqual(result["status"], "REVIEW_REQUIRED")
        self.assertEqual(result["delta_percent"], 15.0)
        self.assertEqual(summary["release"], "NO-GO")

    def test_baseline_equal_threshold_is_diagnostic_ok(self):
        baseline = make_document(wall_values=[100.0 for repeat in range(20)])
        current = make_document(wall_values=[110.0 for repeat in range(20)])
        exit_code, summary = self.run_document(current, baseline=baseline)
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["comparison"]["scenarios"][perf.HELPER_SCENARIO]["status"], "OK")

    def test_baseline_incompatible_environment(self):
        baseline = make_document(
            wall_values=[100.0 for repeat in range(20)],
            environment={"hardware_id": "hw-other"},
        )
        current = make_document(wall_values=[120.0 for repeat in range(20)])
        exit_code, summary = self.run_document(current, baseline=baseline)
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["comparison"]["status"], "NOT_COMPARABLE")
        self.assertIn("ENVIRONMENT_DIFFERS", summary["comparison"]["reasons"])

    def test_differing_source_revision_is_comparable(self):
        baseline = make_document(
            wall_values=[100.0 for repeat in range(20)],
            bindings={"source_sha256": HEX_E},
        )
        current = make_document(wall_values=[101.0 for repeat in range(20)])
        exit_code, summary = self.run_document(current, baseline=baseline)
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["comparison"]["status"], "COMPARED")
        self.assertEqual(summary["comparison"]["scenarios"][perf.HELPER_SCENARIO]["status"], "OK")

    def test_invalid_baseline_does_not_invalidate_report(self):
        baseline = make_document(bindings={"helper_sha256": "not-a-hash"})
        exit_code, summary = self.run_document(make_document(), baseline=baseline)
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["class"], "REPORT_COMPLETE")
        self.assertEqual(summary["comparison"]["status"], "BASELINE_INVALID")

    def test_baseline_scenario_missing_is_unknown(self):
        baseline = make_document(
            helper_row={"scenario": perf.HELPER_SCENARIO, "status": "unknown", "trials": []}
        )
        exit_code, summary = self.run_document(make_document(), baseline=baseline)
        self.assertEqual(exit_code, 0)
        result = summary["comparison"]["scenarios"][perf.HELPER_SCENARIO]
        self.assertEqual(result["status"], "UNKNOWN")

    def test_no_baseline_is_not_pass(self):
        exit_code, summary = self.run_document(make_document())
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["comparison"]["status"], "NO_BASELINE")

    def test_missing_and_unmeasured_rows(self):
        gui_rows = [
            {"scenario": name, "status": "unknown", "trials": []}
            for name in perf.GUI_SCENARIOS
            if name != "idle"
        ]
        exit_code, summary = self.run_document(make_document(gui_rows=gui_rows))
        self.assertEqual(exit_code, 0)
        self.assertIn("idle", summary["coverage"]["required_missing"])
        self.assertEqual(summary["scenarios"]["idle"]["status"], "UNKNOWN")
        self.assertEqual(summary["scenarios"]["idle"]["reason"], "ROW_MISSING")
        self.assertIn("continuous_copy", summary["coverage"]["unknown"])

    def test_telemetry_absent_is_unknown(self):
        exit_code, summary = self.run_document(make_document(telemetry=False))
        self.assertEqual(exit_code, 0)
        helper = summary["scenarios"][perf.HELPER_SCENARIO]
        self.assertEqual(helper["decode_count"], {"status": "UNKNOWN", "unit": "count"})
        self.assertEqual(helper["final_lease_released"], {"status": "UNKNOWN", "unit": "bool"})
        self.assertEqual(helper["wall_ms"]["status"], "MEASURED")

    def test_cpu_and_rss_absent_are_unknown_not_zero(self):
        helper_row = {
            "scenario": perf.HELPER_SCENARIO,
            "status": "measured",
            "trials": [
                make_trial(10.0 + index, cpu_user_ms=None, rss_peak_bytes=None)
                for index in range(20)
            ],
        }
        exit_code, summary = self.run_document(make_document(helper_row=helper_row))
        self.assertEqual(exit_code, 0)
        helper = summary["scenarios"][perf.HELPER_SCENARIO]
        self.assertEqual(helper["cpu_user_ms"], {"status": "UNKNOWN", "unit": "ms"})
        self.assertEqual(helper["rss_peak_bytes"], {"status": "UNKNOWN", "unit": "bytes"})
        self.assertEqual(helper["cpu_system_ms"]["status"], "MEASURED")

    def test_wrong_schema_invalid(self):
        document = make_document()
        document["schema"] = "tailsync-s2-performance/v2"
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_SCHEMA", summary["invalid_codes"])

    def test_duplicate_json_key_invalid(self):
        text = '{"schema": "tailsync-s2-performance/v1", "schema": "tailsync-s2-performance/v1"}'
        path = self.write_raw("duplicate.json", text)
        exit_code, summary = self.run_tool(["--input", str(path), "--evidence-root", str(self.root)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_INPUT_DUPLICATE_KEY", summary["invalid_codes"])

    def test_nan_rejected(self):
        document = make_document()
        document["scenarios"][-1]["trials"][0]["wall_ms"] = float("nan")
        path = self.write_json("nan.json", document)
        exit_code, summary = self.run_tool(["--input", str(path), "--evidence-root", str(self.root)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_INPUT_CONSTANT", summary["invalid_codes"])

    def test_cardinality_below_minimum(self):
        document = make_document(wall_values=[10.0 + index for index in range(19)])
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_CARDINALITY", summary["invalid_codes"])

    def test_trial_count_bound(self):
        report = perf.Report()
        row = {
            "scenario": perf.HELPER_SCENARIO,
            "status": "measured",
            "trials": trials_for(1.0, count=perf.MAX_TRIALS_PER_SCENARIO + 1),
        }
        parsed = perf.validate_scenario(row, "headless_saved_bytes", report)
        self.assertIsNotNone(parsed)
        self.assertIn("E_TRIAL_COUNT", report.invalid)

    def test_invalid_hash(self):
        document = make_document(bindings={"source_sha256": "z" * 64})
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_HASH", summary["invalid_codes"])

    def test_output_hides_freeform_identifiers(self):
        document = make_document(environment={"hardware_id": "<script>alert(1)</script>"})
        document["report_id"] = "<img src=x onerror=alert(1)>"
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 0)
        serialized = json.dumps(summary)
        self.assertNotIn("script", serialized)
        self.assertNotIn("onerror", serialized)
        self.assertNotIn("hw-local-1", serialized)

    def test_input_outside_root_invalid(self):
        outside = tempfile.TemporaryDirectory()
        self.addCleanup(outside.cleanup)
        path = Path(outside.name) / "input.json"
        path.write_text(json.dumps(make_document()), encoding="utf-8")
        exit_code, summary = self.run_tool(["--input", str(path), "--evidence-root", str(self.root)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_INPUT_PATH", summary["invalid_codes"])

    def test_symlink_input_invalid(self):
        real_path = self.write_json("real.json", make_document())
        link_path = self.root / "link.json"
        os.symlink(real_path, link_path)
        exit_code, summary = self.run_tool(["--input", str(link_path), "--evidence-root", str(self.root)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_INPUT_SYMLINK", summary["invalid_codes"])

    def test_directory_input_invalid(self):
        directory = self.root / "folder"
        directory.mkdir()
        exit_code, summary = self.run_tool(["--input", str(directory), "--evidence-root", str(self.root)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_INPUT_NOT_FILE", summary["invalid_codes"])

    def test_input_budget_enforced(self):
        path = self.root / "large.json"
        path.write_text(" " * (perf.MAX_INPUT_BYTES + 1024), encoding="utf-8")
        exit_code, summary = self.run_tool(["--input", str(path), "--evidence-root", str(self.root)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_INPUT_TOO_LARGE", summary["invalid_codes"])

    def test_non_object_json_invalid(self):
        path = self.write_raw("list.json", "[1, 2, 3]")
        exit_code, summary = self.run_tool(["--input", str(path), "--evidence-root", str(self.root)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_INPUT_JSON", summary["invalid_codes"])

    def test_unknown_top_level_field_invalid(self):
        document = make_document()
        document["extra_field"] = 1
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_FIELDS", summary["invalid_codes"])

    def test_unknown_trial_field_invalid(self):
        document = make_document()
        document["scenarios"][-1]["trials"][0]["extra_metric"] = 1
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_TRIAL_FIELDS", summary["invalid_codes"])

    def test_duplicate_scenario_invalid(self):
        document = make_document()
        document["scenarios"].append(
            {"scenario": perf.HELPER_SCENARIO, "status": "unknown", "trials": []}
        )
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_SCENARIO_DUPLICATE", summary["invalid_codes"])

    def test_unknown_scenario_name_invalid(self):
        document = make_document()
        document["scenarios"].append({"scenario": "warp_speed", "status": "unknown", "trials": []})
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_SCENARIO_UNKNOWN", summary["invalid_codes"])

    def test_status_contradiction_invalid(self):
        gui_rows = [{"scenario": "idle", "status": "unknown", "trials": trials_for(5.0, count=1)}]
        gui_rows.extend(
            {"scenario": name, "status": "unknown", "trials": []}
            for name in perf.GUI_SCENARIOS
            if name != "idle"
        )
        exit_code, summary = self.run_document(make_document(gui_rows=gui_rows))
        self.assertEqual(exit_code, 2)
        self.assertIn("E_STATUS_CONTRADICTION", summary["invalid_codes"])

    def test_bool_rejected_for_numeric_metric(self):
        helper_row = {
            "scenario": perf.HELPER_SCENARIO,
            "status": "measured",
            "trials": [make_trial(10.0 + index) for index in range(20)],
        }
        helper_row["trials"][0]["rss_peak_bytes"] = True
        exit_code, summary = self.run_document(make_document(helper_row=helper_row))
        self.assertEqual(exit_code, 2)
        self.assertIn("E_TRIAL_TYPE", summary["invalid_codes"])

    def test_negative_cpu_invalid(self):
        helper_row = {
            "scenario": perf.HELPER_SCENARIO,
            "status": "measured",
            "trials": [make_trial(10.0 + index) for index in range(20)],
        }
        helper_row["trials"][0]["cpu_user_ms"] = -1.0
        exit_code, summary = self.run_document(make_document(helper_row=helper_row))
        self.assertEqual(exit_code, 2)
        self.assertIn("E_TRIAL_VALUE", summary["invalid_codes"])

    def test_template_has_no_measured_data(self):
        exit_code, summary = self.run_tool(["--template"])
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["schema"], perf.SCHEMA)
        self.assertEqual(
            [row["scenario"] for row in summary["scenarios"]],
            list(perf.SCENARIO_NAMES),
        )
        for row in summary["scenarios"]:
            self.assertEqual(row["trials"], [])

    def test_template_rejects_extra_arguments(self):
        exit_code, summary = self.run_tool(["--template", "--input", "placeholder.json"])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_ARGUMENTS", summary["invalid_codes"])

    def test_missing_arguments_invalid(self):
        exit_code, summary = self.run_tool([])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_ARGUMENTS", summary["invalid_codes"])

    def test_input_without_evidence_root_invalid(self):
        path = self.write_json("input.json", make_document())
        exit_code, summary = self.run_tool(["--input", str(path)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_ARGUMENTS", summary["invalid_codes"])

    def test_evidence_root_symlink_invalid(self):
        link_path = Path(self.temp.name).parent / (Path(self.temp.name).name + "-link")
        if link_path.exists() or link_path.is_symlink():
            link_path.unlink()
        os.symlink(self.root, link_path)
        self.addCleanup(link_path.unlink)
        path = self.write_json("input.json", make_document())
        exit_code, summary = self.run_tool(["--input", str(path), "--evidence-root", str(link_path)])
        self.assertEqual(exit_code, 2)
        self.assertIn("E_INPUT_ROOT", summary["invalid_codes"])

    def test_unknown_environment_field_invalid(self):
        document = make_document()
        document["environment"]["gpu"] = "none"
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_ENVIRONMENT_FIELDS", summary["invalid_codes"])

    def test_unknown_binding_field_invalid(self):
        document = make_document()
        document["bindings"]["extra_sha256"] = HEX_E
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_BINDING_FIELDS", summary["invalid_codes"])

    def test_invalid_measurement_kind_invalid(self):
        document = make_document()
        document["measurement_kind"] = "wall_clock_guess"
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_ENUM", summary["invalid_codes"])

    def test_unknown_scenario_field_invalid(self):
        document = make_document()
        document["scenarios"][-1]["note"] = "freeform"
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 2)
        self.assertIn("E_SCENARIO_FIELDS", summary["invalid_codes"])

    def test_native_gui_document_complete(self):
        gui_rows = [
            {"scenario": name, "status": "measured", "trials": trials_for(5.0)}
            for name in perf.GUI_SCENARIOS
        ]
        helper_row = {"scenario": perf.HELPER_SCENARIO, "status": "unknown", "trials": []}
        document = make_document(
            measurement_kind="native_gui",
            gui_rows=gui_rows,
            helper_row=helper_row,
        )
        exit_code, summary = self.run_document(document)
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["class"], "REPORT_COMPLETE")
        self.assertEqual(summary["coverage"]["measured"], list(perf.GUI_SCENARIOS))
        self.assertEqual(summary["coverage"]["helper_analyze_rtfd"], "UNKNOWN")
        self.assertEqual(summary["gui_acceptance"], "UNKNOWN")
        self.assertEqual(summary["release"], "NO-GO")

    def test_baseline_measurement_kind_mismatch_not_comparable(self):
        gui_rows = [
            {"scenario": name, "status": "measured", "trials": trials_for(100.0)}
            for name in perf.GUI_SCENARIOS
        ]
        baseline = make_document(
            measurement_kind="native_gui",
            gui_rows=gui_rows,
            helper_row={"scenario": perf.HELPER_SCENARIO, "status": "unknown", "trials": []},
        )
        exit_code, summary = self.run_document(make_document(), baseline=baseline)
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["comparison"]["status"], "NOT_COMPARABLE")
        self.assertIn("MEASUREMENT_KIND_DIFFERS", summary["comparison"]["reasons"])

    def test_baseline_helper_binding_mismatch_not_comparable(self):
        baseline = make_document(
            wall_values=[100.0 for repeat in range(20)],
            bindings={"helper_sha256": HEX_E},
        )
        exit_code, summary = self.run_document(make_document(), baseline=baseline)
        self.assertEqual(exit_code, 0)
        self.assertEqual(summary["comparison"]["status"], "NOT_COMPARABLE")
        self.assertIn("HELPER_DIFFERS", summary["comparison"]["reasons"])


class ParentPerformanceEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.fixture = PerformanceReportTests(methodName='runTest')
        self.fixture.setUp()

    def tearDown(self):
        self.fixture.doCleanups()

    def test_giant_integer_fails_closed_without_overflow(self):
        document = make_document(wall_values=[10 ** 400] * 20)
        code, summary = self.fixture.run_document(document)
        self.assertEqual(code, 2)
        self.assertEqual(summary['class'], 'INVALID')

    def test_positive_submicrosecond_samples_are_not_rounded_to_zero(self):
        document = make_document(wall_values=[1e-10] * 20)
        code, summary = self.fixture.run_document(document)
        self.assertEqual(code, 0)
        self.assertEqual(summary['scenarios'][perf.HELPER_SCENARIO]['wall_ms']['p95'], 1e-10)

    def test_threshold_verdict_uses_raw_not_display_rounded_delta(self):
        current = make_document(wall_values=[11.000000001] * 20)
        baseline = make_document(wall_values=[10.0] * 20)
        code, summary = self.fixture.run_document(current, baseline)
        self.assertEqual(code, 0)
        self.assertEqual(summary['comparison']['scenarios'][perf.HELPER_SCENARIO]['status'], 'REVIEW_REQUIRED')

    def test_extreme_valid_ratio_never_serializes_infinity(self):
        current = make_document(wall_values=[1e308] * 20)
        baseline = make_document(wall_values=[1e-308] * 20)
        code, summary = self.fixture.run_document(current, baseline)
        self.assertEqual(code, 0)
        delta = summary['comparison']['scenarios'][perf.HELPER_SCENARIO]['delta_percent']
        self.assertTrue(delta is None or math.isfinite(delta))
        json.dumps(summary, allow_nan=False)

    def test_unmeasured_documents_are_not_claimed_compared(self):
        unknown = {'scenario': perf.HELPER_SCENARIO, 'status': 'unknown', 'trials': []}
        current = make_document(helper_row=unknown)
        baseline = make_document(helper_row=unknown)
        code, summary = self.fixture.run_document(current, baseline)
        self.assertEqual(code, 0)
        self.assertNotEqual(summary['comparison']['status'], 'COMPARED')

    def test_secure_reader_syscall_failure_has_fixed_private_error(self):
        path = self.fixture.write_json('input.json', make_document())
        evidence = perf.load_evidence_module()
        output = io.StringIO()
        with mock.patch.object(perf, 'load_evidence_module', return_value=evidence):
            with mock.patch.object(evidence, 'read_bounded', side_effect=OSError('PRIVATE_QQ_PATH_MARKER')):
                with contextlib.redirect_stdout(output):
                    code = perf.main(['--input', str(path), '--evidence-root', str(self.fixture.root)])
        self.assertEqual(code, 2)
        self.assertNotIn('PRIVATE_QQ_PATH_MARKER', output.getvalue())
        self.assertEqual(json.loads(output.getvalue())['class'], 'INVALID')

    def test_large_finite_integer_threshold_is_not_rounded(self):
        current = make_document(wall_values=[11 * 10 ** 99 + 1] * 20)
        baseline = make_document(wall_values=[10 ** 100] * 20)
        code, summary = self.fixture.run_document(current, baseline)
        self.assertEqual(code, 0)
        self.assertEqual(summary['comparison']['scenarios'][perf.HELPER_SCENARIO]['status'], 'REVIEW_REQUIRED')

    def test_bounded_input_counter_sum_fails_closed_on_output_limit(self):
        document = make_document()
        for index, trial in enumerate(document['scenarios'][-1]['trials']):
            trial['decode_count'] = 10 ** 4299 if index < 11 else 0
        self.assertLess(len(json.dumps(document).encode()), perf.MAX_INPUT_BYTES)
        code, summary = self.fixture.run_document(document)
        self.assertEqual(code, 2)
        self.assertEqual(summary['class'], 'INVALID')
        self.assertIn('E_OUTPUT_NUMBER', summary['invalid_codes'])



if __name__ == "__main__":
    unittest.main()
