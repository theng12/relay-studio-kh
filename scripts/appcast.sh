#!/bin/zsh
# Write the appcast item for one release. The EdDSA signature is over the exact DMG bytes, so a
# re-cut DMG must regenerate its item or Sparkle refuses it silently.
#
#   appcast.sh <version> <dmg> <download-url> [docs-dir]
#
# The private key stays outside the repo and CI (~/.kh-sparkle/kh.relay.desktop/private.key).
set -eu
VERSION="${1:?version}"; DMG="${2:?dmg}"; URL="${3:?download url}"; DOCS="${4:-docs}"
SHIM="${KH_SPARKLE_SHIM:-/build/Developer/_orchestra/developer-system/sparkle-shim-20261123}"
KEY="${RELAY_SPARKLE_KEY:-$HOME/.kh-sparkle/kh.relay.desktop/private.key}"
[ -f "$DMG" ] || { echo "no DMG at $DMG"; exit 1; }
[ -f "$KEY" ] || { echo "no signing key at $KEY"; exit 1; }
SIG=$("$SHIM/vendor/bin/sign_update" --ed-key-file "$KEY" -p "$DMG")
LEN=$(stat -f %z "$DMG")
mkdir -p "$DOCS"
VERSION="$VERSION" URL="$URL" SIG="$SIG" LEN="$LEN" DOCS="$DOCS" python3 -I - <<'PY'
import os
from datetime import datetime, timezone

path = os.path.join(os.environ["DOCS"], "appcast.xml")
version = os.environ["VERSION"]
item = (
    "    <item>\n"
    f"      <title>Relay Studio KH {version}</title>\n"
    f"      <sparkle:version>{version}</sparkle:version>\n"
    f"      <sparkle:shortVersionString>{version}</sparkle:shortVersionString>\n"
    f'      <enclosure url="{os.environ["URL"]}" sparkle:edSignature="{os.environ["SIG"]}"'
    f' length="{os.environ["LEN"]}" type="application/octet-stream"/>\n'
    f"      <pubDate>{datetime.now(timezone.utc).strftime('%a, %d %b %Y %H:%M:%S +0000')}</pubDate>\n"
    "    </item>\n"
)
head = (
    '<?xml version="1.0" encoding="utf-8"?>\n'
    '<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">\n'
    "  <channel>\n"
    "    <title>Relay Studio KH</title>\n"
    "    <link>https://theng12.github.io/relay-studio-kh/appcast.xml</link>\n"
    "    <description>Relay Studio KH releases</description>\n"
    "    <language>en</language>\n"
)
text = open(path).read() if os.path.exists(path) else head + "  </channel>\n</rss>\n"
if "<language>" not in text:
    text = head + text
# Replace an existing item for this version, then insert the new one right after <language>.
import re
text = re.sub(r"    <item>(?:(?!</item>).)*?<sparkle:version>%s</sparkle:version>.*?</item>\n" % re.escape(version), "", text, flags=re.S)
text = text.replace("    <language>en</language>\n", "    <language>en</language>\n" + item, 1)
open(path, "w").write(text)
print(f"appcast item written for {version}: {len(os.environ['SIG'])}-char signature, {os.environ['LEN']} bytes")
PY
