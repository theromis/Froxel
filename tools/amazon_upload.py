#!/usr/bin/env python3
"""Upload an APK to the Amazon Appstore with the App Submission API.

    tools/amazon_upload.py froxel.apk [--notes-file notes.txt] [--commit]

AMAZON_CLIENT_ID and AMAZON_CLIENT_SECRET are the security profile that has
App Submission API access (Developer Console, Settings, API Access), and
AMAZON_APP_ID is the app's ID (amzn1.devportal.mobileapp.<id>). Only the
standard library is used.

The script reuses the app's open edit or opens one, replaces the edit's APK
with the new file (the device targeting is kept), removes any other APK in
the edit, and sets the en-US "What's new" (recentChanges) from --notes-file.
Without --commit the edit stays open as a draft in the Developer Console.
With --commit it is committed, which submits the version for Amazon's review;
it goes live when the review passes. An app in review cannot be changed
(412 Precondition Failed): wait until it is published.

The API does not take App Bundles, and it updates only an app that exists:
the first version is submitted by hand in the Developer Console.
"""

import argparse
import json
import os
import pathlib
import sys
import urllib.error
import urllib.parse
import urllib.request

TOKEN = "https://api.amazon.com/auth/o2/token"
API = "https://developer.amazon.com/api/appstore/v1/applications"
SCOPE = "appstore::apps:readwrite"
LANGUAGE = "en-US"


def env(name):
    value = os.environ.get(name, "").strip()
    if not value:
        sys.exit(f"amazon_upload: {name} is not set")
    return value


def request(method, url, token=None, body=None, headers=None, allow=()):
    """Returns (status, headers, parsed body); exits on an HTTP error not in allow."""
    headers = dict(headers or {})
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=600) as response:
            text = response.read()
            status, reply = response.status, response.headers
    except urllib.error.HTTPError as err:
        text = err.read()
        if err.code not in allow:
            sys.exit(f"amazon_upload: {method} {url}: {err.code} {text.decode(errors='replace')}")
        return err.code, err.headers, {}
    try:
        data = json.loads(text) if text.strip() else {}
    except ValueError:
        data = {}
    return status, reply, data


def access_token():
    form = urllib.parse.urlencode(
        {
            "grant_type": "client_credentials",
            "client_id": env("AMAZON_CLIENT_ID"),
            "client_secret": env("AMAZON_CLIENT_SECRET"),
            "scope": SCOPE,
        }
    ).encode()
    _, _, data = request(
        "POST", TOKEN, body=form, headers={"Content-Type": "application/x-www-form-urlencoded"}
    )
    return data["access_token"]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("apk", type=pathlib.Path)
    parser.add_argument("--notes-file", type=pathlib.Path)
    parser.add_argument("--commit", action="store_true", help="submit the edit for review")
    args = parser.parse_args()

    token = access_token()
    app = f"{API}/{urllib.parse.quote(env('AMAZON_APP_ID'))}"

    # One edit can be open per app; a new one starts as a copy of the live app.
    _, _, edit = request("GET", f"{app}/edits", token)
    if not edit.get("id"):
        _, _, edit = request("POST", f"{app}/edits", token)
    edit_url = f"{app}/edits/{edit['id']}"
    print(f"Amazon: edit {edit['id']} ({edit.get('status', 'open')})")

    _, _, apks = request("GET", f"{edit_url}/apks", token)
    apks = apks if isinstance(apks, list) else apks.get("apks", [])
    data = args.apk.read_bytes()
    apk_headers = {
        "Content-Type": "application/vnd.android.package-archive",
        "fileName": args.apk.name,
    }
    if apks:
        first, rest = apks[0], apks[1:]
        _, reply, _ = request("GET", f"{edit_url}/apks/{first['id']}", token)
        _, _, apk = request(
            "PUT",
            f"{edit_url}/apks/{first['id']}/replace",
            token,
            body=data,
            headers={**apk_headers, "If-Match": reply["ETag"]},
        )
        for old in rest:
            _, reply, _ = request("GET", f"{edit_url}/apks/{old['id']}", token)
            request("DELETE", f"{edit_url}/apks/{old['id']}", token, headers={"If-Match": reply["ETag"]})
    else:
        _, _, apk = request("POST", f"{edit_url}/apks/upload", token, body=data, headers=apk_headers)
    print(f"Amazon: uploaded {args.apk.name} as APK {apk.get('id', '?')} (versionCode {apk.get('versionCode', '?')})")

    notes = args.notes_file.read_text().strip() if args.notes_file else ""
    if notes:
        listing_url = f"{edit_url}/listings/{LANGUAGE}"
        _, reply, listing = request("GET", listing_url, token)
        listing["recentChanges"] = notes[:4000]
        request(
            "PUT",
            listing_url,
            token,
            body=json.dumps(listing).encode(),
            headers={"Content-Type": "application/json", "If-Match": reply["ETag"]},
        )
        print(f"Amazon: set the {LANGUAGE} release notes")

    status, _, problems = request("POST", f"{edit_url}/validate", token, allow=(400, 404, 422))
    if status in (400, 422):
        sys.exit(f"amazon_upload: the edit does not validate: {json.dumps(problems)}")

    if args.commit:
        _, reply, _ = request("GET", edit_url, token)
        request("POST", f"{edit_url}/commit", token, headers={"If-Match": reply["ETag"]})
        print("Amazon: committed; the version is submitted for review")
    else:
        print("Amazon: the edit is open as a draft in the Developer Console")


if __name__ == "__main__":
    main()
