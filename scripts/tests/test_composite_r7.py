"""R7 harness rules (repair plan §4 R7).

The harness exists to stop a device phase from being quietly declared done. These tests
pin the three rules that make it trustworthy:

* a PASS without evidence is refused;
* empty evidence is refused;
* evidence that changed after being recorded invalidates the PASS.

If any of these regress, the harness becomes a way to *manufacture* an acceptance record,
which is worse than having no harness at all.
"""
import importlib.util
import json
import tempfile
import unittest
from unittest.mock import patch
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("r7", ROOT / "scripts/run-composite-r7.py")
r7 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r7)


def gate(gate_id):
    return next(g for g in r7.GATES if g["id"] == gate_id)


class R7HarnessRules(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.tmp = Path(scratch.name)
        self.run = self.tmp / "run"
        self.run.mkdir()
        self.evidence = self.tmp / "evidence.txt"
        self.bound = dict(source_sha256="a" * 64, r6_run="candidate", r6_seal_sha256="b" * 64)
        for name, value in (("binding", self.bound),):
            patcher = patch.object(r7, name, return_value=value)
            patcher.start()
            self.addCleanup(patcher.stop)
        patcher = patch.object(r7.ev, "fingerprint", return_value={"source_sha256": "a" * 64})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.raw = self.tmp / "observation.txt"
        self.raw.write_text("synthetic raw observation for harness regression\n")
        self.write_manifest("faults")

    def write_manifest(self, gate_id):
        data = r7.template(gate(gate_id), "candidate")
        data.update(observer="test operator", environment=dict(
            devices=[dict(id="mac", os="macos", version="13"), dict(id="win", os="windows", version="11")],
            apps=[dict(name="test consumer", version="1")]),
            packages=[dict(path=str(self.raw), sha256=r7._sha256(self.raw))])
        for case in data["cases"]:
            case.update(result="PASS", observed="synthetic expected observation", evidence=[str(self.raw)])
        self.evidence.write_text(json.dumps(data))
        return data

    def test_every_plan_gate_is_enumerated(self):
        # The plan's R7 table has ten acceptance groups; dropping one would hide work.
        self.assertEqual(len(r7.GATES), 10)
        self.assertEqual(
            {g["id"] for g in r7.GATES},
            {"mac-app", "windows-native", "source-apps", "target-apps", "two-way-sync",
             "intent-race", "network", "faults", "rollback", "accessibility"},
        )

    def test_a_pass_without_evidence_is_refused(self):
        self.assertFalse(r7.attach(self.run, gate("windows-native"), "PASS", [], "trust me"))
        state = r7.load(self.run)
        self.assertNotIn("windows-native", state["gates"])
        self.assertEqual(r7.gate_state({**state, "_run": str(self.run)}, gate("windows-native")), "UNKNOWN")

    def test_empty_evidence_is_refused(self):
        empty = self.tmp / "empty.txt"
        empty.write_text("   \n")
        self.assertFalse(r7.attach(self.run, gate("target-apps"), "PASS", [empty], ""))
        self.assertNotIn("target-apps", r7.load(self.run)["gates"])

    def test_missing_evidence_is_refused(self):
        self.assertFalse(
            r7.attach(self.run, gate("network"), "PASS", [self.tmp / "nope.txt"], "")
        )

    def test_a_pass_with_evidence_is_recorded_and_hashed(self):
        self.assertTrue(r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "probe run"))
        state = r7.load(self.run)
        record = state["gates"]["faults"]
        self.assertEqual(record["result"], "PASS")
        self.assertEqual(record["evidence"][0]["sha256"], r7._sha256(self.evidence))
        self.assertEqual(r7.gate_state({**state, "_run": str(self.run)}, gate("faults")), "PASS")

    def test_changed_evidence_invalidates_the_pass(self):
        r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "")
        self.evidence.write_text("a different observation\n")
        state = r7.load(self.run)
        self.assertEqual(
            r7.gate_state({**state, "_run": str(self.run)}, gate("faults")),
            "STALE",
            "tampered evidence must not keep reporting PASS",
        )

    def test_fail_and_unknown_are_recordable_without_evidence(self):
        self.assertTrue(r7.attach(self.run, gate("windows-native"), "FAIL", [], "not run"))
        self.assertTrue(r7.attach(self.run, gate("network"), "UNKNOWN", [], "no second device"))
        state = r7.load(self.run)
        self.assertEqual(state["gates"]["windows-native"]["result"], "FAIL")
        self.assertEqual(state["gates"]["network"]["result"], "UNKNOWN")

    def test_the_emitted_report_refuses_a_clean_verdict_while_any_gate_is_unknown(self):
        r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "")
        state = r7.load(self.run)
        state["_run"] = str(self.run)
        out = r7.emit(self.run, state)
        text = out.read_text()
        self.assertIn("Release NO-GO", text)
        self.assertIn("UNKNOWN", text)
        # Every gate must appear, so a reader cannot mistake the list for the full set.
        for item in r7.GATES:
            self.assertIn(item["criteria"], text)

    def test_gates_the_harness_must_not_automate_are_marked(self):
        # These would start a competing App/daemon or touch the user's general clipboard.
        self.assertNotIn("mac-app", r7.AUTO_COMMANDS)
        for item in r7.GATES:
            if item["id"] == "mac-app":
                self.assertEqual(item["mode"], "device")
                self.assertIsNotNone(item["blocked_by"])


    def test_zero_exit_with_failed_probe_assertions_is_not_pass(self):
        output = json.dumps({"RR-03-contract": {"satisfied": False}})
        completed = subprocess.CompletedProcess([], 0, output, "")
        with patch.object(r7.subprocess, "run", return_value=completed):
            r7.run_auto(self.run, gate("faults"), "unused")
        record = r7.load(self.run)["gates"]["faults"]
        self.assertEqual(record["result"], "FAIL")

    def test_a_legacy_pass_without_candidate_binding_is_stale(self):
        state = {"gates": {"faults": {"result": "PASS", "evidence": []}}, "_run": str(self.run)}
        self.assertEqual(r7.gate_state(state, gate("faults")), "STALE")

    def test_invalid_recorded_results_cannot_produce_an_acceptable_verdict(self):
        state = {"run_id": "negative", "gates": {
            item["id"]: {"result": "invented", "evidence": []} for item in r7.GATES},
            "_run": str(self.run)}
        self.assertIn("Release NO-GO", r7.emit(self.run, state).read_text())

    def test_auto_private_board_evidence_does_not_complete_the_device_gate(self):
        r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "local private board", scope="local-support", bound=self.bound)
        state = {**r7.load(self.run), "_run": str(self.run)}
        self.assertNotEqual(r7.gate_state(state, gate("faults")), "PASS")


    def test_nested_observation_change_invalidates_the_pass(self):
        self.assertTrue(r7.attach(self.run, gate("faults"), "PASS", [self.evidence], ""))
        self.raw.write_text("changed raw evidence")
        self.assertEqual(r7.gate_state({**r7.load(self.run), "_run": str(self.run)}, gate("faults")), "STALE")

    def test_source_change_invalidates_unchanged_evidence(self):
        r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "")
        state = {**r7.load(self.run), "_run": str(self.run), "_source": "c" * 64}
        self.assertEqual(r7.gate_state(state, gate("faults")), "STALE")

    def test_r6_seal_change_invalidates_the_pass(self):
        r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "")
        with patch.object(r7, "binding", return_value={**self.bound, "r6_seal_sha256": "changed"}):
            self.assertEqual(r7.gate_state({**r7.load(self.run), "_run": str(self.run)}, gate("faults")), "STALE")

    def test_text_file_does_not_prove_all_device_scenarios(self):
        self.assertFalse(r7.attach(self.run, gate("windows-native"), "PASS", [self.raw], "portable only"))

    def test_missing_device_scenario_is_refused(self):
        data = self.write_manifest("windows-native")
        data["cases"].pop()
        self.evidence.write_text(json.dumps(data))
        self.assertFalse(r7.attach(self.run, gate("windows-native"), "PASS", [self.evidence], ""))

    def test_wrong_source_in_device_manifest_is_refused(self):
        data = self.write_manifest("windows-native")
        data["source_sha256"] = "c" * 64
        self.evidence.write_text(json.dumps(data))
        self.assertFalse(r7.attach(self.run, gate("windows-native"), "PASS", [self.evidence], ""))

    def test_windows_native_rejects_a_host_observation(self):
        data = self.write_manifest("windows-native")
        data["environment"]["devices"] = [dict(id="mac", os="macos", version="13")]
        self.evidence.write_text(json.dumps(data))
        self.assertFalse(r7.attach(self.run, gate("windows-native"), "PASS", [self.evidence], ""))

    def test_two_device_gate_requires_distinct_mac_and_windows_devices(self):
        data = self.write_manifest("two-way-sync")
        data["environment"]["devices"][1]["id"] = "mac"
        self.evidence.write_text(json.dumps(data))
        self.assertFalse(r7.attach(self.run, gate("two-way-sync"), "PASS", [self.evidence], ""))

    def test_directory_is_refused_without_crashing(self):
        self.assertFalse(r7.attach(self.run, gate("faults"), "PASS", [self.tmp], ""))

    def test_run_ids_cannot_escape_the_evidence_root(self):
        for name in ("../escape", "/tmp/escape", "..", "x/y", "x\\y"):
            with self.assertRaises(ValueError):
                r7.run_path(name)

    def test_rerun_preserves_the_previous_observation(self):
        r7.attach(self.run, gate("faults"), "FAIL", [self.raw], "first failure")
        r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "second attempt")
        self.assertEqual(r7.load(self.run)["gates"]["faults"]["history"][0]["result"], "FAIL")

    def test_local_support_does_not_overwrite_device_evidence(self):
        r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "device")
        r7.attach(self.run, gate("faults"), "PASS", [self.raw], "private board", scope="local-support", bound=self.bound)
        record = r7.load(self.run)["gates"]["faults"]
        self.assertEqual(record["scope"], "device")
        self.assertEqual(record["local_support"]["scope"], "local-support")

    def test_rollback_refuses_a_stale_or_unsealed_r6(self):
        with patch.object(r7, "binding", side_effect=ValueError("tested source changed")):
            r7.run_auto(self.run, gate("rollback"), "old")
        self.assertEqual(r7.load(self.run)["gates"]["rollback"]["result"], "UNKNOWN")


    def test_reading_state_revalidates_device_manifest_coverage(self):
        r7.attach(self.run, gate("faults"), "PASS", [self.evidence], "")
        state = {**r7.load(self.run), "_run": str(self.run)}
        state["gates"]["faults"]["evidence"] = [dict(path=str(self.raw), sha256=r7._sha256(self.raw), bytes=self.raw.stat().st_size)]
        self.assertEqual(r7.gate_state(state, gate("faults")), "INVALID")

    def test_native_probe_runner_returns_failure_for_unsatisfied_contracts(self):
        spec = importlib.util.spec_from_file_location("native_probe", ROOT / "tools/repair-probes/native_fault_probe.py")
        probe = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(probe)
        data = {key: dict(satisfied=True) for key in
                ("RR-03-contract", "RR-04-contract", "R6-before-spawn", "R6-output-exceeded")}
        data["RR-07"] = dict(classified_as_file_selection=True)
        self.assertTrue(probe.contracts_satisfied(data))
        data["RR-03-contract"]["satisfied"] = False
        self.assertFalse(probe.contracts_satisfied(data))


if __name__ == "__main__":
    unittest.main()
