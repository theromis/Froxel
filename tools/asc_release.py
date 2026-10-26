#!/usr/bin/env python3
"""Submit an uploaded iOS build for App Store review.

    tools/asc_release.py --version 0.3.1 --build 30199 [--notes-file notes.txt] [--dry-run]

ASC_KEY is the App Store Connect API key's text (or a path to the .p8),
ASC_KEY_ID its ID, and ASC_ISSUER_ID the issuer. The key needs the App Manager
role; a Developer key uploads builds but cannot edit a version or submit it.
Only the standard library and the openssl command are used.

It waits until App Store Connect has processed the build, then takes the iOS
version that can still be edited (or creates one), sets its version string to
the marketing version, sets What's New from --notes-file, attaches the build,
and submits it for review. The release type set on the version (manual or
after approval) is kept. --dry-run reads everything and changes nothing.
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
import urllib.request

API = "https://api.appstoreconnect.apple.com"
BUNDLE = "com.lonje.froxel"
# Versions in these states take a new build and a new submission.
EDITABLE = {
    "PREPARE_FOR_SUBMISSION",
    "DEVELOPER_REJECTED",
    "REJECTED",
    "METADATA_REJECTED",
    "INVALID_BINARY",
}
DEFAULT_NOTES = "Bug fixes and improvements."


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def der_to_raw(der):
    """An ECDSA signature from openssl (DER) as JWT wants it: r and s, 32 bytes each."""
    i = 2 if der[1] < 0x80 else 2 + (der[1] & 0x7F)
    out = b""
    for _ in range(2):
        length = der[i + 1]
        out += der[i + 2 : i + 2 + length].lstrip(b"\0").rjust(32, b"\0")
        i += 2 + length
    return out


def token():
    key = os.environ["ASC_KEY"].strip()
    if not key.startswith("-----"):
        key = pathlib.Path(key).read_text()
    now = int(time.time())
    header = b64url(json.dumps({"alg": "ES256", "kid": os.environ["ASC_KEY_ID"], "typ": "JWT"}).encode())
    claims = b64url(
        json.dumps(
            {"iss": os.environ["ASC_ISSUER_ID"], "iat": now, "exp": now + 1200, "aud": "appstoreconnect-v1"}
        ).encode()
    )
    message = f"{header}.{claims}".encode()
    with tempfile.NamedTemporaryFile("w", suffix=".p8") as file:
        os.chmod(file.name, 0o600)
        file.write(key)
        file.flush()
        der = subprocess.run(
            ["openssl", "dgst", "-sha256", "-sign", file.name], input=message, capture_output=True, check=True
        ).stdout
    return f"{header}.{claims}.{b64url(der_to_raw(der))}"


class Client:
    def __init__(self, dry_run):
        self.dry_run = dry_run
        self.token = token()
        self.issued = time.time()

    def call(self, method, path, body=None):
        if method != "GET" and self.dry_run:
            print(f"dry run: {method} {path} {json.dumps(body)[:200] if body else ''}")
            return {"data": {"id": "dry-run"}}
        if time.time() - self.issued > 900:
            self.token, self.issued = token(), time.time()
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(
            API + path,
            data=data,
            method=method,
            headers={"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=120) as response:
                text = response.read()
        except urllib.error.HTTPError as err:
            sys.exit(f"asc_release: {method} {path}: {err.code} {err.read().decode()[:1000]}")
        return json.loads(text) if text else {}


def wait_for_build(client, app, number, minutes):
    deadline = time.time() + minutes * 60
    while True:
        builds = client.call("GET", f"/v1/builds?filter[app]={app}&filter[version]={number}&limit=1")["data"]
        state = builds[0]["attributes"]["processingState"] if builds else "not uploaded yet"
        if state == "VALID":
            return builds[0]["id"]
        if state in ("FAILED", "INVALID"):
            sys.exit(f"asc_release: build {number} is {state}")
        if time.time() > deadline:
            sys.exit(f"asc_release: build {number} is still {state} after {minutes} minutes")
        print(f"build {number}: {state}; waiting")
        time.sleep(60)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--version", required=True, help="marketing version, e.g. 0.3.1")
    parser.add_argument("--build", required=True, help="CFBundleVersion, e.g. 30199")
    parser.add_argument("--notes-file", type=pathlib.Path)
    parser.add_argument("--wait-minutes", type=int, default=45)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    version = args.version.split("-")[0]
    notes = (args.notes_file.read_text().strip() if args.notes_file else "") or DEFAULT_NOTES

    client = Client(args.dry_run)
    app = client.call("GET", f"/v1/apps?filter[bundleId]={BUNDLE}")["data"][0]["id"]
    build = wait_for_build(client, app, args.build, args.wait_minutes)
    print(f"build {args.build} is processed")

    versions = client.call("GET", f"/v1/apps/{app}/appStoreVersions?filter[platform]=IOS&limit=20")["data"]
    editable = [v for v in versions if v["attributes"]["appStoreState"] in EDITABLE]
    first_release = not any(v["attributes"]["appStoreState"] == "READY_FOR_SALE" for v in versions)
    if editable:
        target = editable[0]["id"]
        if editable[0]["attributes"]["versionString"] != version:
            client.call(
                "PATCH",
                f"/v1/appStoreVersions/{target}",
                {"data": {"type": "appStoreVersions", "id": target, "attributes": {"versionString": version}}},
            )
        print(f"version {version}: reusing the editable version")
    else:
        busy = [v["attributes"]["versionString"] for v in versions if v["attributes"]["appStoreState"] != "READY_FOR_SALE"]
        if busy:
            sys.exit(f"asc_release: version {busy[0]} is still in review or pending release; finish it first")
        target = client.call(
            "POST",
            "/v1/appStoreVersions",
            {
                "data": {
                    "type": "appStoreVersions",
                    "attributes": {"platform": "IOS", "versionString": version},
                    "relationships": {"app": {"data": {"type": "apps", "id": app}}},
                }
            },
        )["data"]["id"]
        print(f"version {version}: created")

    # What's New exists only on updates; the first version has none.
    if not first_release and target != "dry-run":
        for loc in client.call("GET", f"/v1/appStoreVersions/{target}/appStoreVersionLocalizations")["data"]:
            client.call(
                "PATCH",
                f"/v1/appStoreVersionLocalizations/{loc['id']}",
                {"data": {"type": "appStoreVersionLocalizations", "id": loc["id"], "attributes": {"whatsNew": notes[:4000]}}},
            )
        print("what's new set")

    client.call(
        "PATCH",
        f"/v1/appStoreVersions/{target}/relationships/build",
        {"data": {"type": "builds", "id": build}},
    )
    print(f"build {args.build} attached")

    submission = client.call(
        "POST",
        "/v1/reviewSubmissions",
        {
            "data": {
                "type": "reviewSubmissions",
                "attributes": {"platform": "IOS"},
                "relationships": {"app": {"data": {"type": "apps", "id": app}}},
            }
        },
    )["data"]["id"]
    client.call(
        "POST",
        "/v1/reviewSubmissionItems",
        {
            "data": {
                "type": "reviewSubmissionItems",
                "relationships": {
                    "reviewSubmission": {"data": {"type": "reviewSubmissions", "id": submission}},
                    "appStoreVersion": {"data": {"type": "appStoreVersions", "id": target}},
                },
            }
        },
    )
    client.call(
        "PATCH",
        f"/v1/reviewSubmissions/{submission}",
        {"data": {"type": "reviewSubmissions", "id": submission, "attributes": {"submitted": True}}},
    )
    print(f"Froxel {version} ({args.build}) submitted for App Store review")


if __name__ == "__main__":
    main()
