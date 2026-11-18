#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Print the next build number (CFBundleVersion) for OG Paper: one more
than the highest build App Store Connect has, on any platform, so every
upload's number grows whichever workflow made the last one. Reads only.
Needs ASC_KEY, ASC_KEY_ID, ASC_ISSUER."""

import sys

from status import BUNDLE, get


def main():
    apps = get("/v1/apps", **{"filter[bundleId]": BUNDLE})
    if not apps.get("data"):
        sys.exit(f"no app with bundle id {BUNDLE}: {apps}")
    aid = apps["data"][0]["id"]
    top = 0
    # Apps here have far fewer than 200 builds (the API's page limit).
    builds = get("/v1/builds", **{"filter[app]": aid, "limit": 200, "fields[builds]": "version"})
    if "error" in builds:
        sys.exit(f"builds: {builds}")
    for b in builds.get("data", []):
        try:
            top = max(top, int(b["attributes"]["version"].split(".")[0]))
        except ValueError:
            pass
    print(top + 1)


if __name__ == "__main__":
    main()
