#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""OG Paper on Google Play, through the Play Developer API (a service
account's JSON key in PLAY_SERVICE_ACCOUNT_JSON):

  play.py upload og-paper.aab --track internal [--status draft] [--notes FILE]
      upload the bundle and put it on a testing track (internal, alpha =
      closed testing, beta = open testing, production)
  play.py status
      the tracks and the version codes on them (reads only)

A new app's first bundle has to be uploaded by hand in Play Console; the API
only takes later ones. While the app is a draft (never reviewed), releases
can only be drafts too: --status draft."""

import argparse
import json
import os
import sys

from google.oauth2 import service_account
from google.auth.transport.requests import AuthorizedSession

PACKAGE = os.environ.get("PLAY_PACKAGE", "app.ogpaper")
API = f"https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{PACKAGE}"
UPLOAD = f"https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications/{PACKAGE}"


def session():
    info = json.loads(os.environ["PLAY_SERVICE_ACCOUNT_JSON"])
    creds = service_account.Credentials.from_service_account_info(
        info, scopes=["https://www.googleapis.com/auth/androidpublisher"])
    return AuthorizedSession(creds)


def ok(r):
    if r.status_code >= 300:
        sys.exit(f"{r.request.method} {r.url.split('?')[0]}: {r.status_code} {r.text[:1500]}")
    return r.json() if r.content else {}


def status(s):
    edit = ok(s.post(f"{API}/edits"))["id"]
    try:
        for t in ok(s.get(f"{API}/edits/{edit}/tracks")).get("tracks", []):
            for rel in t.get("releases", []) or [{}]:
                print(f"{t['track']}: {rel.get('status', 'empty')} {rel.get('name', '')} "
                      f"codes={rel.get('versionCodes', [])}")
    finally:
        s.delete(f"{API}/edits/{edit}")


def upload(s, aab, track, release_status, notes):
    edit = ok(s.post(f"{API}/edits"))["id"]
    try:
        with open(aab, "rb") as f:
            b = ok(s.post(f"{UPLOAD}/edits/{edit}/bundles", params={"uploadType": "media"},
                          data=f, headers={"Content-Type": "application/octet-stream"},
                          timeout=900))
        code = b["versionCode"]
        print("uploaded bundle", code, b.get("sha256", ""))
        release = {"versionCodes": [str(code)], "status": release_status}
        if notes:
            with open(notes, encoding="utf-8") as f:
                release["releaseNotes"] = [{"language": "en-US", "text": f.read().strip()[:500]}]
        ok(s.put(f"{API}/edits/{edit}/tracks/{track}",
                 json={"track": track, "releases": [release]}))
        print(f"track {track}: {code} ({release_status})")
        ok(s.post(f"{API}/edits/{edit}:commit"))
        print("committed")
    except SystemExit:
        s.delete(f"{API}/edits/{edit}")
        raise


def main():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    u = sub.add_parser("upload")
    u.add_argument("aab")
    u.add_argument("--track", default="internal")
    u.add_argument("--status", default="completed", choices=("draft", "completed"),
                   help="draft while the app has never been reviewed")
    u.add_argument("--notes", help="release notes file (en-US, 500 characters at most)")
    sub.add_parser("status")
    a = p.parse_args()
    s = session()
    if a.cmd == "status":
        status(s)
    else:
        upload(s, a.aab, a.track, a.status, a.notes)
        status(s)


if __name__ == "__main__":
    main()
