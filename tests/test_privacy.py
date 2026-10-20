"""Publication privacy regressions. All identifiers and artifacts are synthetic."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import check_privacy as privacy
import release
import sanitize_log

CANARY = "example-retired-host"
TERMS = [(CANARY, "private publication term")]


class PrivacyTests(unittest.TestCase):
    def scan(self, data, label="fixture.txt", terms=TERMS):
        return privacy.content_findings(data.encode(), label, terms)

    def test_matches_and_locations_never_echo_private_terms(self):
        result = self.scan("safe\n" + CANARY, CANARY + ".txt")
        self.assertTrue(result)
        self.assertNotIn(CANARY, repr(result))
        self.assertEqual(result[0][1], 2)

    def test_json_and_split_svg_are_inspected(self):
        encoded = "".join("\\u%04x" % ord(char) for char in CANARY)
        for _ in range(5):
            self.assertTrue(self.scan(encoded))
            encoded = json.dumps(encoded)
        cells = "".join(f'<text x="{i}" y="10">{char}</text>' for i, char in enumerate(CANARY))
        for prefix in ("", '<!--' + 'a' * 2000 + '-->'):
            self.assertTrue(self.scan(prefix + '<svg xmlns="http://www.w3.org/2000/svg">' + cells + '</svg>', "fixture.svg"))
        self.assertTrue(self.scan('<s:svg xmlns:s="http://www.w3.org/2000/svg"><s:title>' + CANARY + '</s:title></s:svg>', "fixture.svg"))
        self.assertTrue(self.scan('<svg aria-label="' + CANARY + '"/>', "fixture.svg"))

    def test_svg_resources_must_be_inspectable(self):
        for body in ('<image href="data:image/png;base64,AAAA"/>', '<script/>', '<foreignObject/>', '<use href="https://example.com/a.svg"/>', '<style>@import "https://example.com/a.css";</style>', '<rect fill="url( https://example.com/a )"/>'):
            with self.subTest(body=body):
                self.assertTrue(self.scan('<svg>' + body + '</svg>', "fixture.svg", []))
        self.assertFalse(self.scan('<svg><defs><linearGradient id="bg"/></defs><rect fill="url(#bg)"/></svg>', "fixture.svg", []))

    def test_only_documentation_or_localhost_addresses_are_examples(self):
        self.assertFalse(self.scan("192.0.2.10 198.51.100.20 203.0.113.30 2001:db8::1 127.0.0.1 ::1 f64::min", terms=[]))
        for address in (".".join(["10", "23", "45", "67"]), ":".join(["fd00", "abcd", "", "1"])):
            self.assertTrue(self.scan(address, terms=[]))

    def test_terms_file_cannot_inject_private_reporting_categories(self):
        with tempfile.TemporaryDirectory(prefix="ures-privacy-test-") as folder:
            path = Path(folder) / "terms.json"
            path.write_text(json.dumps([{"value": CANARY, "category": CANARY}]))
            findings = self.scan(CANARY, terms=privacy.load_terms(path))
            self.assertNotIn(CANARY, repr(findings))

    def test_nested_generated_directories_and_hidden_files_are_not_skipped(self):
        with tempfile.TemporaryDirectory(prefix="ures-privacy-test-") as folder:
            root = Path(folder)
            (root / "docs/dist").mkdir(parents=True)
            (root / "docs/dist/fixture.txt").write_text(CANARY)
            (root / "docs/.backup").write_text("example")
            self.assertEqual(len(privacy.check_tree(root, TERMS)), 2)

    def test_svg_filename_is_not_parsed_as_xml(self):
        with tempfile.TemporaryDirectory(prefix="ures-privacy-test-") as folder:
            root = Path(folder)
            (root / "fixture.svg").write_text('<svg><text>example</text></svg>')
            self.assertFalse(privacy.check_tree(root, TERMS))

    def test_force_added_private_artifacts_fail_the_gate(self):
        with tempfile.TemporaryDirectory(prefix="ures-privacy-test-") as folder:
            root = Path(folder)
            subprocess.run(["git", "init", "-q", str(root)], check=True, capture_output=True)
            (root / ".logs").mkdir()
            (root / ".logs/fixture.txt").write_text("example")
            subprocess.run(["git", "add", ".logs/fixture.txt"], cwd=root, check=True, capture_output=True)
            result = privacy.check_tree(root, [])
            self.assertIn("private/generated file in Git index", repr(result))

    def test_release_manifest_rejects_links_and_unsafe_names(self):
        with tempfile.TemporaryDirectory(prefix="ures-privacy-test-") as folder:
            root = Path(folder)
            manifest = root / "release-files.txt"
            (root / "README.md").write_text("example")
            manifest.write_text("README.md\n")
            self.assertEqual(set(privacy.release_files(root)), {"README.md"})
            (root / "linked").symlink_to(root, target_is_directory=True)
            (root / "link.md").symlink_to(root / "README.md")
            for entry in ("link.md", "linked/README.md", "../README.md", ".backup", "README.md\nREADME.md"):
                manifest.write_text(entry + "\n")
                with self.subTest(entry=entry), self.assertRaises(ValueError):
                    privacy.release_files(root)
            self.assertIn("source symlink", repr(privacy.check_tree(root, [])))

    def archive(self, path, name, data=b"example", **metadata):
        with tarfile.open(path, "w:gz") as archive:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            for key, value in metadata.items():
                setattr(info, key, value)
            archive.addfile(info, io.BytesIO(data))

    def test_archive_content_metadata_ownership_and_links_are_inspected(self):
        with tempfile.TemporaryDirectory(prefix="ures-privacy-test-") as folder:
            path = Path(folder) / "fixture.tar.gz"
            for name, data, metadata in (
                ("README.md", CANARY.encode(), {}),
                ("ures", b"\x00" + CANARY.encode(), {}),
                ("README.md", b"example", {"pax_headers": {"comment": CANARY}}),
                ("README.md", b"example", {"uid": 1000}),
                ("link", b"", {"type": tarfile.SYMTYPE, "linkname": CANARY}),
                ("../README.md", b"example", {}),
            ):
                self.archive(path, name, data, **metadata)
                findings = privacy.check_archive(path, TERMS)
                self.assertTrue(findings)
                self.assertNotIn(CANARY, repr(findings))
            self.archive(path, "fixture.svg", b"<svg/>")
            self.assertFalse(privacy.check_archive(path, TERMS))
            self.archive(path, "licenses/dep/LICENSE", b"Copyright Example Contributor")
            self.assertFalse(privacy.check_archive(path, TERMS))
            self.archive(path, "licenses/dep/LICENSE", CANARY.encode())
            self.assertTrue(privacy.check_archive(path, TERMS))

    def test_failed_package_does_not_replace_previous_archive(self):
        with tempfile.TemporaryDirectory(prefix="ures-privacy-test-") as folder:
            root = Path(folder)
            (root / "Cargo.toml").write_text('[package]\nversion="0.1.0"\n')
            (root / "release-files.txt").write_text("README.md\n")
            (root / "README.md").write_text("example")
            binary = root / "ures"
            binary.write_bytes(b"example binary")
            dep = root / "dep"
            dep.mkdir()
            (dep / "LICENSE").write_text(CANARY)
            destination = root / "dist"
            destination.mkdir()
            archive = destination / "ures-v0.1.0-x86_64-unknown-linux-musl.tar.gz"
            archive.write_bytes(b"previous artifact")
            cargo = {"packages": [{"name": "dep", "version": "1.0", "manifest_path": str(dep / "Cargo.toml")}]}
            with patch.object(release, "ROOT", root), patch.object(release, "verify"), patch.object(privacy, "local_terms", return_value=TERMS), patch.object(privacy, "check_tree", return_value=[]), patch.object(release.subprocess, "check_output", return_value=json.dumps(cargo).encode()), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(ValueError):
                release.package(binary, "x86_64-unknown-linux-musl", destination)
            self.assertEqual(archive.read_bytes(), b"previous artifact")
            self.assertEqual(list(destination.iterdir()), [archive])

    def test_build_log_redaction_uses_local_facts(self):
        with patch.object(sanitize_log, "LOCAL_TERMS", [(CANARY, "local hostname")]):
            self.assertEqual(sanitize_log.redact(CANARY), "${HOSTNAME}")


if __name__ == "__main__":
    unittest.main()
