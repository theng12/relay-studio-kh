#!/bin/zsh
# What Relay does when it has nothing to do (owner, 2026-11-19: an app that holds what it no longer
# needs is a defect). One launch with a throwaway HOME, no work asked of it, sampled twice over a
# settled window: ports, fds, threads and footprint must not grow.
#
#   idle-footprint.sh <app bundle> [settle seconds] [idle seconds]
#
# Launched hidden and never brought to the front (`open -g -j`); the frontmost app is checked before
# and after and reported, so a run that changed it is visible in the output rather than assumed away.
set -u
APP="${1:?usage: idle-footprint.sh <app bundle> [settle s] [idle s]}"
SETTLE="${2:-45}"
IDLE="${3:-120}"
[ -d "$APP" ] || { echo "no app at $APP"; exit 2; }
NAME=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleName' "$APP/Contents/Info.plist" 2>/dev/null || basename "$APP" .app)

WORK=$(mktemp -d "${TMPDIR:-/tmp}/relay-idle.XXXXXX")
HOME_DIR="$WORK/home"; mkdir -p "$HOME_DIR/Library/Application Support" "$HOME_DIR/Library/Caches"
front() { osascript -e 'tell application "System Events" to name of first application process whose frontmost is true' 2>/dev/null; }
BEFORE_FRONT=$(front)
env HOME="$HOME_DIR" open -g -j -n "$APP"
PID=""
for _ in {1..40}; do PID=$(pgrep -f "$APP/Contents/MacOS/" | head -1); [ -n "$PID" ] && break; sleep 0.5; done
[ -n "$PID" ] || { echo "UNMEASURED: the app did not start"; rm -rf "$WORK"; exit 2; }

sample() { # ports fds threads footprintMB
  local p=$1
  local ports fds threads foot
  # `-a` ANDs the selections: without it lsof ORs them and this counts every listener on the Mac.
  ports=$(lsof -nP -a -p "$p" -iTCP -sTCP:LISTEN 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
  fds=$(lsof -nP -p "$p" 2>/dev/null | wc -l | tr -d ' ')
  threads=$(ps -M -p "$p" 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
  foot=$(footprint -p "$p" 2>/dev/null | awk '/phys_footprint/{for(i=1;i<=NF;i++) if($i ~ /^[0-9.]+$/){u=$(i+1); v=$i; if(u=="GB"){printf "%d", v*1024; exit} if(u=="MB"){printf "%d", v; exit}}}')
  [ -n "$foot" ] || foot=$(ps -o rss= -p "$p" 2>/dev/null | awk '{printf "%d", $1/1024}')
  echo "$ports $fds $threads ${foot:-0}"
}

sleep "$SETTLE"
read -r P1 F1 T1 M1 <<< "$(sample "$PID")"
sleep "$IDLE"
read -r P2 F2 T2 M2 <<< "$(sample "$PID")"
AFTER_FRONT=$(front)

kill -TERM "$PID" 2>/dev/null
for _ in {1..20}; do ps -p "$PID" >/dev/null 2>&1 || break; sleep 0.25; done
ps -p "$PID" >/dev/null 2>&1 && kill -KILL "$PID" 2>/dev/null

printf 'idle footprint %s pid %s\n' "$NAME" "$PID"
printf '  start: ports=%s fds=%s threads=%s footprint=%sMB (after %ss settle)\n' "$P1" "$F1" "$T1" "$M1" "$SETTLE"
printf '  end:   ports=%s fds=%s threads=%s footprint=%sMB (after %ss idle)\n' "$P2" "$F2" "$T2" "$M2" "$IDLE"
printf '  frontmost: "%s" -> "%s"\n' "$BEFORE_FRONT" "$AFTER_FRONT"
rm -rf "$WORK"

fail=0
(( P2 > P1 )) && { echo "  FAIL ports grew $P1 -> $P2"; fail=1; }
(( F2 > F1 + 4 )) && { echo "  FAIL fds grew $F1 -> $F2"; fail=1; }
(( T2 > T1 + 2 )) && { echo "  FAIL threads grew $T1 -> $T2"; fail=1; }
(( M2 > M1 + 32 )) && { echo "  FAIL footprint grew ${M1}MB -> ${M2}MB"; fail=1; }
(( fail == 0 )) && echo "  PASS flat while idle"
exit $fail
