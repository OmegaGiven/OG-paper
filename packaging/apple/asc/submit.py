#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Submit the prepared version of a platform for App Review:

  submit.py --platform IOS --version 1.0

A submission Apple left with unresolved issues (a rejection) is answered
by resubmitting it; otherwise a new one is made with the version in it.
Needs ASC_KEY, ASC_KEY_ID, ASC_ISSUER."""

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from listing import call, version  # noqa: E402


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--platform", required=True)
    p.add_argument("--version", default="1.0")
    p.add_argument("--fresh", action="store_true",
                   help="cancel a rejected submission and make a new one")
    a = p.parse_args()
    aid, v = version(a.platform, a.version)
    if not call("GET", f"/v1/appStoreVersions/{v['id']}/build").get("data"):
        sys.exit("no build attached")
    subs = call("GET", "/v1/reviewSubmissions", **{
        "filter[app]": aid, "filter[platform]": a.platform, "limit": 20})["data"]
    open_sub = next((s for s in subs if s["attributes"]["state"] in
                     ("UNRESOLVED_ISSUES", "READY_FOR_REVIEW")), None)
    if open_sub and a.fresh and open_sub["attributes"]["state"] == "UNRESOLVED_ISSUES":
        call("PATCH", f"/v1/reviewSubmissions/{open_sub['id']}", {"data": {
            "type": "reviewSubmissions", "id": open_sub["id"], "attributes": {"canceled": True}}})
        print("canceled the rejected submission", open_sub["id"])
        open_sub = None
    if open_sub is None:
        open_sub = call("POST", "/v1/reviewSubmissions", {"data": {
            "type": "reviewSubmissions", "attributes": {"platform": a.platform},
            "relationships": {"app": {"data": {"type": "apps", "id": aid}}}}})["data"]
        print("new submission", open_sub["id"])
    else:
        print("reusing submission", open_sub["id"], open_sub["attributes"]["state"])
    items = call("GET", f"/v1/reviewSubmissions/{open_sub['id']}/items",
                 include="appStoreVersion")["data"]
    has = any((i["relationships"].get("appStoreVersion", {}).get("data") or {}).get("id") == v["id"]
              for i in items)
    if not has:
        call("POST", "/v1/reviewSubmissionItems", {"data": {
            "type": "reviewSubmissionItems",
            "relationships": {
                "reviewSubmission": {"data": {"type": "reviewSubmissions", "id": open_sub["id"]}},
                "appStoreVersion": {"data": {"type": "appStoreVersions", "id": v["id"]}}}}})
        print("added", a.platform, a.version)
    r = call("PATCH", f"/v1/reviewSubmissions/{open_sub['id']}", {"data": {
        "type": "reviewSubmissions", "id": open_sub["id"], "attributes": {"submitted": True}}})
    print("submitted:", r["data"]["attributes"]["state"])


if __name__ == "__main__":
    main()
