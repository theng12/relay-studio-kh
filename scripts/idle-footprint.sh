#!/bin/zsh
# What Relay does when it has nothing to do (owner, 2026-11-19: an app that holds what it no longer
# needs is a defect). One launch with a throwaway HOME, no work asked of it, sampled twice over a
# settled window: ports, fds, threads and footprint must not grow.
#
#   idle-footprint.sh <app bundle> [settle seconds] [idle seconds]
#
# Launched hidden and never brought to the front (`open -g -j`). The frontmost app is sampled once a
# second for the whole window: Relay appearing even once fails the run, and a person switching windows
# makes it INVALID (exit 75) rather than a pass — focus provenance is never assumed.
#
# An instance someone else left running is reused and left running; only an instance this run started
# is ended, and that is said in one line. Set EVIDENCE_FOCUS=<file> to keep the sample series.
set -u
APP="${1:?usage: idle-footprint.sh <app bundle> [settle s] [idle s]}"
SETTLE="${2:-45}"
IDLE="${3:-120}"
[ -d "$APP" ] || { echo "no app at $APP"; exit 2; }
NAME=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleName' "$APP/Contents/Info.plist" 2>/dev/null || basename "$APP" .app)

WORK=$(mktemp -d "${TMPDIR:-/tmp}/relay-idle.XXXXXX")
HOME_DIR="$WORK/home"; mkdir -p "$HOME_DIR/Library/Application Support" "$HOME_DIR/Library/Caches"
# Read-only LaunchServices query: never invokes AppleScript / System Events.
front() {
  local asn
  asn=$(lsappinfo front 2>/dev/null) || return 0
  [ -n "$asn" ] && lsappinfo info -only name "$asn" 2>/dev/null | sed -n '1s/^"\([^"]*\)".*/\1/p'
}
# An instance someone else left running is reused, never killed: a measurement must not end a
# window it did not open.
RUNNING=$(pgrep -f "$APP/Contents/MacOS/" | head -1)
STARTED=0
if [ -n "$RUNNING" ]; then
  PID="$RUNNING"
  echo "reused the instance already running (pid $PID); this run does not own it and will not end it"
else
  env HOME="$HOME_DIR" open -g -j -n "$APP"
  STARTED=1
  PID=""
  for _ in {1..40}; do PID=$(pgrep -f "$APP/Contents/MacOS/" | head -1); [ -n "$PID" ] && break; sleep 0.5; done
  [ -n "$PID" ] || { echo "UNMEASURED: the app did not start"; rm -rf "$WORK"; exit 2; }
  echo "launched hidden (pid $PID); this run will end it"
fi
# Sample the frontmost app once a second for the whole window: Relay appearing even once is a
# failure, a person switching windows is environmental and is named as such.
FOCUS="$WORK/focus.log"
( while :; do front >> "$FOCUS" 2>/dev/null; sleep 1; done ) & FOCUS_SAMPLER=$!

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
kill "$FOCUS_SAMPLER" 2>/dev/null; wait "$FOCUS_SAMPLER" 2>/dev/null

if (( STARTED )); then
  kill -TERM "$PID" 2>/dev/null
  for _ in {1..20}; do ps -p "$PID" >/dev/null 2>&1 || break; sleep 0.25; done
  ps -p "$PID" >/dev/null 2>&1 && kill -KILL "$PID" 2>/dev/null
  echo "ended the instance this run started (pid $PID)"
fi

printf 'idle footprint %s pid %s\n' "$NAME" "$PID"
printf '  start: ports=%s fds=%s threads=%s footprint=%sMB (after %ss settle)\n' "$P1" "$F1" "$T1" "$M1" "$SETTLE"
printf '  end:   ports=%s fds=%s threads=%s footprint=%sMB (after %ss idle)\n' "$P2" "$F2" "$T2" "$M2" "$IDLE"
SAMPLES=$(grep -c . "$FOCUS" 2>/dev/null || echo 0)
DISTINCT=$(grep . "$FOCUS" 2>/dev/null | sort -u | tr '\n' ',')
printf '  frontmost across %s one-second samples: %s\n' "$SAMPLES" "${DISTINCT:-none}"
if [ -n "${EVIDENCE_FOCUS:-}" ]; then cp "$FOCUS" "$EVIDENCE_FOCUS" 2>/dev/null; fi

fail=0
(( P2 > P1 )) && { echo "  FAIL ports grew $P1 -> $P2"; fail=1; }
(( F2 > F1 + 4 )) && { echo "  FAIL fds grew $F1 -> $F2"; fail=1; }
(( T2 > T1 + 2 )) && { echo "  FAIL threads grew $T1 -> $T2"; fail=1; }
(( M2 > M1 + 32 )) && { echo "  FAIL footprint grew ${M1}MB -> ${M2}MB"; fail=1; }
in_use=0
grep -qx "$NAME" "$FOCUS" 2>/dev/null && { echo "  FAIL Relay came to the front during the run"; fail=1; }
OTHERS=$(grep -v -x "$NAME" "$FOCUS" 2>/dev/null | grep . | sort -u | grep -vc '^$')
(( OTHERS > 1 )) && in_use=1
rm -rf "$WORK"
if (( fail )); then exit 1; fi
if (( in_use )); then
  # 75 is the gate runner's "measured nothing" code: the run is INVALID, never a pass or a failure.
  echo "  INVALID — desktop in use: the frontmost app changed while idle was measured, so focus provenance is unknown; repeat on a quiet machine"
  exit 75
fi
echo "  PASS flat while idle, and Relay never took the front"
exit 0
