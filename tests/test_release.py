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
sanitize = load("ures_sanitize", ROOT / "scripts/sanitize_log.py")

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
            sidecar.write_text(sidecar.read_text() + "# example-retired-host\n")
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

    def test_private_build_identifiers_are_redacted(self):
        with patch.dict(os.environ, {"HOME": "/home/alice", "URES_PROJECT": "/home/alice/local/src/ures", "USER": "alice", "LOGNAME": "alice"}):
            result = sanitize.redact("alice /home/alice/local/src/ures/src/lib.rs /home/alice/log")
        self.assertNotIn("alice", result)
        self.assertIn("${URES_PROJECT}", result)
        self.assertIn("${HOME}", result)

if __name__ == "__main__":
    unittest.main()
