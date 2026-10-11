import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "run-repair-regressions.sh"


class RepairGateTests(unittest.TestCase):
    def evaluate(self, raw, log=None, ident="RR-20", extra=None):
        source = SCRIPT.read_text()
        start = source.rindex("<<'PY'\n") + len("<<'PY'\n")
        body = source[start:source.index("\nPY", start)]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "logs").mkdir()
            (root / ".raw").write_text(raw)
            native = {"RR-03-contract": {"satisfied": True}, "RR-04-contract": {"satisfied": True}, "RR-07": {"classified_as_file_selection": True}}
            (root / "logs/RR-NATIVE.log").write_text(json.dumps(native))
            if log is not None:
                (root / "logs" / (ident + ".log")).write_text(log)
            ledger = root / "ledger.json"
            ledger.write_text(json.dumps({"entries": extra or [], "closed": [{"id": ident, "native_expectation": "contract-satisfied"} for ident in ("RR-03", "RR-04", "RR-07")]}))
            result = subprocess.run([sys.executable, "-", str(root), str(ledger)], input=body, capture_output=True, text=True)
            return result.returncode, json.loads((root / "results.json").read_text())

    def test_an_unexpected_failure_fails_the_gate(self):
        code, report = self.evaluate("RR-01\t1\tred\tgreen\n")
        self.assertTrue(report["unexpected"])
        self.assertNotEqual(code, 0)

    def test_zero_executed_tests_cannot_close_a_regression(self):
        code, report = self.evaluate("RR-20\t0\tgreen\tgreen\n", "running 0 tests\ntest result: ok. 0 passed; 0 failed; 611 filtered out;\n")
        self.assertTrue(report["unexpected"])
        self.assertNotEqual(code, 0)

    def test_every_rr20_case_must_execute(self):
        code, report = self.evaluate("RR-20\t0\tgreen\tgreen\n", "test result: ok. 1 passed; 0 failed;\n")
        self.assertTrue(report["unexpected"])
        self.assertNotEqual(code, 0)

    def test_zero_swift_cases_fail_the_gate(self):
        code, report = self.evaluate("RR-25\t0\tgreen\tgreen\n", "Executed 0 tests", "RR-25")
        self.assertNotEqual(code, 0)
        self.assertTrue(report["unexpected"])

    def test_executed_swift_cases_pass(self):
        code, report = self.evaluate("RR-25\t0\tgreen\tgreen\n", "Test Case '-[Tests.test_real_code]' passed (0.003 seconds).", "RR-25")
        self.assertEqual(code, 0)
        self.assertFalse(report["unexpected"])

    def test_missing_ledger_case_fails_the_gate(self):
        code, report = self.evaluate("RR-01\t0\tgreen\tgreen\n", extra=[{"id": "RR-14", "expected": "green"}])
        self.assertNotEqual(code, 0)
        self.assertTrue(any(case["id"] == "RR-14" for case in report["unexpected"]))

    def test_matching_results_pass(self):
        code, report = self.evaluate("RR-01\t0\tgreen\tgreen\n")
        self.assertFalse(report["unexpected"])
        self.assertEqual(code, 0)


if __name__ == "__main__":
    unittest.main()
