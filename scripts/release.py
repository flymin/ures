"""Verify and package native static release executables. Python standard library."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import struct
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {"x86_64-unknown-linux-musl": 62, "aarch64-unknown-linux-musl": 183}

def version():
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]

def metadata():
    expected = "v" + version()
    ref = os.environ.get("GITHUB_REF", "")
    if ref.startswith("refs/tags/") and ref != "refs/tags/" + expected:
        raise ValueError(f"release tag must equal {expected}")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    assert any(p["name"] == "ures" and p["version"] == version() for p in lock["package"]), "Cargo.lock version mismatch"
    print(expected)

def release_notes():
    """Return only the current version's changelog section."""
    heading = re.compile(r"^## \[?" + re.escape(version()) + r"\]?(?:\s|$)")
    selected = False
    lines = []
    for line in (ROOT / "CHANGELOG.md").read_text().splitlines():
        if line.startswith("## "):
            if selected:
                break
            selected = bool(heading.match(line))
        elif selected:
            lines.append(line)
    notes = "\n".join(lines).strip()
    if not notes:
        raise ValueError("missing release notes for the current package version")
    return notes

def verify(binary, target):
    data = Path(binary).read_bytes()
    if data[:6] != b"\x7fELF\x02\x01":
        raise ValueError("expected a 64-bit little-endian ELF")
    if len(data) < 64 or struct.unpack_from("<H", data, 16)[0] not in (2, 3):
        raise ValueError("expected an ELF executable")
    machine = struct.unpack_from("<H", data, 18)[0]
    if machine != TARGETS[target]:
        raise ValueError(f"ELF architecture does not match {target}")
    phoff = struct.unpack_from("<Q", data, 32)[0]
    phsize, count = struct.unpack_from("<HH", data, 54)
    if not count or phsize < 56 or phoff + count * phsize > len(data):
        raise ValueError("invalid ELF program headers")
    for index in range(count):
        start = phoff + index * phsize
        kind = struct.unpack_from("<I", data, start)[0]
        if kind == 3:
            raise ValueError("ELF has an interpreter (PT_INTERP)")
        if kind == 2:
            offset = struct.unpack_from("<Q", data, start + 8)[0]
            size = struct.unpack_from("<Q", data, start + 32)[0]
            for pos in range(offset, offset + size, 16):
                tag = struct.unpack_from("<q", data, pos)[0]
                if tag == 0:
                    break
                if tag == 1:
                    raise ValueError("ELF has a shared-library dependency (DT_NEEDED)")
    print(f"Verified static ELF: {target}")

def package(binary, target, destination):
    verify(binary, target)
    files = {"ures": Path(binary).resolve(), **release_files(ROOT)}
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    name = f"ures-v{version()}-{target}.tar.gz"
    archive = destination / name
    cargo = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--offline", "--format-version=1",
        "--filter-platform", target,
    ], cwd=ROOT))
    notices = []
    for item in sorted(cargo["packages"], key=lambda p: (p["name"], p["version"])):
        if item["name"] == "ures":
            continue
        key = item["name"] + "-" + item["version"]
        notices.append(f"{key}: {item.get('license') or 'see license file'}")
        directory = Path(item["manifest_path"]).parent
        candidates = set()
        for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*"):
            candidates.update(directory.glob(pattern))
        if item.get("license_file"):
            candidates.add(directory / item["license_file"])
        for path in sorted(candidates):
            if path.is_file():
                if path.is_symlink() or any(parent.is_symlink() for parent in path.parents):
                    raise ValueError("license input must not be a symbolic link")
                if key != Path(key).name or path.name.startswith("."):
                    raise ValueError("invalid dependency license name")
                files[f"licenses/{key}/{path.name}"] = path
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    with tempfile.TemporaryDirectory(prefix=".ures-package-", dir=destination) as temporary:
        pending = Path(temporary) / name
        write_archive(pending, files, notices, epoch)
        digest = hashlib.sha256(pending.read_bytes()).hexdigest()
        checksum = Path(temporary) / (name + ".sha256")
        checksum.write_text(f"{digest}  {name}\n")
        pending.replace(archive)
        checksum.replace(destination / checksum.name)
    print(archive.name)


def release_files(root):
    """Load the explicit archive manifest, rejecting unsafe paths and links."""
    files = {}
    for line in (root / "release-files.txt").read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        name = PurePosixPath(line)
        if name.is_absolute() or any(part.startswith(".") for part in name.parts) or str(name) != line or line in files:
            raise ValueError("invalid release manifest entry")
        path = root / line
        if any(parent.is_symlink() for parent in (path, *path.parents) if parent != root.parent):
            raise ValueError("release inputs must not be symbolic links")
        if not path.is_file():
            raise ValueError("missing release input")
        files[line] = path
    if not files:
        raise ValueError("empty release manifest")
    return files


def checksums(directory):
    """Validate canonical sidecars; never copy arbitrary checksum-file text."""
    directory = Path(directory)
    lines = []
    for target in TARGETS:
        name = f"ures-v{version()}-{target}.tar.gz"
        archive = directory / name
        checksum = directory / (name + ".sha256")
        if archive.is_symlink() or checksum.is_symlink():
            raise ValueError("release checksum inputs must not be symbolic links")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        canonical = f"{digest}  {name}\n".encode()
        if checksum.read_bytes() != canonical:
            raise ValueError("release checksum file is not canonical or does not match")
        lines.append(canonical)
    with tempfile.TemporaryDirectory(prefix=".ures-checksums-", dir=directory) as temporary:
        pending = Path(temporary) / "SHA256SUMS"
        pending.write_bytes(b"".join(lines))
        pending.replace(directory / "SHA256SUMS")


def write_archive(archive, files, notices, epoch):
    with archive.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=epoch) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as tar:
            for filename, path in sorted(files.items()):
                info = tar.gettarinfo(str(path), arcname=filename)
                info.uid = info.gid = 0
                info.uname = info.gname = "root"
                info.mtime = epoch
                info.pax_headers = {}
                info.mode = 0o755 if filename == "ures" else 0o644
                with path.open("rb") as source:
                    tar.addfile(info, source)
            data = ("Third-party dependencies (including build-time dependencies)\n\n" + "\n".join(notices) + "\n").encode()
            info = tarfile.TarInfo("THIRD_PARTY_NOTICES.txt")
            info.size = len(data)
            info.mtime = epoch
            info.mode = 0o644
            tar.addfile(info, io.BytesIO(data))

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("metadata")
    commands.add_parser("version")
    commands.add_parser("notes")
    sub = commands.add_parser("checksums")
    sub.add_argument("--directory", required=True)
    for action in ("verify", "package"):
        sub = commands.add_parser(action)
        sub.add_argument("--binary", required=True)
        sub.add_argument("--target", choices=TARGETS, required=True)
        if action == "package":
            sub.add_argument("--destination", required=True)
    args = parser.parse_args()
    if args.command == "version":
        print(version())
    elif args.command == "metadata":
        metadata()
    elif args.command == "notes":
        print(release_notes())
    elif args.command == "verify":
        verify(args.binary, args.target)
    elif args.command == "checksums":
        checksums(args.directory)
    else:
        package(args.binary, args.target, args.destination)
