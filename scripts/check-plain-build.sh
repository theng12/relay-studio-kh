#!/bin/zsh
# Issue 20261123-13: a plain `cargo build --release` produced a binary naming this Mac's home folder.
# The build boundary now refuses that (build.rs), and the bundled-assets feature makes a hand build
# clean too. This checks both halves, the way a person would try them.
#
#   check-plain-build.sh
set -u
cd "${0:A:h}/.."
export PATH="$HOME/.cargo/bin:$PATH"
ME=$(id -un)
TARGET="$PWD/src-tauri/target/plain-check"
rc=0

echo "=== 1. a plain Release build must refuse, not leak"
OUT=$(CARGO_TARGET_DIR="$TARGET" cargo build --release --manifest-path src-tauri/Cargo.toml 2>&1); plain_rc=$?
if [ "$plain_rc" -eq 0 ]; then
  echo "FAIL the plain Release build succeeded — the boundary did not refuse it"; rc=1
elif printf '%s' "$OUT" | grep -q 'refusing a Release build that would embed'; then
  echo "PASS the plain Release build refused with its reason"
else
  echo "FAIL the plain build failed for another reason:"; printf '%s\n' "$OUT" | tail -15; rc=1
fi

echo "=== 2. the same build with the bundled-assets feature must succeed and carry no home path"
CARGO_TARGET_DIR="$TARGET" cargo build --release --manifest-path src-tauri/Cargo.toml --features bundled --quiet 2>/dev/null
build_rc=$?
BIN="$TARGET/release/relay-studio-kh"
if [ "$build_rc" -ne 0 ] || [ ! -f "$BIN" ]; then
  echo "FAIL the featured build did not produce $BIN"; rc=1
else
  HOME_HITS=$(strings -a "$BIN" | grep -c "/Users/$ME" || true)
  OTHER=$(strings -a "$BIN" | grep -o '/Users/[A-Za-z0-9._-]*' | grep -vc "/Users/$ME" || true)
  echo "featured build: own-home strings=$HOME_HITS other-/Users/ strings=$OTHER"
  if [ "$HOME_HITS" -eq 0 ]; then echo "PASS the Release build carries no $ME home path"
  else echo "FAIL the featured build still names $ME's home"; rc=1; fi
fi
exit $rc