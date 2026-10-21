#!/usr/bin/env bash
# Called only by the tag-gated CI release job, after both archives pass.
set -euo pipefail
URES_PROJECT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$URES_PROJECT"
URES_VERSION=$(python3 -B scripts/release.py version)
URES_TAG="v$URES_VERSION"
test "${GITHUB_REF:-}" = "refs/tags/$URES_TAG"
URES_ARTIFACTS=()
for URES_TARGET in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
    URES_ARCHIVE="ures-v${URES_VERSION}-${URES_TARGET}.tar.gz"
    URES_ARTIFACTS+=("dist/$URES_ARCHIVE")
done
python3 -B scripts/release.py checksums --directory dist
URES_RELEASE_TEMP=$(mktemp -d)
trap 'rm -rf -- "$URES_RELEASE_TEMP"' EXIT
URES_RELEASE_STATE="$URES_RELEASE_TEMP/state.json"
URES_RELEASE_NOTES="$URES_RELEASE_TEMP/notes.md"
python3 -B scripts/release.py notes > "$URES_RELEASE_NOTES"
if gh release view "$URES_TAG" --json isDraft > "$URES_RELEASE_STATE" 2>/dev/null; then
    python3 -B -c 'import json,sys; assert json.load(open(sys.argv[1]))["isDraft"], "Refusing to overwrite a published release"' "$URES_RELEASE_STATE"
else
    gh release create "$URES_TAG" --draft --title "ures $URES_VERSION" --notes-file "$URES_RELEASE_NOTES"
fi
gh release edit "$URES_TAG" --notes-file "$URES_RELEASE_NOTES"
gh release upload "$URES_TAG" "${URES_ARTIFACTS[@]}" dist/SHA256SUMS --clobber
URES_PRERELEASE=false
if [[ "$URES_VERSION" == *-* ]]; then URES_PRERELEASE=true; fi
gh release edit "$URES_TAG" --draft=false --prerelease="$URES_PRERELEASE"
