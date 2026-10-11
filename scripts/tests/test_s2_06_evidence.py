import contextlib
import hashlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]

SPEC = importlib.util.spec_from_file_location(
    "s2_06_evidence",
    Path(__file__).resolve().parents[1] / "validate-s2-06-evidence.py",
)
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)


class EvidenceValidatorTests(unittest.TestCase):
    def setUp(self):
        self._temporary = tempfile.TemporaryDirectory()
        self.root = Path(self._temporary.name)

    def tearDown(self):
        self._temporary.cleanup()

    def write(self, name, content=b"evidence"):
        target = self.root / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)
        return {"path": name, "size": len(content), "sha256": hashlib.sha256(content).hexdigest()}

    def cell(self, sample_id, label, observation="observed", receipts=None):
        if receipts is None:
            receipts = [self.write(f"{sample_id}/{label}.bin", f"{sample_id}-{label}".encode())]
        return {"observation": observation, "result": "ok" if observation == "observed" else "unknown", "source_action_id": f"action-{sample_id}", "receipts": receipts}

    def sample(self, case):
        sample_id = f"sample-{case}"
        return {
            "sample_id": sample_id,
            "run_id": "run-1",
            "session_id": "session-1",
            "source_action_id": f"action-{sample_id}",
            "case": case,
            "source_inventory": {
                "source_action_id": f"action-{sample_id}",
                "declared": ["image", "text"],
                "receipts": [self.write(f"{sample_id}/inventory.bin", b"inventory")],
            },
            "stages": {stage: self.cell(sample_id, f"stage-{stage}") for stage in validator.STAGES},
            "targets": {
                key: dict(self.cell(sample_id, f"target-{key}"), target_app="QQ", target_version="9.9.1")
                for key in validator.PAIR_KEYS
            },
            "conditions": {
                condition: self.cell(sample_id, f"condition-{condition}")
                for condition in validator.CONDITIONS
            },
        }

    def manifest(self, provenance="real_qq"):
        return {
            "schema": validator.SCHEMA,
            "run_id": "run-1",
            "session_id": "session-1",
            "provenance": provenance,
            "authorization": {
                "public_test_content": True,
                "consent_confirmed": True,
                "scope": "public_test_only",
            },
            "environment": {
                "source_app": "qq",
                "source_version": "9.9.1",
                "os_version": "macOS-15",
                "target_app": "QQ",
                "target_version": "9.9.1",
                "helper_sha256": "a" * 64,
                "candidate_sha256": "b" * 64,
                "source_sha256": "c" * 64,
            },
            "samples": [self.sample(case) for case in validator.CASES],
            "notifications": [
                {
                    "notification_id": "notice-1",
                    "session_id": "session-1",
                    "source_action_id": f"action-sample-{validator.CASES[0]}",
                    "observation": "observed",
                    "original_text": "TailSync notification",
                    "observed_at": "2026-10-08T10:00:00+08:00",
                    "emitter": "qq",
                    "log_code": "TS-NOTIFY-001",
                    "receipts": [self.write("notifications/notice-1.bin", b"notification")],
                }
            ],
        }

    def run_validator(self, manifest):
        path = self.root / "manifest.json"
        path.write_text(json.dumps(manifest))
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = validator.main(["--manifest", str(path), "--evidence-root", str(self.root)])
        output = stream.getvalue()
        return code, output, json.loads(output)

    def test_exit_codes_are_distinct(self):
        self.assertEqual(len({validator.EXIT_READY, validator.EXIT_INVALID, validator.EXIT_INCOMPLETE}), 3)

    def test_public_receipt_bundle_is_ready_for_manual_review(self):
        code, _, summary = self.run_validator(self.manifest())
        self.assertEqual(code, validator.EXIT_READY)
        self.assertEqual(summary["class"], "READY_FOR_MANUAL_REVIEW")
        self.assertEqual(summary["invalid_codes"], [])
        self.assertEqual(summary["incomplete_codes"], [])
        self.assertEqual(summary["real_a07"], "UNKNOWN")
        self.assertEqual(summary["real_a02"], "UNKNOWN")
        self.assertFalse(summary["s2_complete"])
        self.assertEqual(summary["release"], "NO-GO")

    def test_template_is_empty_and_validates_incomplete(self):
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = validator.main(["--template"])
        self.assertEqual(code, validator.EXIT_READY)
        template = json.loads(stream.getvalue())
        self.assertEqual(template["samples"], [])
        self.assertEqual(template["notifications"], [])
        path = self.root / "template.json"
        path.write_text(stream.getvalue())
        capture = io.StringIO()
        with contextlib.redirect_stdout(capture):
            code = validator.main(["--manifest", str(path), "--evidence-root", str(self.root)])
        summary = json.loads(capture.getvalue())
        self.assertEqual(code, validator.EXIT_INCOMPLETE)
        self.assertEqual(summary["class"], "INCOMPLETE")
        self.assertEqual(summary["invalid_codes"], [])
        self.assertIn("I_MISSING_CASE", summary["incomplete_codes"])

    def test_missing_pair_is_incomplete(self):
        manifest = self.manifest()
        del manifest["samples"][0]["targets"]["history"]
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INCOMPLETE)
        self.assertIn("I_MISSING_PAIR", summary["incomplete_codes"])

    def test_target_mismatch_is_invalid(self):
        manifest = self.manifest()
        manifest["samples"][0]["targets"]["direct"]["target_app"] = "OtherApp"
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_TARGET_MISMATCH", summary["invalid_codes"])

    def test_synthetic_manifest_is_never_ready(self):
        code, _, summary = self.run_validator(self.manifest(provenance="synthetic"))
        self.assertEqual(code, validator.EXIT_INCOMPLETE)
        self.assertIn("I_PROVENANCE_SYNTHETIC", summary["incomplete_codes"])

    def test_synthetic_with_user_content_scope_is_invalid(self):
        manifest = self.manifest(provenance="synthetic")
        manifest["authorization"]["scope"] = "user_content_authorized"
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_CONTRADICTION", summary["invalid_codes"])

    def test_missing_condition_is_incomplete(self):
        manifest = self.manifest()
        del manifest["samples"][1]["conditions"]["offline"]
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INCOMPLETE)
        self.assertIn("I_MISSING_CONDITION", summary["incomplete_codes"])

    def test_unknown_stage_is_incomplete(self):
        manifest = self.manifest()
        manifest["samples"][0]["stages"]["preview"] = {"observation": "unknown", "receipts": []}
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INCOMPLETE)
        self.assertIn("I_UNKNOWN_STAGE", summary["incomplete_codes"])

    def test_malformed_observation_is_invalid(self):
        manifest = self.manifest()
        manifest["samples"][0]["stages"]["saved"]["observation"] = "pass"
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_ENUM", summary["invalid_codes"])

    def test_observed_without_receipts_is_invalid(self):
        manifest = self.manifest()
        manifest["samples"][0]["stages"]["db"] = {"observation": "observed", "receipts": []}
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_CONTRADICTION", summary["invalid_codes"])

    def test_receipt_hash_mismatch_is_invalid(self):
        manifest = self.manifest()
        manifest["samples"][0]["stages"]["helper"]["receipts"][0]["sha256"] = "0" * 64
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_RECEIPT_HASH", summary["invalid_codes"])

    def test_receipt_size_mismatch_is_invalid(self):
        manifest = self.manifest()
        manifest["samples"][0]["stages"]["helper"]["receipts"][0]["size"] += 1
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_RECEIPT_SIZE", summary["invalid_codes"])

    def test_traversal_path_is_invalid(self):
        manifest = self.manifest()
        manifest["samples"][0]["stages"]["list"]["receipts"][0]["path"] = "../escape.bin"
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_RECEIPT_PATH", summary["invalid_codes"])

    def test_absolute_and_url_paths_are_invalid(self):
        for value in ("/etc/hosts", "https://example.com/x"):
            manifest = self.manifest()
            manifest["samples"][0]["stages"]["list"]["receipts"][0]["path"] = value
            code, _, summary = self.run_validator(manifest)
            self.assertEqual(code, validator.EXIT_INVALID)
            self.assertIn("E_RECEIPT_PATH", summary["invalid_codes"])

    def test_symlink_receipt_is_invalid(self):
        manifest = self.manifest()
        target = self.root / "sample-single_image" / "link.bin"
        os.symlink(self.root / "sample-single_image" / "stage-saved.bin", target)
        entry = {"path": "sample-single_image/link.bin", "size": 8, "sha256": "0" * 64}
        manifest["samples"][0]["stages"]["list"]["receipts"] = [entry]
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_RECEIPT_SYMLINK", summary["invalid_codes"])

    def test_oversize_file_is_invalid(self):
        manifest = self.manifest()
        original = validator.MAX_FILE_BYTES
        validator.MAX_FILE_BYTES = 4
        try:
            code, _, summary = self.run_validator(manifest)
        finally:
            validator.MAX_FILE_BYTES = original
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_BUDGET_FILE", summary["invalid_codes"])

    def test_duplicate_sample_ids_are_invalid(self):
        manifest = self.manifest()
        manifest["samples"][1]["sample_id"] = manifest["samples"][0]["sample_id"]
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_DUPLICATE", summary["invalid_codes"])

    def test_malformed_manifest_json_is_invalid(self):
        path = self.root / "manifest.json"
        path.write_text("{ not json")
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = validator.main(["--manifest", str(path), "--evidence-root", str(self.root)])
        summary = json.loads(stream.getvalue())
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_MANIFEST_JSON", summary["invalid_codes"])

    def test_missing_manifest_file_is_invalid(self):
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = validator.main(
                ["--manifest", str(self.root / "absent.json"), "--evidence-root", str(self.root)]
            )
        summary = json.loads(stream.getvalue())
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_MANIFEST_NOT_FILE", summary["invalid_codes"])

    def test_missing_arguments_is_invalid(self):
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            code = validator.main([])
        summary = json.loads(stream.getvalue())
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn("E_ARGUMENTS", summary["invalid_codes"])

    def test_missing_notification_is_incomplete(self):
        manifest = self.manifest()
        manifest["notifications"] = []
        code, _, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INCOMPLETE)
        self.assertIn("I_MISSING_NOTIFICATION", summary["incomplete_codes"])

    def test_summary_does_not_leak_paths_or_content(self):
        manifest = self.manifest()
        marker = "SECRET_MARKER_9f3a"
        manifest["samples"][0]["stages"]["list"]["receipts"][0]["path"] = f"../{marker}.bin"
        code, output, summary = self.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertNotIn(marker, output)
        self.assertNotIn("..", output)
        self.assertIn("E_RECEIPT_PATH", summary["invalid_codes"])


class ParentEvidenceRegressionTests(unittest.TestCase):
    def setUp(self):
        self.fixture = EvidenceValidatorTests(methodName='runTest')
        self.fixture.setUp()

    def tearDown(self):
        self.fixture.tearDown()

    def test_observed_failure_is_ready_for_review_not_success(self):
        manifest = self.fixture.manifest()
        manifest['samples'][0]['stages']['db']['result'] = 'failed'
        code, _, summary = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_READY)
        self.assertEqual(summary['real_a07'], 'UNKNOWN')

    def test_observed_absence_of_prompt_does_not_require_fake_text(self):
        manifest = self.fixture.manifest()
        notification = manifest['notifications'][0]
        notification.update(observation='not_observed', original_text='', emitter='', log_code='')
        code, _, summary = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_READY)
        self.assertEqual(summary['real_a02'], 'UNKNOWN')

    def test_wrong_action_in_history_pair_is_rejected(self):
        manifest = self.fixture.manifest()
        manifest['samples'][0]['targets']['history']['source_action_id'] = 'wrong-action'
        code, _, summary = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn('E_ACTION_MISMATCH', summary['invalid_codes'])

    def test_boolean_is_not_a_receipt_size(self):
        manifest = self.fixture.manifest()
        manifest['samples'][0]['stages']['db']['receipts'][0]['size'] = True
        code, _, summary = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn('E_TYPE', summary['invalid_codes'])

    def test_unauthorized_bundle_does_not_open_any_receipt(self):
        manifest = self.fixture.manifest()
        manifest['authorization']['consent_confirmed'] = False
        with mock.patch.object(validator, 'verify_evidence') as verify:
            code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INCOMPLETE)
        verify.assert_not_called()

    def test_fifo_receipt_is_rejected_without_blocking(self):
        manifest = self.fixture.manifest()
        target = self.fixture.root / 'pipe.bin'
        os.mkfifo(target)
        manifest['samples'][0]['stages']['db']['receipts'] = [{'path': target.name, 'size': 1, 'sha256': hashlib.sha256(b'x').hexdigest()}]
        code, _, summary = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn('E_RECEIPT_NOT_FILE', summary['invalid_codes'])

    def test_fd_path_swap_cannot_read_external_file(self):
        manifest = self.fixture.manifest()
        receipt = manifest['samples'][0]['stages']['helper']['receipts'][0]
        target = self.fixture.root / receipt['path']
        outside = self.fixture.root.parent / (self.fixture.root.name + '-outside.bin')
        outside.write_bytes(b'PUBLIC_ATTACK_SIMULATION')
        real_stat = os.stat
        swapped = []

        def swap_after_stat(path, *arguments, **keywords):
            metadata = real_stat(path, *arguments, **keywords)
            if path == target.name and keywords.get('dir_fd') is not None and not swapped:
                target.unlink()
                target.symlink_to(outside)
                swapped.append(True)
            return metadata

        try:
            with mock.patch.object(validator.os, 'stat', swap_after_stat):
                code, _, summary = self.fixture.run_validator(manifest)
            self.assertEqual(swapped, [True])
            self.assertEqual(code, validator.EXIT_INVALID)
            self.assertIn('E_RECEIPT_SYMLINK', summary['invalid_codes'])
        finally:
            outside.unlink()

    def test_fd_file_growth_is_bounded_and_invalid(self):
        manifest = self.fixture.manifest()
        receipt = manifest['samples'][0]['stages']['helper']['receipts'][0]
        target = self.fixture.root / receipt['path']
        inode = target.stat().st_ino
        real_read = os.read
        consumed = []
        grown = []

        def grow_before_read(descriptor, count):
            matches = os.fstat(descriptor).st_ino == inode
            if matches and not grown:
                with target.open('ab') as output:
                    output.write(b'x' * (validator.MAX_FILE_BYTES + 1))
                grown.append(True)
            payload = real_read(descriptor, count)
            if matches:
                consumed.append(len(payload))
            return payload

        with mock.patch.object(validator.os, 'read', grow_before_read):
            code, _, summary = self.fixture.run_validator(manifest)
        self.assertEqual(grown, [True])
        self.assertLessEqual(sum(consumed), receipt['size'] + 1)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn('E_RECEIPT_CHANGED', summary['invalid_codes'])

    def test_aggregate_budget_is_checked_before_receipt_open(self):
        receipts = [{'path': f'{number}.bin', 'size': validator.MAX_FILE_BYTES, 'sha256': '0' * 64} for number in range(5)]
        report = validator.Report()
        with mock.patch.object(validator, 'open_regular') as open_file:
            validator.verify_evidence(self.fixture.root, receipts, report)
        open_file.assert_not_called()
        self.assertIn('E_BUDGET_AGGREGATE', report.invalid)

    def test_manifest_parent_symlink_is_rejected(self):
        real_parent = self.fixture.root / 'real'
        real_parent.mkdir()
        path = real_parent / 'manifest.json'
        path.write_text(json.dumps(self.fixture.manifest()))
        linked = self.fixture.root / 'linked'
        linked.symlink_to(real_parent, target_is_directory=True)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = validator.main(['--manifest', str(linked / path.name), '--evidence-root', str(self.fixture.root)])
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertEqual(json.loads(output.getvalue())['class'], 'INVALID')

    def test_template_and_validation_flags_are_mutually_exclusive(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = validator.main(['--template', '--manifest', 'PRIVATE_MARKER'])
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertNotIn('PRIVATE_MARKER', output.getvalue())

    def test_invalid_provenance_never_reflects_secret(self):
        manifest = self.fixture.manifest()
        manifest['provenance'] = 'PRIVATE_QQ_BODY_MARKER'
        code, output, summary = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertNotIn('PRIVATE_QQ_BODY_MARKER', output)
        self.assertIsNone(summary['provenance'])

    def test_extra_claim_fields_are_rejected(self):
        manifest = self.fixture.manifest()
        manifest['real_a07'] = 'PASS'
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)

    def test_missing_target_identity_is_not_ready(self):
        manifest = self.fixture.manifest()
        del manifest['samples'][0]['targets']['history']['target_app']
        del manifest['samples'][0]['targets']['history']['target_version']
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INCOMPLETE)

    def test_no_authorization_scope_is_not_ready(self):
        manifest = self.fixture.manifest()
        manifest['authorization']['scope'] = 'none'
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertNotEqual(code, validator.EXIT_READY)

    def test_unobserved_stage_is_incomplete(self):
        manifest = self.fixture.manifest()
        manifest['samples'][0]['stages']['db']['observation'] = 'not_observed'
        manifest['samples'][0]['stages']['db']['result'] = 'unknown'
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INCOMPLETE)

    def test_missing_source_binding_is_not_ready(self):
        manifest = self.fixture.manifest()
        manifest['environment'].pop('source_sha256', None)
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INCOMPLETE)

    def test_wrong_sample_session_cannot_be_merged(self):
        manifest = self.fixture.manifest()
        manifest['samples'][0]['session_id'] = 'other-session'
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)

    def test_receipt_parent_symlink_is_rejected(self):
        manifest = self.fixture.manifest()
        link = self.fixture.root / 'linked-parent'
        link.symlink_to(self.fixture.root / 'sample-menu', target_is_directory=True)
        receipt = manifest['samples'][0]['stages']['helper']['receipts'][0]
        receipt['path'] = 'linked-parent/stage-helper.bin'
        code, _, summary = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertIn('E_RECEIPT_SYMLINK', summary['invalid_codes'])

    def test_duplicate_json_keys_fail_closed(self):
        manifest = self.fixture.manifest()
        payload = json.dumps(manifest).replace('"provenance": "real_qq"', '"provenance": "synthetic", "provenance": "real_qq"')
        path = self.fixture.root / 'duplicate.json'
        path.write_text(payload)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = validator.main(['--manifest', str(path), '--evidence-root', str(self.fixture.root)])
        self.assertEqual(code, validator.EXIT_INVALID)

    def test_notification_timestamp_is_a_real_zoned_time(self):
        manifest = self.fixture.manifest()
        manifest['notifications'][0]['observed_at'] = 'tomorrow'
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)

    def test_actual_lowercase_log_code_is_accepted(self):
        manifest = self.fixture.manifest()
        manifest['notifications'][0]['log_code'] = 'capture_failed'
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_READY)

    def test_unknown_cli_argument_never_echoes_private_input(self):
        process = subprocess.run([sys.executable, str(ROOT / 'scripts/validate-s2-06-evidence.py'), '--PRIVATE_QQ_BODY_MARKER'], capture_output=True, text=True, check=False)
        self.assertEqual(process.returncode, validator.EXIT_INVALID)
        self.assertNotIn('PRIVATE_QQ_BODY_MARKER', process.stdout + process.stderr)
        self.assertEqual(json.loads(process.stdout)['class'], 'INVALID')

    def test_deep_json_is_fixed_error_not_traceback(self):
        path = self.fixture.root / 'deep.json'
        path.write_text('[' * 1500 + '0' + ']' * 1500)
        process = subprocess.run([sys.executable, str(ROOT / 'scripts/validate-s2-06-evidence.py'), '--manifest', str(path), '--evidence-root', str(self.fixture.root)], capture_output=True, text=True, check=False)
        self.assertEqual(process.returncode, validator.EXIT_INVALID)
        self.assertNotIn('Traceback', process.stdout + process.stderr)
        self.assertEqual(json.loads(process.stdout)['class'], 'INVALID')



class ParentEdgeEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.fixture = EvidenceValidatorTests(methodName='runTest')
        self.fixture.setUp()

    def tearDown(self):
        self.fixture.tearDown()

    def test_empty_file_is_not_an_evidence_receipt(self):
        manifest = self.fixture.manifest()
        receipt = manifest['samples'][0]['stages']['helper']['receipts'][0]
        (self.fixture.root / receipt['path']).write_bytes(b'')
        receipt.update(size=0, sha256=hashlib.sha256(b'').hexdigest())
        code, _, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_INVALID)

    def test_original_multiline_prompt_is_accepted_without_rewriting(self):
        manifest = self.fixture.manifest()
        manifest['notifications'][0]['original_text'] = '实际提示第一行\n第二行\t细节'
        code, output, _ = self.fixture.run_validator(manifest)
        self.assertEqual(code, validator.EXIT_READY)
        self.assertNotIn('实际提示', output)

    def test_null_root_is_fixed_error(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = validator.main(['--manifest', 'absent.json', '--evidence-root', '\x00PRIVATE_MARKER'])
        self.assertEqual(code, validator.EXIT_INVALID)
        self.assertNotIn('PRIVATE_MARKER', output.getvalue())
        self.assertEqual(json.loads(output.getvalue())['class'], 'INVALID')



if __name__ == "__main__":
    unittest.main()
