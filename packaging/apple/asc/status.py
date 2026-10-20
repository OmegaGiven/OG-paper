#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Where OG Paper stands in App Store Connect (read only): its versions on
each platform and their states, the latest builds, what each version's
listing still lacks, and review submissions. Needs the API key in the
environment: ASC_KEY (the .p8's text), ASC_KEY_ID, ASC_ISSUER."""

import json
import os
import sys
import time
import urllib.parse
import urllib.request

import jwt  # PyJWT

BUNDLE = os.environ.get("ASC_BUNDLE", "com.omegagiven.ogpaper")
API = "https://api.appstoreconnect.apple.com"


def token():
    now = int(time.time())
    return jwt.encode(
        {"iss": os.environ["ASC_ISSUER"], "iat": now, "exp": now + 1000, "aud": "appstoreconnect-v1"},
        os.environ["ASC_KEY"],
        algorithm="ES256",
        headers={"kid": os.environ["ASC_KEY_ID"], "typ": "JWT"},
    )


TOKEN = token()


def get(path, **params):
    url = API + path + ("?" + urllib.parse.urlencode(params) if params else "")
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {TOKEN}"})
    try:
        with urllib.request.urlopen(req) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        return {"error": e.code, "body": e.read().decode()[:400]}


def main():
    apps = get("/v1/apps", **{"filter[bundleId]": BUNDLE})
    if not apps.get("data"):
        print("No app with bundle id", BUNDLE, json.dumps(apps)[:400])
        return 1
    app = apps["data"][0]
    aid = app["id"]
    a = app["attributes"]
    print(f"App: {a.get('name')} ({aid}) sku={a.get('sku')} primaryLocale={a.get('primaryLocale')}")

    infos = get(f"/v1/apps/{aid}/appInfos", include="primaryCategory,ageRatingDeclaration")
    for info in infos.get("data", []):
        ia = info["attributes"]
        cat = info["relationships"].get("primaryCategory", {}).get("data")
        print(f"App info: state={ia.get('appStoreState') or ia.get('state')} ageRating={ia.get('appStoreAgeRating')} category={cat and cat['id']}")
        locs = get(f"/v1/appInfos/{info['id']}/appInfoLocalizations")
        for l in locs.get("data", []):
            la = l["attributes"]
            print(f"  [{la['locale']}] name={la.get('name')!r} subtitle={la.get('subtitle')!r} privacyPolicyUrl={la.get('privacyPolicyUrl')!r}")

    vers = get(f"/v1/apps/{aid}/appStoreVersions", limit=20)
    for v in vers.get("data", []):
        va = v["attributes"]
        print(f"\nVersion {va['platform']} {va['versionString']}: {va.get('appStoreState')} (release {va.get('releaseType')})")
        b = get(f"/v1/appStoreVersions/{v['id']}/build")
        bd = b.get("data")
        print("  build:", bd and f"{bd['attributes'].get('version')} {bd['attributes'].get('processingState')}")
        locs = get(f"/v1/appStoreVersions/{v['id']}/appStoreVersionLocalizations")
        for l in locs.get("data", []):
            la = l["attributes"]
            missing = [k for k in ("description", "keywords", "supportUrl") if not la.get(k)]
            sets = get(f"/v1/appStoreVersionLocalizations/{l['id']}/appScreenshotSets")
            shots = []
            for s in sets.get("data", []):
                n = len(get(f"/v1/appScreenshotSets/{s['id']}/appScreenshots").get("data", []))
                shots.append(f"{s['attributes']['screenshotDisplayType']}={n}")
            print(f"  [{la['locale']}] missing={missing or 'nothing'} screenshots: {', '.join(shots) or 'none'}")
        rd = get(f"/v1/appStoreVersions/{v['id']}/appStoreReviewDetail")
        if rd.get("data"):
            ra = rd["data"]["attributes"]
            print(f"  review contact: {bool(ra.get('contactEmail'))} notes:")
            for line in (ra.get("notes") or "").splitlines():
                print("    | " + line)
        else:
            print("  review details: none yet")

    builds = get("/v1/builds", **{"filter[app]": aid, "sort": "-uploadedDate", "limit": 6, "include": "preReleaseVersion"})
    pre = {i["id"]: i["attributes"] for i in builds.get("included", [])}
    print("\nLatest builds:")
    for b in builds.get("data", []):
        ba = b["attributes"]
        pv = b["relationships"].get("preReleaseVersion", {}).get("data")
        p = pre.get(pv["id"]) if pv else {}
        print(f"  {p.get('platform')} {p.get('version')} ({ba.get('version')}) {ba.get('processingState')} uploaded {ba.get('uploadedDate')} expired={ba.get('expired')}")

    subs = get("/v1/reviewSubmissions", **{"filter[app]": aid, "limit": 5})
    print("\nReview submissions:")
    for s in subs.get("data", []):
        sa = s["attributes"]
        print(f"  {sa.get('platform')} {sa.get('state')} submitted {sa.get('submittedDate')}")
    if not subs.get("data"):
        print("  none")
    return 0


if __name__ == "__main__":
    sys.exit(main())
