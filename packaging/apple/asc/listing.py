#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Update OG Paper's App Store listing (the version being prepared):

  listing.py --platform IOS --version 1.0 --build latest
      attach the newest processed build of that version
  listing.py --platform IOS --version 1.0 --shots DIR --display APP_IPAD_PRO_3GEN_129
      replace that screenshot set (en-US) with the PNGs in DIR, in name order
  listing.py --platform MAC_OS --version 1.0 --notes FILE
      set the App Review notes from FILE

It never submits for review. Needs ASC_KEY, ASC_KEY_ID, ASC_ISSUER."""

import argparse
import hashlib
import json
import os
import sys
import time
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(__file__))
from status import API, BUNDLE, token  # noqa: E402

TOKEN = token()


def call(method, path, body=None, **params):
    url = (path if path.startswith("http") else API + path) + (
        "?" + urllib.parse.urlencode(params) if params else ""
    )
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers={
        "Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req) as r:
            raw = r.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        sys.exit(f"{method} {path}: {e.code} {e.read().decode()[:800]}")


def version(platform, ver):
    aid = call("GET", "/v1/apps", **{"filter[bundleId]": BUNDLE})["data"][0]["id"]
    for v in call("GET", f"/v1/apps/{aid}/appStoreVersions", limit=50)["data"]:
        a = v["attributes"]
        if a["platform"] == platform and a["versionString"] == ver:
            return aid, v
    sys.exit(f"no {platform} version {ver}")


def attach_latest(aid, v, platform, ver):
    builds = call("GET", "/v1/builds", **{
        "filter[app]": aid, "filter[preReleaseVersion.version]": ver,
        "filter[preReleaseVersion.platform]": platform,
        "filter[processingState]": "VALID", "sort": "-uploadedDate", "limit": 1})["data"]
    if not builds:
        sys.exit("no processed build yet")
    b = builds[0]
    call("PATCH", f"/v1/appStoreVersions/{v['id']}/relationships/build",
         {"data": {"type": "builds", "id": b["id"]}})
    print("attached build", b["attributes"]["version"])


def replace_shots(v, folder, display):
    locs = call("GET", f"/v1/appStoreVersions/{v['id']}/appStoreVersionLocalizations")["data"]
    loc = next(l for l in locs if l["attributes"]["locale"] == "en-US")
    sets = call("GET", f"/v1/appStoreVersionLocalizations/{loc['id']}/appScreenshotSets")["data"]
    s = next((x for x in sets if x["attributes"]["screenshotDisplayType"] == display), None)
    if s is None:
        s = call("POST", "/v1/appScreenshotSets", {"data": {
            "type": "appScreenshotSets", "attributes": {"screenshotDisplayType": display},
            "relationships": {"appStoreVersionLocalization": {"data": {
                "type": "appStoreVersionLocalizations", "id": loc["id"]}}}}})["data"]
    for old in call("GET", f"/v1/appScreenshotSets/{s['id']}/appScreenshots")["data"]:
        call("DELETE", f"/v1/appScreenshots/{old['id']}")
    for name in sorted(f for f in os.listdir(folder) if f.endswith(".png")):
        data = open(os.path.join(folder, name), "rb").read()
        shot = call("POST", "/v1/appScreenshots", {"data": {
            "type": "appScreenshots", "attributes": {"fileName": name, "fileSize": len(data)},
            "relationships": {"appScreenshotSet": {"data": {"type": "appScreenshotSets", "id": s["id"]}}}}})["data"]
        for op in shot["attributes"]["uploadOperations"]:
            part = data[op["offset"]: op["offset"] + op["length"]]
            req = urllib.request.Request(op["url"], data=part, method=op["method"],
                                         headers={h["name"]: h["value"] for h in op["requestHeaders"]})
            urllib.request.urlopen(req).read()
        call("PATCH", f"/v1/appScreenshots/{shot['id']}", {"data": {
            "type": "appScreenshots", "id": shot["id"],
            "attributes": {"uploaded": True, "sourceFileChecksum": hashlib.md5(data).hexdigest()}}})
        print("uploaded", name)
    time.sleep(2)


def replace_previews(v, folder, kind):
    """Replace preview set `kind` (IPHONE_67, IPAD_PRO_3GEN_129) with the
    .mp4 files in `folder`, in name order (at most 3)."""
    locs = call("GET", f"/v1/appStoreVersions/{v['id']}/appStoreVersionLocalizations")["data"]
    loc = next(l for l in locs if l["attributes"]["locale"] == "en-US")
    sets = call("GET", f"/v1/appStoreVersionLocalizations/{loc['id']}/appPreviewSets")["data"]
    s = next((x for x in sets if x["attributes"]["previewType"] == kind), None)
    if s is None:
        s = call("POST", "/v1/appPreviewSets", {"data": {
            "type": "appPreviewSets", "attributes": {"previewType": kind},
            "relationships": {"appStoreVersionLocalization": {"data": {
                "type": "appStoreVersionLocalizations", "id": loc["id"]}}}}})["data"]
    for old in call("GET", f"/v1/appPreviewSets/{s['id']}/appPreviews")["data"]:
        call("DELETE", f"/v1/appPreviews/{old['id']}")
    for name in sorted(f for f in os.listdir(folder) if f.endswith(".mp4"))[:3]:
        data = open(os.path.join(folder, name), "rb").read()
        pv = call("POST", "/v1/appPreviews", {"data": {
            "type": "appPreviews",
            "attributes": {"fileName": name, "fileSize": len(data), "mimeType": "video/mp4",
                           "previewFrameTimeCode": "00:00:05:00"},
            "relationships": {"appPreviewSet": {"data": {"type": "appPreviewSets", "id": s["id"]}}}}})["data"]
        for op in pv["attributes"]["uploadOperations"]:
            part = data[op["offset"]: op["offset"] + op["length"]]
            req = urllib.request.Request(op["url"], data=part, method=op["method"],
                                         headers={h["name"]: h["value"] for h in op["requestHeaders"]})
            urllib.request.urlopen(req).read()
        call("PATCH", f"/v1/appPreviews/{pv['id']}", {"data": {
            "type": "appPreviews", "id": pv["id"],
            "attributes": {"uploaded": True, "sourceFileChecksum": hashlib.md5(data).hexdigest()}}})
        print("uploaded preview", name)


def set_notes(v, path):
    notes = open(path).read().strip()
    rd = call("GET", f"/v1/appStoreVersions/{v['id']}/appStoreReviewDetail").get("data")
    if not rd:
        sys.exit("no review details yet: add a contact in App Store Connect first")
    call("PATCH", f"/v1/appStoreReviewDetails/{rd['id']}", {"data": {
        "type": "appStoreReviewDetails", "id": rd["id"], "attributes": {"notes": notes}}})
    print("review notes set from", path)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--platform", default="IOS")
    p.add_argument("--version", default="1.0")
    p.add_argument("--build")
    p.add_argument("--shots")
    p.add_argument("--display")
    p.add_argument("--notes")
    p.add_argument("--previews")
    p.add_argument("--preview-type")
    a = p.parse_args()
    aid, v = version(a.platform, a.version)
    if a.build:
        attach_latest(aid, v, a.platform, a.version)
    if a.shots:
        replace_shots(v, a.shots, a.display)
    if a.notes:
        set_notes(v, a.notes)
    if a.previews:
        replace_previews(v, a.previews, a.preview_type)


if __name__ == "__main__":
    main()
