import importlib.util, json, unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('evidence',Path(__file__).parents[1]/'composite-evidence.py');ev=importlib.util.module_from_spec(spec);spec.loader.exec_module(ev)
class EvidenceTests(unittest.TestCase):
    def diagnostic(self):
        return json.dumps(dict(reason='compiler-message',message=dict(level='warning',code=dict(code='unused'),message='unused value',spans=[dict(file_name='src/lib.rs',line_start=3,column_start=1,line_end=3,column_end=4,is_primary=True)])))
    def test_unique_diagnostics_not_summaries(self):
        out=ev.parse_log(self.diagnostic()+'\n'+self.diagnostic()+'\nwarning: `probe` (lib) generated 1 warning\n')
        self.assertEqual(out['unique_compiler_warnings'],1);self.assertEqual(out['tool_warning_lines'],[])
    def test_distinct_spans_are_distinct(self):
        a=self.diagnostic();self.assertEqual(ev.parse_log(a+'\n'+a.replace('"line_start": 3','"line_start": 4'))['unique_compiler_warnings'],2)
    def test_different_packages_with_same_relative_span_are_distinct(self):
        a=json.loads(self.diagnostic());a['manifest_path']='/tmp/a/Cargo.toml'
        b=json.loads(self.diagnostic());b['manifest_path']='/tmp/b/Cargo.toml'
        self.assertEqual(ev.parse_log(json.dumps(a)+'\n'+json.dumps(b))['unique_compiler_warnings'],2)
    def test_absolute_and_relative_paths_to_same_diagnostic_dedupe(self):
        a=json.loads(self.diagnostic());a['manifest_path']='/tmp/a/Cargo.toml'
        b=json.loads(json.dumps(a));b['message']['spans'][0]['file_name']='/tmp/a/src/lib.rs'
        self.assertEqual(ev.parse_log(json.dumps(a)+'\n'+json.dumps(b))['unique_compiler_warnings'],1)
    def test_zero_test_filter(self):
        self.assertEqual(ev.parse_log('test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 99 filtered out;')['executed_tests'],0)
    def test_rust_multi_suite(self):
        out=ev.parse_log('test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 9 filtered out;\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;')
        self.assertEqual(out['tests'],dict(passed=5,failed=0,skipped=1))
    def test_swift_aggregate_not_double_counted(self):
        out=ev.parse_log("Test Case 'S.a' passed (0.1 seconds).\nTest Case 'S.b' skipped (0.0 seconds).\nExecuted 2 tests, with 1 test skipped\nExecuted 2 tests, with 1 test skipped")
        self.assertEqual(out['tests'],dict(passed=1,failed=0,skipped=1))
    def test_vitest(self):
        self.assertEqual(ev.parse_log(' Tests  268 passed (268)')['executed_tests'],268)
    def test_unittest(self):
        self.assertEqual(ev.parse_log("Ran 7 tests in 0.1s\nOK")["executed_tests"],7)
    def test_vitest_failures_and_skips(self):
        self.assertEqual(ev.parse_log(' Tests  2 failed | 3 passed | 1 skipped (6)')['tests'],dict(passed=3,failed=2,skipped=1))
    def test_node_failures_are_not_extra_tests(self):
        self.assertEqual(ev.parse_log('# tests 6\n# fail 2\n# skipped 1')['tests'],dict(passed=3,failed=2,skipped=1))
    def test_node(self):
        self.assertEqual(ev.parse_log('# tests 23\n# fail 0\n# skipped 0')['executed_tests'],23)
if __name__=='__main__': unittest.main()
