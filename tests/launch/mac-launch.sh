#!/usr/bin/env bash
# First launch of the Mac app in its real sandbox, on a clean container: what
# App Review does. App Store signing can't run outside the store, so this
# signs a copy ad hoc with the same sandbox (launch-test.entitlements).
#
#   tests/launch/mac-launch.sh "dist/OG Paper.app" OUT_DIR
#
# Two cases, each with launch.log and a screenshot in OUT_DIR:
#   clean     a plain first launch
#   dropped   the first frames lost on purpose
# A case fails unless launch.log shows the sandbox container in use and the
# UI on screen with its textures. The log also says why any frame was lost
# ("occluded" is the macOS case behind the App Review empty window).
set -uo pipefail
app=$1; out=$2
id=com.omegagiven.ogpaper
container=$HOME/Library/Containers/$id
log="$container/Data/OG Paper/launch.log"
mkdir -p "$out"
test="$out/test/OG Paper.app"
rm -rf "$out/test"; mkdir -p "$out/test"
cp -R "$app" "$test"
rm -f "$test/Contents/embedded.provisionprofile"
codesign --force --options runtime --sign - \
  --entitlements packaging/apple/macos/launch-test.entitlements "$test" || exit 1
codesign -d --entitlements - "$test" 2>&1 | grep -q app-sandbox || { echo "::error::no sandbox"; exit 1; }
fail=0

run_case() { # name, frames to drop
  local name=$1 drop=$2 up=no
  pkill -x og-paper 2>/dev/null
  rm -rf "$container"
  open -n --env "OGP_TEST_DROP_FRAMES=$drop" \
    --stdout "$PWD/$out/$name.stdout" --stderr "$PWD/$out/$name.stderr" "$test"
  for _ in $(seq 1 40); do
    if grep -q "ui on screen" "$log" 2>/dev/null; then up=yes; break; fi
    sleep 1
  done
  sleep 3
  screencapture -x "$out/$name.png" || echo "no screenshot for $name"
  cp "$log" "$out/$name.launch.log" 2>/dev/null
  pgrep -x og-paper >/dev/null || { echo "::error::$name: the app is not running"; fail=1; }
  pkill -x og-paper 2>/dev/null
  echo "--- $name"; cat "$out/$name.launch.log" 2>/dev/null || echo "(no launch.log)"
  grep -q "Library/Containers/$id" "$out/$name.launch.log" 2>/dev/null ||
    { echo "::error::$name: not in the sandbox container"; fail=1; }
  grep -qE "ui on screen: [0-9]+ meshes, [1-9][0-9]* textures" "$out/$name.launch.log" 2>/dev/null ||
    { echo "::error::$name: the UI did not reach the screen (up=$up)"; fail=1; }
}

run_case clean 0
run_case dropped 3
rm -rf "$out/test"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  { echo '### Launch test'; echo '```'; grep -hE "lost|ui on screen|no frame|failed|panic" "$out"/*.launch.log; echo '```'; } >> "$GITHUB_STEP_SUMMARY"
fi
exit $fail
