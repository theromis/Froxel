#!/usr/bin/env python3
"""Upload an .aab to a Google Play track with the Play Developer API.

    tools/play_upload.py froxel.aab --version 0.2.0 [--track internal] [--status draft]
    tools/play_upload.py froxel.aab --version 0.2.0 --track internal --track production \
        --status completed --notes-file notes.txt

PLAY_SERVICE_ACCOUNT_JSON holds the service account key (its JSON text, or a
path to it); PLAY_ACCESS_TOKEN, an OAuth token with the androidpublisher scope,
stands in for it when set. The account needs the Release permission for the app
in Play Console. Only the standard library and the openssl command are used:
openssl signs the token request.

The bundle is uploaded once and released on every --track given. Committing a
completed production release sends it for review; Play publishes it when the
review passes unless managed publishing is on. --notes-file holds the en-US
release notes, cut to Play's 500 characters.

A new app's first bundle is uploaded by hand in Play Console; the API can
update only an app that exists. Until the app has passed its first review,
Play accepts only `--status draft`, and the release is rolled out from Play
Console.
"""

import argparse
import base64
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://androidpublisher.googleapis.com/androidpublisher/v3/applications"
UPLOAD = "https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications"
SCOPE = "https://www.googleapis.com/auth/androidpublisher"


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def service_account():
    raw = os.environ.get("PLAY_SERVICE_ACCOUNT_JSON", "").strip()
    if not raw:
        sys.exit("play_upload: PLAY_SERVICE_ACCOUNT_JSON is not set")
    if not raw.startswith("{"):
        raw = pathlib.Path(raw).read_text()
    return json.loads(raw)


def signed_jwt(account, now):
    header = b64url(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    claims = b64url(
        json.dumps(
            {
                "iss": account["client_email"],
                "scope": SCOPE,
                "aud": account["token_uri"],
                "iat": now,
                "exp": now + 3600,
            }
        ).encode()
    )
    message = f"{header}.{claims}".encode()
    with tempfile.NamedTemporaryFile("w", suffix=".pem") as key:
        os.chmod(key.name, 0o600)
        key.write(account["private_key"])
        key.flush()
        signature = subprocess.run(
            ["openssl", "dgst", "-sha256", "-sign", key.name],
            input=message,
            capture_output=True,
            check=True,
        ).stdout
    return f"{header}.{claims}.{b64url(signature)}"


def request(method, url, token=None, body=None, content_type="application/json"):
    headers = {}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if body is not None:
        headers["Content-Type"] = content_type
    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=600) as response:
            text = response.read()
    except urllib.error.HTTPError as err:
        sys.exit(f"play_upload: {method} {url.split('?')[0]}: {err.code} {err.read().decode()}")
    return json.loads(text) if text else {}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("bundle", type=pathlib.Path)
    parser.add_argument("--version", required=True, help="release name, e.g. 0.2.0")
    parser.add_argument("--package", default="com.lonje.froxel")
    parser.add_argument("--track", action="append", help="internal (default), production, ...")
    parser.add_argument("--status", default="draft", choices=["draft", "completed"])
    parser.add_argument("--notes-file", type=pathlib.Path)
    args = parser.parse_args()
    tracks = args.track or ["internal"]

    token = os.environ.get("PLAY_ACCESS_TOKEN", "").strip()
    if not token:
        account = service_account()
        form = urllib.parse.urlencode(
            {
                "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
                "assertion": signed_jwt(account, int(time.time())),
            }
        ).encode()
        token = request(
            "POST",
            account["token_uri"],
            body=form,
            content_type="application/x-www-form-urlencoded",
        )["access_token"]

    app = f"{API}/{args.package}"
    edit = request("POST", f"{app}/edits", token, b"{}")["id"]
    uploaded = request(
        "POST",
        f"{UPLOAD}/{args.package}/edits/{edit}/bundles?uploadType=media",
        token,
        args.bundle.read_bytes(),
        "application/octet-stream",
    )
    code = uploaded["versionCode"]
    release = {"name": args.version, "versionCodes": [str(code)], "status": args.status}
    notes = args.notes_file.read_text().strip() if args.notes_file else ""
    if notes:
        release["releaseNotes"] = [{"language": "en-US", "text": notes[:500]}]
    for track in tracks:
        request(
            "PUT",
            f"{app}/edits/{edit}/tracks/{track}",
            token,
            json.dumps({"track": track, "releases": [release]}).encode(),
        )
    request("POST", f"{app}/edits/{edit}:commit", token, b"{}")
    for track in tracks:
        print(f"{args.package} {args.version} (versionCode {code}) on {track} as {args.status}")


if __name__ == "__main__":
    main()
