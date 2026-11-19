#!/usr/bin/env bash
# OG Paper's Google Play store listing (en-US) from packaging/android/play/,
# through the forge's Play credential (no key file here):
#
#   scripts/play-listing.sh [--text-only] [--website URL]
#
# Sets title, short and full description (listing/*.txt), and replaces the icon,
# feature graphic and phone / 7" / 10" tablet screenshots when they differ
# from Play's (re-uploading identical images makes the commit fail with 403) (phone/, tab7/, tab10/; regenerate with
# `node tests/ui/features/store-play.mjs` and make-art.py). Commits one edit.
# --website sets the contact website, but this service account gets 403 on the
# details commit: contact details, category, privacy policy and the App content
# forms are set in Play Console (see docs/RELEASING.md).
set -euo pipefail
cd "$(dirname "$0")/.."
PKG=app.ogpaper
CRED=${PLAY_CRED:-play-main}
API=https://androidpublisher.googleapis.com/androidpublisher/v3/applications/$PKG
UP=https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications/$PKG
DIR=packaging/android/play
LANG_=en-US
text_only="" website=""
while [ $# -gt 0 ]; do
  case $1 in
    --text-only) text_only=1 ;;
    --website) website=$2; shift ;;
    *) echo "usage: $0 [--text-only] [--website URL]" >&2; exit 2 ;;
  esac
  shift
done

if [ -z "${OGFORGE_URL:-}" ] || [ -z "${OGFORGE_TOKEN:-}" ]; then
  eval "$(python3 -c '
import json, os, shlex
c = json.load(open(os.path.expanduser("~/.config/ogforge/config.json")))
print("OGFORGE_URL=%s OGFORGE_TOKEN=%s" % (shlex.quote(c["url"]), shlex.quote(c["token"])))')"
fi
# [CT=type] call METHOD URL [curl args]: body on stdout, fails on HTTP >= 300
# (as in play-release.sh). CT is the Content-Type the forge sends upstream.
call() {
  local m=$1 u=$2; shift 2
  local q
  q=$(python3 -c 'import sys, urllib.parse as p; print("method=%s&url=%s&repo=og-paper" % (sys.argv[1], p.quote(sys.argv[2], safe="")) + ("&content_type=" + p.quote(sys.argv[3], safe="") if sys.argv[3] else ""))' "$m" "$u" "${CT:-}")
  curl -sS --max-time 600 -X POST -H "Authorization: Bearer $OGFORGE_TOKEN" \
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

edit=$(call POST "$API/edits" | field id)
trap '[ -n "${committed:-}" ] || call DELETE "$API/edits/$edit" >/dev/null 2>&1 || true' EXIT

listing=$(python3 - "$DIR/listing" <<'PY'
import json, sys
d = sys.argv[1]
r = lambda f: open(f"{d}/{f}", encoding="utf-8").read().strip()
t, s, f = r("title.txt"), r("short-description.txt"), r("full-description.txt")
assert len(t) <= 30 and len(s) <= 80 and len(f) <= 4000, (len(t), len(s), len(f))
print(json.dumps({"language": "en-US", "title": t, "shortDescription": s, "fullDescription": f}))
PY
)
call PUT "$API/edits/$edit/listings/$LANG_" --data-binary "$listing" >/dev/null
echo "== listing text"

if [ -n "$website" ]; then
  details=$(call GET "$API/edits/$edit/details")
  details=$(python3 -c 'import json,sys; d=json.loads(sys.argv[1]); d["contactWebsite"]=sys.argv[2]; print(json.dumps(d))' "$details" "$website")
  call PUT "$API/edits/$edit/details" --data-binary "$details" >/dev/null
  echo "== contact website $website"
fi

if [ -z "$text_only" ]; then
  img() { # TYPE FILE...: replaced only when the set differs from what Play has
    local type=$1; shift
    local have want
    have=$(call GET "$API/edits/$edit/listings/$LANG_/$type" | python3 -c 'import json,sys; print(" ".join(sorted(i.get("sha256","") for i in json.loads(sys.stdin.read() or "{}").get("images",[]))))')
    want=$(sha256sum "$@" | cut -c1-64 | sort | tr '\n' ' ' | sed 's/ $//')
    if [ "$have" = "$want" ]; then echo "== $type: unchanged"; return; fi
    call DELETE "$API/edits/$edit/listings/$LANG_/$type" >/dev/null
    for f in "$@"; do
      CT=image/png call POST "$UP/edits/$edit/listings/$LANG_/$type?uploadType=media" \
        --data-binary @"$f" >/dev/null
    done
    echo "== $type: $#"
  }
  img icon "$DIR/icon-512.png"
  img featureGraphic "$DIR/feature-graphic.png"
  img phoneScreenshots "$DIR"/phone/*.png
  img sevenInchScreenshots "$DIR"/tab7/*.png
  img tenInchScreenshots "$DIR"/tab10/*.png
fi

call POST "$API/edits/$edit:commit" >/dev/null
committed=1
echo "-> $PKG store listing ($LANG_) committed"
