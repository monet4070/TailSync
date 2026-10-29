import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("summary", Path(__file__).parents[1] / "summarize.py")
summary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(summary)


class SummaryTests(unittest.TestCase):
    def run_summary(self, rows, suffix=""):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "events.jsonl"
            path.write_text("\n".join(json.dumps(r) for r in rows) + suffix)
            return summary.summarize(path)

    @staticmethod
    def event(kind, **data):
        return {"kind": kind, "elapsed_ms": 0, "data": data}

    def test_missing_private_working_set_is_not_zero(self):
        e = self.event
        result = self.run_summary([e("environment", engine="X"), e("resource", private_bytes=1024, working_set_bytes=2048, private_working_set_bytes=None), e("benchmark_complete"), e("shutdown", exit_code=0)])
        self.assertEqual(result["status"], "complete")
        self.assertIsNone(result["peak_private_working_set_mib"])

    def test_incomplete_and_corrupt_runs_cannot_pass(self):
        e = self.event
        self.assertEqual(self.run_summary([e("environment")])["status"], "incomplete_or_failed")
        rows = [e("benchmark_complete"), e("shutdown", exit_code=0)]
        self.assertEqual(self.run_summary(rows, '\n{"kind":')["status"], "incomplete_or_failed")
        self.assertEqual(self.run_summary(rows + [e("error", error="load failed")])["status"], "incomplete_or_failed")

    def test_first_open_does_not_pollute_warm_reopen(self):
        e = self.event
        rows = [e("frame_assigned", reason="first_page", first_from_open_request_ms=n) for n in [900, 20, 30]]
        result = self.run_summary(rows)
        self.assertEqual(result["first_open_frame_assigned_ms"], 900)
        self.assertEqual(result["warm_reopen_frame_assigned_p95_ms"], 30)

    def test_baseline_is_separate_from_post_close(self):
        e = self.event
        def memory(mib): return e("resource", private_bytes=mib * 1048576, working_set_bytes=mib * 1048576)
        result = self.run_summary([e("baseline_begin"), memory(10), e("baseline_end"), memory(500), e("idle_after_close_begin"), memory(40), e("idle_after_close_end")])
        self.assertEqual(result["baseline_last_private_mib"], 10)
        self.assertEqual(result["closed_last_private_mib"], 40)
        self.assertEqual(result["peak_private_mib"], 500)


if __name__ == "__main__": unittest.main()
