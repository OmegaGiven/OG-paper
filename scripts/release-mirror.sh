#!/usr/bin/env bash
# Puts an OG Paper release (product og-paper) on the update mirror, signed and leak-audited by
# og-vault's package/release-app.sh (signs on og-host2; the release key never leaves it).
#   scripts/release-mirror.sh [--public] [--apk] ["notes"]
#     default   the PRIVATE mirror: og-host2 apps/og-mirror-private/site,
#               https://og-host2.tail6c6721.ts.net:8499 (tailnet only)
#     --public  the PUBLIC mirror (og-host2 apps/og-mirror/site = ogstudio.org/releases); the
#               owner's rule: every deploy reaches it. The demo vault follows it:
#               ssh root@192.168.0.138 'sh /var/lib/og-host/apps/og-vault-demo/src/deploy/demo/follow.sh'
#     --apk     include the Android app: the APK scripts/publish-android-vault.sh built for this
#               commit (target/release/apk/og-paper.apk + og-paper.version); run that first
#   version: the same numbers as publish-android-vault.sh (0.<2 + n/250>.<n%250>, n = commits)
#   OGV_SRC=~/src/og-vault   use this og-vault checkout instead of a fresh clone of master
# The web build is web/app with its wasm (web/app/pkg: build it first, e.g. scripts/build-web.sh
# or scripts/deploy-vault-web.sh) and no vault default, so a vault anywhere can serve it.
set -euo pipefail
MIRROR_URL=https://og-host2.tail6c6721.ts.net:8499
export OGV_MIRROR_DIR=/var/lib/og-host/apps/og-mirror-private/site
if [ "${1:-}" = --public ]; then
  shift
  MIRROR_URL=https://og-host2.tail6c6721.ts.net:8498
  export OGV_MIRROR_DIR=/var/lib/og-host/apps/og-mirror/site
fi
apk=0
if [ "${1:-}" = --apk ]; then apk=1; shift; fi
OGV_GIT=${OGV_GIT:-https://og-host2.tail6c6721.ts.net/git/og-vault.git}
cd "$(dirname "$0")/.."
if [ -n "$(git status --porcelain -- web/app crates Cargo.toml Cargo.lock)" ]; then
  echo "!! uncommitted changes in web/app or crates: commit first so the release means something" >&2
  exit 1
fi
count="$(git rev-list --count HEAD)"
ver="0.$((2 + count / 250)).$((count % 250))"
sha="$(git rev-parse --short HEAD)"
notes="${1:-OG Paper $ver: $(git log -1 --format=%s)}"
[ -f web/app/pkg/og_paper_bg.wasm ] || { echo "!! web/app/pkg is missing: build the wasm first" >&2; exit 1; }
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT

if [ -n "${OGV_SRC:-}" ]; then
  rel="$OGV_SRC/package/release-app.sh"
else
  git clone -q --depth 1 "$OGV_GIT" "$T/og-vault"
  rel="$T/og-vault/package/release-app.sh"
fi
[ -f "$rel" ] || { echo "!! no package/release-app.sh in the og-vault checkout" >&2; exit 1; }

mkdir -p "$T/web"
cp -r web/app/. "$T/web/"
echo "$ver $sha $(git log -1 --format=%cI)" > "$T/web/VERSION.txt"
find "$T/web" -name OG-DESIGN-VERSION -delete
if grep -rIl -E 'tail6c6721|og-host[12]|100\.(98|117)\.' "$T/web" >/dev/null; then
  echo "!! a homelab host name is in the web build:" >&2
  grep -rIl -E 'tail6c6721|og-host[12]|100\.(98|117)\.' "$T/web" >&2
  exit 1
fi

args=(og-paper "$ver" "$T/web")
if [ "$apk" = 1 ]; then
  a=target/release/apk/og-paper.apk
  read -r aver code < target/release/apk/og-paper.version 2>/dev/null || aver=""
  [ -f "$a" ] && [ "$aver" = "$ver" ] || { echo "!! no APK for $ver: run scripts/publish-android-vault.sh first" >&2; exit 1; }
  args+=("$a" "$code")
else
  args+=("" 0)
fi

echo "== og-paper $ver -> $MIRROR_URL (${OGV_CHANNEL:-stable})"
sh "$rel" "${args[@]}" "$notes"
got="$(curl -fsS --max-time 10 "$MIRROR_URL/og-paper/channels/${OGV_CHANNEL:-stable}.json")"
echo "$got" | grep -Eq "\"version\": ?\"$ver\"" || { echo "!! mirror channel does not show $ver: $got" >&2; exit 1; }
echo "-> on the mirror: og-paper $ver ($MIRROR_URL/og-paper/releases/$ver/)"
