#!/usr/bin/env bash
# Screenshots of the account UI in a virtual screen, at desktop and phone width:
# tests/account/shots.sh OUT_DIR. Linux only. Software graphics as in
# tests/launch/first-run.sh.
set -uo pipefail
cargo build --release -p og-paper --bin og-paper 2>&1 | tail -1
bin=$(realpath target/release/og-paper)
out=$1; mkdir -p "$out"; out=$(realpath "$out")
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
shot() { # scene, width, height
  local scene=$1 w=$2 h=$3 home d=$((70 + RANDOM % 20))
  home=$(mktemp -d)
  Xvfb ":$d" -screen 0 "${w}x${h}x24" >/dev/null 2>&1 &
  local xv=$!
  sleep 1
  (
    export HOME=$home XDG_RUNTIME_DIR=$home WGPU_BACKEND=vulkan DISPLAY=":$d"
    export OGP_ACCOUNT_SITE=http://127.0.0.1:9 OGP_TEST_ACCOUNT=$scene OGP_TEST_SIZE="${w}x${h}"
    unset WAYLAND_DISPLAY
    timeout 30 "$bin" >"$out/$scene-$w.log" 2>&1 &
    pid=$!
    sleep "${WAIT:-14}"
    import -window root "$out/$scene-$w.png"
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    true
  )
  kill "$xv" 2>/dev/null
  rm -rf "$home"
}
for s in prompt pairing pages signed-in; do
  shot "$s" 1280 800
  shot "$s" 390 844
done
ls "$out"
