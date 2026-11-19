#!/bin/zsh
# Strip local symbols from the app's executable and re-sign with the shared identity. ring's C-built
# objects embed the build machine's home path as local symbol names, which no rustc remap can rewrite
# and which the public download must not carry. Debug info goes too; panic messages and exported
# symbols stay.
set -eu
APP="${1:?usage: strip-app.sh <App.app>}"
IDENTITY='KH Studio Local Signing'
EXE="$APP/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
before=$(grep -ac '/Users/' "$EXE" || true)
strip -x -S "$EXE"
# strip changes the executable, so the packager's signature is void. Sign again with the same
# identity, never ad-hoc: an ad-hoc signature has no stable designated requirement, so macOS drops
# the app's screen-recording and accessibility grants on every update.
codesign --force --sign "$IDENTITY" "$APP"
codesign --verify --strict "$APP"
after=$(grep -ac '/Users/' "$EXE" || true)
echo "home paths before=$before after=$after (signed by $IDENTITY, verify ok)"
[ "$after" = 0 ]
