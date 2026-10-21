import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import tarfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

release = load("ures_release", ROOT / "scripts/release.py")

class ReleaseTests(unittest.TestCase):
    def test_checksums_reject_comments_and_keep_previous_summary(self):
        with tempfile.TemporaryDirectory(prefix="ures-checksum-test-") as folder:
            root = Path(folder)
            for target in release.TARGETS:
                name = f"ures-v{release.version()}-{target}.tar.gz"
                (root / name).write_bytes(b"example archive")
                line = f"{hashlib.sha256(b'example archive').hexdigest()}  {name}\n"
                (root / (name + ".sha256")).write_text(line)
            release.checksums(root)
            previous = (root / "SHA256SUMS").read_bytes()
            self.assertEqual(len(previous.splitlines()), 2)
            sidecar = next(root.glob("*.sha256"))
            sidecar.write_text(sidecar.read_text() + "# extra text\n")
            with self.assertRaises(ValueError):
                release.checksums(root)
            self.assertEqual((root / "SHA256SUMS").read_bytes(), previous)

    def test_package_filters_native_dependencies_and_includes_licenses_and_themes(self):
        with tempfile.TemporaryDirectory(prefix="ures-package-test-") as folder:
            root = Path(folder)
            binary = root / "ures"
            binary.write_bytes(b"test binary")
            dep = root / "dep"
            dep.mkdir()
            (dep / "LICENSE").write_text("dependency license")
            packages = {"packages": [{"name": "native-dep", "version": "1.0", "license": "MIT", "manifest_path": str(dep / "Cargo.toml")}]}
            with patch.object(release, "verify"), patch.object(release.subprocess, "check_output", return_value=json.dumps(packages).encode()) as metadata, contextlib.redirect_stdout(io.StringIO()):
                release.package(binary, "aarch64-unknown-linux-musl", root / "dist")
            args = metadata.call_args.args[0]
            self.assertEqual(args[args.index("--filter-platform") + 1], "aarch64-unknown-linux-musl")
            with tarfile.open(next((root / "dist").glob("*.tar.gz"))) as archive:
                self.assertEqual(archive.extractfile("licenses/native-dep-1.0/LICENSE").read(), b"dependency license")
                self.assertIn("config/themes/light.toml", archive.getnames())
                self.assertIn(b"native-dep-1.0: MIT", archive.extractfile("THIRD_PARTY_NOTICES.txt").read())

    def test_version_tag_must_match_package(self):
        with patch.dict(os.environ, {"GITHUB_REF": "refs/tags/v999.0.0"}):
            with self.assertRaises(ValueError):
                release.metadata()
        with patch.dict(os.environ, {"GITHUB_REF": "refs/tags/v" + release.version()}), contextlib.redirect_stdout(io.StringIO()):
            release.metadata()

    def test_release_notes_select_only_current_version(self):
        with tempfile.TemporaryDirectory(prefix="ures-notes-test-") as folder:
            root = Path(folder)
            (root / "Cargo.toml").write_text('[package]\nversion="1.2.3"\n')
            changelog = root / "CHANGELOG.md"
            changelog.write_text("# Changelog\n\n## Unreleased\nFuture changes.\n\n## 1.2.3 — 2026-01-01\n\nCurrent features.\n\n### Details\nMore information.\n\n## 1.2.2\nEarlier features.\n")
            with patch.object(release, "ROOT", root):
                self.assertEqual(release.release_notes(), "Current features.\n\n### Details\nMore information.")
                changelog.write_text("# Changelog\n\n## 1.2.30\nDifferent version.\n")
                with self.assertRaises(ValueError):
                    release.release_notes()

    def test_elf_architecture_and_interpreter_are_verified(self):
        data = bytearray(120)
        data[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", data, 16, 2)
        struct.pack_into("<H", data, 18, 183)
        struct.pack_into("<Q", data, 32, 64)
        struct.pack_into("<HH", data, 54, 56, 1)
        struct.pack_into("<I", data, 64, 1)
        with tempfile.TemporaryDirectory(prefix="ures-elf-test-") as folder:
            path = Path(folder) / "ures"
            path.write_bytes(data)
            with contextlib.redirect_stdout(io.StringIO()):
                release.verify(path, "aarch64-unknown-linux-musl")
            with self.assertRaises(ValueError):
                release.verify(path, "x86_64-unknown-linux-musl")
            struct.pack_into("<I", data, 64, 3)
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                release.verify(path, "aarch64-unknown-linux-musl")

    def test_release_manifest_rejects_links_and_unsafe_names(self):
        with tempfile.TemporaryDirectory(prefix="ures-manifest-test-") as folder:
            root = Path(folder)
            manifest = root / "release-files.txt"
            (root / "README.md").write_text("example")
            manifest.write_text("README.md\n")
            self.assertEqual(set(release.release_files(root)), {"README.md"})
            (root / "linked").symlink_to(root, target_is_directory=True)
            (root / "link.md").symlink_to(root / "README.md")
            for entry in ("link.md", "linked/README.md", "../README.md", ".backup", "README.md\nREADME.md"):
                manifest.write_text(entry + "\n")
                with self.subTest(entry=entry), self.assertRaises(ValueError):
                    release.release_files(root)

    def test_failed_package_preserves_previous_archive(self):
        with tempfile.TemporaryDirectory(prefix="ures-package-test-") as folder:
            root = Path(folder)
            binary = root / "ures"
            binary.write_bytes(b"test binary")
            destination = root / "dist"
            destination.mkdir()
            archive = destination / f"ures-v{release.version()}-x86_64-unknown-linux-musl.tar.gz"
            archive.write_bytes(b"previous artifact")
            with patch.object(release, "verify"), patch.object(release.subprocess, "check_output", return_value=b'{"packages":[]}'), patch.object(release, "write_archive", side_effect=OSError("write failed")), self.assertRaises(OSError):
                release.package(binary, "x86_64-unknown-linux-musl", destination)
            self.assertEqual(archive.read_bytes(), b"previous artifact")
            self.assertEqual(list(destination.iterdir()), [archive])

if __name__ == "__main__":
    unittest.main()
