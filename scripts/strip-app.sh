#!/bin/zsh
# Strip local symbols from the app's executable and re-sign. ring's C-built objects embed the
# build machine's home path as local symbol names, which no rustc remap can rewrite and which the
# public download must not carry. Debug info goes too; panic messages and exported symbols stay.
set -eu
APP="${1:?usage: strip-app.sh <App.app>}"
EXE="$APP/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
before=$(grep -ac '/Users/' "$EXE" || true)
strip -x -S "$EXE"
codesign --force --sign - "$APP"
codesign --verify --strict "$APP"
after=$(grep -ac '/Users/' "$EXE" || true)
echo "home paths before=$before after=$after (ad-hoc signature replaced, verify ok)"
[ "$after" = 0 ]
