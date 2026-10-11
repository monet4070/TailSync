"""Exercise the R6 consumer with actual scratch records/logs/package inventories."""
import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('evidence', Path(__file__).parents[1] / 'composite-evidence.py')
ev = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ev)


class R6ValidityTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.run = Path(scratch.name) / 'run'
        self.run.mkdir()
        self.source = dict(source_sha256='a' * 64, files={'product.rs': 'b' * 64})
        self.expected = [dict(id='probe', command=['test', 'probe'], environment={}, min_tests=1, exact_tests=1)]
        self.write('source-fingerprint.json', self.source)
        self.write('matrix.json', dict(required=['probe'], selected=['probe'], commands=self.expected))
        (self.run / 'logs').mkdir()
        (self.run / 'logs/probe.1.log').write_text('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
        (self.run / 'commands').mkdir()
        self.record = dict(self.expected[0], attempt=1, exit_code=0, source_sha256='a' * 64, log='logs/probe.1.log')
        self.write('commands/probe.1.json', self.record)
        for name in ('rollback-proof', 'package-probes'):
            self.write(name + '.json', dict(status='PASS', source_sha256='a' * 64))
        self.write('fault-matrix.json', dict(source_sha256='a' * 64, cases=[dict(status='PASS')] * 13))
        self.packages = []
        for kind in ('default-native', 'default-universal-apple-darwin', 'enabled-native', 'enabled-universal-apple-darwin', 'debug'):
            package = self.run / (kind + '.app')
            package.mkdir()
            (package / 'binary').write_bytes(b'synthetic packaged executable')
            inventory = {'binary': ev.digest(package / 'binary')}
            sha = hashlib.sha256(''.join(f'{n}\0{h}\n' for n, h in sorted(inventory.items())).encode()).hexdigest()
            self.packages.append(dict(kind=kind, path=str(package), status='PASS', source_sha256='a' * 64,
                                      all_files_sha256=inventory, package_sha256=sha))
        self.write('package-manifest.json', dict(source_sha256='a' * 64, packages=self.packages))
        self.refresh()

    def write(self, name, data):
        (self.run / name).write_text(json.dumps(data, indent=2) + '\n')

    def refresh(self):
        ev.generate(self.run)
        self.write('status.json', dict(state='LOCAL_COMPLETE', source_sha256='a' * 64))
        files = {str(p.relative_to(self.run)): ev.digest(p) for p in self.run.rglob('*')
                 if p.is_file() and p.name != 'evidence-index.json' and not any(x.endswith('.app') for x in p.relative_to(self.run).parts)}
        self.write('evidence-index.json', dict(source_sha256='a' * 64, files=files))

    def verify(self):
        return ev.verify_r6(self.run, source=self.source, expected=self.expected)

    def test_complete_current_sealed_evidence_passes_without_writing(self):
        before = {str(p): p.read_bytes() for p in self.run.rglob('*') if p.is_file()}
        self.assertEqual(self.verify()['unexpected'], 0)
        self.assertEqual(before, {str(p): p.read_bytes() for p in self.run.rglob('*') if p.is_file()})

    def test_current_source_must_match(self):
        with self.assertRaisesRegex(ValueError, 'source changed'):
            ev.verify_r6(self.run, source={**self.source, 'source_sha256': 'c' * 64}, expected=self.expected)

    def test_file_inventory_must_match_even_when_aggregate_is_copied(self):
        with self.assertRaises(ValueError):
            ev.verify_r6(self.run, source={**self.source, 'files': {}}, expected=self.expected)

    def test_each_command_must_bind_candidate(self):
        self.write('commands/probe.1.json', {**self.record, 'source_sha256': 'old'})
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'source mismatch'):
            self.verify()

    def test_minimum_test_count_cannot_be_lowered_in_record(self):
        self.write('commands/probe.1.json', {**self.record, 'min_tests': 0})
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'altered min_tests'):
            self.verify()

    def test_command_cannot_be_replaced_with_true(self):
        self.write('commands/probe.1.json', {**self.record, 'command': ['true']})
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'altered command'):
            self.verify()

    def test_matrix_cannot_reduce_its_own_requirements(self):
        self.write('matrix.json', dict(required=[], selected=[]))
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'required matrix'):
            self.verify()

    def test_zero_executed_tests_fail(self):
        (self.run / 'logs/probe.1.log').write_text('test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out;')
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'command failed'):
            self.verify()

    def test_skip_cannot_be_recorded_as_pass(self):
        self.write('commands/probe.1.json', {**self.record, 'command': ['test', '--skip', 'probe']})
        self.refresh()
        with self.assertRaises(ValueError):
            self.verify()

    def test_old_failure_is_retained_even_after_a_green_retry(self):
        (self.run / 'logs/probe.2.log').write_text('test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;')
        self.write('commands/probe.2.json', {**self.record, 'attempt': 2, 'exit_code': 1, 'log': 'logs/probe.2.log'})
        self.refresh()
        with self.assertRaises(ValueError):
            self.verify()

    def test_changed_log_is_rejected_without_resealing(self):
        (self.run / 'logs/probe.1.log').write_text('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;')
        with self.assertRaises(ValueError):
            self.verify()

    def test_missing_seal_is_not_local_complete(self):
        (self.run / 'evidence-index.json').unlink()
        with self.assertRaises(OSError):
            self.verify()

    def test_empty_package_list_cannot_vacuously_pass(self):
        self.write('package-manifest.json', dict(source_sha256='a' * 64, packages=[]))
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'package variants'):
            self.verify()

    def test_package_bytes_are_rehashed(self):
        (self.run / 'default-native.app/binary').write_bytes(b'changed binary')
        with self.assertRaisesRegex(ValueError, 'package bytes changed'):
            self.verify()

    def test_extra_package_file_is_rejected(self):
        (self.run / 'default-native.app/extra').write_bytes(b'undeclared file')
        with self.assertRaisesRegex(ValueError, 'inventory incomplete'):
            self.verify()

    def test_external_debug_cache_reference_is_rejected(self):
        cache = self.run.parent / 'build-cache'
        cache.mkdir()
        daemon = cache / 'tailsync'
        daemon.write_bytes(b'cached daemon')
        inventory = {str(daemon): ev.digest(daemon)}
        self.packages[-1].update(path=str(daemon), all_files_sha256=inventory,
                                package_sha256=hashlib.sha256(''.join(f'{n}\0{h}\n' for n, h in inventory.items()).encode()).hexdigest())
        self.write('package-manifest.json', dict(source_sha256='a' * 64, packages=self.packages))
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'escapes run'):
            self.verify()

    def test_external_debug_directory_is_rejected(self):
        cache = self.run.parent / 'build-cache'
        cache.mkdir()
        (cache / 'binary').write_bytes(b'synthetic packaged executable')
        self.packages[-1]['path'] = str(cache)
        self.write('package-manifest.json', dict(source_sha256='a' * 64, packages=self.packages))
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'escapes run'):
            self.verify()

    def test_debug_symlink_to_mutable_cache_is_rejected(self):
        cache = self.run.parent / 'cached-binary'
        cache.write_bytes(b'synthetic packaged executable')
        binary = Path(self.packages[-1]['path']) / 'binary'
        binary.unlink()
        binary.symlink_to(cache)
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'escapes run'):
            self.verify()

    def test_debug_snapshot_survives_build_cache_change_and_deletion(self):
        builder_spec = importlib.util.spec_from_file_location(
            'package_builder', Path(__file__).parents[1] / 'build-composite-package-manifest.py')
        builder = importlib.util.module_from_spec(builder_spec)
        builder_spec.loader.exec_module(builder)
        with self.assertRaisesRegex(ValueError, 'sealed run is immutable'):
            builder.snapshot_debug(self.run, self.run / 'nonexistent-daemon')
        (self.run / 'evidence-index.json').unlink()
        cache = self.run.parent / 'build-cache'
        cache.mkdir()
        for name in ('tailsync', 'clipboard-helper'):
            (cache / name).write_bytes(name.encode())
            (cache / name).chmod(0o755)
        snapshot = builder.snapshot_debug(self.run, cache / 'tailsync')
        inventory = {p.name: ev.digest(p) for p in sorted(snapshot.iterdir())}
        self.packages[-1].update(path=str(snapshot), all_files_sha256=inventory,
                                package_sha256=hashlib.sha256(''.join(f'{n}\0{h}\n' for n, h in inventory.items()).encode()).hexdigest())
        self.write('package-manifest.json', dict(source_sha256='a' * 64, packages=self.packages))
        self.refresh()
        for path in cache.iterdir():
            path.write_bytes(b'new cache bytes')
        self.assertEqual(self.verify()['unexpected'], 0)
        for path in cache.iterdir():
            path.unlink()
        cache.rmdir()
        self.assertEqual(self.verify()['unexpected'], 0)
        for name in ('tailsync', 'clipboard-helper'):
            self.assertEqual((snapshot / name).read_bytes(), name.encode())
            self.assertTrue((snapshot / name).stat().st_mode & 0o111)

    def test_changed_auxiliary_source_is_rejected(self):
        self.write('rollback-proof.json', dict(status='PASS', source_sha256='old'))
        self.refresh()
        with self.assertRaisesRegex(ValueError, 'rollback-proof: source mismatch'):
            self.verify()

    def test_unsealed_extra_file_is_rejected(self):
        (self.run / 'unexpected.log').write_text('not sealed')
        with self.assertRaisesRegex(ValueError, 'sealed inventory incomplete'):
            self.verify()


    def test_debug_package_aggregate_uses_the_producer_inventory_order(self):
        package = self.packages[-1]
        base = Path(package["path"])
        (base / "binary").unlink()
        inventory = {}
        for name in ("z-daemon", "a-helper"):
            (base / name).write_bytes(name.encode())
            inventory[name] = ev.digest(base / name)
        package["all_files_sha256"] = inventory
        package["package_sha256"] = hashlib.sha256(''.join(f'{n}\0{h}\n' for n, h in inventory.items()).encode()).hexdigest()
        self.write('package-manifest.json', dict(source_sha256='a' * 64, packages=self.packages))
        self.refresh()
        self.assertEqual(self.verify()['unexpected'], 0)


if __name__ == '__main__':
    unittest.main()
