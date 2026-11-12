#!/bin/zsh
# Remove Relay Studio KH without touching your saved accounts, profiles or settings.
# The app cannot reliably delete itself, so this is a script you run yourself.
set -e
set -o pipefail

APP="/Applications/Relay Studio KH.app"
PLIST="$HOME/Library/LaunchAgents/Relay Studio KH.plist"
DATA="$HOME/Library/Application Support/kh.relay.desktop"

echo "Relay Studio KH — uninstall"
echo

if pgrep -f "$APP/Contents/MacOS/Relay Studio KH" >/dev/null; then
  echo "1. Quitting Relay..."
  pkill -TERM -f "$APP/Contents/MacOS/Relay Studio KH" || true
  for _ in {1..20}; do pgrep -f "$APP/Contents/MacOS/Relay Studio KH" >/dev/null || break; sleep 0.5; done
else
  echo "1. Relay is not running."
fi

if [ -f "$PLIST" ]; then
  echo "2. Removing the Start at Login item..."
  launchctl unload "$PLIST" 2>/dev/null || true
  mv "$PLIST" "$HOME/.Trash/Relay Studio KH.plist" 2>/dev/null || rm -f "$PLIST"
else
  echo "2. No Start at Login item was registered."
fi

if [ -d "$APP" ]; then
  echo "3. Moving the app to the Trash (recoverable)..."
  mv "$APP" "$HOME/.Trash/" 2>/dev/null || { echo "   Could not move the app. Drag it to the Trash yourself."; }
else
  echo "3. The app is not in /Applications."
fi

echo
echo "Kept on purpose: $DATA"
echo "  Saved accounts, Chrome profiles, extension folders and settings stay until you delete"
echo "  that folder yourself."
echo
echo "Done. Open Chrome to confirm no Relay window returns at the next login."
