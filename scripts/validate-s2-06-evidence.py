#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime
import errno
import hashlib
import json
import os
import stat
import sys
from pathlib import Path

SCHEMA = "tailsync-s2-06-evidence/v1"
EXIT_READY = 0
EXIT_INVALID = 2
EXIT_INCOMPLETE = 3

MAX_MANIFEST_BYTES = 64 * 1024
MAX_SAMPLES = 64
MAX_NOTIFICATIONS = 64
MAX_RECEIPTS_PER_ITEM = 32
MAX_TOTAL_RECEIPTS = 128
MAX_FILE_BYTES = 16 * 1024 * 1024
MAX_TOTAL_EVIDENCE_BYTES = 64 * 1024 * 1024
MAX_IDENTIFIER_LENGTH = 128
MAX_TEXT_LENGTH = 4096
MAX_PATH_LENGTH = 1024

CASES = ("menu", "selection", "single_image", "text_plus_one", "text_plus_two")
STAGES = ("analysis", "db", "helper", "list", "preview", "restore_plan", "saved")
CONDITIONS = ("immediate", "offline", "restarted", "source_closed")
PAIR_KEYS = ("direct", "history")
OBSERVATIONS = ("not_observed", "observed", "unknown")
PROVENANCE_VALUES = ("real_qq", "synthetic")
SCOPES = ("none", "public_test_only", "user_content_authorized")
SOURCE_APPS = ("qq",)
HASH_KEYS = ("candidate_sha256", "helper_sha256", "source_sha256")
ENVIRONMENT_KEYS = (
    "candidate_sha256",
    "helper_sha256",
    "source_sha256",
    "os_version",
    "source_app",
    "source_version",
    "target_app",
    "target_version",
)
HEX_DIGITS = frozenset("0123456789abcdef")
LOG_CODE_ALPHABET = frozenset("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_.-")
RESULTS = ("ok", "failed", "partial", "unavailable", "unknown")

TEMPLATE = {
    "schema": SCHEMA,
    "run_id": "",
    "session_id": "",
    "provenance": "synthetic",
    "authorization": {
        "public_test_content": False,
        "consent_confirmed": False,
        "scope": "none",
    },
    "environment": {
        "source_app": "qq",
        "source_version": "",
        "os_version": "",
        "target_app": "",
        "target_version": "",
        "helper_sha256": "",
        "candidate_sha256": "",
        "source_sha256": "",
    },
    "samples": [],
    "notifications": [],
}


class Report:
    def __init__(self):
        self.invalid = set()
        self.incomplete = set()
        self.cases_present = set()
        self.run_id = None
        self.session_id = None
        self.source_action_id = None
        self.source_actions = set()

    def bad(self, code):
        self.invalid.add(code)

    def missing(self, code):
        self.incomplete.add(code)


def is_hex64(value):
    return isinstance(value, str) and len(value) == 64 and all(character in HEX_DIGITS for character in value)


def is_bounded_text(value, limit):
    if not isinstance(value, str) or not value or len(value) > limit:
        return False
    return all(character.isprintable() for character in value)


def is_identifier(value):
    return is_bounded_text(value, MAX_IDENTIFIER_LENGTH)


def is_original_text(value):
    return isinstance(value, str) and 0 < len(value) <= MAX_TEXT_LENGTH and all(character.isprintable() or character in "\n\r\t" for character in value)


def is_log_code(value):
    if not isinstance(value, str) or len(value) < 3 or len(value) > 64:
        return False
    if not value[0].isalpha():
        return False
    return all(character in LOG_CODE_ALPHABET for character in value)


def safe_relative(value):
    if not isinstance(value, str) or not value or len(value) > MAX_PATH_LENGTH:
        return False
    if not value.isprintable() or "\\" in value or ":" in value:
        return False
    if value.startswith(("/", "~")):
        return False
    if "://" in value or value.lower().startswith("file:"):
        return False
    parts = value.split("/")
    return all(part not in ("", ".", "..") for part in parts)


def check_fields(value, allowed, report):
    if not isinstance(value, dict):
        report.bad("E_TYPE")
        return False
    if set(value) - set(allowed):
        report.bad("E_FIELDS")
    return True


def check_action(value, report):
    action = value.get("source_action_id")
    if action is None or action == "":
        report.missing("I_ACTION_BINDING")
    elif not is_identifier(action):
        report.bad("E_TYPE")
    elif action != report.source_action_id:
        report.bad("E_ACTION_MISMATCH")


def is_zoned_time(value):
    if not is_bounded_text(value, MAX_IDENTIFIER_LENGTH):
        return False
    try:
        moment = datetime.datetime.fromisoformat(value)
        return moment.utcoffset() is not None
    except ValueError:
        return False


def collect_receipts(receipts, report, collector):
    if not isinstance(receipts, list):
        report.bad("E_TYPE")
        return
    if len(receipts) > MAX_RECEIPTS_PER_ITEM:
        report.bad("E_BUDGET_COUNT")
        return
    for item in receipts:
        if not check_fields(item, ("path", "size", "sha256"), report):
            continue
        path_value = item.get("path")
        size_value = item.get("size")
        hash_value = item.get("sha256")
        if not isinstance(path_value, str) or not path_value:
            report.bad("E_TYPE")
            continue
        if not isinstance(size_value, int) or isinstance(size_value, bool) or size_value < 0:
            report.bad("E_TYPE")
            continue
        if size_value == 0:
            report.bad("E_RECEIPT_EMPTY")
            continue
        if not is_hex64(hash_value):
            report.bad("E_TYPE")
            continue
        if len(collector) >= MAX_TOTAL_RECEIPTS:
            report.bad("E_BUDGET_COUNT")
            return
        collector.append({"path": path_value, "size": size_value, "sha256": hash_value})


def check_cell(cell, report, collector, missing_code, unknown_code, target=False):
    fields = ("observation", "result", "receipts", "source_action_id")
    if target:
        fields += ("target_app", "target_version")
    if not check_fields(cell, fields, report):
        return
    check_action(cell, report)
    observation = cell.get("observation")
    if observation is None or observation == "":
        report.missing(missing_code)
    elif observation not in OBSERVATIONS:
        report.bad("E_ENUM")
    elif observation in ("unknown", "not_observed"):
        report.missing(unknown_code)
    result = cell.get("result")
    if result is None or result == "" or result == "unknown":
        report.missing(unknown_code)
    elif result not in RESULTS:
        report.bad("E_ENUM")
    if observation in ("unknown", "not_observed") and result not in (None, "", "unknown"):
        report.bad("E_CONTRADICTION")
    receipts = cell.get("receipts")
    if not isinstance(receipts, list):
        report.bad("E_TYPE")
        return
    if observation in ("observed", "not_observed") and not receipts:
        report.bad("E_CONTRADICTION")
    collect_receipts(receipts, report, collector)


def validate_identity(manifest, report):
    if manifest.get("schema") != SCHEMA:
        report.bad("E_SCHEMA")
    for key in ("run_id", "session_id"):
        value = manifest.get(key)
        if value is None or value == "":
            report.missing("I_IDENTITY")
        elif not is_identifier(value):
            report.bad("E_TYPE")
    report.run_id = manifest.get("run_id")
    report.session_id = manifest.get("session_id")


def validate_environment(manifest, report):
    environment = manifest.get("environment")
    if not check_fields(environment, ENVIRONMENT_KEYS, report):
        environment = {}
    for key in ENVIRONMENT_KEYS:
        value = environment.get(key)
        if value is None or value == "":
            report.missing("I_ENVIRONMENT")
        elif key in HASH_KEYS:
            if not is_hex64(value):
                report.bad("E_TYPE")
        elif not is_identifier(value):
            report.bad("E_TYPE")
    source_app = environment.get("source_app")
    if isinstance(source_app, str) and source_app and source_app not in SOURCE_APPS:
        report.bad("E_ENUM")


def validate_authorization(manifest, report):
    authorization = manifest.get("authorization")
    if not check_fields(authorization, ("scope", "public_test_content", "consent_confirmed"), report):
        authorization = {}
    scope = authorization.get("scope")
    if scope not in SCOPES:
        report.bad("E_ENUM")
    public_test = authorization.get("public_test_content")
    consent = authorization.get("consent_confirmed")
    for value in (public_test, consent):
        if not isinstance(value, bool):
            report.bad("E_TYPE")
    if consent is not True or scope == "none":
        report.missing("I_AUTHORIZATION")
    if scope == "public_test_only" and public_test is not True:
        report.bad("E_CONTRADICTION")
    if public_test is not True and scope != "user_content_authorized":
        report.missing("I_AUTHORIZATION")


def validate_provenance(manifest, report):
    provenance = manifest.get("provenance")
    if provenance not in PROVENANCE_VALUES:
        report.bad("E_ENUM")
        return
    if provenance == "synthetic":
        report.missing("I_PROVENANCE_SYNTHETIC")
        authorization = manifest.get("authorization")
        if isinstance(authorization, dict) and authorization.get("scope") == "user_content_authorized":
            report.bad("E_CONTRADICTION")


def validate_inventory(sample, report, collector):
    inventory = sample.get("source_inventory")
    if inventory is None:
        report.missing("I_SOURCE_INVENTORY")
        return
    if not check_fields(inventory, ("declared", "receipts", "source_action_id"), report):
        return
    check_action(inventory, report)
    declared = inventory.get("declared")
    if not isinstance(declared, list):
        report.bad("E_TYPE")
    else:
        if len(declared) > MAX_RECEIPTS_PER_ITEM:
            report.bad("E_BUDGET_COUNT")
            return
        if len(set(item for item in declared if isinstance(item, str))) != len(declared):
            report.bad("E_DUPLICATE")
        for item in declared:
            if not is_identifier(item):
                report.bad("E_TYPE")
    receipts = inventory.get("receipts")
    if not isinstance(receipts, list):
        report.bad("E_TYPE")
        return
    if not receipts:
        report.missing("I_SOURCE_INVENTORY")
    collect_receipts(receipts, report, collector)


def validate_stages(sample, report, collector):
    stages = sample.get("stages")
    if not isinstance(stages, dict):
        if stages is not None:
            report.bad("E_TYPE")
        for _ in STAGES:
            report.missing("I_MISSING_STAGE")
        return
    for stage in STAGES:
        if stage not in stages:
            report.missing("I_MISSING_STAGE")
            continue
        check_cell(stages[stage], report, collector, "I_MISSING_STAGE", "I_UNKNOWN_STAGE")
    check_fields(stages, STAGES, report)


def validate_targets(sample, environment, report, collector):
    targets = sample.get("targets")
    if not isinstance(targets, dict):
        if targets is not None:
            report.bad("E_TYPE")
        for _ in PAIR_KEYS:
            report.missing("I_MISSING_PAIR")
        return
    expected_app = environment.get("target_app") if isinstance(environment, dict) else None
    expected_version = environment.get("target_version") if isinstance(environment, dict) else None
    for key in PAIR_KEYS:
        if key not in targets:
            report.missing("I_MISSING_PAIR")
            continue
        cell = targets[key]
        check_cell(cell, report, collector, "I_MISSING_PAIR", "I_UNKNOWN_TARGET", target=True)
        if not isinstance(cell, dict):
            continue
        target_app = cell.get("target_app")
        target_version = cell.get("target_version")
        if target_app in (None, "") or target_version in (None, ""):
            report.missing("I_TARGET_IDENTITY")
        if target_app not in (None, ""):
            if not is_identifier(target_app):
                report.bad("E_TYPE")
            elif expected_app not in (None, "") and target_app != expected_app:
                report.bad("E_TARGET_MISMATCH")
        if target_version not in (None, ""):
            if not is_identifier(target_version):
                report.bad("E_TYPE")
            elif expected_version not in (None, "") and target_version != expected_version:
                report.bad("E_TARGET_MISMATCH")
    check_fields(targets, PAIR_KEYS, report)


def validate_conditions(sample, report, collector):
    conditions = sample.get("conditions")
    if not isinstance(conditions, dict):
        if conditions is not None:
            report.bad("E_TYPE")
        for _ in CONDITIONS:
            report.missing("I_MISSING_CONDITION")
        return
    for condition in CONDITIONS:
        if condition not in conditions:
            report.missing("I_MISSING_CONDITION")
            continue
        check_cell(conditions[condition], report, collector, "I_MISSING_CONDITION", "I_UNKNOWN_CONDITION")
    check_fields(conditions, CONDITIONS, report)


def validate_samples(manifest, report, collector):
    samples = manifest.get("samples")
    if not isinstance(samples, list):
        report.bad("E_TYPE")
        samples = []
    if len(samples) > MAX_SAMPLES:
        report.bad("E_BUDGET_COUNT")
        return
    seen_ids = set()
    environment = manifest.get("environment")
    for sample in samples:
        if not check_fields(sample, ("sample_id", "case", "run_id", "session_id", "source_action_id", "source_inventory", "stages", "targets", "conditions"), report):
            continue
        for key, expected in (("run_id", report.run_id), ("session_id", report.session_id)):
            value = sample.get(key)
            if value in (None, ""):
                report.missing("I_SAMPLE_BINDING")
            elif not is_identifier(value):
                report.bad("E_TYPE")
            elif expected not in (None, "") and value != expected:
                report.bad("E_SESSION_MISMATCH")
        action = sample.get("source_action_id")
        report.source_action_id = action
        if action in (None, ""):
            report.missing("I_ACTION_BINDING")
        elif not is_identifier(action):
            report.bad("E_TYPE")
        elif action in report.source_actions:
            report.bad("E_DUPLICATE")
        else:
            report.source_actions.add(action)
        sample_id = sample.get("sample_id")
        if not is_identifier(sample_id):
            report.bad("E_TYPE")
        elif sample_id in seen_ids:
            report.bad("E_DUPLICATE")
        else:
            seen_ids.add(sample_id)
        case = sample.get("case")
        if case not in CASES:
            report.bad("E_ENUM")
        else:
            report.cases_present.add(case)
        validate_inventory(sample, report, collector)
        validate_stages(sample, report, collector)
        validate_targets(sample, environment, report, collector)
        validate_conditions(sample, report, collector)
    for case in CASES:
        if case not in report.cases_present:
            report.missing("I_MISSING_CASE")


def validate_notifications(manifest, report, collector):
    notifications = manifest.get("notifications")
    if not isinstance(notifications, list):
        report.bad("E_TYPE")
        notifications = []
    if len(notifications) > MAX_NOTIFICATIONS:
        report.bad("E_BUDGET_COUNT")
        return
    if not notifications:
        report.missing("I_MISSING_NOTIFICATION")
    seen_ids = set()
    for notification in notifications:
        if not check_fields(notification, ("notification_id", "session_id", "source_action_id", "observation", "original_text", "observed_at", "emitter", "log_code", "receipts"), report):
            continue
        session = notification.get("session_id")
        action = notification.get("source_action_id")
        if session in (None, "") or action in (None, ""):
            report.missing("I_SAMPLE_BINDING")
        elif not is_identifier(session) or not is_identifier(action):
            report.bad("E_TYPE")
        elif session != report.session_id or action not in report.source_actions:
            report.bad("E_SESSION_MISMATCH")
        observation = notification.get("observation")
        if observation in (None, "", "unknown"):
            report.missing("I_NOTIFICATION")
        elif observation not in OBSERVATIONS:
            report.bad("E_ENUM")
        notification_id = notification.get("notification_id")
        if not is_identifier(notification_id):
            report.bad("E_TYPE")
        elif notification_id in seen_ids:
            report.bad("E_DUPLICATE")
        else:
            seen_ids.add(notification_id)
        original_text = notification.get("original_text")
        if (original_text is None or original_text == "") and observation != "not_observed":
            report.missing("I_NOTIFICATION")
        elif original_text not in (None, "") and not is_original_text(original_text):
            report.bad("E_TYPE")
        for key in ("observed_at", "emitter"):
            value = notification.get(key)
            if (value is None or value == "") and not (key == "emitter" and observation == "not_observed"):
                report.missing("I_NOTIFICATION")
            elif value not in (None, "") and not (is_zoned_time(value) if key == "observed_at" else is_identifier(value)):
                report.bad("E_TYPE")
        log_code = notification.get("log_code")
        if (log_code is None or log_code == "") and observation != "not_observed":
            report.missing("I_NOTIFICATION")
        elif log_code not in (None, "") and not is_log_code(log_code):
            report.bad("E_TYPE")
        if observation == "not_observed" and any(notification.get(key) not in (None, "") for key in ("original_text", "emitter", "log_code")):
            report.bad("E_CONTRADICTION")
        receipts = notification.get("receipts")
        if not isinstance(receipts, list):
            report.bad("E_TYPE")
            continue
        if not receipts:
            report.missing("I_NOTIFICATION")
        collect_receipts(receipts, report, collector)


def validate_manifest(manifest, report):
    collector = []
    check_fields(manifest, TEMPLATE.keys(), report)
    validate_identity(manifest, report)
    validate_environment(manifest, report)
    validate_authorization(manifest, report)
    validate_provenance(manifest, report)
    validate_samples(manifest, report, collector)
    validate_notifications(manifest, report, collector)
    return collector


class EvidenceReadError(Exception):
    pass


def open_regular(root, relative):
    if not safe_relative(relative):
        raise EvidenceReadError("E_RECEIPT_PATH")
    if os.open not in os.supports_dir_fd or not hasattr(os, "O_NOFOLLOW"):
        raise EvidenceReadError("E_SECURE_IO_UNAVAILABLE")
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    descriptor = os.open(root, directory_flags)
    try:
        parts = relative.split("/")
        for component in parts[:-1]:
            metadata = os.stat(component, dir_fd=descriptor, follow_symlinks=False)
            if stat.S_ISLNK(metadata.st_mode):
                raise EvidenceReadError("E_RECEIPT_SYMLINK")
            child = os.open(component, directory_flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        metadata = os.stat(parts[-1], dir_fd=descriptor, follow_symlinks=False)
        if stat.S_ISLNK(metadata.st_mode):
            raise EvidenceReadError("E_RECEIPT_SYMLINK")
        file_descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=descriptor)
        if not stat.S_ISREG(os.fstat(file_descriptor).st_mode):
            os.close(file_descriptor)
            raise EvidenceReadError("E_RECEIPT_NOT_FILE")
        return file_descriptor
    except OSError as error:
        if error.errno == errno.ELOOP:
            raise EvidenceReadError("E_RECEIPT_SYMLINK") from None
        raise EvidenceReadError("E_RECEIPT_MISSING") from None
    finally:
        os.close(descriptor)


def stable_metadata(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mtime_ns, metadata.st_ctime_ns)


def read_bounded(descriptor, limit, expected_size=None, collect=False):
    before = os.fstat(descriptor)
    if before.st_size > limit:
        raise EvidenceReadError("E_BUDGET_FILE")
    if expected_size is not None and before.st_size != expected_size:
        raise EvidenceReadError("E_RECEIPT_SIZE")
    maximum = before.st_size
    total = 0
    payload = bytearray()
    hasher = hashlib.sha256()
    while True:
        chunk = os.read(descriptor, min(64 * 1024, maximum - total + 1))
        if not chunk:
            break
        total += len(chunk)
        if total > maximum:
            raise EvidenceReadError("E_RECEIPT_CHANGED")
        hasher.update(chunk)
        if collect:
            payload.extend(chunk)
    after = os.fstat(descriptor)
    if total != maximum or stable_metadata(before) != stable_metadata(after):
        raise EvidenceReadError("E_RECEIPT_CHANGED")
    return bytes(payload) if collect else hasher.hexdigest()


def verify_evidence(root, receipts, report):
    if len(receipts) > MAX_TOTAL_RECEIPTS:
        report.bad("E_BUDGET_COUNT")
        return
    seen = set()
    for entry in receipts:
        if entry["path"] in seen:
            report.bad("E_DUPLICATE")
        seen.add(entry["path"])
        if not safe_relative(entry["path"]):
            report.bad("E_RECEIPT_PATH")
        if entry["size"] > MAX_FILE_BYTES:
            report.bad("E_BUDGET_FILE")
    if sum(entry["size"] for entry in receipts) > MAX_TOTAL_EVIDENCE_BYTES:
        report.bad("E_BUDGET_AGGREGATE")
    if report.invalid:
        return
    for entry in receipts:
        descriptor = None
        try:
            descriptor = open_regular(root, entry["path"])
            digest = read_bounded(descriptor, MAX_FILE_BYTES, expected_size=entry["size"])
            if digest != entry["sha256"]:
                report.bad("E_RECEIPT_HASH")
        except EvidenceReadError as error:
            report.bad(error.args[0])
        except OSError:
            report.bad("E_RECEIPT_MISSING")
        finally:
            if descriptor is not None:
                os.close(descriptor)


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("E_DUPLICATE_KEY")
        value[key] = item
    return value


def reject_constant(value):
    raise ValueError("E_JSON_CONSTANT")


def load_manifest(path, report, root=None):
    descriptor = None
    try:
        absolute = Path(os.path.abspath(path))
        if root is None:
            root = absolute.parent.resolve()
            relative = absolute.name
        else:
            relative = str(absolute.relative_to(Path(os.path.abspath(root))))
            root = root.resolve()
        descriptor = open_regular(root, relative)
        payload = read_bounded(descriptor, MAX_MANIFEST_BYTES, collect=True)
        data = json.loads(payload.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=reject_constant)
    except EvidenceReadError as error:
        report.bad("E_MANIFEST_TOO_LARGE" if error.args[0] == "E_BUDGET_FILE" else "E_MANIFEST_NOT_FILE")
        return None
    except (OSError, UnicodeDecodeError, ValueError, RecursionError):
        report.bad("E_MANIFEST_JSON")
        return None
    finally:
        if descriptor is not None:
            os.close(descriptor)
    if not isinstance(data, dict):
        report.bad("E_MANIFEST_JSON")
        return None
    return data


def load_root(path, report):
    try:
        if path.is_symlink() or not path.is_dir():
            report.bad("E_EVIDENCE_ROOT")
            return None
        return path.resolve()
    except (OSError, ValueError):
        report.bad("E_EVIDENCE_ROOT")
        return None


def emit(report, manifest):
    if report.invalid:
        classification = "INVALID"
        exit_code = EXIT_INVALID
    elif report.incomplete:
        classification = "INCOMPLETE"
        exit_code = EXIT_INCOMPLETE
    else:
        classification = "READY_FOR_MANUAL_REVIEW"
        exit_code = EXIT_READY
    provenance = manifest.get("provenance") if isinstance(manifest, dict) else None
    if provenance not in PROVENANCE_VALUES:
        provenance = None
    summary = {
        "schema": SCHEMA,
        "class": classification,
        "invalid_codes": sorted(report.invalid),
        "incomplete_codes": sorted(report.incomplete),
        "cases_present": sorted(report.cases_present),
        "provenance": provenance,
        "real_a07": "UNKNOWN",
        "real_a02": "UNKNOWN",
        "release": "NO-GO",
        "s2_complete": False,
    }
    print(json.dumps(summary, sort_keys=True))
    return exit_code


class SafeArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        raise ValueError("E_ARGUMENTS")


def main(argv=None):
    parser = SafeArgumentParser(
        description="Offline validator for TailSync S2-06 QQ evidence manifests.",
    )
    parser.add_argument("--manifest", type=Path, help="path to a JSON evidence manifest")
    parser.add_argument("--evidence-root", type=Path, help="directory that referenced evidence files must stay inside")
    parser.add_argument("--template", action="store_true", help="print an empty manifest template to stdout and exit")
    report = Report()
    try:
        arguments = parser.parse_args(argv)
    except ValueError:
        report.bad("E_ARGUMENTS")
        return emit(report, None)

    if arguments.template:
        if arguments.manifest is not None or arguments.evidence_root is not None:
            report.bad("E_ARGUMENTS")
            return emit(report, None)
        sys.stdout.write(json.dumps(TEMPLATE, indent=2, sort_keys=True) + "\n")
        return EXIT_READY

    if arguments.manifest is None or arguments.evidence_root is None:
        report.bad("E_ARGUMENTS")
        return emit(report, None)

    root = load_root(arguments.evidence_root, report)
    manifest = load_manifest(arguments.manifest, report, arguments.evidence_root) if root is not None else None
    collector = []
    if manifest is not None:
        collector = validate_manifest(manifest, report)
    if root is not None and collector and not report.invalid and "I_AUTHORIZATION" not in report.incomplete:
        verify_evidence(root, collector, report)
    return emit(report, manifest)


if __name__ == "__main__":
    raise SystemExit(main())
