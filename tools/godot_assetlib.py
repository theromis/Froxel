#!/usr/bin/env python3
"""Ask the Godot Asset Library to move Froxel Bridge to a new release.

    GODOT_ASSETLIB_USER=... GODOT_ASSETLIB_PASSWORD=... \\
        python3 tools/godot_assetlib.py --asset-id 1234 --version 0.3.3

The listing uses the Custom download provider, so a version is the URL of that
release's froxel-<version>-godot-assetlib.zip rather than a commit hash. The
script logs in, checks the zip is on the release, and posts an edit with the
new version string and download URL; every other field stays as it is. A
moderator accepts the edit before users see it. --dry-run prints the request
without logging in. See docs/registries.md.
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

API = "https://godotengine.org/asset-library/api"
RELEASES = "https://github.com/theromis/Froxel/releases/download"


def call(path, body):
    request = urllib.request.Request(
        f"{API}{path}",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as reply:
            return json.load(reply)
    except urllib.error.HTTPError as error:
        sys.exit(f"{path}: HTTP {error.code}: {error.read().decode(errors='replace')}")


def exists(url):
    request = urllib.request.Request(url, method="HEAD")
    try:
        with urllib.request.urlopen(request, timeout=60) as reply:
            return reply.status == 200
    except urllib.error.HTTPError:
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--asset-id", required=True, type=int, help="the number in the asset's URL")
    parser.add_argument("--version", required=True, help="the Froxel release, e.g. 0.3.3")
    parser.add_argument("--godot-version", help="minimum Godot version, if it changed (e.g. 4.3)")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    url = f"{RELEASES}/v{args.version}/froxel-{args.version}-godot-assetlib.zip"
    edit = {
        "version_string": args.version,
        "download_provider": "Custom",
        "download_commit": url,
    }
    if args.godot_version:
        edit["godot_version"] = args.godot_version
    if args.dry_run:
        print(f"POST {API}/asset/{args.asset_id}")
        print(json.dumps(edit, indent=2))
        return 0
    if not exists(url):
        sys.exit(f"{url} is not on a published release yet")

    user = os.environ.get("GODOT_ASSETLIB_USER")
    password = os.environ.get("GODOT_ASSETLIB_PASSWORD")
    if not user or not password:
        sys.exit("set GODOT_ASSETLIB_USER and GODOT_ASSETLIB_PASSWORD")
    token = call("/login", {"username": user, "password": password}).get("token")
    if not token:
        sys.exit("login failed")
    reply = call(f"/asset/{args.asset_id}", dict(edit, token=token))
    print(f"edit {reply.get('id')} waits for a moderator: https://godotengine.org/asset-library/{reply.get('url')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
