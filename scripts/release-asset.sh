#!/bin/zsh
# The release asset comes before the feed row. A row is real only when its url downloads
# anonymously with the matching sha, so this refuses while the repo is private.
#
#   release-asset.sh <version> <dmg> [repo] [docs-dir]
set -eu
VERSION="${1:?version}"; DMG="${2:?dmg}"; REPO="${3:-theng12/relay-studio-kh}"; DOCS="${4:-docs}"
TAG="relay-$VERSION"
NAME=$(basename "$DMG")
SHA=$(shasum -a 256 "$DMG" | awk '{print $1}')

if ! gh release view "$TAG" -R "$REPO" >/dev/null 2>&1; then
  gh release create "$TAG" -R "$REPO" --title "Relay Studio KH $VERSION" \
    --notes-file CHANGELOG.md >/dev/null
fi
gh release upload "$TAG" "$DMG" -R "$REPO" --clobber >/dev/null

URL="https://github.com/$REPO/releases/download/$TAG/$NAME"
zsh scripts/appcast.sh "$VERSION" "$DMG" "$URL" "$DOCS"

# Anonymous proof: no credentials, the exact bytes, the exact sha.
TMP=$(mktemp "${TMPDIR:-/tmp}/relay-asset.XXXXXX")
if ! curl -fsSL --max-time 120 "$URL" -o "$TMP"; then
  echo "FAIL $URL does not download anonymously — the repo is still private or the asset is missing"
  rm -f "$TMP"; exit 1
fi
GOT=$(shasum -a 256 "$TMP" | awk '{print $1}')
rm -f "$TMP"
[ "$GOT" = "$SHA" ] || { echo "FAIL downloaded sha $GOT != $SHA"; exit 1; }
echo "RELEASE ASSET OK $URL sha256=$SHA bytes=$(stat -f %z "$DMG")"
echo "appcast: $DOCS/appcast.xml"
