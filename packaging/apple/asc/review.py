#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Everything a version's listing shows (read only): its texts, URLs, age
rating, and the screenshots and previews themselves, saved under OUT for a
look. Needs ASC_KEY, ASC_KEY_ID, ASC_ISSUER.

  review.py --platform IOS --version 1.0 --out listing/"""

import argparse
import os
import sys
import urllib.request

sys.path.insert(0, os.path.dirname(__file__))
from listing import call, version  # noqa: E402


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--platform", default="IOS")
    p.add_argument("--version", default="1.0")
    p.add_argument("--out", default="listing")
    a = p.parse_args()
    aid, v = version(a.platform, a.version)
    os.makedirs(a.out, exist_ok=True)
    print("state:", v["attributes"]["appStoreState"], "copyright:", v["attributes"].get("copyright"))
    for info in call("GET", f"/v1/apps/{aid}/appInfos")["data"]:
        for l in call("GET", f"/v1/appInfos/{info['id']}/appInfoLocalizations")["data"]:
            for k, val in l["attributes"].items():
                print(f"appInfo.{k}: {val}")
        age = call("GET", f"/v1/appInfos/{info['id']}/ageRatingDeclaration").get("data")
        if age:
            print("ageRating:", {k: val for k, val in age["attributes"].items() if val not in (None, False, "NONE")})
    for l in call("GET", f"/v1/appStoreVersions/{v['id']}/appStoreVersionLocalizations")["data"]:
        la = l["attributes"]
        for k in ("locale", "promotionalText", "keywords", "supportUrl", "marketingUrl", "whatsNew"):
            print(f"{k}: {la.get(k)}")
        print("description:\n" + (la.get("description") or ""))
        for s in call("GET", f"/v1/appStoreVersionLocalizations/{l['id']}/appScreenshotSets")["data"]:
            kind = s["attributes"]["screenshotDisplayType"]
            for i, shot in enumerate(call("GET", f"/v1/appScreenshotSets/{s['id']}/appScreenshots")["data"]):
                asset = shot["attributes"].get("imageAsset") or {}
                st = shot["attributes"].get("assetDeliveryState", {}).get("state")
                url = (asset.get("templateUrl") or "").replace("{w}", str(asset.get("width"))).replace("{h}", str(asset.get("height"))).replace("{f}", "png")
                print(f"shot {kind} {i + 1}: {asset.get('width')}x{asset.get('height')} {st}")
                if url:
                    urllib.request.urlretrieve(url, os.path.join(a.out, f"{kind}-{i + 1}.png"))
        for s in call("GET", f"/v1/appStoreVersionLocalizations/{l['id']}/appPreviewSets")["data"]:
            n = len(call("GET", f"/v1/appPreviewSets/{s['id']}/appPreviews")["data"])
            print(f"previews {s['attributes']['previewType']}: {n}")


if __name__ == "__main__":
    main()
