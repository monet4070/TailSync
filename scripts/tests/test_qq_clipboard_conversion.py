"""QQ private message conversion through the production macOS helper."""
import base64
import json
import os
from pathlib import Path
import plistlib
import struct
import subprocess
import unittest
import zlib


def png(color):
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 1, 1, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(b'\0' + bytes(color))) + chunk(b'IEND', b'')


@unittest.skipUnless(os.environ.get('TAILSYNC_QQ_HELPER'), 'set TAILSYNC_QQ_HELPER for native helper tests')
class QQConversionTests(unittest.TestCase):
    def run_conversion(self, rows, items=2, supplied=None, fallback=None, raw=None):
        data = raw if raw is not None else plistlib.dumps(rows, fmt=plistlib.FMT_BINARY)
        request = {'elements': base64.b64encode(data).decode(), 'items': items}
        if supplied is not None:
            request['attachments'] = {key: base64.b64encode(value).decode() for key, value in supplied.items()}
        if fallback is not None:
            request['fallback_png'] = base64.b64encode(fallback).decode()
        result = subprocess.run([os.environ['TAILSYNC_QQ_HELPER'], '--qq-to-portable'], input=json.dumps(request).encode(), capture_output=True, timeout=10)
        if result.returncode:
            return None
        value = json.loads(result.stdout)
        return base64.b64decode(value['html']).decode(), base64.b64decode(value['text']).decode(), value['missing_images']

    def rows(self):
        return [{'TencentElementType': kind, 'TencentElementValue': value} for kind, value in [(1, '/selected/first.png'), (11, '\n中文 <b>& "\n'), (1, '/selected/second.png'), (11, '\nafter')]]

    def test_full_message_retains_image_and_text_order(self):
        red, blue = png([255, 0, 0, 255]), png([0, 0, 255, 255])
        html, text, missing = self.run_conversion(self.rows(), supplied={'/selected/first.png': red, '/selected/second.png': blue})
        self.assertEqual(missing, 0)
        self.assertEqual(text, '\n中文 <b>& "\n\nafter')
        self.assertEqual(html.count('data:image/png;base64,'), 2)
        self.assertLess(html.index('<img'), html.index('中文'))
        self.assertLess(html.index('中文'), html.rindex('<img'))
        self.assertLess(html.rindex('<img'), html.index('after'))
        self.assertIn('&lt;b&gt;&amp; &quot;', html)
        self.assertNotIn('/selected/', html)

    def test_inaccessible_files_preserve_text_and_image_positions(self):
        html, text, missing = self.run_conversion(self.rows())
        self.assertEqual(missing, 2)
        self.assertIn('中文', text)
        self.assertEqual(html.count('<img'), 2)
        self.assertNotIn('/selected/', html)

    def test_first_item_fallback_is_not_reused_for_other_images(self):
        html, _, missing = self.run_conversion(self.rows(), fallback=png([1, 2, 3, 255]))
        self.assertEqual(missing, 1)
        self.assertEqual(html.count('data:image/png;base64,'), 1)
        self.assertIn('qq-missing-image:1', html)

    def test_invalid_fallback_never_claims_a_saved_image(self):
        self.assertEqual(self.run_conversion(self.rows(), fallback=b'not an image')[2], 2)

    def test_text_only_qq_message_is_readable(self):
        value = self.run_conversion([{'TencentElementType': 11, 'TencentElementValue': 'hello 中文'}], items=1)
        self.assertEqual(value[1:], ('hello 中文', 0))

    def test_unknown_elements_are_not_silently_discarded(self):
        rows = self.rows(); rows.append({'TencentElementType': 99, 'TencentElementValue': 'unknown'})
        self.assertIsNone(self.run_conversion(rows))

    def test_malformed_element_types_and_keys_are_refused(self):
        for row in [{'TencentElementType': True, 'TencentElementValue': 'x'}, {'TencentElementType': 1.5, 'TencentElementValue': 'x'}, {'TencentElementType': '11', 'TencentElementValue': 'x'}, {'TencentElementType': 11, 'TencentElementValue': b'x'}, {'TencentElementType': 11}, {'TencentElementType': 11, 'TencentElementValue': 'x', 'extra': 1}]:
            with self.subTest(row=row): self.assertIsNone(self.run_conversion([row], items=1))

    def test_truncated_or_non_plist_input_is_refused(self):
        for data in [b'bad', b'bplist00', plistlib.dumps(self.rows())[:40]]:
            with self.subTest(length=len(data)): self.assertIsNone(self.run_conversion([], raw=data))

    def test_other_native_items_are_not_flattened(self):
        self.assertIsNone(self.run_conversion(self.rows(), items=3))

    def test_resource_and_text_limits_are_enforced(self):
        self.assertIsNone(self.run_conversion([{'TencentElementType': 1, 'TencentElementValue': '/x'}] * 9, items=9))
        self.assertIsNone(self.run_conversion([{'TencentElementType': 11, 'TencentElementValue': 'x' * (1024 * 1024 + 1)}], items=1))


if __name__ == '__main__':
    unittest.main()
