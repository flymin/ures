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
    python3 -B scripts/check_privacy.py --archive "dist/$URES_ARCHIVE"
    URES_ARTIFACTS+=("dist/$URES_ARCHIVE")
done
python3 -B scripts/release.py checksums --directory dist
URES_RELEASE_STATE=$(mktemp /tmp/ures-release-state.XXXXXX)
trap 'rm -f -- "$URES_RELEASE_STATE"' EXIT
if gh release view "$URES_TAG" --json isDraft > "$URES_RELEASE_STATE" 2>/dev/null; then
    python3 -B -c 'import json,sys; assert json.load(open(sys.argv[1]))["isDraft"], "Refusing to overwrite a published release"' "$URES_RELEASE_STATE"
else
    gh release create "$URES_TAG" --draft --title "ures $URES_VERSION" --notes-file CHANGELOG.md
fi
gh release upload "$URES_TAG" "${URES_ARTIFACTS[@]}" dist/SHA256SUMS --clobber
URES_PRERELEASE=false
if [[ "$URES_VERSION" == *-* ]]; then URES_PRERELEASE=true; fi
gh release edit "$URES_TAG" --draft=false --prerelease="$URES_PRERELEASE"
