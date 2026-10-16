"""Exercise the production helper's Finder routing on named AppKit pasteboards."""
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
import uuid


@unittest.skipUnless(os.environ.get('TAILSYNC_FILE_HELPER'), 'set TAILSYNC_FILE_HELPER for native routing tests')
class FinderFileCaptureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workspace = tempfile.TemporaryDirectory(prefix='tailsync-finder-tests-')
        cls.fixture = str(Path(cls.workspace.name) / 'fixture')
        root = Path(__file__).resolve().parents[2]
        subprocess.run(['swiftc', str(root / 'tools/repair-probes/finder-files-fixture.swift'), '-o', cls.fixture], check=True, timeout=60)

    @classmethod
    def tearDownClass(cls):
        cls.workspace.cleanup()

    def capture(self, mode, count=1):
        board = 'tailsync-probe.finder-' + str(uuid.uuid4())
        self.addCleanup(subprocess.run, [self.fixture, board, '1', 'clear'], check=True, timeout=5)
        fixture = subprocess.run([self.fixture, board, str(count), mode], capture_output=True, check=True, timeout=5)
        metadata = json.loads(fixture.stdout)
        self.assertEqual(len(metadata), count)
        if mode == 'finder':
            self.assertEqual(set(metadata[0]), {'public.file-url', 'public.utf8-plain-text', 'public.utf16-external-plain-text', 'com.apple.icns'})
        captures = []
        for command in ['--read-snapshot', '--read-archive']:
            result = subprocess.run([os.environ['TAILSYNC_FILE_HELPER'], '--pasteboard', board, command], capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            data = result.stdout
            self.assertGreaterEqual(len(data), 32)
            self.assertEqual(data[:5], b'CNS1\x01' if command == '--read-snapshot' else b'CNA1\x01')
            self.assertEqual(data[8:16], data[16:24], 'capture must be stable')
            captures.append(data)
        return captures

    def assert_file_selection(self, mode, count):
        for data in self.capture(mode, count):
            self.assertEqual(data[5], 5, 'Finder file URLs plus auxiliary formats must route to the file pipeline')
            self.assertEqual(len(data), 32, 'classification must not read filename or icon payloads')
            self.assertEqual(struct.unpack_from('>H', data, 6 if data[:4] == b'CNS1' else 24)[0], count)

    def test_real_finder_format_set_routes_single_file(self):
        self.assert_file_selection('finder', 1)

    def test_finder_formats_route_multiple_and_large_selections(self):
        for count in [2, 17]:
            with self.subTest(count=count):
                self.assert_file_selection('finder', count)

    def test_file_urls_without_auxiliary_formats_still_route(self):
        self.assert_file_selection('url-only', 17)

    def test_source_marker_does_not_turn_files_into_documents(self):
        self.assert_file_selection('source', 1)

    def test_document_and_qq_formats_keep_mixed_semantics(self):
        for mode in ['html', 'qq', 'opaque']:
            with self.subTest(mode=mode):
                for data in self.capture(mode, 2):
                    self.assertEqual(data[5], 0, 'mixed document items must stay in native/composite capture')

    def test_concealed_marker_suppresses_file_selection_before_capture(self):
        for count in [1, 17]:
            with self.subTest(count=count):
                for data in self.capture('suppressed', count):
                    self.assertEqual(data[5], 6)
                    self.assertEqual(len(data), 32)

    def test_filename_and_icon_without_file_url_are_not_a_file_selection(self):
        for data in self.capture('text-only'):
            self.assertEqual(data[5], 0)


if __name__ == '__main__':
    unittest.main()
