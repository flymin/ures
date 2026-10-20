"""Check source/release artifacts without echoing sensitive matches. Standard library only."""
import argparse
import html
import ipaddress
import json
import os
from pathlib import Path, PurePosixPath
import re
import socket
import stat
import subprocess
import sys
import tarfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
SKIP = {".git", ".logs", "dist", "target", "__pycache__"}
TEXT_SUFFIXES = {"", ".rs", ".py", ".sh", ".md", ".toml", ".yml", ".yaml", ".json", ".svg", ".txt", ".lock"}
DOCUMENTATION_NETS = tuple(ipaddress.ip_network(value) for value in (
    "192.0.2.0/24", "198.51.100.0/24", "203.0.113.0/24", "2001:db8::/32",
))
GENERIC_ACCOUNTS = {"root", "user", "runner", "nobody", "alice", "bob", "demo", "test", "example"}


def local_terms(root=ROOT):
    """Discover in memory; never save machine facts to the repository or output."""
    result = []
    def add(value, category):
        if value and len(value) >= 4 and value != "/":
            result.append((value, category))
    for key in ("HOME", "URES_VENV", "URES_PROJECT", "URES_TOOLCHAIN", "URES_INSTALL_DIR", "CARGO_HOME", "CARGO_TARGET_DIR"):
        add(os.environ.get(key), "local filesystem path")
    add(str(root.resolve()), "local project path")
    names = [os.environ.get(key) for key in ("USER", "LOGNAME")]
    try:
        import pwd
        account = pwd.getpwuid(os.geteuid())
        names.append(account.pw_name)
        add(account.pw_dir, "local account path")
        # Components specific to a mount root can leak even without a full path.
        for part in Path(account.pw_dir).parts:
            if len(part) >= 10 and not part.startswith("."):
                add(part, "local filesystem identifier")
    except (ImportError, KeyError):
        pass
    for name in names:
        if name and name not in GENERIC_ACCOUNTS:
            add(name, "local account identifier")
    try:
        import grp
        for gid in set(os.getgroups()) | {os.getgid()}:
            try:
                name = grp.getgrgid(gid).gr_name
                if name not in GENERIC_ACCOUNTS | {"users", "staff", "wheel", "sudo", "nogroup", "docker"}:
                    add(name, "local group identifier")
            except KeyError:
                pass
    except ImportError:
        pass
    for host in (socket.gethostname(), os.environ.get("HOSTNAME")):
        if host and host != "localhost":
            add(host, "local hostname")
    for file in ("/etc/hostname", "/proc/sys/kernel/hostname"):
        try:
            host = Path(file).read_text().strip()
            if host != "localhost":
                add(host, "local hostname")
        except OSError:
            pass
    uid = str(os.geteuid())
    if uid not in {"0", "1000", "1001"}:
        add(uid, "local UID")
    # Read interface addresses without DNS queries or outbound connections.
    try:
        fib = Path("/proc/net/fib_trie").read_text()
        for value in re.findall(r"\|-- ([0-9.]+)\s+\n\s*/32 host LOCAL", fib):
            address = ipaddress.ip_address(value)
            if not address.is_loopback:
                add(str(address), "local interface address")
        for line in Path("/proc/net/if_inet6").read_text().splitlines():
            address = ipaddress.IPv6Address(int(line.split()[0], 16))
            if not address.is_loopback:
                add(str(address), "local interface address")
                add(address.exploded, "local interface address")
    except (OSError, ValueError):
        pass
    return sorted(set(result), key=lambda item: len(item[0]), reverse=True)


def load_terms(path):
    if not path:
        return []
    entries = json.loads(Path(path).read_text())
    if not isinstance(entries, list) or any(
        not isinstance(item, dict) or not isinstance(item.get("value"), str)
        or not item["value"] or not isinstance(item.get("category"), str)
        for item in entries
    ):
        raise ValueError("private terms file must contain value/category objects")
    return [(item["value"], "private publication term") for item in entries]


def term_pattern(value, category):
    if category == "local UID":
        return re.compile(r"(?<![\w])" + re.escape(value) + r"(?![\w])")
    if category == "local account identifier":
        return re.compile(r"(?<![\w.-])" + re.escape(value) + r"(?![\w.-])", re.I)
    return re.compile(re.escape(value), re.I)


def safe_label(label, terms):
    for value, category in terms:
        label = term_pattern(value, category).sub("[redacted]", label)
    return label


def external_svg_url(value):
    return any(not match[1].strip().strip("'\"").startswith("#") for match in re.finditer(r"url\(([^)]*)\)", value, re.I))


def content_findings(data, label, terms, *, text=True):
    """Only return locations/categories. A failure must not disclose the match."""
    decoded = data.decode("utf-8", errors="replace")
    views = [decoded]
    if text:
        # Escaped JSON strings and per-cell SVG text must not bypass the gate.
        unescaped = html.unescape(decoded)
        for _ in range(3):
            unescaped = re.sub(r"\\+", lambda _: "\\", unescaped)
            unescaped = re.sub(r"\\u([0-9a-fA-F]{4})", lambda m: chr(int(m[1], 16)), unescaped).replace(r"\/", "/")
        views.append(unescaped)
        if label.lower().endswith(".svg") or decoded.lstrip().startswith("<svg"):
            try:
                svg = ET.fromstring(decoded)
                rows = {}
                for element in svg.iter():
                    views.extend(element.attrib.values())
                    tag = element.tag.rsplit("}", 1)[-1]
                    views.append("".join(element.itertext()))
                    if tag in {"image", "foreignObject", "script"} or any(
                        (key.rsplit("}", 1)[-1] == "href" and not value.startswith("#"))
                        or external_svg_url(value)
                        for key, value in element.attrib.items()
                    ) or (tag == "style" and ("@import" in "".join(element.itertext()).lower() or external_svg_url("".join(element.itertext())))):
                        return [(safe_label(label, terms), 1, "non-inspectable SVG resource")]
                    if tag == "text":
                        rows.setdefault(element.get("y", "0"), []).append((float(element.get("x", "0")), "".join(element.itertext())))
                views.extend("".join(value for _, value in sorted(row)) for row in rows.values())
            except (ET.ParseError, ValueError):
                return [(safe_label(label, terms), 1, "unreadable SVG")]
    found = set()
    for value in views:
        for private, category in terms:
            for match in term_pattern(private, category).finditer(value):
                found.add((safe_label(label, terms), value.count("\n", 0, match.start()) + 1, category))
        if not text:
            continue
        for match in re.finditer(r"(?<![\w.])(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?![\w.])|(?<![\w:])[0-9a-fA-F]*:[0-9a-fA-F:.]+(?![\w:])", value):
            try:
                address = ipaddress.ip_address(match[0])
            except ValueError:
                continue
            if not address.is_loopback and not address.is_unspecified and not any(address in network for network in DOCUMENTATION_NETS if address.version == network.version):
                found.add((safe_label(label, terms), value.count("\n", 0, match.start()) + 1, "non-example IP address"))
        if re.search(r"\b[\w-]+\.(?:internal|cluster\.local)\b", value, re.I):
            found.add((safe_label(label, terms), 1, "internal hostname"))
        if any(match[1] not in GENERIC_ACCOUNTS | {"u"} for match in re.finditer(r"/(?:home|Users)/([A-Za-z0-9_.-]+)(?:/|(?=['\"\s]))", value)) or re.search(r"/mnt/[A-Za-z0-9_.-]+/", value):
            found.add((safe_label(label, terms), 1, "non-example filesystem path"))
        if re.search(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----", value):
            found.add((safe_label(label, terms), 1, "private key material"))
    return sorted(found)


def check_tree(root=ROOT, terms=()):
    findings = []
    def walk(folder):
        for path in sorted(folder.iterdir()):
            relative = path.relative_to(root)
            if path.name in SKIP and folder == root:
                continue
            if path.is_symlink():
                findings.append((safe_label(str(relative), terms), 1, "source symlink"))
            elif path.name.startswith(".") and path.name not in {".gitignore", ".github", ".cargo"}:
                findings.append((safe_label(str(relative), terms), 1, "unexpected hidden source file"))
            elif path.is_dir():
                walk(path)
            elif not stat.S_ISREG(path.stat().st_mode):
                findings.append((safe_label(str(relative), terms), 1, "non-regular source file"))
            elif (path.name.startswith(".") and path.name != ".gitignore") or path.suffix not in TEXT_SUFFIXES:
                findings.append((safe_label(str(relative), terms), 1, "unexpected or generated source file"))
            else:
                data = path.read_bytes()
                try:
                    data.decode("utf-8")
                except UnicodeDecodeError:
                    findings.append((safe_label(str(relative), terms), 1, "binary file in source tree"))
                    continue
                findings.extend(content_findings(data, str(relative), terms))
                findings.extend(content_findings(str(relative).encode(), str(relative) + " (filename)", terms))
    walk(root)
    # gitignored files must not be force-added to a commit. Author metadata is
    # deliberately outside this scan; commit bodies/source remain reviewable.
    if (root / ".git").exists():
        tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
        for name in tracked:
            if name and (any(part in SKIP for part in PurePosixPath(name).parts) or any(part.startswith(".") and part not in {".github", ".cargo", ".gitignore"} for part in PurePosixPath(name).parts)):
                findings.append((safe_label(name, terms), 1, "private/generated file in Git index"))
    return findings


def check_archive(path, terms=()):
    findings = []
    with tarfile.open(path, "r:gz") as archive:
        for member in archive:
            name = PurePosixPath(member.name)
            label = "archive/" + member.name
            metadata = json.dumps({"name": member.name, "link": member.linkname, "owner": [member.uname, member.gname], "pax": member.pax_headers}, ensure_ascii=False)
            findings.extend(content_findings(metadata.encode(), label + " (metadata)", terms))
            if name.is_absolute() or ".." in name.parts or not member.isfile():
                findings.append((safe_label(label, terms), 1, "unsafe archive member"))
                continue
            if member.uid != 0 or member.gid != 0 or member.uname not in {"", "root"} or member.gname not in {"", "root"}:
                findings.append((safe_label(label, terms), 1, "non-anonymous archive ownership"))
            # Preserve upstream legal attribution; only local facts apply there.
            data = archive.extractfile(member).read()
            findings.extend(content_findings(data, label, terms, text=member.name != "ures" and not member.name.startswith("licenses/")))
    return findings


def release_files(root=ROOT):
    """Explicit publication inputs; neither files nor their parents may be links."""
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


def require_clean(findings):
    if findings:
        for path, line, category in sorted(set(findings)):
            print(f"{path}:{line}: {category}", file=sys.stderr)
        raise ValueError("publication privacy check failed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--terms-file", default=os.environ.get("URES_PRIVACY_TERMS_FILE"))
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    try:
        terms = local_terms(args.root) + load_terms(args.terms_file)
        if args.binary:
            findings = content_findings(args.binary.read_bytes(), "release binary", terms, text=False)
        elif args.archive:
            findings = check_archive(args.archive, terms)
        else:
            findings = check_tree(args.root, terms)
    except (OSError, ValueError, tarfile.TarError, subprocess.CalledProcessError):
        print("Privacy check failed: an input could not be inspected.", file=sys.stderr)
        return 1
    for path, line, category in sorted(set(findings)):
        print(f"{path}:{line}: {category}", file=sys.stderr)
    if findings:
        return 1
    print("Privacy check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
