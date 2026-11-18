#!/usr/bin/env bash
# OG Paper to a Google Play testing track, entirely through the forge: no key
# file on this machine or on GitHub.
#
#   scripts/play-release.sh [--version 1.0] [--track internal] [--draft]
#                           [--notes FILE] [--build-only] [--code N]
#
# 1. Next version code = highest bundle Play has + 1 (at least 1000), read
#    through the forge's Play credential (PLAY_CRED, default play-main).
# 2. Builds and signs the .aab on og-host2's forge runner with the upload key
#    /cache/android/keys/og-paper-upload.keystore (made there on first use;
#    it never leaves the runner). Comes back as ./og-paper.aab.
# 3. Uploads it through the forge store API (the forge signs the request; the
#    service-account key stays in its encrypted store), puts it on the track
#    and commits the edit.
#
# Testing tracks only: internal, alpha (closed testing), beta (open testing).
# The release goes out to the track's testers at once (--draft leaves it for
# Play Console). Production is never set here: promote in Play Console.
# --build-only stops after step 2.
#
# Play's "automatic protection" (anti-tamper) must stay off for this app: it
# needs dex code and OG Paper has none (NativeActivity), so uploads fail with
# "Anti-tamper protection is not available". Protected with Play > Automatic
# protection > Installer check off.
set -euo pipefail
cd "$(dirname "$0")/.."
PKG=app.ogpaper
CRED=${PLAY_CRED:-play-main}
API=https://androidpublisher.googleapis.com/androidpublisher/v3/applications/$PKG
UP=https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications/$PKG
version=1.0 track=internal status=completed notes="" build_only="" code=""
while [ $# -gt 0 ]; do
  case $1 in
    --version) version=$2; shift ;;
    --track) track=$2; shift ;;
    --draft) status=draft ;;
    --completed) status=completed ;;
    --notes) notes=$2; shift ;;
    --build-only) build_only=1 ;;
    --code) code=$2; shift ;;
    *) echo "usage: $0 [--version V] [--track internal|alpha|beta] [--draft] [--notes FILE] [--build-only] [--code N]" >&2; exit 2 ;;
  esac
  shift
done
case $track in internal|alpha|beta) ;; *) echo "!! track must be internal, alpha or beta" >&2; exit 2 ;; esac
[ -z "$notes" ] || [ -f "$notes" ] || { echo "!! no notes file $notes" >&2; exit 2; }

# call METHOD URL [curl args]: prints the response body, fails on HTTP >= 300.
# Goes straight to the forge's store API (POST /api/v1/store/creds/NAME/call):
# the forge signs the request with the stored service account, which never
# leaves it. Forge URL and token: OGFORGE_URL/OGFORGE_TOKEN or ~/.config/ogforge.
if [ -z "${OGFORGE_URL:-}" ] || [ -z "${OGFORGE_TOKEN:-}" ]; then
  eval "$(python3 -c '
import json, os, shlex
c = json.load(open(os.path.expanduser("~/.config/ogforge/config.json")))
print("OGFORGE_URL=%s OGFORGE_TOKEN=%s" % (shlex.quote(c["url"]), shlex.quote(c["token"])))')"
fi
call() {
  local m=$1 u=$2; shift 2
  local q
  q=$(python3 -c 'import sys, urllib.parse as p; print("method=%s&url=%s&repo=og-paper" % (sys.argv[1], p.quote(sys.argv[2], safe="")))' "$m" "$u")
  curl -sS --max-time 1800 -X POST -H "Authorization: Bearer $OGFORGE_TOKEN" \
    "$OGFORGE_URL/api/v1/store/creds/$CRED/call?$q" "$@" | python3 -c '
import json, sys
r = json.load(sys.stdin)
if "error" in r:
    sys.exit("forge: %s" % json.dumps(r["error"]))
b = r.get("body")
if not isinstance(b, str):
    b = json.dumps(b)
if int(r.get("status", 0)) >= 300:
    sys.exit("%s %s: %s %s" % (sys.argv[1], sys.argv[2], r.get("status"), b[:1500]))
print(b)' "$m" "$u"
}
field() { python3 -c 'import json,sys; print(json.loads(sys.stdin.read() or "{}").get(sys.argv[1], ""))' "$1"; }

if [ -z "$code" ]; then
  if edit=$(call POST "$API/edits" | field id) && [ -n "$edit" ]; then
    max=$(call GET "$API/edits/$edit/bundles" | python3 -c '
import json, sys
print(max([b["versionCode"] for b in json.load(sys.stdin).get("bundles", [])] or [0]))')
    call DELETE "$API/edits/$edit" >/dev/null || true
    code=$(( max + 1 > 1000 ? max + 1 : 1000 ))
  elif [ -n "$build_only" ]; then
    echo "   Play has no bundle of $PKG yet: first version code 1000"
    code=1000
  else
    echo "!! Play refused an edit for $PKG (does the app exist in Play Console?)" >&2
    exit 1
  fi
fi

echo "== building $PKG $version ($code) on og-host2"
runner=$(mktemp)
trap 'rm -f "$runner"' EXIT
cat > "$runner" <<EOF
set -euo pipefail
. /cache/android/env.sh; unset ANDROID_SDK_ROOT
export ANDROID_NDK_ROOT=/cache/android/sdk/ndk/27.2.12479018 PATH=/cache/cargo/bin:\$PATH
ks=/cache/android/keys/og-paper-upload.keystore pw=/cache/android/keys/og-paper-upload.pass
if [ ! -f \$ks ]; then
  umask 077
  head -c 32 /dev/urandom | base64 | tr -d '/+=' > \$pw
  keytool -genkeypair -keystore \$ks -storetype PKCS12 -storepass "\$(cat \$pw)" -alias upload \
    -keyalg RSA -keysize 4096 -validity 20000 -dname 'CN=OG Paper upload,O=OG Studio' >/dev/null
  echo "   made upload key \$ks"
fi
export KEYSTORE=\$ks KEYSTORE_PASSWORD="\$(cat \$pw)" KEY_ALIAS=upload
rm -f og-paper.aab
scripts/android-aab.sh "$version" "$code" og-paper.aab
EOF
rm -f og-paper.aab
forge run --on og-host2 --no-fallback --pull og-paper.aab --script "$runner"
[ -f og-paper.aab ] || { echo "!! no og-paper.aab came back" >&2; exit 1; }
[ -n "$build_only" ] && { echo "-> og-paper.aab ($code), not uploaded"; exit 0; }

echo "== uploading to Play ($track, $status)"
edit=$(call POST "$API/edits" | field id)
trap 'rm -f "$runner"; [ -n "${committed:-}" ] || call DELETE "$API/edits/$edit" >/dev/null 2>&1 || true' EXIT
got=$(call POST "$UP/edits/$edit/bundles?uploadType=media" \
  --data-binary @og-paper.aab -H 'Content-Type: application/octet-stream' | field versionCode)
[ "$got" = "$code" ] || { echo "!! Play read version code '$got', expected $code" >&2; exit 1; }
release=$(python3 - "$code" "$status" "$version" "$notes" <<'PY'
import json, sys
code, status, name, notes = sys.argv[1:]
r = {"name": f"{name} ({code})", "versionCodes": [code], "status": status}
if notes:
    r["releaseNotes"] = [{"language": "en-US", "text": open(notes, encoding="utf-8").read().strip()[:500]}]
print(json.dumps(r))
PY
)
call PUT "$API/edits/$edit/tracks/$track" --data-binary "{\"track\":\"$track\",\"releases\":[$release]}" >/dev/null
call POST "$API/edits/$edit:commit" >/dev/null
committed=1
echo "-> $PKG $version ($code) on $track ($status)"
