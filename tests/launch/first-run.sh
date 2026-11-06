#!/usr/bin/env bash
# First launch of the desktop app on a clean machine, in a virtual screen
# with software graphics: an empty home folder, no settings, no canvases.
#
#   tests/launch/first-run.sh BINARY OUT_DIR [--expect-blank]
#
# Runs three cases and keeps a screenshot and launch.log of each in OUT_DIR:
#   clean      a plain first launch
#   dropped    the first frames are lost, as when macOS hasn't put the
#              window on screen yet (the App Review "empty window")
#   readonly   the home folder can't be written
# A case fails if no UI reached the screen. Linux only; the Mac's own launch
# test is in .github/workflows/appstore.yml.
set -uo pipefail
bin=$(realpath "$1"); out=$2; expect_blank=${3:-}
mkdir -p "$out"; out=$(realpath "$out")
fail=0

# Colours in the whole shot: paper alone has very few, the UI adds many.
colours() { convert "$1" -format %k info:; }

run_case() { # name, frames to drop, read-only home?
  local name=$1 drop=$2 ro=$3 home run_home shot="$out/$1.png" colors
  home=$(mktemp -d)
  # (A folder nothing can be made in, even for root.)
  run_home=$home; [ "$ro" = yes ] && run_home=/proc/og-paper-no-home
  (
    export HOME=$run_home OGP_TEST_DROP_FRAMES=$drop WGPU_BACKEND=vulkan
    export XDG_RUNTIME_DIR=$home
    unset WAYLAND_DISPLAY
    timeout 25 "$bin" >"$out/$name.stderr" 2>&1 &
    pid=$!
    sleep "${WAIT:-12}"
    import -window root "$shot"
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    true
  )
  cp "$home/OG Paper/launch.log" "$out/$name.launch.log" 2>/dev/null
  colors=$(colours "$shot")
  local line
  line=$(grep -h "ui on screen" "$out/$name.launch.log" 2>/dev/null || true)
  echo "$name: $colors colours; ${line:-no launch.log line}"
  [ -n "$line" ] || [ "$ro" = yes ] || tail -12 "$out/$name.stderr" | cut -c1-200
  local blank=no
  [ "$colors" -lt 40 ] && blank=yes
  if [ "$expect_blank" = --expect-blank ] && [ "$name" = dropped ]; then
    [ "$blank" = yes ] || { echo "FAIL $name: expected blank"; fail=1; }
  elif [ "$blank" = yes ]; then
    echo "FAIL $name: the window is blank"; fail=1
  fi
  if grep -q "panicked" "$out/$name.stderr"; then
    echo "FAIL $name: panic"; grep -m3 -A2 panicked "$out/$name.stderr"; fail=1
  fi
  rm -rf "$home"
}

# A build runner with no desktop lacks winit's X keyboard library and a
# software Vulkan: unpack them next to the build instead of installing them.
if ! ldconfig -p | grep -q libxkbcommon-x11 || ! ls /usr/share/vulkan/icd.d/lvp_icd* >/dev/null 2>&1; then
  libs=$(realpath "$(dirname "$bin")/..")/xlibs
  if ! ls "$libs"/root/usr/share/vulkan/icd.d/lvp_icd*.json >/dev/null 2>&1; then
    mkdir -p "$libs"
    (cd "$libs" && apt-get download libxkbcommon-x11-0 libxcb-xkb1 mesa-vulkan-drivers >/dev/null 2>&1
     for d in *.deb; do dpkg-deb -x "$d" root; done)
  fi
  lib=$libs/root/usr/lib/x86_64-linux-gnu
  export LD_LIBRARY_PATH=$lib:${LD_LIBRARY_PATH:-}
  sed "s#\"library_path\": \".*\"#\"library_path\": \"$lib/libvulkan_lvp.so\"#" \
    "$libs"/root/usr/share/vulkan/icd.d/lvp_icd*.json > "$libs/lvp.json"
  export VK_DRIVER_FILES=$libs/lvp.json VK_ICD_FILENAMES=$libs/lvp.json
fi

Xvfb :77 -screen 0 1280x800x24 >/dev/null 2>&1 &
xv=$!
trap 'kill $xv 2>/dev/null' EXIT
sleep 1
export DISPLAY=:77
run_case clean 0 no
run_case dropped 2 no
run_case readonly 0 yes
exit $fail
