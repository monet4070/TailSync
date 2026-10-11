#!/usr/bin/env python3
from __future__ import annotations

import contextlib
import hashlib
import html
import importlib.util
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "rich-text-r4-fixtures.py"
MODULE_SPEC = importlib.util.spec_from_file_location("rich_text_r4_fixtures", MODULE_PATH)
fixtures = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(fixtures)

REPO_ROOT = MODULE_PATH.parents[1]
HTML_DOCUMENTS = ("reading.html", "styles.html", "partial.html")
EXPECTED_TREE = (
    "editor.rtf",
    "images/color.jpg",
    "images/static.webp",
    "images/transparent.png",
    "manifest.json",
    "partial.html",
    "reading.html",
    "styles.html",
)


@contextlib.contextmanager
def quiet():
    with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
        yield


def visible_text(markup):
    return html.unescape(re.sub(r"<[^>]+>", "", markup))


def image_sources(markup):
    return re.findall(r'<img[^>]*src="([^"]+)"', markup)


def generate_samples(target, public_base=None):
    arguments = ["--output", str(target)]
    if public_base is not None:
        arguments += ["--public-base", public_base]
    with quiet():
        return fixtures.main(arguments)


class OutputDirectoryRules(unittest.TestCase):
    def test_rejects_relative_output(self):
        with tempfile.TemporaryDirectory() as scratch:
            relative = os.path.relpath(Path(scratch) / "samples", Path.cwd())
            with quiet():
                code = fixtures.main(["--output", relative])
            self.assertEqual(code, fixtures.EXIT_USAGE)
            self.assertFalse((Path(scratch) / "samples").exists())

    def test_rejects_empty_output(self):
        with quiet():
            code = fixtures.main(["--output", ""])
        self.assertEqual(code, fixtures.EXIT_USAGE)

    def test_refuses_existing_directory_without_touching_it(self):
        with tempfile.TemporaryDirectory() as scratch:
            existing = Path(scratch) / "samples"
            existing.mkdir()
            marker = existing / "keep.txt"
            marker.write_text("do not touch\n", encoding="utf-8")
            with quiet():
                code = fixtures.main(["--output", str(existing)])
            self.assertEqual(code, fixtures.EXIT_USAGE)
            self.assertEqual(marker.read_text(encoding="utf-8"), "do not touch\n")
            self.assertEqual(sorted(entry.name for entry in existing.iterdir()), ["keep.txt"])

    def test_refuses_existing_file(self):
        with tempfile.TemporaryDirectory() as scratch:
            existing = Path(scratch) / "samples"
            existing.write_text("occupied\n", encoding="utf-8")
            with quiet():
                code = fixtures.main(["--output", str(existing)])
            self.assertEqual(code, fixtures.EXIT_USAGE)
            self.assertEqual(existing.read_text(encoding="utf-8"), "occupied\n")

    def test_requires_output_argument(self):
        with self.assertRaises(SystemExit) as caught, quiet():
            fixtures.main([])
        self.assertNotEqual(caught.exception.code, 0)

    def test_second_run_never_overwrites_first_output(self):
        with tempfile.TemporaryDirectory() as scratch:
            target = Path(scratch) / "samples"
            self.assertEqual(generate_samples(target), 0)
            manifest_before = (target / "manifest.json").read_bytes()
            with quiet():
                code = fixtures.main(["--output", str(target)])
            self.assertEqual(code, fixtures.EXIT_USAGE)
            self.assertEqual((target / "manifest.json").read_bytes(), manifest_before)


class PublicBaseBoundaries(unittest.TestCase):
    REJECTED_BASES = (
        "",
        "   ",
        "https://exa mple.com/",
        "ftp://example.com/",
        "file:///tmp/samples/",
        "javascript:alert(1)",
        "//example.com/",
        "https:///samples/",
        "https://user:secret@example.com/",
        "https://example.com/?query=1",
        "https://example.com/#fragment",
        "https://example.com:0/",
        "https://example.com:99999/",
        "https://example.com/a/../b",
        "https://example.com/a/./b",
        "http://localhost/",
        "http://localhost:8080/samples/",
        "https://sub.localhost/",
        "http://127.0.0.1/",
        "http://127.1.2.3/",
        "http://[::1]/",
        "http://0.0.0.0/",
        "http://10.0.0.5/",
        "http://172.16.3.4/",
        "http://192.168.1.10/",
        "http://169.254.10.10/",
        "http://100.64.0.1/",
        "http://192.0.2.5/",
        "http://198.18.0.5/",
        "http://224.0.0.1/",
        "http://printer/",
        "http://box.local/",
        "http://box.localdomain/",
        "http://box.internal/",
        "http://box.intranet/",
        "http://box.lan/",
        "http://box.home/",
        "http://box.home.arpa/",
        "http://box.corp/",
        "http://box.private/",
        "http://box.test/",
        "http://box.example/",
        "http://box.invalid/",
    )

    ACCEPTED_BASES = {
        "https://cdn.example.org/assets": "https://cdn.example.org/assets/",
        "https://cdn.example.org/assets/": "https://cdn.example.org/assets/",
        "http://example.com": "http://example.com/",
        "https://93.184.216.34/samples": "https://93.184.216.34/samples/",
        "https://EXAMPLE.com/Samples": "https://example.com/Samples/",
    }

    def test_rejects_local_private_and_credentialed_bases(self):
        with tempfile.TemporaryDirectory() as scratch:
            for index, base in enumerate(self.REJECTED_BASES):
                target = Path(scratch) / f"samples-{index}"
                with self.subTest(base=base):
                    with quiet():
                        code = fixtures.main(["--output", str(target), "--public-base", base])
                    self.assertEqual(code, fixtures.EXIT_USAGE)
                    self.assertFalse(target.exists())

    def test_normalizes_public_bases(self):
        for raw, expected in self.ACCEPTED_BASES.items():
            with self.subTest(base=raw):
                self.assertEqual(fixtures.normalize_public_base(raw), expected)

    def test_rejects_ambiguous_addresses_and_encoded_traversal(self):
        rejected = (
            "https://[", " https://example.com/", "https://example.com/ ",
            "https://example.com/\\reading", "https://example.com/\u0080reading",
            "https://example.com/%2e%2e/reading", "http://0x7f.0.0.1/",
            "http://127.1/", "http://127.000.000.001/", "http://127%2e0%2e0%2e1/",
            "http://[ff02::1]/", "https://example.com/?", "https://example.com/#",
        )
        with tempfile.TemporaryDirectory() as scratch:
            for index, base in enumerate(rejected):
                target = Path(scratch) / f"rejected-{index}"
                with self.subTest(base=base), quiet():
                    self.assertEqual(fixtures.main(["--output", str(target), "--public-base", base]), fixtures.EXIT_USAGE)
                    self.assertFalse(target.exists())

    def test_public_ipv6_literal_keeps_its_authority_brackets(self):
        self.assertEqual(
            fixtures.normalize_public_base("https://[2606:4700:4700::1111]/samples"),
            "https://[2606:4700:4700::1111]/samples/",
        )

    def test_has_no_validation_bypass_flag(self):
        parser = fixtures.build_parser()
        options = sorted({option for action in parser._actions for option in action.option_strings})
        self.assertEqual(options, ["--help", "--output", "--public-base", "-h"])
        for option in options:
            for forbidden in ("allow", "insecure", "bypass", "skip", "force", "private", "local"):
                with self.subTest(option=option, forbidden=forbidden):
                    self.assertNotIn(forbidden, option)


class GeneratedSampleTree(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.output = Path(scratch.name) / "samples"
        self.assertEqual(generate_samples(self.output), 0)
        self.manifest = json.loads((self.output / "manifest.json").read_text(encoding="utf-8"))

    def test_writes_exact_tree(self):
        written = sorted(
            str(path.relative_to(self.output))
            for path in self.output.rglob("*")
            if path.is_file()
        )
        self.assertEqual(written, list(EXPECTED_TREE))

    def test_manifest_digests_match_written_files(self):
        for relative, entry in self.manifest["files"].items():
            payload = (self.output / relative).read_bytes()
            with self.subTest(path=relative):
                self.assertEqual(entry["sha256"], hashlib.sha256(payload).hexdigest())
                self.assertEqual(entry["bytes"], len(payload))
        self.assertNotIn("manifest.json", self.manifest["files"])

    def test_source_images_stay_byte_identical(self):
        self.assertEqual(len(self.manifest["source_images"]), 3)
        for entry in self.manifest["source_images"]:
            source = REPO_ROOT / entry["source"]
            copied = self.output / "images" / entry["name"]
            with self.subTest(image=entry["name"]):
                self.assertTrue(source.is_file())
                self.assertEqual(copied.read_bytes(), source.read_bytes())
                digest = hashlib.sha256(source.read_bytes()).hexdigest()
                self.assertEqual(entry["source_sha256"], digest)
                file_entry = self.manifest["files"]["images/" + entry["name"]]
                self.assertEqual(file_entry["sha256"], digest)
                self.assertTrue(file_entry["unchanged_from_source"])

    def test_html_documents_have_no_script_or_tracking(self):
        forbidden = (
            "<script",
            "<iframe",
            "<link",
            "<object",
            "<embed",
            "@import",
            "url(",
            "javascript:",
            "onload=",
            "onclick=",
            "onerror=",
            "fetch(",
            "xmlhttprequest",
            "srcset",
        )
        for name in HTML_DOCUMENTS:
            markup = (self.output / name).read_text(encoding="utf-8").lower()
            for needle in forbidden:
                with self.subTest(document=name, needle=needle):
                    self.assertNotIn(needle, markup)

    def test_html_references_only_local_images(self):
        for name in HTML_DOCUMENTS:
            markup = (self.output / name).read_text(encoding="utf-8")
            for reference in image_sources(markup):
                with self.subTest(document=name, reference=reference):
                    self.assertTrue(reference.startswith("images/"))
                    self.assertNotIn("..", reference)

    def test_reading_image_order_matches_manifest(self):
        markup = (self.output / "reading.html").read_text(encoding="utf-8")
        expected = self.manifest["files"]["reading.html"]["expected"]
        self.assertEqual(image_sources(markup), [image["path"] for image in expected["images"]])
        self.assertEqual(expected["image_count"], 3)
        self.assertEqual(expected["image_encodings"], ["png", "jpeg", "webp"])
        for index, image in enumerate(expected["images"]):
            with self.subTest(order=index + 1):
                self.assertEqual(image["order"], index + 1)
                self.assertTrue(image["present"])
                self.assertEqual(image["url"], self.manifest["public_base"] + image["path"])
                self.assertTrue((self.output / image["path"]).is_file())

    def test_partial_references_one_missing_image(self):
        markup = (self.output / "partial.html").read_text(encoding="utf-8")
        expected = self.manifest["files"]["partial.html"]["expected"]
        self.assertEqual(image_sources(markup), [image["path"] for image in expected["images"]])
        self.assertEqual(expected["image_count"], 3)
        self.assertEqual(expected["present_image_count"], 2)
        self.assertEqual(expected["missing_image_count"], 1)
        missing = [image for image in expected["images"] if not image["present"]]
        self.assertEqual(len(missing), 1)
        self.assertEqual(missing[0]["path"], fixtures.MISSING_IMAGE_PATH)
        self.assertIn(fixtures.MISSING_IMAGE_PATH, markup)
        self.assertFalse((self.output / missing[0]["path"]).exists())
        for image in expected["images"]:
            if image["present"]:
                self.assertTrue((self.output / image["path"]).is_file())

    def test_styles_cover_semantics_and_inline_css(self):
        markup = (self.output / "styles.html").read_text(encoding="utf-8")
        expected = self.manifest["files"]["styles.html"]["expected"]
        self.assertEqual(expected["image_count"], 0)
        self.assertEqual(expected["image_encodings"], [])
        for tag in ("<h1>", "<h2>", "<h3>", "<blockquote>", "<code>", "<pre>", "<ul>", "<ol>", "<mark>", "<strong>", "<em>", "<u>", "<s>", "<sub>", "<sup>"):
            with self.subTest(tag=tag):
                self.assertIn(tag, markup)
        for declaration in expected["inline_css"]:
            with self.subTest(declaration=declaration):
                self.assertIn(declaration, markup)
        self.assertIn("style=", markup)

    def test_styles_include_explicit_links_and_a_simple_table(self):
        markup = (self.output / "styles.html").read_text(encoding="utf-8")
        expected = self.manifest["files"]["styles.html"]["expected"]
        self.assertIn("<table>", markup)
        self.assertIn('<a href="https://example.com/', markup)
        self.assertIn('<a href="mailto:', markup)
        self.assertEqual(len(expected["links"]), 2)
        self.assertTrue(expected["links"][0]["clickable_in_preview"])
        self.assertFalse(expected["links"][1]["clickable_in_preview"])
        self.assertEqual(expected["table_cells"], [["项目", "内容"], ["公开样本", "简单表格"]])

    def test_rtf_documents_six_basic_emphases(self):
        payload = (self.output / "editor.rtf").read_bytes()
        text = payload.decode("ascii")
        self.assertTrue(text.startswith("{\\rtf1"))
        self.assertTrue(text.rstrip().endswith("}"))
        self.assertIn("\\u", text)
        markers = ("{\\b ", "{\\i ", "{\\ul ", "{\\strike ", "{\\sub ", "{\\sup ")
        positions = [text.index(marker) for marker in markers]
        self.assertEqual(positions, sorted(positions))
        expected = self.manifest["files"]["editor.rtf"]["expected"]
        self.assertEqual(expected["emphasis_count"], 6)
        self.assertEqual(
            expected["emphasis_order"],
            ["bold", "italic", "underline", "strikethrough", "subscript", "superscript"],
        )
        self.assertEqual(expected["image_count"], 0)

    def test_expected_text_appears_in_documents(self):
        for name in HTML_DOCUMENTS:
            markup = (self.output / name).read_text(encoding="utf-8")
            flattened = visible_text(markup)
            for line in self.manifest["files"][name]["expected"]["text"]:
                with self.subTest(document=name, line=line):
                    self.assertIn(line, flattened)

    def test_every_file_entry_is_unverified(self):
        for relative, entry in self.manifest["files"].items():
            with self.subTest(path=relative):
                self.assertEqual(entry["acceptance_status"], "UNKNOWN")


class ManifestStatus(unittest.TestCase):
    def generate_into(self, public_base=None):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        target = Path(scratch.name) / "samples"
        self.assertEqual(generate_samples(target, public_base), 0)
        return target, json.loads((target / "manifest.json").read_text(encoding="utf-8"))

    def test_documentation_default_is_not_deployable(self):
        _, manifest = self.generate_into()
        self.assertTrue(manifest["public_base"].startswith("https://example.com/"))
        deployment = manifest["deployment"]
        self.assertEqual(deployment["public_base_origin"], "documentation_default")
        self.assertFalse(deployment["published_by_tool"])
        self.assertTrue(deployment["manual_publish_required"])
        self.assertFalse(deployment["acceptance_eligible"])
        self.assertIn("未部署", deployment["note"])
        self.assertEqual(manifest["task_id"], "TAILSYNC_R4_FIXTURES_20261009_HIGH")

    def test_public_base_argument_keeps_acceptance_unknown(self):
        target, manifest = self.generate_into("https://samples.example.org/r4")
        self.assertEqual(manifest["public_base"], "https://samples.example.org/r4/")
        deployment = manifest["deployment"]
        self.assertEqual(deployment["public_base_origin"], "cli_argument")
        self.assertFalse(deployment["acceptance_eligible"])
        self.assertFalse(deployment["network_verified"])
        self.assertFalse(deployment["published_by_tool"])
        self.assertEqual(manifest["acceptance"]["status"], "UNKNOWN")
        self.assertFalse(manifest["acceptance"]["real_device_tested"])
        self.assertEqual(
            manifest["files"]["reading.html"]["expected"]["images"][0]["url"],
            "https://samples.example.org/r4/images/transparent.png",
        )
        self.assertEqual(
            manifest["files"]["partial.html"]["expected"]["images"][1]["url"],
            "https://samples.example.org/r4/images/absent-color.jpg",
        )
        self.assertFalse((target / "images" / "absent-color.jpg").exists())

    def test_only_the_styles_link_target_changes_with_public_base(self):
        _, default_manifest = self.generate_into()
        _, hosted_manifest = self.generate_into("https://samples.example.org/r4/")
        for name in ("reading.html", "partial.html", "editor.rtf"):
            with self.subTest(document=name):
                self.assertEqual(
                    default_manifest["files"][name]["sha256"],
                    hosted_manifest["files"][name]["sha256"],
                )
        self.assertNotEqual(default_manifest["files"]["styles.html"]["sha256"], hosted_manifest["files"]["styles.html"]["sha256"])
        self.assertNotEqual(
            default_manifest["files"]["reading.html"]["expected"]["images"][0]["url"],
            hosted_manifest["files"]["reading.html"]["expected"]["images"][0]["url"],
        )


class StandaloneCli(unittest.TestCase):
    def test_runs_as_standalone_cli(self):
        with tempfile.TemporaryDirectory() as scratch:
            target = Path(scratch) / "samples"
            completed = subprocess.run(
                [sys.executable, str(MODULE_PATH), "--output", str(target)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertTrue((target / "manifest.json").is_file())
            repeated = subprocess.run(
                [sys.executable, str(MODULE_PATH), "--output", str(target)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(repeated.returncode, 0)
            self.assertIn("--output", repeated.stderr)

    def test_cli_rejects_private_base(self):
        with tempfile.TemporaryDirectory() as scratch:
            target = Path(scratch) / "samples"
            completed = subprocess.run(
                [sys.executable, str(MODULE_PATH), "--output", str(target), "--public-base", "http://localhost:8000/"],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(completed.returncode, 0)
            self.assertFalse(target.exists())


class RtfEscaping(unittest.TestCase):
    def test_non_bmp_characters_use_signed_utf16_surrogate_units(self):
        self.assertEqual(fixtures._rtf_escape("😀"), "\\u-10179?\\u-8704?")


if __name__ == "__main__":
    unittest.main()
