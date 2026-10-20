# Publication privacy

All checked-in fixtures, examples, golden output and SVG previews use invented
accounts, processes, paths and dates. Use `alice`/UID `1000`, `login.example`,
`/opt/ures-demo` and documentation addresses such as `192.0.2.10` for examples.
Generate previews from `examples/preview.rs` or the TUI test backend with
`TZ=UTC`; never substitute a live resource report.

The application intentionally displays the monitored user's real environment.
Runtime JSON, terminal captures, OOM messages, configuration and process
arguments can contain private information. Replace those values with fictional
equivalents before sharing a bug report. Smoke tests keep live captures in
memory and withhold them from assertion failures.

Run the publication gate with an existing Python 3.11+ interpreter:

```bash
python3 -B scripts/check_privacy.py
python3 -B scripts/check_privacy.py --binary /path/to/ures
python3 -B scripts/check_privacy.py --archive dist/ures-v0.1.0-x86_64-unknown-linux-musl.tar.gz
```

The gate discovers local account, group, hostname, interface address and path
identifiers in memory. It checks text, escaped JSON, SVG text/metadata, binary
strings and archive metadata without printing matched values. It rejects
non-example IP addresses, private key markers, source symlinks, unexpected
hidden/generated source files and external/embedded SVG resources. Archive
ownership is normalized. Upstream license attribution and Git commit authors
are retained; upstream license contents are still checked for local identifiers.

For identifiers from another machine or retired workload, set
`URES_PRIVACY_TERMS_FILE` to a JSON file **outside the repository**:

```json
[{"value": "example-retired-host", "category": "private identifier"}]
```

Only fixed categories and redacted locations are reported. The term file is
never copied into an archive. The scanner is a publication aid; review new
fixtures and assets for identifying details that cannot be inferred locally.

`build.sh` checks source and the release binary. The release packager checks
source and the completed archive before replacing any existing artifact. Its
data files come exclusively from `release-files.txt`; it does not recursively
copy directories. Cargo's source package also has an explicit include list.
CI uploads exact expected filenames, and publishing rechecks both architectures.

Keep `.logs`, `target`, `dist`, compiler caches, private term files and runtime
reports out of Git. The gate rejects excluded/generated paths in the Git index.
Distribute a verified release archive or Cargo source package rather than
copying a working directory. Review any pre-existing Git history separately;
the source gate does not rewrite or scan commit author metadata or old blobs.
